//! Límites de proveedor: `_limits_cache` (bin/cc-dash:414-419),
//! `usage_provider_limits` (1309) y `_refresh_provider_limits` (1160).
//! La red y la lectura de archivos corren en una tarea; quien pide responde
//! con lo que hay (D3). Sin efectos de uso (`usage_effects = false`, la sombra)
//! no hay refresco: ni red ni escrituras (R4).
use super::super::{
    NativeOptions,
    lanes::{Lane, UsageBackend},
};
use comandos_core::json::{truthy, workspace_loads};
use comandos_runtime::limits::{self as lim, AbortRefresh, Raised};
use comandos_store::usage_read;
use futures_util::future::BoxFuture;
use serde_json::{Map, Value};
use std::{
    net::SocketAddr,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, MutexGuard},
    time::Duration,
};

/// `LIMITS_TTL` (cc-dash:416); tras un error de la cuenta `main`, el triple.
pub const LIMITS_TTL_S: i64 = 60;

/// `_http_json(..., timeout=8)`: plazo por operación de socket.
const OAUTH_TIMEOUT: Duration = Duration::from_secs(8);

/// Plazo total por cuenta (R4): DNS, conexión y lectura de 8 s cada una y un
/// cuerpo que gotea no dejan el refresco en vuelo para siempre.
const ACCOUNT_DEADLINE: Duration = Duration::from_secs(30);

pub type HttpFuture = BoxFuture<'static, Result<Value, String>>;

/// GET JSON con `Authorization: Bearer <token>`, `anthropic-beta:
/// oauth-2025-04-20` y `Content-Type: application/json`; `Err` = `str(e)` del
/// `urllib` del Python. El token nunca se registra ni se devuelve.
pub trait OauthHttp: Send + Sync + 'static {
    fn get_json(&self, url: &'static str, token: String, timeout: Duration) -> HttpFuture;
}

pub struct Limits {
    pub rows: Vec<Map<String, Value>>,
    pub health: Map<String, Value>,
}

#[derive(Default)]
struct LimitsState {
    /// `int(time.time())` del último refresco completo; 0 = nunca.
    at: i64,
    rows: Vec<Map<String, Value>>,
    health: Map<String, Value>,
    refreshing: bool,
}

/// Una entrada: la última lectura completa (cota explícita, regla 6).
#[derive(Default)]
pub struct LimitsCache {
    state: Mutex<LimitsState>,
    emails: Mutex<lim::EmailCache>,
}

/// Lo que la tarea de refresco necesita, sin `&Native` (D13).
#[derive(Clone)]
pub struct RefreshDeps {
    pub opts: NativeOptions,
    pub usage: Arc<Lane<UsageBackend>>,
}

impl LimitsCache {
    /// Solo para las pruebas: una caché ya llena que nunca se refrescó.
    #[doc(hidden)]
    pub fn with_rows(rows: Vec<Map<String, Value>>) -> Self {
        Self {
            state: Mutex::new(LimitsState {
                rows,
                ..LimitsState::default()
            }),
            ..Self::default()
        }
    }

