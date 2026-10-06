//! Vigilante de modelos: port de `lib/model_watch.py` (`installed_versions`,
//! `discover_models`, `discover_addons`, `registry_model_ids` y
//! `watch_models`; `latest_models` vive en `cli_catalog`).
//!
//! Nada aquí habla con el frente ni con la red: los programas los ejecuta el
//! `Host` de quien llama (el frente, con su entorno y sus plazos; las pruebas,
//! los CLIs falsos de su `fakebin`). Todo es síncrono: quien llama lo corre en
//! el pool de bloqueo.
//!
//! Errores: `Fault::Raises` es una excepción que `watch_models` del Python no
//! captura (el ciclo de `cc-dash` la traga y no escribe nada); `Fault::Unsure`
//! es algo que el port no reproduce con certeza (texto que las expresiones de
//! Rust y de `re` clasificarían distinto, enteros enormes, ids no ASCII): quien
//! llama lo trata igual que la excepción (esa vuelta no escribe nada).
//!
//! Los binarios versionados de los CLIs (cientos de MB) se recorren por trozos
//! de 1 MiB con un solape del largo máximo de coincidencia: las coincidencias
//! son las mismas que las de `re.finditer` sobre el archivo entero, sin
//! cargarlo en memoria.
use crate::{Unsure, agent_procs::realpath, cli_catalog::version_of, cli_help::regex_safe};
use comandos_core::{
    json::{PythonLoads, indent_dumps, parse_value, python_loads, workspace_loads_bytes},
    text::splitlines,
};
use regex::{Regex, bytes::Regex as BytesRegex};
use serde_json::{Map, Value, json};
use std::{
    cmp::Ordering,
    collections::{BTreeMap, BTreeSet},
    ffi::{OsStr, OsString},
    fs,
    io::{self, Read, Write},
    os::unix::ffi::{OsStrExt, OsStringExt},
    path::{Path, PathBuf},
    sync::LazyLock,
    time::Duration,
};

/// `_VERSION_CLIS`.
pub const VERSION_CLIS: [&str; 5] = ["claude", "codex", "grok", "opencode", "agy"];
/// `run([exe, "--version"], timeout=15)`.
pub const VERSION_TIMEOUT: Duration = Duration::from_secs(15);
/// `_run([grok_exe, "models"], timeout=25, env=env)`.
pub const GROK_TIMEOUT: Duration = Duration::from_secs(25);
/// `_codex_catalog`: tope de `models_cache.json`.
const MAX_CATALOG_BYTES: u64 = 4 * 1024 * 1024;
/// Trozo de lectura de los binarios y solape (≥ el largo máximo de una
/// coincidencia de los patrones de bytes: 38 para Claude, 29 para Codex).
const CHUNK: usize = 1024 * 1024;
const OVERLAP: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fault {
    /// Excepción sin capturar de `watch_models`.
    Raises,
    /// No se reproduce con certeza.
    Unsure,
}

impl From<Unsure> for Fault {
    fn from(_: Unsure) -> Self {
        Fault::Unsure
    }
}

pub type Result<T, E = Fault> = std::result::Result<T, E>;

/// Lo que `model_watch` toma del proceso: `providers.which` y `_run`.
pub trait Host {
    /// `providers.which(name)`.
    fn which(&self, name: &str) -> Option<PathBuf>;
    /// `subprocess.run(cmd, capture_output=True, text=True, timeout,
    /// stdin=DEVNULL)` con `env` añadido al entorno del proceso: la salida
    /// estándar seguida de la de errores, con saltos universales. `None` es
    /// cualquier excepción (no arranca, vence el plazo, no es UTF-8).
    fn run(
        &self,
        exe: &Path,
        args: &[&str],
        timeout: Duration,
        env: &[(&str, &OsStr)],
    ) -> Option<String>;
}

/// Rutas que el Python saca del proceso.
pub struct Paths {
    /// `os.path.expanduser("~")`.
    pub home: PathBuf,
    /// `os.environ.get("CODEX_HOME")` (vacía = ausente).
    pub codex_home: Option<PathBuf>,
    /// Directorio de trabajo (rutas relativas de `CODEX_HOME`).
    pub cwd: PathBuf,
}

impl Paths {
    fn codex_cache(&self) -> PathBuf {
        let base = self
            .codex_home
            .clone()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| self.home.join(".codex"));
        let path = base.join("models_cache.json");
        if path.is_absolute() {
            path
        } else {
            self.cwd.join(path)
        }
    }
}

fn compile(pattern: &str) -> Option<Regex> {
    Regex::new(pattern).ok()
}

