//! Noticias de skills y MCPs: port de `lib/news_watch.py` (los siete
//! colectores, `collect` y `watch_news` con su historial SQLite).
//!
//! La red la hace el `Fetch` de quien llama (el frente: `reqwest` con la
//! política de redirecciones de `urllib`; las pruebas: respuestas fijas o un
//! servidor local). Las URLs de los feeds salen de `Endpoints`: en producción
//! las del Python (`COMANDOS_SEARX` incluido); en las pruebas, un servidor de
//! `127.0.0.1`. Las URLs que se guardan en los ítems (`news.ycombinator.com`,
//! `reddit.com`, `superteam.fun`, `dorahacks.io`) son siempre las del Python.
//!
//! Cada consulta de cada fuente es independiente, como en el Python: una
//! excepción corta esa consulta (lo ya leído se queda) y las demás siguen.
//! Desviación documentada: lo que el port no reproduce con certeza (un `url`
//! que no es texto, enteros de más de 64 bits, fechas sin zona horaria, texto
//! que `re` y `regex` clasificarían distinto, JSON que el parser portado no
//! sabe leer como `json`) corta la consulta como si fuera una excepción.
use crate::{
    cli_catalog::py_str,
    cli_help::regex_safe,
    hooks::py,
    model_watch::{Fault, write_tmp_replace},
};
use comandos_core::{
    json::{PythonLoads, parse_value, python_loads, response_dumps_unicode},
    text::{NumError, int as py_int_text, splitlines, strip},
};
use regex::Regex;
use serde_json::{Map, Number, Value, json};
use std::{
    collections::{BTreeSet, HashSet},
    path::Path,
    sync::LazyLock,
    time::Duration,
};

pub const UA: &str = "ComandOS-news/1.0 (local dashboard; +localhost)";
pub const UA_BROWSER: &str = "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 \
(KHTML, like Gecko) Chrome/124.0 Safari/537.36";
/// `SEARX` sin `COMANDOS_SEARX`.
pub const DEFAULT_SEARX: &str = "http://localhost:27950";
/// `_get_json(..., timeout=12)`; las fuentes de hackathones usan 15.
const TIMEOUT: Duration = Duration::from_secs(12);
const SLOW_TIMEOUT: Duration = Duration::from_secs(15);
/// `max_age_days=7` de `watch_news`.
const MAX_AGE_DAYS: i64 = 7;

/// A dónde van las peticiones de cada fuente.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Endpoints {
    /// `SEARX` (también va al snapshot, `sources.searxng`).
    pub searx: String,
    pub hn: String,
    pub reddit: String,
    pub github: String,
    pub devpost: String,
    pub superteam: String,
    pub dorahacks: String,
}

impl Endpoints {
    /// Las del Python: `COMANDOS_SEARX` (o el valor por omisión) y los hosts
    /// públicos fijos.
    pub fn production(searx: Option<&str>) -> Self {
        Self {
            searx: searx.unwrap_or(DEFAULT_SEARX).to_owned(),
            hn: "https://hn.algolia.com".into(),
            reddit: "https://www.reddit.com".into(),
            github: "https://api.github.com".into(),
            devpost: "https://devpost.com".into(),
            superteam: "https://earn.superteam.fun".into(),
            dorahacks: "https://dorahacks.io".into(),
        }
    }

    /// Todas las fuentes en un mismo servidor (pruebas): las rutas de cada
    /// fuente no se pisan.
    pub fn local(base: &str) -> Self {
        Self {
            searx: base.into(),
            hn: base.into(),
            reddit: base.into(),
            github: base.into(),
            devpost: base.into(),
            superteam: base.into(),
            dorahacks: base.into(),
        }
    }
}

/// Lo que devuelve `urlopen` (con las redirecciones 301, 302, 303 y 307 ya
/// seguidas, como `urllib`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Fetched {
    /// 2xx: el cuerpo entero.
    Body(Vec<u8>),
    /// `HTTPError`: el código y su `Location`.
    Status { code: u16, location: Option<String> },
    /// Cualquier otra excepción (red, plazo, DNS…), con su texto.
    Failed(String),
}

/// La red de `urllib.request.urlopen`: GET con estas cabeceras y este plazo
/// por operación de socket.
pub trait Fetch {
    fn get(&self, url: &str, headers: &[(&str, &str)], timeout: Duration) -> Fetched;
}

/// Un fallo de una fuente (`{"source", "error"}` de `collect`). El texto no es
/// el `str(exc)` del Python: es el del port.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Failure {
    pub source: String,
    pub error: String,
}

/// Una excepción dentro de una consulta: su texto, o lo que no se reproduce.
#[derive(Debug, Clone)]
enum Exc {
    Raised(String),
    Unsure,
}

type Q<T> = Result<T, Exc>;

fn raised<T>(msg: &str) -> Q<T> {
    Err(Exc::Raised(msg.to_owned()))
}

fn compile(pattern: &str) -> Option<Regex> {
    Regex::new(pattern).ok()
}

static SKILL_RE: LazyLock<Option<Regex>> = LazyLock::new(|| compile(r"(?i)\bskills?\b"));
static MCP_RE: LazyLock<Option<Regex>> =
    LazyLock::new(|| compile(r"(?i)\bmcp\b|model context protocol"));
static HN_TOPIC: LazyLock<Option<Regex>> =
    LazyLock::new(|| compile(r"(?i)claude|anthropic|\bmcp\b|model context"));
static TAGS: LazyLock<Option<Regex>> = LazyLock::new(|| compile(r"<[^>]+>"));