    fn lock(&self) -> MutexGuard<'_, LimitsState> {
        self.state.lock().unwrap_or_else(|p| p.into_inner())
    }

    pub fn refreshing(&self) -> bool {
        self.lock().refreshing
    }

    /// `usage_provider_limits(force=False)`: si venció y no hay refresco en
    /// vuelo, lanza uno en una tarea; siempre devuelve la copia actual.
    pub fn get(self: &Arc<Self>, deps: &RefreshDeps) -> Limits {
        let now = (deps.opts.clock)() as f64 / 1000.0;
        let start = {
            let mut st = self.lock();
            let error = st
                .health
                .get("claude_oauth")
                .and_then(|h| h.get("status"))
                .and_then(Value::as_str)
                == Some("error");
            let ttl = if error {
                LIMITS_TTL_S * 3
            } else {
                LIMITS_TTL_S
            };
            let stale = now - st.at as f64 > ttl as f64;
            let start = deps.opts.usage_effects && stale && !st.refreshing;
            if start {
                st.refreshing = true;
            }
            start
        };
        if start {
            match tokio::runtime::Handle::try_current() {
                Ok(handle) => {
                    handle.spawn(refresh(self.clone(), deps.clone()));
                }
                // Fuera de un runtime no hay dónde correr la red: no se lanza.
                Err(_) => self.lock().refreshing = false,
            }
        }
        let st = self.lock();
        Limits {
            rows: st.rows.clone(),
            health: st.health.clone(),
        }
    }

    /// D5: `attach_token_counts` sobre las filas cacheadas, como el Python (que
    /// muta las mismas `dict` que guarda): el primer valor se queda.
    pub fn attach_tokens(&self, windows: &Value) -> Vec<Map<String, Value>> {
        let mut st = self.lock();
        comandos_core::usage_state::attach_token_counts(&mut st.rows, windows);
        st.rows.clone()
    }
}

/// `finally: _limits_refreshing = False`, también si la tarea se cancela.
struct Reset(Arc<LimitsCache>);

impl Drop for Reset {
    fn drop(&mut self) {
        self.0.lock().refreshing = false;
    }
}

fn secs(opts: &NativeOptions) -> i64 {
    (opts.clock)().div_euclid(1000)
}

/// `~` a partir de `~/.claude/hooks`.
fn home_of(hooks: &Path) -> PathBuf {
    hooks
        .parent()
        .and_then(Path::parent)
        .map_or_else(|| hooks.join("../.."), Path::to_path_buf)
}

/// `spawn_blocking`; un pánico del lector es `None` (el `except` del Python).
async fn blocking<T: Send + 'static>(job: impl FnOnce() -> T + Send + 'static) -> Option<T> {
    tokio::task::spawn_blocking(job).await.ok()
}

struct Collected {
    rows: Vec<Map<String, Value>>,
    /// `claude + codex + grok + agy`: lo que va a `record_quota_snapshots`.
    snapshot: Vec<Value>,
    health: Map<String, Value>,
}

async fn refresh(cache: Arc<LimitsCache>, deps: RefreshDeps) {
    let _reset = Reset(cache.clone());
    let mut networked = false;
    // `AbortRefresh`: el hilo del Python moría y la caché no cambiaba. Si ya
    // hubo red, se fija `at` igualmente (las filas y la salud no cambian): sin
    // eso cada `get` posterior repetiría la petición OAuth sin TTL. Diferencia
    // aceptada con el Python, que reintentaba en cada sondeo.
    let done = match collect(&cache, &deps, &mut networked).await {
        Ok(done) => done,
        Err(AbortRefresh) => {
            if networked {
                cache.lock().at = secs(&deps.opts);
            }
            return;
        }
    };
    let at = secs(&deps.opts);
    {
        let mut st = cache.lock();
        st.at = at;
        st.rows = done.rows;
        st.health = done.health;
    }
    // `try: record_quota_snapshots(...) except: pass`. Va después de publicar la
    // caché (R3: nadie espera la escritura) y nunca crea la base (A3).
    if !done.snapshot.is_empty() && db_exists(&deps.opts.usage_db).await {
        let snapshot = done.snapshot;
        let _ = deps
            .usage
            .with(move |u| usage_read::record_quota_snapshots(&u.conn, &snapshot, at))
            .await;
    }
}

/// `os.path.exists` en un hilo de bloqueo (un `stat` puede bloquear).
async fn db_exists(path: &Path) -> bool {
    let path = path.to_path_buf();
    blocking(move || path.exists()).await.unwrap_or(false)
}