fn compile_bytes(pattern: &str) -> Option<BytesRegex> {
    BytesRegex::new(pattern).ok()
}

fn rule<T>(cell: &'static LazyLock<Option<T>>) -> Result<&'static T, Unsure> {
    cell.as_ref().ok_or(Unsure)
}

static ANSI: LazyLock<Option<Regex>> = LazyLock::new(|| compile(r"\x1b\[[0-9;]*[A-Za-z]"));
static CLAUDE_RE: LazyLock<Option<Regex>> = LazyLock::new(|| {
    compile(r"^claude-(?:fable|opus|sonnet|haiku|mythos)-\d(?:-\d)?(?:-\d{8})?(?:\[1m\])?$")
});
static CODEX_RE: LazyLock<Option<Regex>> =
    LazyLock::new(|| compile(r"^gpt-\d+(?:\.\d+)?(?:-codex)?(?:-[a-z]{2,12})?$"));
static GROK_RE: LazyLock<Option<Regex>> = LazyLock::new(|| compile(r"^grok-[\w.-]+$"));
static GROK_LINE: LazyLock<Option<Regex>> = LazyLock::new(|| compile(r"^\s*[-*]\s+([\w.-]+)"));
static CLAUDE_BYTES: LazyLock<Option<BytesRegex>> = LazyLock::new(|| {
    compile_bytes(r"(?-u)claude-(?:fable|opus|sonnet|haiku|mythos)-[\w.\[\]\-]{1,24}")
});
static CODEX_BYTES: LazyLock<Option<BytesRegex>> =
    LazyLock::new(|| compile_bytes(r"(?-u)gpt-\d[\w.\-]{1,24}"));
static MCP_SECTION: LazyLock<Option<Regex>> =
    LazyLock::new(|| compile(r"(?m)^\[mcp_servers\.([\w-]+)\]"));
static DATE_TAIL: LazyLock<Option<Regex>> = LazyLock::new(|| compile(r"-\d{8}$"));
static BRACKET_TAIL: LazyLock<Option<Regex>> = LazyLock::new(|| compile(r"\[.*$"));
static CLAUDE_FAMILY: LazyLock<Option<Regex>> =
    LazyLock::new(|| compile(r"^claude-([a-z]+)-(\d+(?:-\d+)*)"));
static GPT_FAMILY: LazyLock<Option<Regex>> = LazyLock::new(|| compile(r"^(gpt)-(\d+(?:\.\d+)?)"));
static GROK_FAMILY: LazyLock<Option<Regex>> = LazyLock::new(|| compile(r"^(grok)-(\d+(?:\.\d+)?)"));
static VARIANT_HEAD: LazyLock<Option<Regex>> =
    LazyLock::new(|| compile(r"^(?:gpt|grok)-\d+(?:\.\d+)?"));

// ---------------------------------------------------------------------------
// Procesos
// ---------------------------------------------------------------------------

/// `_ANSI.sub("", stdout + stderr)`; cualquier excepción es `""`.
fn run_text(
    host: &dyn Host,
    exe: &Path,
    args: &[&str],
    timeout: Duration,
    env: &[(&str, &OsStr)],
) -> Result<String, Unsure> {
    let Some(text) = host.run(exe, args, timeout, env) else {
        return Ok(String::new());
    };
    Ok(rule(&ANSI)?.replace_all(&text, "").into_owned())
}

/// `installed_versions()`: la versión de cada uno de los cinco CLIs que
/// `which` encuentra (`"?"` si su salida no la tiene), en el orden de
/// `_VERSION_CLIS`.
pub fn installed_versions(host: &dyn Host) -> Result<Map<String, Value>, Unsure> {
    let mut out = Map::new();
    for prov in VERSION_CLIS {
        let Some(exe) = host.which(prov) else {
            continue;
        };
        let text = run_text(host, &exe, &["--version"], VERSION_TIMEOUT, &[])?;
        out.insert(prov.into(), Value::from(version_of(&text)?));
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// Binarios
// ---------------------------------------------------------------------------

/// `_binary_ids(path, pattern, canon)`: candidatos de `pattern` en los bytes
/// del archivo que casan enteros con `canon`. Un error de E/S es el conjunto
/// vacío (el Python lee el archivo entero de una vez: `OSError` → `set()`).
fn binary_ids(path: &Path, pattern: &BytesRegex, canon: &Regex) -> BTreeSet<String> {
    let mut ids = BTreeSet::new();
    let Ok(mut file) = fs::File::open(path) else {
        return ids;
    };
    let mut buf: Vec<u8> = Vec::with_capacity(CHUNK + OVERLAP);
    let mut chunk = vec![0u8; CHUNK];
    loop {
        let n = match file.read(&mut chunk) {
            Ok(n) => n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(_) => return BTreeSet::new(),
        };
        let eof = n == 0;
        buf.extend_from_slice(chunk.get(..n).unwrap_or_default());
        // Un comienzo antes de `limit` tiene su coincidencia entera en `buf`.
        let limit = if eof {
            buf.len()
        } else {
            buf.len().saturating_sub(OVERLAP)
        };
        let mut resume = 0;
        for m in pattern.find_iter(&buf) {
            if !eof && m.start() >= limit {
                break;
            }
            // `m.group(0).decode(errors="replace")`: los patrones solo casan ASCII.
            if let Ok(cand) = std::str::from_utf8(m.as_bytes())
                && canon.is_match(cand)
            {
                ids.insert(cand.to_owned());
            }
            resume = m.end();
        }
        if eof {
            return ids;
        }
        buf.drain(..resume.max(limit));
    }
}

/// Dígitos ASCII de un nombre como entero sin ceros a la izquierda.
fn digit_key(text: &str) -> &str {
    let t = text.trim_start_matches('0');
    if t.is_empty() { "0" } else { t }
}

/// Comparación de enteros escritos en decimal (sin límite de tamaño).
fn cmp_digits(a: &str, b: &str) -> Ordering {
    let (a, b) = (digit_key(a), digit_key(b));
    a.len().cmp(&b.len()).then_with(|| a.cmp(b))
}

/// `[int(x) for x in re.findall(r"\d+", v)]` comparadas como listas de Python.
fn cmp_runs(a: &[String], b: &[String]) -> Ordering {
    for (x, y) in a.iter().zip(b) {
        let o = cmp_digits(x, y);
        if o != Ordering::Equal {
            return o;
        }
    }
    a.len().cmp(&b.len())
}

/// Las tiradas de dígitos de un nombre; un dígito no ASCII (lo que `\d` de
/// `re` también acepta) no se reproduce.
fn digit_runs(name: &str) -> Result<Vec<String>, Unsure> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for c in name.chars() {
        if c.is_ascii_digit() {
            cur.push(c);
            continue;
        }
        if !c.is_ascii() && c.is_numeric() {
            return Err(Unsure);
        }
        if !cur.is_empty() {
            out.push(std::mem::take(&mut cur));
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    Ok(out)
}

fn py_realpath(path: &Path) -> PathBuf {
    PathBuf::from(OsString::from_vec(realpath(path.as_os_str().as_bytes())))
}

/// `os.path.dirname` sobre bytes.
fn py_dirname(path: &Path) -> PathBuf {
    let bytes = path.as_os_str().as_bytes();
    let cut = bytes.iter().rposition(|b| *b == b'/').map_or(0, |i| i + 1);
    let mut head = bytes.get(..cut).unwrap_or_default();
    if !head.is_empty() && head.iter().any(|b| *b != b'/') {
        while head.len() > 1 && head.ends_with(b"/") {
            head = head.get(..head.len() - 1).unwrap_or_default();
        }
    }
    PathBuf::from(OsStr::from_bytes(head))
}

/// `_claude_binary()`: la versión más alta de `~/.local/share/claude/versions`
/// (orden por las tiradas de dígitos, estable), o el `realpath` de `claude`.
fn claude_binary(host: &dyn Host, home: &Path) -> Result<Option<PathBuf>, Unsure> {
    let base = home.join(".local/share/claude/versions");
    let entries = match fs::read_dir(&base) {
        Ok(entries) => entries,
        Err(_) => return Ok(host.which("claude").map(|p| py_realpath(&p))),
    };
    let mut names: Vec<(Vec<String>, OsString)> = Vec::new();
    for entry in entries {
        let Ok(entry) = entry else {
            return Err(Unsure);
        };
        let name = entry.file_name();
        let text = name.to_str().ok_or(Unsure)?;
        names.push((digit_runs(text)?, name.clone()));
    }
    names.sort_by(|a, b| cmp_runs(&a.0, &b.0));
    Ok(names.pop().map(|(_, name)| base.join(name)))
}

/// `_codex_binary()`: el binario de `codex-linux-x64/vendor` junto al paquete
/// de npm, o el `realpath` de `codex`.
fn codex_binary(host: &dyn Host) -> Option<PathBuf> {
    let real = host.which("codex")?;
    let resolved = py_realpath(&real);
    let root = py_dirname(&py_dirname(&resolved));
    let cand = py_dirname(&root).join("codex-linux-x64/vendor/x86_64-unknown-linux-musl/bin/codex");
    if cand.is_file() {
        Some(cand)
    } else {
        Some(resolved)
    }
}

/// `_codex_catalog()`: slugs visibles de `models_cache.json`; `None` sin
/// catálogo utilizable.
fn codex_catalog(paths: &Paths) -> Result<Option<Vec<String>>, Unsure> {
    let path = paths.codex_cache();
    let mut raw = Vec::new();
    let read =
        fs::File::open(&path).and_then(|f| f.take(MAX_CATALOG_BYTES + 1).read_to_end(&mut raw));
    if read.is_err() || raw.len() as u64 > MAX_CATALOG_BYTES {
        return Ok(None);
    }
    let Some(Value::Object(data)) = workspace_loads_bytes(&raw) else {
        return Ok(None);
    };
    let Some(Value::Array(rows)) = data.get("models") else {
        return Ok(None);
    };
    let canon = rule(&CODEX_RE)?;
    let mut out = BTreeSet::new();
    for row in rows {
        let Some(row) = row.as_object() else {
            continue;
        };
        if row.get("visibility").and_then(Value::as_str) != Some("list") {
            continue;
        }
        let Some(slug) = row.get("slug").and_then(Value::as_str) else {
            continue;
        };
        if canon.is_match(slug) {
            // `\d` de Rust admite dígitos de versiones de Unicode más nuevas.
            if !slug.is_ascii() {
                return Err(Unsure);
            }
            out.insert(slug.to_owned());
        }
    }
    Ok(Some(out.into_iter().collect()))
}

/// `discover_models(grok_home)`: ids de cada CLI, ordenados.
pub fn discover_models(
    host: &dyn Host,
    paths: &Paths,
    grok_home: Option<&Path>,
) -> Result<Vec<(&'static str, Vec<String>)>, Unsure> {
    let mut claude = BTreeSet::new();
    if let Some(cb) = claude_binary(host, &paths.home)? {
        claude = binary_ids(&cb, rule(&CLAUDE_BYTES)?, rule(&CLAUDE_RE)?);
    }
    let codex = match codex_catalog(paths)? {
        Some(catalog) => catalog.into_iter().collect(),
        None => match codex_binary(host) {
            Some(xb) => binary_ids(&xb, rule(&CODEX_BYTES)?, rule(&CODEX_RE)?),
            None => BTreeSet::new(),
        },
    };
    let mut grok = BTreeSet::new();
    if let Some(exe) = host.which("grok") {
        let env: Vec<(&str, &OsStr)> = grok_home
            .map(|h| vec![("GROK_HOME", h.as_os_str())])
            .unwrap_or_default();
        let out = run_text(host, &exe, &["models"], GROK_TIMEOUT, &env)?;
        let (line_rule, canon) = (rule(&GROK_LINE)?, rule(&GROK_RE)?);
        for line in splitlines(&out) {
            if !regex_safe(line) {
                return Err(Unsure);
            }
            if let Some(id) = line_rule.captures(line).and_then(|c| c.get(1))
                && canon.is_match(id.as_str())
            {
                grok.insert(id.as_str().to_owned());
            }
        }
    }
    Ok(vec![
        ("claude", claude.into_iter().collect()),
        ("codex", codex.into_iter().collect()),
        ("grok", grok.into_iter().collect()),
    ])
}

// ---------------------------------------------------------------------------
// Skills y MCPs
// ---------------------------------------------------------------------------

/// `sorted(os.listdir(dir))` filtrado; `OSError` → `None`.
fn listdir(dir: &Path) -> Result<Option<Vec<String>>, Unsure> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Ok(None);
    };
    let mut names = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|_| Unsure)?;
        names.push(entry.file_name().into_string().map_err(|_| Unsure)?);
    }
    names.sort();
    Ok(Some(names))
}