fn rule(cell: &'static LazyLock<Option<Regex>>) -> Q<&'static Regex> {
    cell.as_ref().ok_or(Exc::Unsure)
}

/// `re.search(pattern, text, re.I)`; texto que `re` y `regex` clasificarían
/// distinto no se reproduce.
fn search(cell: &'static LazyLock<Option<Regex>>, text: &str) -> Q<bool> {
    if !regex_safe(text) {
        return Err(Exc::Unsure);
    }
    Ok(rule(cell)?.is_match(text))
}

/// `_kind(title)`.
fn kind_of(title: &str) -> Q<&'static str> {
    if search(&MCP_RE, title)? {
        return Ok("mcp");
    }
    if search(&SKILL_RE, title)? {
        return Ok("skill");
    }
    Ok("noticia")
}

/// `int(x)` de Python sobre un valor de JSON.
fn py_int(value: &Value) -> Q<i64> {
    match value {
        Value::Bool(b) => Ok(i64::from(*b)),
        Value::Number(n) => number_int(n),
        Value::String(s) => match py_int_text(s) {
            Ok(n) => Ok(n),
            Err(NumError::Invalid) => raised("invalid literal for int()"),
            Err(NumError::Exotic) => Err(Exc::Unsure),
        },
        Value::Null => raised("int() argument must be a string or a number, not 'NoneType'"),
        _ => raised("int() argument must be a string or a number"),
    }
}

fn number_int(n: &Number) -> Q<i64> {
    if py::is_float(n) {
        let f = py::as_f64(n);
        if !f.is_finite() {
            return raised("cannot convert float to integer");
        }
        let t = f.trunc();
        // ±2^63 exactos: fuera de ahí Python da un entero grande.
        if (-9_223_372_036_854_775_808.0..9_223_372_036_854_775_808.0).contains(&t) {
            Ok(t as i64)
        } else {
            Err(Exc::Unsure)
        }
    } else {
        n.as_i64().ok_or(Exc::Unsure)
    }
}

/// `x.get(key)` de un ítem que debe ser `dict`.
fn obj(value: &Value) -> Q<&Map<String, Value>> {
    match value {
        Value::Object(map) => Ok(map),
        _ => raised("object has no attribute 'get'"),
    }
}

/// `x or {}` antes de un `.get`: un valor verdadero que no es objeto lanza.
fn dict_or_empty(value: Option<&Value>) -> Q<Map<String, Value>> {
    match value {
        Some(Value::Object(map)) => Ok(map.clone()),
        Some(v) if py::truthy(v) => raised("object has no attribute 'get'"),
        _ => Ok(Map::new()),
    }
}

/// `x or ""` antes de un método de `str`.
fn str_or_empty(value: Option<&Value>) -> Q<String> {
    match value {
        Some(Value::String(s)) => Ok(s.clone()),
        Some(v) if py::truthy(v) => raised("object has no attribute of str"),
        _ => Ok(String::new()),
    }
}

/// `(x or [])[:n]` de una lista (o de un texto: caracteres, que luego no
/// tienen `.get`); un objeto o un número no se pueden recortar.
fn slice_list(value: Option<&Value>, n: usize) -> Q<Vec<Value>> {
    match value {
        Some(v) if !py::truthy(v) => Ok(Vec::new()),
        None => Ok(Vec::new()),
        Some(Value::Array(items)) => Ok(items.iter().take(n).cloned().collect()),
        Some(Value::String(s)) => Ok(s
            .chars()
            .take(n)
            .map(|c| Value::from(c.to_string()))
            .collect()),
        Some(_) => raised("unhashable type: 'slice'"),
    }
}

/// `for x in (value or [])`: lista, caracteres de un texto o claves.
fn iterate(value: Option<&Value>) -> Q<Vec<Value>> {
    match value {
        Some(v) if !py::truthy(v) => Ok(Vec::new()),
        None => Ok(Vec::new()),
        Some(Value::Array(items)) => Ok(items.clone()),
        Some(Value::String(s)) => Ok(s.chars().map(|c| Value::from(c.to_string())).collect()),
        Some(Value::Object(map)) => Ok(map.keys().map(|k| Value::from(k.clone())).collect()),
        Some(_) => raised("object is not iterable"),
    }
}

/// `f"{x}"` (= `str(x)`) de un valor de JSON.
fn fstr(value: &Value) -> Q<String> {
    py_str(value).map_err(|_| Exc::Unsure)
}

/// `title[:n]`.
fn take(text: &str, n: usize) -> String {
    text.chars().take(n).collect()
}

/// `urllib.parse.quote_plus(s, safe="")`.
fn quote_plus(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'_' | b'.' | b'-' | b'~' => {
                out.push(b as char)
            }
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// `urllib.parse.urlencode(pairs)`.
fn urlencode(pairs: &[(&str, &str)]) -> String {
    pairs
        .iter()
        .map(|(k, v)| format!("{}={}", quote_plus(k), quote_plus(v)))
        .collect::<Vec<_>>()
        .join("&")
}

/// `urllib.parse.urljoin(base, loc)` para lo que manda un `Location`:
/// absoluta, sin esquema (`//host/…`), de raíz (`/ruta`) o relativa sin
/// segmentos `.`/`..` (esos no se reproducen).
fn urljoin(base: &str, loc: &str) -> Q<String> {
    let scheme_end = base.find("://").ok_or(Exc::Unsure)?;
    let scheme = base.get(..scheme_end).unwrap_or_default();
    if loc.contains("://") && loc.split("://").next().is_some_and(|s| !s.contains('/')) {
        return Ok(loc.to_owned());
    }
    if let Some(rest) = loc.strip_prefix("//") {
        return Ok(format!("{scheme}://{rest}"));
    }
    let after = base.get(scheme_end + 3..).unwrap_or_default();
    let host_end = after.find(['/', '?', '#']).unwrap_or(after.len());
    let origin = base.get(..scheme_end + 3 + host_end).unwrap_or(base);
    if loc.starts_with('/') {
        return Ok(format!("{origin}{loc}"));
    }
    if loc
        .split(['?', '#'])
        .next()
        .is_some_and(|p| p.split('/').any(|seg| seg == "." || seg == ".."))
        || loc.is_empty()
    {
        return Err(Exc::Unsure);
    }
    let path = after.get(host_end..).unwrap_or_default();
    let path = path.split(['?', '#']).next().unwrap_or_default();
    let dir = match path.rfind('/') {
        Some(i) => path.get(..=i).unwrap_or("/"),
        None => "/",
    };
    Ok(format!("{origin}{dir}{loc}"))
}

/// Hora de `time.time()` (segundos con fracción).
pub type Clock<'a> = &'a (dyn Fn() -> f64 + Sync);

/// Las fuentes de noticias con su red y su reloj.
pub struct Sources<'a> {
    pub fetch: &'a dyn Fetch,
    pub endpoints: &'a Endpoints,
    pub clock: Clock<'a>,
}