/// `_measured_usage` por el carril de uso; cualquier fallo es el `except` (`None`).
/// Sin base no se abre el carril: el refresco nunca la crea (A3).
async fn measured(deps: &RefreshDeps, provider: &'static str) -> Option<Value> {
    if !db_exists(&deps.opts.usage_db).await {
        return None;
    }
    let now = secs(&deps.opts);
    let day = deps.opts.zone.day_start(now)?;
    deps.usage
        .with(move |u| usage_read::measured_usage(&u.conn, provider, now, day))
        .await
        .ok()?
        .ok()?
}

/// `health.update({"status": "error", "error": str(e)[:240]})`.
fn mark_error(health: &mut Map<String, Value>, error: &str) {
    health.insert("status".into(), "error".into());
    health.insert(
        "error".into(),
        error.chars().take(240).collect::<String>().into(),
    );
}

/// Texto de `health.error` cuando la respuesta OAuth no se puede interpretar
/// con certeza como el Python (`Raised::Unsure`). Diferencia aceptada: el
/// Python guardaría el `str(e)` de su excepción o las filas.
pub const OAUTH_UNSURE_ERROR: &str = "respuesta OAuth no reproducible por el frente";

/// `networked` pasa a `true` antes de la primera petición de red.
async fn collect(
    cache: &Arc<LimitsCache>,
    deps: &RefreshDeps,
    networked: &mut bool,
) -> Result<Collected, AbortRefresh> {
    let o = &deps.opts;
    let home = home_of(&o.hooks);
    let zone = o.zone.clone();
    // Lectura local en un salto: rollouts de Codex, cuentas y sus tokens. El
    // Python lee cada token justo antes de su petición; un error en cualquiera
    // mata el hilo igual y la caché queda como estaba.
    let now = secs(o);
    let (codex, accounts) = blocking({
        let home = home.clone();
        let zone = zone.clone();
        move || -> Result<_, AbortRefresh> {
            let codex =
                lim::read_codex_rate_limits(&home.join(".codex/sessions"), now, 16, zone.as_ref())?;
            let mut accounts = Vec::new();
            for (alias, path) in lim::claude_account_creds(&home) {
                let token = lim::oauth_token(&path)?;
                accounts.push((alias, path, token));
            }
            Ok((codex, accounts))
        }
    })
    .await
    .ok_or(AbortRefresh)??;

    let mut claude: Vec<Value> = Vec::new();
    let mut health = Map::new();
    for (alias, path, token) in accounts {
        let ts = secs(o);
        let mut h = Map::new();
        h.insert("provider".into(), "claude".into());
        h.insert("source".into(), "oauth".into());
        h.insert("configured".into(), false.into());
        h.insert("status".into(), "missing".into());
        let mut rows = Vec::new();
        if !token.is_empty() {
            h.insert("configured".into(), true.into());
            *networked = true;
            let call = o
                .oauth
                .get_json(lim::CLAUDE_OAUTH_USAGE_URL, token, OAUTH_TIMEOUT);
            match tokio::time::timeout(ACCOUNT_DEADLINE, call).await {
                Ok(Ok(payload)) => {
                    h.insert("status".into(), "ok".into());
                    h.insert("last_success_at".into(), ts.into());
                    match lim::parse_claude_oauth_limits(&payload, ts, zone.as_ref()) {
                        Ok(parsed) => rows = parsed,
                        Err(Raised::Exception(e)) => mark_error(&mut h, &e),
                        // Tras la red no se aborta (abortar dejaba `at` sin
                        // fijar): error propio y el TTL de error (180 s).
                        Err(Raised::Unsure) => mark_error(&mut h, OAUTH_UNSURE_ERROR),
                    }
                }
                Ok(Err(e)) => mark_error(&mut h, &e),
                Err(_) => mark_error(&mut h, "<urlopen error timed out>"),
            }
        }
        // `account_email_for_dir(os.path.dirname(path), "claude")`.
        let dir = path.parent().map(Path::to_path_buf).unwrap_or_default();
        let email = blocking({
            let cache = cache.clone();
            move || {
                let mut emails = cache.emails.lock().unwrap_or_else(|p| p.into_inner());
                lim::email_for_dir(&mut emails, &dir, "claude")
            }
        })
        .await
        .and_then(Result::ok);
        for row in &mut rows {
            if let Some(row) = row.as_object_mut() {
                row.insert("account".into(), alias.as_str().into());
                if let Some(email) = email.as_ref().filter(|e| truthy(e)) {
                    row.insert("accountEmail".into(), email.clone());
                }
                if alias != "main" {
                    let id = row.get("id").and_then(Value::as_str).unwrap_or("");
                    let id = format!("{alias}:{id}");
                    row.insert("id".into(), id.into());
                }
            }
        }
        if rows.is_empty() && h.get("status").and_then(Value::as_str) == Some("error") {
            // Fetch fallido (p. ej. 429): el último dato bueno DE ESA cuenta.
            rows = cache
                .lock()
                .rows
                .iter()
                .filter(|r| {
                    r.get("provider").and_then(Value::as_str) == Some("claude")
                        && match r.get("account").filter(|a| truthy(a)) {
                            Some(account) => account.as_str() == Some(alias.as_str()),
                            None => alias == "main",
                        }
                })
                .map(|r| Value::Object(r.clone()))
                .collect();
            h.insert("stale".into(), (!rows.is_empty()).into());
        }
        h.insert("account".into(), alias.as_str().into());
        let key = if alias == "main" {
            "claude_oauth".to_owned()
        } else {
            format!("claude_oauth:{alias}")
        };
        health.insert(key, Value::Object(h));
        claude.extend(rows);
    }

    let quotas = blocking({
        let path = o.hooks.join("provider-quotas.json");
        move || lim::user_quotas(&path)
    })
    .await
    .unwrap_or_default();

    // Grok: consumo medido local y el límite oficial del log del CLI.
    let grok_measured = measured(deps, "grok").await;
    let official = blocking({
        let home = home.clone();
        let zone = zone.clone();
        let now = secs(o);
        move || {
            let homes: Vec<PathBuf> = lim::grok_account_homes(&home)
                .into_iter()
                .map(|(_, h)| h)
                .collect();
            lim::read_grok_credit_limits(&homes, now, zone.as_ref())
        }
    })
    .await
    .flatten();
    let mut grok = Vec::new();
    if grok_measured.is_some() || official.is_some() {
        let quota = lim::quota_tokens_7d(&quotas, "grok")?;
        grok.extend(lim::grok_row(
            grok_measured.as_ref(),
            official.as_ref(),
            quota,
            secs(o),
        ));
    }

    // Groq: consumo medido y las cabeceras `x-ratelimit-*` guardadas.
    let groq_measured = measured(deps, "groq").await;
    let headers = blocking({
        let path = o.hooks.join("groq-ratelimit.json");
        let now = secs(o);
        move || lim::read_groq_headers(&path, now)
    })
    .await
    .unwrap_or_default();
    let mut groq = Vec::new();
    if groq_measured.is_some() || !headers.is_empty() {
        let quota = lim::quota_tokens_7d(&quotas, "groq")?;
        groq = lim::groq_rows(groq_measured.as_ref(), headers, quota, secs(o));
    }

    // agy: cuota que su barra de estado guarda en `H/agy-quota.json`.
    let agy = blocking({
        let path = o.hooks.join("agy-quota.json");
        let zone = zone.clone();
        let now = (o.clock)() as f64 / 1000.0;
        move || lim::read_agy_quota(&path, now, zone.as_ref())
    })
    .await
    .unwrap_or_default();

    let snapshot: Vec<Value> = claude
        .into_iter()
        .chain(codex)
        .chain(grok)
        .chain(agy)
        .collect();
    let rows = snapshot
        .iter()
        .chain(&groq)
        .filter_map(|v| v.as_object().cloned())
        .collect();
    Ok(Collected {
        rows,
        snapshot,
        health,
    })
}