/// `open(path).read()` en modo texto (UTF-8, saltos universales); cualquier
/// error es la excepción que el llamador captura (`None`).
fn read_text(path: &Path) -> Option<String> {
    let bytes = fs::read(path).ok()?;
    let text = String::from_utf8(bytes).ok()?;
    Some(text.replace("\r\n", "\n").replace('\r', "\n"))
}

/// `json.load(open(path))`: `Ok(None)` es la excepción de lectura o de
/// decodificación (los llamadores la capturan); `Unsure` si no se sabe.
fn load_json(path: &Path) -> Result<Option<Value>, Unsure> {
    let Some(text) = read_text(path) else {
        return Ok(None);
    };
    match python_loads(&text) {
        PythonLoads::Ok => parse_value(&text).map(Some).map_err(|_| Unsure),
        PythonLoads::Error(_) => Ok(None),
        PythonLoads::Unsure => Err(Unsure),
    }
}

/// `sorted(set(re.findall(r"^\[mcp_servers\.([\w-]+)\]", toml, re.M)))`.
fn toml_mcps(path: &Path) -> Result<Vec<String>, Unsure> {
    let Some(text) = read_text(path) else {
        return Ok(Vec::new());
    };
    // `^` multilínea y `[\w-]` no cruzan saltos: solo importan las líneas de
    // sección (el resto del archivo puede tener cualquier texto).
    if text
        .split('\n')
        .any(|line| line.starts_with("[mcp_servers.") && !regex_safe(line))
    {
        return Err(Unsure);
    }
    let names: BTreeSet<String> = rule(&MCP_SECTION)?
        .captures_iter(&text)
        .filter_map(|c| c.get(1).map(|m| m.as_str().to_owned()))
        .collect();
    Ok(names.into_iter().collect())
}