impl Sources<'_> {
    /// `int(time.time())`.
    fn now_int(&self) -> i64 {
        let t = (self.clock)().trunc();
        if t.is_finite() { t as i64 } else { 0 }
    }

    /// `_get_json(url, timeout, headers, browser)`: 308 seguidos a mano (hasta
    /// tres saltos), cuerpo decodificado con sustitutos y `json.loads`.
    fn get_json(
        &self,
        url: &str,
        timeout: Duration,
        extra: &[(&str, &str)],
        browser: bool,
    ) -> Q<Value> {
        let mut url = url.to_owned();
        let mut hops = 0;
        loop {
            let mut headers: Vec<(&str, &str)> = vec![
                ("User-Agent", if browser { UA_BROWSER } else { UA }),
                ("Accept", "application/json"),
            ];
            for (k, v) in extra {
                match headers.iter_mut().find(|(h, _)| h == k) {
                    Some(slot) => slot.1 = v,
                    None => headers.push((k, v)),
                }
            }
            match self.fetch.get(&url, &headers, timeout) {
                Fetched::Body(bytes) => {
                    let text = String::from_utf8_lossy(&bytes);
                    return match python_loads(&text) {
                        PythonLoads::Ok => parse_value(&text).map_err(|_| Exc::Unsure),
                        PythonLoads::Error(msg) => Err(Exc::Raised(msg)),
                        PythonLoads::Unsure => Err(Exc::Unsure),
                    };
                }
                Fetched::Status { code, location } => {
                    if code == 308
                        && hops < 3
                        && let Some(loc) = location.filter(|l| !l.is_empty())
                    {
                        url = urljoin(&url, &loc)?;
                        hops += 1;
                        continue;
                    }
                    return Err(Exc::Raised(format!("HTTP Error {code}")));
                }
                Fetched::Failed(msg) => return Err(Exc::Raised(msg)),
            }
        }
    }

    /// `_mk(source, title, url, at, published)`.
    fn mk(
        &self,
        source: &str,
        title: Option<&Value>,
        url: Option<&Value>,
        at: Option<&Value>,
        published: Option<&Value>,
    ) -> Q<Option<Map<String, Value>>> {
        let title = match title {
            Some(Value::String(s)) => strip(s).to_owned(),
            Some(v) if py::truthy(v) => return raised("object has no attribute 'strip'"),
            _ => String::new(),
        };
        let url = match url {
            Some(v) if py::truthy(v) => v,
            _ => return Ok(None),
        };
        if title.is_empty() {
            return Ok(None);
        }
        let Value::String(url) = url else {
            return Err(Exc::Unsure);
        };
        let kind = kind_of(&title)?;
        let at = match at {
            Some(v) if py::truthy(v) => py_int(v)?,
            _ => self.now_int(),
        };
        let mut item = Map::new();
        item.insert("source".into(), source.into());
        item.insert("kind".into(), kind.into());
        item.insert("title".into(), take(&title, 180).into());
        item.insert("url".into(), url.clone().into());
        item.insert("at".into(), at.into());
        if let Some(p) = published.filter(|v| py::truthy(v)) {
            item.insert("publishedAt".into(), py_int(p)?.into());
        }
        Ok(Some(item))
    }

    /// `fetch_searxng(now, errors)`.
    pub fn fetch_searxng(&self, now: i64, errors: Option<&mut Vec<Failure>>) -> Vec<Value> {
        let mut out = Vec::new();
        let mut errors = errors;
        for q in [
            "\"claude code\" skill",
            "\"claude\" mcp server",
            "model context protocol server nuevo",
        ] {
            let params = urlencode(&[
                ("q", q),
                ("format", "json"),
                ("time_range", "week"),
                ("pageno", "1"),
            ]);
            let url = format!("{}/search?{params}", self.endpoints.searx);
            let result: Q<()> = (|| {
                let data = self.get_json(&url, TIMEOUT, &[], false)?;
                for r in slice_list(obj(&data)?.get("results"), 8)? {
                    let r = obj(&r)?;
                    let at = Value::from(now);
                    if let Some(it) =
                        self.mk("searxng", r.get("title"), r.get("url"), Some(&at), None)?
                    {
                        out.push(Value::Object(it));
                    }
                }
                Ok(())
            })();
            fail(&mut errors, "searxng", result);
        }
        out
    }

    /// `fetch_hn(now, errors)`.
    pub fn fetch_hn(&self, _now: i64, errors: Option<&mut Vec<Failure>>) -> Vec<Value> {
        let mut out = Vec::new();
        let mut errors = errors;
        for q in ["\"claude code\" skill", "\"claude\" skills", "mcp server"] {
            let params = urlencode(&[("query", q), ("tags", "story"), ("hitsPerPage", "8")]);
            let url = format!("{}/api/v1/search_by_date?{params}", self.endpoints.hn);
            let result: Q<()> = (|| {
                let data = self.get_json(&url, TIMEOUT, &[], false)?;
                for h in iterate(obj(&data)?.get("hits"))? {
                    let h = obj(&h)?;
                    let title = str_or_empty(h.get("title"))?;
                    if !search(&HN_TOPIC, &title)? {
                        continue;
                    }
                    let url = match h.get("url") {
                        Some(v) if py::truthy(v) => v.clone(),
                        _ => Value::from(format!(
                            "https://news.ycombinator.com/item?id={}",
                            fstr(h.get("objectID").unwrap_or(&Value::Null))?
                        )),
                    };
                    let created = h.get("created_at_i");
                    let title = Value::from(title);
                    if let Some(it) =
                        self.mk("hackernews", Some(&title), Some(&url), created, created)?
                    {
                        out.push(Value::Object(it));
                    }
                }
                Ok(())
            })();
            fail(&mut errors, "hackernews", result);
        }
        out
    }

    /// `fetch_reddit(now, errors)`.
    pub fn fetch_reddit(&self, _now: i64, errors: Option<&mut Vec<Failure>>) -> Vec<Value> {
        let mut out = Vec::new();
        let mut errors = errors;
        for sub in ["ClaudeAI", "ClaudeCode", "mcp"] {
            let url = format!("{}/r/{sub}/new.json?limit=15", self.endpoints.reddit);
            let result: Q<()> = (|| {
                let data = self.get_json(&url, TIMEOUT, &[("Accept", "*/*")], false)?;
                let listing = dict_or_empty(obj(&data)?.get("data"))?;
                for c in iterate(listing.get("children"))? {
                    let p = dict_or_empty(obj(&c)?.get("data"))?;
                    let title = str_or_empty(p.get("title"))?;
                    if !(search(&SKILL_RE, &title)? || search(&MCP_RE, &title)?) {
                        continue;
                    }
                    let permalink = str_or_empty(p.get("permalink"))?;
                    let url = Value::from(format!("https://reddit.com{permalink}"));
                    let created = p.get("created_utc");
                    let title = Value::from(title);
                    if let Some(it) = self.mk(
                        &format!("r/{sub}"),
                        Some(&title),
                        Some(&url),
                        created,
                        created,
                    )? {
                        out.push(Value::Object(it));
                    }
                }
                Ok(())
            })();
            fail(&mut errors, "reddit", result);
        }
        out
    }

    /// `fetch_github(now, errors)`.
    pub fn fetch_github(&self, now: i64, errors: Option<&mut Vec<Failure>>) -> Vec<Value> {
        let mut out = Vec::new();
        let mut errors = errors;
        for (repo, kind) in [
            ("anthropics/skills", "skill"),
            ("modelcontextprotocol/servers", "mcp"),
        ] {
            let url = format!("{}/repos/{repo}/commits?per_page=5", self.endpoints.github);
            let owner = repo.split('/').next().unwrap_or_default();
            let result: Q<()> = (|| {
                let data = self.get_json(
                    &url,
                    TIMEOUT,
                    &[("Accept", "application/vnd.github+json")],
                    false,
                )?;
                let commits = match &data {
                    Value::Array(items) => items.clone(),
                    _ => Vec::new(),
                };
                for c in commits {
                    let c = obj(&c)?;
                    let commit = dict_or_empty(c.get("commit"))?;
                    let message = str_or_empty(commit.get("message"))?;
                    let msg = match splitlines(&message).first() {
                        Some(line) => (*line).to_owned(),
                        None => return raised("list index out of range"),
                    };
                    let when = dict_or_empty(dict_or_empty(c.get("commit"))?.get("author"))?
                        .get("date")
                        .cloned()
                        .filter(py::truthy)
                        .unwrap_or_else(|| Value::from(""));
                    // `int(datetime.fromisoformat(when.replace("Z", "+00:00")).timestamp())`.
                    let at = match when.as_str() {
                        Some(text) => match iso_timestamp(&text.replace('Z', "+00:00")) {
                            Iso::Seconds(at) => at,
                            Iso::Invalid => now,
                            Iso::Unsure => return Err(Exc::Unsure),
                        },
                        None => now,
                    };
                    let published = (at != now).then(|| Value::from(at));
                    let (msg, at) = (Value::from(msg), Value::from(at));
                    if let Some(mut it) = self.mk(
                        &format!("github/{owner}"),
                        Some(&msg),
                        c.get("html_url"),
                        Some(&at),
                        published.as_ref(),
                    )? {
                        it.insert("kind".into(), kind.into());
                        out.push(Value::Object(it));
                    }
                }
                Ok(())
            })();
            fail(&mut errors, "github", result);
        }
        out
    }

    /// `fetch_devpost(now, errors)`.
    pub fn fetch_devpost(&self, now: i64, errors: Option<&mut Vec<Failure>>) -> Vec<Value> {
        let mut out = Vec::new();
        let mut errors = errors;
        let url = format!("{}/api/hackathons?page=1", self.endpoints.devpost);
        let result: Q<()> = (|| {
            let data = self.get_json(&url, SLOW_TIMEOUT, &[], true)?;
            for h in slice_list(obj(&data)?.get("hackathons"), 20)? {
                let h = obj(&h)?;
                let open = h.get("open_state").unwrap_or(&Value::Null);
                if !(open.is_null() || open == "open" || open == "upcoming") {
                    continue;
                }
                if h.get("invite_only").is_some_and(py::truthy) {
                    continue;
                }
                let loc =
                    str_or_empty(dict_or_empty(h.get("displayed_location"))?.get("location"))?;
                if !loc.to_lowercase().contains("online") {
                    continue;
                }
                let prize_raw = str_or_empty(h.get("prize_amount"))?;
                let prize = strip(&rule(&TAGS)?.replace_all(&prize_raw, "")).to_owned();
                let at = Value::from(now);
                if let Some(mut it) =
                    self.mk("devpost", h.get("title"), h.get("url"), Some(&at), None)?
                {
                    it.insert("kind".into(), "hackathon".into());
                    it.insert(
                        "meta".into(),
                        json!({
                            "prize": prize,
                            "participants": h.get("registrations_count").cloned().unwrap_or(Value::Null),
                            "deadline": h.get("submission_period_dates").cloned().unwrap_or(Value::Null),
                        }),
                    );
                    out.push(Value::Object(it));
                }
            }
            Ok(())
        })();
        fail(&mut errors, "devpost", result);
        out
    }

    /// `fetch_superteam(now, errors)`.
    pub fn fetch_superteam(&self, now: i64, errors: Option<&mut Vec<Failure>>) -> Vec<Value> {
        let mut out = Vec::new();
        let mut errors = errors;
        let url = format!("{}/api/listings", self.endpoints.superteam);
        let result: Q<()> = (|| {
            let data = self.get_json(&url, SLOW_TIMEOUT, &[], true)?;
            let rows = match &data {
                Value::Array(items) => items.iter().take(20).cloned().collect(),
                _ => Vec::new(),
            };
            for it0 in rows {
                let it0 = obj(&it0)?;
                let token = match it0.get("token") {
                    Some(v) if py::truthy(v) => v.clone(),
                    _ => Value::from("USDC"),
                };
                let token = fstr(&token)?;
                let prize = if it0.get("compensationType").and_then(Value::as_str) == Some("range")
                {
                    format!(
                        "{}-{} {token}",
                        fstr(it0.get("minRewardAsk").unwrap_or(&Value::Null))?,
                        fstr(it0.get("maxRewardAsk").unwrap_or(&Value::Null))?
                    )
                } else {
                    let reward = match it0.get("rewardAmount") {
                        Some(v) if py::truthy(v) => fstr(v)?,
                        _ => "?".to_owned(),
                    };
                    format!("{reward} {token}")
                };
                let url = match it0.get("slug") {
                    Some(v) if py::truthy(v) => {
                        Value::from(format!("https://superteam.fun/earn/listing/{}", fstr(v)?))
                    }
                    _ => Value::from(""),
                };
                let at = Value::from(now);
                if let Some(mut it) =
                    self.mk("superteam", it0.get("title"), Some(&url), Some(&at), None)?
                {
                    it.insert("kind".into(), "bounty".into());
                    it.insert(
                        "meta".into(),
                        json!({
                            "prize": prize,
                            "deadline": it0.get("deadline").cloned().unwrap_or(Value::Null),
                        }),
                    );
                    out.push(Value::Object(it));
                }
            }
            Ok(())
        })();
        fail(&mut errors, "superteam", result);
        out
    }

    /// `fetch_dorahacks(now, errors)`.
    pub fn fetch_dorahacks(&self, now: i64, errors: Option<&mut Vec<Failure>>) -> Vec<Value> {
        let mut out = Vec::new();
        let mut errors = errors;
        let url = format!("{}/api/hackathon/?page=1", self.endpoints.dorahacks);
        let result: Q<()> = (|| {
            let data = self.get_json(&url, SLOW_TIMEOUT, &[], true)?;
            let rows = match &data {
                Value::Object(map) => map.get("results").cloned().unwrap_or(Value::Null),
                other => other.clone(),
            };
            for h in slice_list(Some(&rows), 15)? {
                let h = obj(&h)?;
                let end = or_get(h, "end_time", "deadline");
                if py::truthy(&end) && py_int(&end)? < now {
                    continue;
                }
                let uname = match or_get(h, "uname", "slug") {
                    v if py::truthy(&v) => v,
                    _ => Value::from(""),
                };
                let url = if py::truthy(&uname) {
                    Value::from(format!("https://dorahacks.io/hackathon/{}", fstr(&uname)?))
                } else {
                    h.get("url").cloned().unwrap_or(Value::Null)
                };
                let title = or_get(h, "name", "title");
                let at = Value::from(now);
                if let Some(mut it) =
                    self.mk("dorahacks", Some(&title), Some(&url), Some(&at), None)?
                {
                    it.insert("kind".into(), "hackathon".into());
                    it.insert(
                        "meta".into(),
                        json!({"prize": or_get(h, "bounty_prize", "prize_pool"), "deadline": end}),
                    );
                    out.push(Value::Object(it));
                }
            }
            Ok(())
        })();
        fail(&mut errors, "dorahacks", result);
        out
    }

    /// Las siete fuentes en el orden de `FETCHERS`.
    fn all(&self, ts: i64, errors: Option<&mut Vec<Failure>>) -> Vec<Value> {
        let mut errors = errors;
        let mut items = Vec::new();
        items.extend(self.fetch_searxng(ts, errors.as_deref_mut()));
        items.extend(self.fetch_hn(ts, errors.as_deref_mut()));
        items.extend(self.fetch_reddit(ts, errors.as_deref_mut()));
        items.extend(self.fetch_github(ts, errors.as_deref_mut()));
        items.extend(self.fetch_devpost(ts, errors.as_deref_mut()));
        items.extend(self.fetch_superteam(ts, errors.as_deref_mut()));
        items.extend(self.fetch_dorahacks(ts, errors));
        items
    }

    /// `collect(now)`: todos los ítems y un fallo por fuente (el primero).
    pub fn collect(&self, now: i64) -> (Vec<Value>, Vec<Failure>) {
        let mut failures = Vec::new();
        let items = self.all(now, Some(&mut failures));
        let mut seen = HashSet::new();
        failures.retain(|f| seen.insert(f.source.clone()));
        (items, failures)
    }
}