// ---------------------------------------------------------------- red real

/// Tope del cuerpo de la respuesta OAuth (1 MiB). Pasarlo es un error propio
/// del frente (D4: diagnóstico, diferencia aceptada).
const OAUTH_BODY_CAP: usize = 1 << 20;
const OAUTH_BODY_TOO_LARGE: &str = "respuesta OAuth de más de 1 MiB";

/// El cliente de producción: `reqwest` con HTTP/1.1, plazos de 8 s y la
/// resolución hecha antes por el frente para reproducir los errores de DNS.
pub struct ReqwestOauth;

impl OauthHttp for ReqwestOauth {
    fn get_json(&self, url: &'static str, token: String, timeout: Duration) -> HttpFuture {
        Box::pin(fetch_json(url, token, timeout))
    }
}

async fn fetch_json(url: &'static str, token: String, timeout: Duration) -> Result<Value, String> {
    use reqwest::header::{AUTHORIZATION, CONTENT_TYPE, HeaderValue};
    let parsed = reqwest::Url::parse(url).map_err(|e| e.to_string())?;
    let host = parsed.host_str().unwrap_or_default().to_owned();
    let port = parsed.port_or_known_default().unwrap_or(443);
    let addrs: Vec<SocketAddr> = tokio::net::lookup_host((host.as_str(), port))
        .await
        .map_err(|e| dns_error(&e))?
        .collect();
    let client = reqwest::Client::builder()
        .http1_only()
        .no_proxy()
        .connect_timeout(timeout)
        .read_timeout(timeout)
        .resolve_to_addrs(&host, &addrs)
        // La misma petición que veía el servidor con el Python.
        .user_agent("Python-urllib/3.10")
        .build()
        .map_err(|e| e.to_string())?;
    // Un token con caracteres que no caben en una cabecera: el Python incluiría el
    // token en el texto del error; aquí nunca.
    let mut auth = HeaderValue::from_str(&format!("Bearer {token}"))
        .map_err(|_| "Invalid header value for Authorization".to_owned())?;
    auth.set_sensitive(true);
    let response = client
        .get(url)
        .header(AUTHORIZATION, auth)
        .header("anthropic-beta", "oauth-2025-04-20")
        .header(CONTENT_TYPE, "application/json")
        .send()
        .await
        .map_err(|e| python_error(&e))?;
    let status = response.status();
    if !status.is_success() {
        // `HTTPError`: la frase del servidor (latin-1) o la canónica.
        let reason = response
            .extensions()
            .get::<hyper::ext::ReasonPhrase>()
            .map(|r| r.as_bytes().iter().map(|&b| char::from(b)).collect())
            .or_else(|| status.canonical_reason().map(str::to_owned))
            .unwrap_or_default();
        return Err(format!("HTTP Error {}: {reason}", status.as_u16()));
    }
    // Cuerpo acotado: la respuesta real mide unos cientos de bytes.
    if response
        .content_length()
        .is_some_and(|n| n > OAUTH_BODY_CAP as u64)
    {
        return Err(OAUTH_BODY_TOO_LARGE.to_owned());
    }
    let mut response = response;
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|e| python_error(&e))? {
        if body.len() + chunk.len() > OAUTH_BODY_CAP {
            return Err(OAUTH_BODY_TOO_LARGE.to_owned());
        }
        body.extend_from_slice(&chunk);
    }
    // Un cuerpo que no es UTF-8 o JSON da un texto propio (D4, diferencia aceptada).
    let text = std::str::from_utf8(&body).map_err(|_| "respuesta no UTF-8".to_owned())?;
    workspace_loads(text).map_err(|e| format!("respuesta no JSON: {e}"))
}