/// `discover_addons()`: nombres de skills y MCPs por cuenta (nunca comandos,
/// entornos ni URLs).
pub fn discover_addons(home: &Path) -> Result<Value, Unsure> {
    let mut skills = Map::new();
    for (label, base) in [
        ("claude", ".claude/skills"),
        ("claude:relotto", ".claude-accounts/relotto/skills"),
        ("grok", ".grok/skills"),
    ] {
        let names: Vec<String> = listdir(&home.join(base))?
            .unwrap_or_default()
            .into_iter()
            .filter(|d| !d.starts_with('.'))
            .collect();
        if !names.is_empty() {
            skills.insert(label.into(), json!(names));
        }
    }
    if let Some(files) = listdir(&home.join(".codex/prompts"))? {
        let mut names: Vec<String> = files
            .into_iter()
            .filter_map(|f| f.strip_suffix(".md").map(str::to_owned))
            .collect();
        names.sort();
        if !names.is_empty() {
            skills.insert("codex".into(), json!(names));
        }
    }
    let mut mcps = Map::new();
    if let Some(Value::Object(cfg)) = load_json(&home.join(".claude.json"))? {
        // `(cfg.get("mcpServers") or {}).keys()`: solo un objeto tiene claves.
        if let Some(Value::Object(servers)) = cfg.get("mcpServers") {
            let mut names: Vec<String> = servers.keys().cloned().collect();
            names.sort();
            if !names.is_empty() {
                mcps.insert("claude".into(), json!(names));
            }
        }
    }
    for (label, rel) in [
        ("codex", ".codex/config.toml"),
        ("grok", ".grok/config.toml"),
    ] {
        let names = toml_mcps(&home.join(rel))?;
        if !names.is_empty() {
            mcps.insert(label.into(), json!(names));
        }
    }
    Ok(json!({"skills": skills, "mcps": mcps}))
}