/// `h.get(a) or h.get(b)`.
fn or_get(h: &Map<String, Value>, a: &str, b: &str) -> Value {
    match h.get(a) {
        Some(v) if py::truthy(v) => v.clone(),
        _ => h.get(b).cloned().unwrap_or(Value::Null),
    }
}

/// `except Exception as exc: _fail(errors, source, exc)`.
fn fail(errors: &mut Option<&mut Vec<Failure>>, source: &str, result: Q<()>) {
    let error = match result {
        Ok(()) => return,
        Err(Exc::Raised(msg)) => take(&msg, 200),
        Err(Exc::Unsure) => "respuesta no reproducible por el frente".to_owned(),
    };
    if let Some(errors) = errors.as_deref_mut() {
        errors.push(Failure {
            source: source.to_owned(),
            error,
        });
    }
}

// ---------------------------------------------------------------------------
// `datetime.fromisoformat` de CPython 3.10 (módulo C) y `.timestamp()`
// ---------------------------------------------------------------------------

#[derive(Debug, PartialEq, Eq)]
enum Iso {
    /// `int(dt.timestamp())`.
    Seconds(i64),
    /// `ValueError` (el colector usa `now`).
    Invalid,
    /// Sin zona (hora local de `mktime`) o fuera de lo exacto en `f64`.
    Unsure,
}