/// `strerror(code)` sin el sufijo ` (os error N)` de Rust.
fn strerror(code: i32) -> String {
    let text = std::io::Error::from_raw_os_error(code).to_string();
    let suffix = format!(" (os error {code})");
    text.strip_suffix(&suffix).unwrap_or(&text).to_owned()
}

/// `socket.gaierror` dentro de `URLError`: `<urlopen error [Errno -3] …>`.
fn dns_error(e: &std::io::Error) -> String {
    if let Some(code) = e.raw_os_error() {
        return format!("<urlopen error [Errno {code}] {}>", strerror(code));
    }
    let text = e.to_string();
    let detail = text
        .strip_prefix("failed to lookup address information: ")
        .unwrap_or(&text);
    let code = match detail {
        "Name or service not known" => Some(-2),
        "Temporary failure in name resolution" => Some(-3),
        "Non-recoverable failure in name resolution" => Some(-4),
        "No address associated with hostname" => Some(-5),
        "ai_family not supported" => Some(-6),
        "Servname not supported for ai_socktype" => Some(-8),
        "Address family for hostname not supported" => Some(-9),
        "Memory allocation failure" => Some(-10),
        _ => None,
    };
    match code {
        Some(code) => format!("<urlopen error [Errno {code}] {detail}>"),
        None => format!("<urlopen error {detail}>"),
    }
}