// ---------------------------------------------------------------------------
// Registro y novedades
// ---------------------------------------------------------------------------

/// `set(x)` de Python sobre un valor de JSON: los elementos de una lista, los
/// caracteres de un texto o las claves de un objeto; un escalar no iterable o
/// un elemento que no se puede «hashear» lanza.
fn py_set(value: &Value) -> Result<Vec<Value>> {
    let items = match value {
        Value::Array(items) => items.clone(),
        Value::String(s) => s.chars().map(|c| Value::from(c.to_string())).collect(),
        Value::Object(map) => map.keys().map(|k| Value::from(k.clone())).collect(),
        _ => return Err(Fault::Raises),
    };
    if items
        .iter()
        .any(|v| matches!(v, Value::Array(_) | Value::Object(_)))
    {
        return Err(Fault::Raises);
    }
    Ok(items)
}

/// Los textos de `set(x or [])` (con un `str` solo casa un `str`).
fn str_set(value: Option<&Value>) -> Result<BTreeSet<String>> {
    let Some(value) = value.filter(|v| crate::hooks::py::truthy(v)) else {
        return Ok(BTreeSet::new());
    };
    Ok(py_set(value)?
        .into_iter()
        .filter_map(|v| v.as_str().map(str::to_owned))
        .collect())
}

/// `x or {}` de un valor que después recibe `.get`/`.items()`: un valor
/// verdadero que no es objeto lanza.
fn dict_or_empty(value: Option<&Value>) -> Result<Map<String, Value>> {
    match value {
        Some(Value::Object(map)) => Ok(map.clone()),
        Some(v) if crate::hooks::py::truthy(v) => Err(Fault::Raises),
        _ => Ok(Map::new()),
    }
}