/// `parse_digits(p, n)`: `n` dígitos ASCII desde `i`.
fn digits(s: &[u8], i: usize, n: usize) -> Option<(i64, usize)> {
    let mut v = 0i64;
    for k in 0..n {
        let b = *s.get(i + k)?;
        if !b.is_ascii_digit() {
            return None;
        }
        v = v * 10 + i64::from(b - b'0');
    }
    Some((v, i + n))
}

/// `parse_hh_mm_ss_ff(s[from..end])` con `s[end]` como el carácter que el C
/// lee tras el final (el NUL del búfer o el signo de la zona).
fn hh_mm_ss_ff(s: &[u8], from: usize, end: usize) -> Result<(bool, [i64; 4]), ()> {
    let mut vals = [0i64; 4];
    let mut p = from;
    let mut fraction = false;
    for slot in vals.iter_mut().take(3) {
        let (v, next) = digits(s, p, 2).ok_or(())?;
        *slot = v;
        p = next;
        let c = s.get(p).copied().unwrap_or(0);
        p += 1;
        if p >= end {
            return Ok((c != 0, vals));
        }
        match c {
            b':' => continue,
            b'.' => {
                fraction = true;
                break;
            }
            _ => return Err(()),
        }
    }
    let _ = fraction;
    let remains = end.checked_sub(p).ok_or(())?;
    if remains != 6 && remains != 3 {
        return Err(());
    }
    let (mut micro, next) = digits(s, p, remains).ok_or(())?;
    if remains == 3 {
        micro *= 1000;
    }
    vals[3] = micro;
    Ok((s.get(next).copied().unwrap_or(0) != 0, vals))
}