/// El primer `io::Error` con código del sistema en la cadena de causas.
fn os_error(e: &(dyn std::error::Error + 'static)) -> Option<i32> {
    let mut current = Some(e);
    while let Some(err) = current {
        if let Some(code) = err
            .downcast_ref::<std::io::Error>()
            .and_then(std::io::Error::raw_os_error)
        {
            return Some(code);
        }
        current = err.source();
    }
    None
}

/// `str(e)` del `urllib` para los casos comunes (D4); lo demás, el texto de
/// `reqwest` (solo lleva la URL, nunca cabeceras).
fn python_error(e: &reqwest::Error) -> String {
    if e.is_connect() {
        if e.is_timeout() {
            return "<urlopen error timed out>".into();
        }
        if let Some(code) = os_error(e) {
            return format!("<urlopen error [Errno {code}] {}>", strerror(code));
        }
    }
    if e.is_timeout() {
        return "The read operation timed out".into();
    }
    e.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dns_errors_read_like_urllib() {
        let e = std::io::Error::other(
            "failed to lookup address information: Temporary failure in name resolution",
        );
        assert_eq!(
            dns_error(&e),
            "<urlopen error [Errno -3] Temporary failure in name resolution>"
        );
        let e = std::io::Error::other(
            "failed to lookup address information: Name or service not known",
        );
        assert_eq!(
            dns_error(&e),
            "<urlopen error [Errno -2] Name or service not known>"
        );
        assert_eq!(
            dns_error(&std::io::Error::from_raw_os_error(111)),
            "<urlopen error [Errno 111] Connection refused>"
        );
        assert_eq!(strerror(101), "Network is unreachable");
    }

    #[test]
    fn home_is_two_levels_above_hooks() {
        assert_eq!(
            home_of(Path::new("/h/u/.claude/hooks")),
            PathBuf::from("/h/u")
        );
    }

    /// Conexión rechazada de verdad (puerto local sin nadie escuchando, sin DNS
    /// externo): el texto del `urllib`. Nunca toca `api.anthropic.com`.
    #[tokio::test]
    async fn refused_connection_reads_like_urllib() {
        let port = std::net::TcpListener::bind("127.0.0.1:0")
            .and_then(|l| l.local_addr())
            .map(|a| a.port())
            .unwrap_or(9);
        let client = reqwest::Client::builder()
            .http1_only()
            .no_proxy()
            .connect_timeout(Duration::from_secs(2))
            .build();
        let Ok(client) = client else {
            return;
        };
        let result = client.get(format!("http://127.0.0.1:{port}/")).send().await;
        match result {
            Err(e) => assert_eq!(
                python_error(&e),
                "<urlopen error [Errno 111] Connection refused>"
            ),
            Ok(_) => panic!("nadie escucha en ese puerto"),
        }
    }
}