/// `registry_model_ids(registry_path)`: ids de modelo por motor (con los
/// `soon`). Un archivo ausente o roto es `{}`; un registro con otra forma
/// lanza, como el Python.
pub fn registry_model_ids(path: &Path) -> Result<Vec<(String, Vec<Value>)>> {
    let Some(reg) = load_json(path)? else {
        return Ok(Vec::new());
    };
    let reg = reg.as_object().ok_or(Fault::Raises)?;
    let mut out = Vec::new();
    for (motor, spec) in dict_or_empty(reg.get("motors"))? {
        let spec = spec.as_object().ok_or(Fault::Raises)?;
        let models = match spec.get("models") {
            Some(v) if crate::hooks::py::truthy(v) => py_iter(v)?,
            _ => Vec::new(),
        };
        let mut ids: Vec<Value> = Vec::new();
        for model in models {
            let model = model.as_object().ok_or(Fault::Raises)?;
            let id = model.get("id").cloned().unwrap_or_else(|| Value::from(""));
            if matches!(id, Value::Array(_) | Value::Object(_)) {
                return Err(Fault::Raises);
            }
            if !ids.iter().any(|v| crate::hooks::py::eq(v, &id)) {
                ids.push(id);
            }
        }
        out.push((motor, ids));
    }
    Ok(out)
}

/// `for x in value` de Python sobre un valor verdadero de JSON.
fn py_iter(value: &Value) -> Result<Vec<Value>> {
    match value {
        Value::Array(items) => Ok(items.clone()),
        Value::String(s) => Ok(s.chars().map(|c| Value::from(c.to_string())).collect()),
        Value::Object(map) => Ok(map.keys().map(|k| Value::from(k.clone())).collect()),
        _ => Err(Fault::Raises),
    }
}

/// Un id de modelo como texto: lo que no es `str` lanza en `.lower()`; uno no
/// ASCII (minúsculas y `\d` de Unicode) o con saltos (`$` de `re`) no se
/// reproduce.
fn model_text(id: &Value) -> Result<&str> {
    ascii_id(id.as_str().ok_or(Fault::Raises)?)
}

fn ascii_id(id: &str) -> Result<&str> {
    if !id.is_ascii() || id.contains(['\n', '\r']) {
        return Err(Fault::Unsure);
    }
    Ok(id)
}

/// `_norm(mid)`.
fn norm(mid: &str) -> Result<String, Unsure> {
    let lower = mid.to_ascii_lowercase();
    let dated = rule(&DATE_TAIL)?.replace(&lower, "").into_owned();
    Ok(rule(&BRACKET_TAIL)?.replace(&dated, "").into_owned())
}

type Version = Vec<u128>;

fn parse_version(digits: &str, sep: char) -> Result<Version, Unsure> {
    digits
        .split(sep)
        .map(|d| d.parse::<u128>().map_err(|_| Unsure))
        .collect()
}

/// `_family_ver(mid)`.
fn family_ver(mid: &str) -> Result<(String, Version), Unsure> {
    let n = norm(mid)?;
    if let Some(c) = rule(&CLAUDE_FAMILY)?.captures(&n) {
        let fam = c.get(1).map_or("", |m| m.as_str()).to_owned();
        return Ok((
            fam,
            parse_version(c.get(2).map_or("", |m| m.as_str()), '-')?,
        ));
    }
    for family in [&GPT_FAMILY, &GROK_FAMILY] {
        if let Some(c) = rule(family)?.captures(&n) {
            let fam = c.get(1).map_or("", |m| m.as_str()).to_owned();
            return Ok((
                fam,
                parse_version(c.get(2).map_or("", |m| m.as_str()), '.')?,
            ));
        }
    }
    Ok((n, Vec::new()))
}

/// Un ciclo de `watch_models`.
#[derive(Debug, Clone)]
pub struct Watch {
    /// `news`: lo nuevo por CLI desde el snapshot anterior, en su orden.
    pub news: Vec<(String, Vec<String>)>,
    pub snapshot: Value,
}

/// Dónde vive cada cosa de `watch_models(hooks_dir, registry_path, grok_home)`.
pub struct WatchPaths<'a> {
    pub paths: &'a Paths,
    pub hooks: &'a Path,
    pub registry: &'a Path,
    pub grok_home: Option<&'a Path>,
}