fn days_in_month(year: i64, month: i64) -> i64 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        _ if (year % 4 == 0 && year % 100 != 0) || year % 400 == 0 => 29,
        _ => 28,
    }
}

/// Días desde 1970-01-01 (calendario gregoriano proléptico).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn iso_timestamp(text: &str) -> Iso {
    let raw = text.as_bytes();
    let len = raw.len();
    // El C trabaja sobre un búfer terminado en NUL.
    let mut s = raw.to_vec();
    s.push(0);
    let date = (|| {
        let (year, p) = digits(&s, 0, 4)?;
        (s.get(p) == Some(&b'-')).then_some(())?;
        let (month, p) = digits(&s, p + 1, 2)?;
        (s.get(p) == Some(&b'-')).then_some(())?;
        let (day, _) = digits(&s, p + 1, 2)?;
        Some((year, month, day))
    })();
    let Some((year, month, day)) = date else {
        return Iso::Invalid;
    };
    let (mut time, mut tz) = ([0i64; 4], None);
    if len > 10 {
        let lead = s.get(10).copied().unwrap_or(0);
        let start = 10
            + match lead {
                b if b & 0x80 == 0 => 1,
                b if b & 0xf0 == 0xe0 => 3,
                b if b & 0xf0 == 0xf0 => 4,
                _ => 2,
            };
        if start > len {
            return Iso::Unsure;
        }
        let end = len;
        let tz_pos = (start..end)
            .find(|&i| matches!(s.get(i), Some(b'+' | b'-')))
            .unwrap_or(end);
        let Ok((more, vals)) = hh_mm_ss_ff(&s, start, tz_pos) else {
            return Iso::Invalid;
        };
        time = vals;
        if tz_pos == end {
            if more {
                return Iso::Invalid;
            }
        } else {
            let tzlen = end - tz_pos;
            if !matches!(tzlen, 6 | 9 | 16) {
                return Iso::Invalid;
            }
            let sign = if s.get(tz_pos) == Some(&b'-') { -1 } else { 1 };
            let Ok((more, tzv)) = hh_mm_ss_ff(&s, tz_pos + 1, end) else {
                return Iso::Invalid;
            };
            if more {
                return Iso::Invalid;
            }
            let offset = sign * (tzv[0] * 3600 + tzv[1] * 60 + tzv[2]);
            let micro = sign * tzv[3];
            // `timezone(timedelta(seconds=offset, microseconds=micro))`:
            // estrictamente entre -24 h y 24 h.
            let total = offset * 1_000_000 + micro;
            if total.abs() >= 86_400 * 1_000_000 {
                return Iso::Invalid;
            }
            tz = Some(total);
        }
    }
    let [hour, minute, second, micro] = time;
    if !(1..=9999).contains(&year)
        || !(1..=12).contains(&month)
        || day < 1
        || day > days_in_month(year, month)
        || !(0..24).contains(&hour)
        || !(0..60).contains(&minute)
        || !(0..60).contains(&second)
    {
        return Iso::Invalid;
    }
    let Some(tz) = tz else {
        return Iso::Unsure;
    };
    let secs = days_from_civil(year, month, day) * 86_400 + hour * 3600 + minute * 60 + second;
    let total = i128::from(secs) * 1_000_000 + i128::from(micro) - i128::from(tz);
    // `total_seconds()` = entero / 10**6 redondeado a `float`: exacto si cabe
    // en 2^53.
    if total.unsigned_abs() >= 1u128 << 53 {
        return Iso::Unsure;
    }
    let seconds = (total as f64) / 1e6;
    Iso::Seconds(seconds.trunc() as i64)
}

// ---------------------------------------------------------------------------
// `watch_news`
// ---------------------------------------------------------------------------

/// Un ciclo de `watch_news`.
#[derive(Debug, Clone)]
pub struct NewsWatch {
    /// `news`: lo nuevo (vacío en el primer arranque).
    pub news: Vec<Value>,
    pub snapshot: Value,
}

/// `json.load(open(path))`; `Ok(None)` es el error que el Python captura.
fn load_prev(path: &Path) -> Result<Option<Value>, Fault> {
    let Ok(bytes) = std::fs::read(path) else {
        return Ok(None);
    };
    let Ok(text) = String::from_utf8(bytes) else {
        return Ok(None);
    };
    match python_loads(&text) {
        PythonLoads::Ok => parse_value(&text).map(Some).map_err(|_| Fault::Unsure),
        PythonLoads::Error(_) => Ok(None),
        PythonLoads::Unsure => Err(Fault::Unsure),
    }
}

/// La clave `-x["at"]` de `sorted(recent, …)` como número exacto en `f64`.
fn at_key(item: &Value) -> Result<f64, Fault> {
    let at = item
        .as_object()
        .ok_or(Fault::Raises)?
        .get("at")
        .ok_or(Fault::Raises)?;
    match at {
        Value::Bool(b) => Ok(f64::from(u8::from(*b))),
        Value::Number(n) if py::is_float(n) => {
            let f = py::as_f64(n);
            if f.is_nan() {
                Err(Fault::Unsure)
            } else {
                Ok(f)
            }
        }
        Value::Number(n) => match n.as_i64() {
            Some(i) if i.unsigned_abs() <= 1 << 53 => Ok(i as f64),
            _ => Err(Fault::Unsure),
        },
        _ => Err(Fault::Raises),
    }
}

/// `_history_append(hooks_dir, items, ts)`: historial `news-history.sqlite`
/// (inserción o actualización por URL); `None` ante cualquier error.
fn history_append(hooks: &Path, items: &[Value], ts: i64) -> Option<i64> {
    let con = rusqlite::Connection::open(hooks.join("news-history.sqlite")).ok()?;
    con.busy_timeout(Duration::from_secs(10)).ok()?;
    con.execute(
        "create table if not exists events(
            url text primary key, source text not null, kind text not null,
            title text not null, at integer not null,
            first_seen integer not null, last_seen integer not null,
            meta text not null default '{}')",
        [],
    )
    .ok()?;
    if !items.is_empty() {
        let tx = con.unchecked_transaction().ok()?;
        for it in items {
            let it = it.as_object()?;
            let text = |k: &str| it.get(k).and_then(Value::as_str).map(str::to_owned);
            let meta = match it.get("meta") {
                Some(v) if py::truthy(v) => v.clone(),
                _ => Value::Object(Map::new()),
            };
            let meta = response_dumps_unicode(&meta).ok()?;
            tx.execute(
                "insert into events(url,source,kind,title,at,first_seen,last_seen,meta)
                values(?,?,?,?,?,?,?,?)
                on conflict(url) do update set last_seen=excluded.last_seen,
                  title=excluded.title, meta=excluded.meta",
                rusqlite::params![
                    text("url")?,
                    text("source")?,
                    text("kind")?,
                    text("title")?,
                    it.get("at").and_then(Value::as_i64)?,
                    ts,
                    ts,
                    meta
                ],
            )
            .ok()?;
        }
        tx.commit().ok()?;
    }
    con.query_row("select count(*) from events", [], |row| {
        row.get::<_, i64>(0)
    })
    .ok()
}