/// `watch_models(...)`: descubre, compara contra el registro y el snapshot
/// previo, escribe `model-watch.json` (`.tmp` + `os.replace`, `indent=1`) y
/// devuelve las novedades. `now` es `int(time.time())`.
pub fn watch_models(host: &dyn Host, at: &WatchPaths, now: i64) -> Result<Watch> {
    let ts = now;
    let snap_path = at.hooks.join("model-watch.json");
    let prev = match load_json(&snap_path)? {
        Some(v) => v,
        None => Value::Object(Map::new()),
    };
    let versions = installed_versions(host)?;
    let discovered = discover_models(host, at.paths, at.grok_home)?;
    let registry = registry_model_ids(at.registry)?;
    let mut reg_norm: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for (prov, ids) in &registry {
        let mut set = BTreeSet::new();
        for id in ids {
            set.insert(norm(model_text(id)?)?);
        }
        reg_norm.insert(prov.clone(), set);
    }
    let prev = prev.as_object().ok_or(Fault::Raises)?;
    let mut prev_seen: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for (prov, v) in dict_or_empty(prev.get("newSince"))? {
        let v = v.as_object().ok_or(Fault::Raises)?;
        prev_seen.insert(prov, str_set(v.get("models"))?);
    }
    // Techo por familia según el registro.
    let mut reg_ceiling: BTreeMap<(String, String), Version> = BTreeMap::new();
    for (prov, ids) in &registry {
        for id in ids {
            let (fam, ver) = family_ver(model_text(id)?)?;
            let key = (prov.clone(), fam);
            if reg_ceiling.get(&key).is_none_or(|cur| ver > *cur) && !ver.is_empty() {
                reg_ceiling.insert(key, ver);
            }
        }
    }
    let mut news: Vec<(String, Vec<String>)> = Vec::new();
    let mut pending: Vec<(String, Vec<String>)> = Vec::new();
    for (prov, ids) in &discovered {
        // Por clave, `(versión, id)` del primero con la versión más alta.
        let mut best: Vec<((String, String), (Version, String))> = Vec::new();
        for m in ids {
            let n = norm(ascii_id(m)?)?;
            if reg_norm.get(*prov).is_some_and(|set| set.contains(&n)) {
                continue;
            }
            let (fam, ver) = family_ver(ascii_id(m)?)?;
            if let Some(ceil) = reg_ceiling.get(&((*prov).to_owned(), fam.clone()))
                && (ver < *ceil || (ver == *ceil && fam != "gpt" && fam != "grok"))
            {
                continue;
            }
            let variant = rule(&VARIANT_HEAD)?.replace(&n, "").into_owned();
            let key = if fam == "gpt" || fam == "grok" {
                (fam, variant)
            } else {
                (fam, String::new())
            };
            match best.iter_mut().find(|(k, _)| *k == key) {
                Some((_, cur)) => {
                    if ver > cur.0 {
                        *cur = (ver, m.clone());
                    }
                }
                None => best.push((key, (ver, m.clone()))),
            }
        }
        let mut pend: Vec<String> = best.into_iter().map(|(_, (_, m))| m).collect();
        pend.sort();
        let seen = prev_seen.get(*prov);
        let fresh: Vec<String> = pend
            .iter()
            .filter(|m| !seen.is_some_and(|s| s.contains(*m)))
            .cloned()
            .collect();
        if !pend.is_empty() {
            pending.push(((*prov).to_owned(), pend));
        }
        if !fresh.is_empty() {
            news.push(((*prov).to_owned(), fresh));
        }
    }
    let addons = discover_addons(&at.paths.home)?;
    let prev_addons_raw = prev.get("addons").cloned().unwrap_or(Value::Null);
    let prev_addons_true = crate::hooks::py::truthy(&prev_addons_raw);
    let mut addon_news = Map::new();
    for kind in ["skills", "mcps"] {
        let mut added = Map::new();
        let owners = addons
            .get(kind)
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        for (owner, names) in owners {
            // `prev_addons.get(kind)`: solo se evalúa si hay dueños.
            let prev_kind = if prev_addons_true {
                let map = prev_addons_raw.as_object().ok_or(Fault::Raises)?;
                dict_or_empty(map.get(kind))?
            } else {
                Map::new()
            };
            let before = str_set(prev_kind.get(&owner))?;
            let plus: Vec<Value> = names
                .as_array()
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .filter(|n| n.as_str().is_some_and(|s| !before.contains(s)))
                .collect();
            if !plus.is_empty() && prev_addons_true {
                added.insert(owner, Value::Array(plus));
            }
        }
        if !added.is_empty() {
            addon_news.insert(kind.into(), json!({"added": added, "at": ts}));
        }
    }
    let addon_news = if !addon_news.is_empty() {
        Value::Object(addon_news)
    } else if !prev_addons_true {
        prev.get("addonNews").cloned().unwrap_or(Value::Null)
    } else {
        Value::Object(Map::new())
    };
    let mut discovered_map = Map::new();
    for (prov, ids) in discovered {
        discovered_map.insert(prov.into(), json!(ids));
    }
    let mut new_since = Map::new();
    for (prov, ms) in &pending {
        let cli = versions
            .get(prov)
            .cloned()
            .unwrap_or_else(|| Value::from("?"));
        new_since.insert(prov.clone(), json!({"models": ms, "at": ts, "cli": cli}));
    }
    let snap = json!({
        "checkedAt": ts,
        "versions": versions,
        "discovered": discovered_map,
        "addons": addons,
        "addonNews": addon_news,
        "newSince": new_since,
    });
    write_tmp_replace(&snap_path, &snap)?;
    Ok(Watch {
        news,
        snapshot: snap,
    })
}

/// `json.dump(snap, fh, indent=1)` + `os.replace`: cualquier error lanza.
/// El temporal es exclusivo de esta escritura; el heredado sigue usando
/// `path + ".tmp"` mientras ambos procesos conviven.
pub fn write_tmp_replace(path: &Path, value: &Value) -> Result<()> {
    let text = indent_dumps(value, 1, true).map_err(|_| Fault::Unsure)?;
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(format!(
        ".{}.tmp",
        crate::fresh_id("rust").map_err(|_| Fault::Raises)?
    ));
    let tmp = PathBuf::from(tmp);
    // Abrir fuera del bloque de limpieza: si la creación exclusiva falla,
    // no somos dueños de ese nombre y no debemos borrarlo.
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&tmp)
        .map_err(|_| Fault::Raises)?;
    let written = (|| -> io::Result<()> {
        file.write_all(text.as_bytes())?;
        file.flush()?;
        drop(file);
        fs::rename(&tmp, path)
    })();
    if written.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    written.map_err(|_| Fault::Raises)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dirname_like_posixpath() {
        assert_eq!(py_dirname(Path::new("/a/b/c")), PathBuf::from("/a/b"));
        assert_eq!(py_dirname(Path::new("/a")), PathBuf::from("/"));
        assert_eq!(py_dirname(Path::new("/")), PathBuf::from("/"));
        assert_eq!(py_dirname(Path::new("a")), PathBuf::from(""));
        assert_eq!(py_dirname(Path::new("//a//b")), PathBuf::from("//a"));
    }

    #[test]
    fn version_runs_compare_as_python_lists() {
        let k = |s: &str| digit_runs(s).unwrap();
        assert_eq!(cmp_runs(&k("2.1.10"), &k("2.1.9")), Ordering::Greater);
        assert_eq!(cmp_runs(&k("2.1"), &k("2.1.0")), Ordering::Less);
        assert_eq!(cmp_runs(&k("007"), &k("7")), Ordering::Equal);
        assert_eq!(
            cmp_runs(&k("99999999999999999999999999999999999999999"), &k("1")),
            Ordering::Greater
        );
        assert!(digit_runs("v٣").is_err());
    }

    #[test]
    fn family_versions() {
        assert_eq!(
            family_ver("claude-opus-4-1-20250805").unwrap(),
            ("opus".into(), vec![4, 1])
        );
        assert_eq!(
            family_ver("gpt-5.6-codex").unwrap(),
            ("gpt".into(), vec![5, 6])
        );
        assert_eq!(family_ver("GROK-4[x]").unwrap(), ("grok".into(), vec![4]));
        assert_eq!(family_ver("other").unwrap(), ("other".into(), vec![]));
    }

    /// Una coincidencia que cruza el borde de un trozo se encuentra igual que
    /// en el archivo entero.
    #[test]
    fn binary_scan_crosses_chunks() {
        let dir = std::env::temp_dir().join(format!("mw-scan-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("bin");
        let mut data = vec![b'x'; CHUNK - 10];
        data.extend_from_slice(b"claude-opus-5-6\0claude-fable-5-1-20260101[1m]\0");
        data.extend(vec![b'y'; CHUNK]);
        data.extend_from_slice(b"claude-haiku-4-5-xx\0claude-sonnet-4");
        fs::write(&path, &data).unwrap();
        let ids = binary_ids(
            &path,
            rule(&CLAUDE_BYTES).unwrap(),
            rule(&CLAUDE_RE).unwrap(),
        );
        assert_eq!(
            ids.into_iter().collect::<Vec<_>>(),
            vec![
                "claude-fable-5-1-20260101[1m]".to_owned(),
                "claude-opus-5-6".to_owned(),
                "claude-sonnet-4".to_owned()
            ]
        );
        let _ = fs::remove_dir_all(&dir);
    }
}