/// `watch_news(hooks_dir, now)`: las siete fuentes, deduplicado por URL contra
/// `news-watch.json`, historial y snapshot (`.tmp` + `os.replace`).
pub fn watch_news(sources: &Sources, hooks: &Path, now: i64) -> Result<NewsWatch, Fault> {
    let ts = now;
    let snap_path = hooks.join("news-watch.json");
    let prev = load_prev(&snap_path)?.unwrap_or_else(|| Value::Object(Map::new()));
    let prev = prev.as_object().ok_or(Fault::Raises)?;
    let mut seen: BTreeSet<String> = BTreeSet::new();
    if let Some(v) = prev.get("seen").filter(|v| py::truthy(v)) {
        let items = match v {
            Value::Array(items) => items.clone(),
            Value::String(s) => s.chars().map(|c| Value::from(c.to_string())).collect(),
            Value::Object(map) => map.keys().map(|k| Value::from(k.clone())).collect(),
            _ => return Err(Fault::Raises),
        };
        for item in items {
            match item {
                Value::String(s) => {
                    seen.insert(s);
                }
                Value::Array(_) | Value::Object(_) => return Err(Fault::Raises),
                // `sorted` con tipos mezclados lanzaría o no según el resto.
                _ => return Err(Fault::Unsure),
            }
        }
    }
    let first_run = prev.is_empty();
    let items = sources.all(ts, None);
    let cutoff = ts - MAX_AGE_DAYS * 86_400;
    let mut fresh: Vec<Value> = Vec::new();
    let mut kept: Vec<String> = Vec::new();
    let mut kept_set: HashSet<String> = HashSet::new();
    for it in &items {
        let url = it.get("url").and_then(Value::as_str).unwrap_or_default();
        let at = it.get("at").and_then(Value::as_i64).unwrap_or_default();
        if kept_set.contains(url) || at < cutoff {
            continue;
        }
        kept_set.insert(url.to_owned());
        kept.push(url.to_owned());
        if !seen.contains(url) {
            fresh.push(it.clone());
        }
    }
    seen.extend(kept);
    let mut recent: Vec<Value> = if first_run {
        fresh
            .iter()
            .filter(|it| it.get("at").and_then(Value::as_i64).unwrap_or_default() >= ts - 48 * 3600)
            .take(10)
            .cloned()
            .collect()
    } else {
        let mut prior = match prev.get("recent") {
            Some(Value::Array(items)) => items.clone(),
            Some(v) if py::truthy(v) => return Err(Fault::Raises),
            _ => Vec::new(),
        };
        prior.extend(fresh.iter().cloned());
        prior
    };
    let keys: Vec<f64> = recent.iter().map(at_key).collect::<Result<_, _>>()?;
    let mut order: Vec<usize> = (0..recent.len()).collect();
    order.sort_by(|&a, &b| {
        keys.get(b)
            .partial_cmp(&keys.get(a))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    recent = order
        .into_iter()
        .take(40)
        .filter_map(|i| recent.get(i).cloned())
        .collect();
    let history_count = history_append(hooks, &items, ts);
    let seen: Vec<String> = seen.into_iter().collect();
    let tail = seen.len().saturating_sub(800);
    let snap = json!({
        "checkedAt": ts,
        "recent": recent,
        "seen": seen.get(tail..).unwrap_or_default(),
        "historyCount": history_count,
        "sources": {"searxng": sources.endpoints.searx, "note": "patrón Radar de LifeOS"},
    });
    write_tmp_replace(&snap_path, &snap)?;
    Ok(NewsWatch {
        news: if first_run { Vec::new() } else { fresh },
        snapshot: snap,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn isoformat_like_cpython_310() {
        assert_eq!(
            iso_timestamp("2026-10-05T12:30:45+00:00"),
            Iso::Seconds(1_791_203_445)
        );
        assert_eq!(iso_timestamp("2026-10-05T12:30:45.5+00:00"), Iso::Invalid);
        assert_eq!(
            iso_timestamp("2026-10-05T12:30:45.500+00:00"),
            Iso::Seconds(1_791_203_445)
        );
        assert_eq!(
            iso_timestamp("2026-10-05 12:30+02:00"),
            Iso::Seconds(1_791_196_200)
        );
        assert_eq!(
            iso_timestamp("2026-10-05T12.500+00:00"),
            Iso::Seconds(1_791_201_600)
        );
        assert_eq!(
            iso_timestamp("2026-10-05T10:00+23:59"),
            Iso::Seconds(1_791_108_060)
        );
        assert_eq!(
            iso_timestamp("2026-10-05x10:00+00:99"),
            Iso::Seconds(1_791_188_460)
        );
        assert_eq!(
            iso_timestamp("2026-10-05T10:00:00+00:00:30"),
            Iso::Seconds(1_791_194_370)
        );
        assert_eq!(
            iso_timestamp("2026-10-05T10:00:00+00:00:30.000001"),
            Iso::Seconds(1_791_194_369)
        );
        assert_eq!(iso_timestamp("2026-10-05T1"), Iso::Invalid);
        assert_eq!(
            iso_timestamp("2026-10-05T10:00:00.123456+05:30"),
            Iso::Seconds(1_791_174_600)
        );
        assert_eq!(iso_timestamp("2026-1-05"), Iso::Invalid);
        assert_eq!(
            iso_timestamp("2026-10-05\u{e9}10:00+00:00"),
            Iso::Seconds(1_791_194_400)
        );
        assert_eq!(iso_timestamp("2026-10-05T12:30:45"), Iso::Unsure);
        assert_eq!(iso_timestamp("2026-10-05"), Iso::Unsure);
        assert_eq!(iso_timestamp("2026-13-05T00:00+00:00"), Iso::Invalid);
        assert_eq!(iso_timestamp("2026-02-29T00:00+00:00"), Iso::Invalid);
        assert_eq!(
            iso_timestamp("1969-12-31T23:59:59.500000+00:00"),
            Iso::Seconds(0)
        );
        assert_eq!(iso_timestamp("garbage"), Iso::Invalid);
        assert_eq!(iso_timestamp("2026-10-05T25:00+00:00"), Iso::Invalid);
        assert_eq!(iso_timestamp("2026-10-05T10:00+24:00"), Iso::Invalid);
    }

    #[test]
    fn quote_plus_like_urllib() {
        assert_eq!(
            urlencode(&[("q", "\"claude code\" skill"), ("pageno", "1")]),
            "q=%22claude+code%22+skill&pageno=1"
        );
    }

    #[test]
    fn urljoin_cases() {
        let base = "https://a.example/x/y?z=1";
        assert_eq!(
            urljoin(base, "https://b.example/q").unwrap(),
            "https://b.example/q"
        );
        assert_eq!(
            urljoin(base, "//c.example/r").unwrap(),
            "https://c.example/r"
        );
        assert_eq!(
            urljoin(base, "/root?p=2").unwrap(),
            "https://a.example/root?p=2"
        );
        assert_eq!(urljoin(base, "w").unwrap(), "https://a.example/x/w");
        assert!(urljoin(base, "../w").is_err());
    }
}
