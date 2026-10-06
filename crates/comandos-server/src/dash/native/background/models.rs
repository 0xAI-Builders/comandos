//! Vigilante de modelos del frente (Tarea 6 de la 2f-3): `_model_watch_cycle`
//! (bin/cc-dash:696), `_model_watch_cycle_locked` (701), `_model_watch_notify`
//! (678), `_model_watch_loop` (766) y `_force_model_watch_cycle` (1523).
//!
//! - Un ciclo a la vez: el candado es `CliState::watch` (= `_MODEL_WATCH_LOCK`,
//!   junto al estado `_model_watch_state`), compartido por el bucle y por
//!   `?refresh=1` de `/commands/catalog`.
//! - Cada vuelta: versiones de los CLIs y firma de los catálogos locales; con
//!   `force`, un cambio o 6 h desde el último escaneo completo: calentar el
//!   catálogo de CLIs (`catalog_cli::warm`), `watch_models`, avisos de
//!   modelos nuevos, `watch_news` y su aviso, y el latido `heartbeatAt`; si
//!   no, solo el latido (con `versions` si faltaba).
//! - Avisos: `notice_emit("announcement", …)` (app-state) y `POST
//!   127.0.0.1:4778/notify` con plazo de 3 s (`NotifyPost::post_checked`),
//!   sin mirar `DESKTOP_NOTIFY`, como el Python.
//! - `except Exception: pass`: lo que el Python lanzaría y lo que el port no
//!   reproduce con certeza (`model_watch::Fault`) terminan la vuelta sin
//!   tocar `_model_watch_state` ni escribir más.
//!
//! Ligereza: los CLIs y la red corren en el pool de bloqueo: en el ciclo
//! completo, un salto para versiones y firma, otro para `watch_models`, otro
//! para las noticias y otro para el latido (más los del calentamiento del
//! catálogo); en el barato, dos. Los binarios versionados se leen por trozos
//! (`model_watch`).
//! Desviaciones de la red de noticias (`Feeds`): plazo total por petición de
//! cuatro veces el de socket del Python y cuerpo acotado a 16 MiB (el
//! `urllib` del Python no tiene ni lo uno ni lo otro).
use super::Stop;
use crate::dash::native::{
    Native, NativeOptions, catalog_cli, files::write_text_atomic, light, tmux::run_program,
};
use comandos_core::json::{indent_dumps, response_dumps};
use comandos_runtime::{
    hooks::py,
    model_catalog::{catalog_paths, catalog_signature},
    model_watch::{self, Host, Paths, Watch, WatchPaths},
    news_watch::{self, Endpoints, Fetch, Fetched, Sources},
    providers,
};
use serde_json::{Map, Value, json};
use std::{
    ffi::{OsStr, OsString},
    path::{Path, PathBuf},
    sync::{Arc, Mutex, Weak},
    time::Duration,
};
use tokio::runtime::Handle;

/// `time.sleep(90)` antes de la primera vuelta (no competir con el arranque).
pub const FIRST_DELAY: Duration = Duration::from_secs(90);
/// `time.sleep(600)` entre vueltas.
pub const PERIOD: Duration = Duration::from_secs(600);
/// Escaneo completo como mucho cada 6 h sin cambios.
const FULL_EVERY: f64 = 6.0 * 3600.0;
/// `urlopen(..., timeout=3)` de los avisos.
const NOTIFY_TIMEOUT: Duration = Duration::from_secs(3);
/// Tope del cuerpo de un feed.
const FEED_BODY_CAP: usize = 16 * 1024 * 1024;
/// Plazo total de una petición de feed, en múltiplos del de socket.
const FEED_DEADLINE_FACTOR: u32 = 4;

/// `_model_watch_state`: versiones y firma del catálogo de la última vuelta y
/// hora (`time.time()`) del último escaneo completo.
#[derive(Default)]
pub struct WatchState {
    versions: Map<String, Value>,
    last_full: f64,
    catalog: Option<Value>,
}

/// El candado del vigilante (`_MODEL_WATCH_LOCK`) de este frente.
pub fn lock(native: &Native) -> &Arc<tokio::sync::Mutex<WatchState>> {
    &native.cli.watch
}

// ---------------------------------------------------------------------------
// Red de noticias
// ---------------------------------------------------------------------------

/// Los feeds de `news_watch`: a dónde se pide y los clientes `reqwest` (uno
/// por plazo, sin conexiones ociosas: como el `urllib`, cada petición abre la
/// suya).
pub struct Feeds {
    endpoints: Endpoints,
    clients: Mutex<Vec<(Duration, reqwest::Client)>>,
}

impl Feeds {
    pub fn new(endpoints: Endpoints) -> Self {
        Self {
            endpoints,
            clients: Mutex::new(Vec::new()),
        }
    }

    /// Las URLs del Python: `COMANDOS_SEARX` del proceso (o su valor por
    /// omisión) y los hosts públicos.
    pub fn production() -> Self {
        let searx = std::env::var("COMANDOS_SEARX").ok();
        Self::new(Endpoints::production(searx.as_deref()))
    }

    pub fn endpoints(&self) -> &Endpoints {
        &self.endpoints
    }

    fn client(&self, timeout: Duration) -> Result<reqwest::Client, String> {
        let mut clients = self.clients.lock().unwrap_or_else(|p| p.into_inner());
        if let Some((_, client)) = clients.iter().find(|(t, _)| *t == timeout) {
            return Ok(client.clone());
        }
        let client = reqwest::Client::builder()
            .http1_only()
            .connect_timeout(timeout)
            .read_timeout(timeout)
            .pool_max_idle_per_host(0)
            .redirect(urllib_redirects())
            .build()
            .map_err(|e| e.to_string())?;
        clients.push((timeout, client.clone()));
        Ok(client)
    }
}

/// `HTTPRedirectHandler` de `urllib`: sigue 301, 302, 303 y 307 (como mucho
/// 10 destinos y 4 veces el mismo); el resto (308 incluido) vuelve como
/// respuesta y `news_watch` decide.
fn urllib_redirects() -> reqwest::redirect::Policy {
    reqwest::redirect::Policy::custom(|attempt| match attempt.status().as_u16() {
        301 | 302 | 303 | 307 => {
            let target = attempt.url().clone();
            let previous = attempt.previous();
            let repeats = previous.iter().skip(1).filter(|u| **u == target).count();
            if previous.len() > 10 || repeats >= 4 {
                attempt.error("redirect loop")
            } else {
                attempt.follow()
            }
        }
        _ => attempt.stop(),
    })
}

/// El `Fetch` del frente: la petición en el runtime, esperada desde el pool
/// de bloqueo.
struct FrontFetch {
    handle: Handle,
    feeds: Arc<Feeds>,
}

impl Fetch for FrontFetch {
    fn get(&self, url: &str, headers: &[(&str, &str)], timeout: Duration) -> Fetched {
        let client = match self.feeds.client(timeout) {
            Ok(client) => client,
            Err(e) => return Fetched::Failed(e),
        };
        let mut request = client.get(url);
        for (name, value) in headers {
            request = request.header(*name, *value);
        }
        let fetch = async move {
            let response = request.send().await.map_err(|e| e.to_string())?;
            let status = response.status();
            if !status.is_success() {
                let location = response
                    .headers()
                    .get(reqwest::header::LOCATION)
                    .and_then(|v| v.to_str().ok())
                    .map(str::to_owned);
                return Ok(Fetched::Status {
                    code: status.as_u16(),
                    location,
                });
            }
            let mut response = response;
            let mut body = Vec::new();
            while let Some(chunk) = response.chunk().await.map_err(|e| e.to_string())? {
                if body.len() + chunk.len() > FEED_BODY_CAP {
                    return Err("respuesta de más de 16 MiB".to_owned());
                }
                body.extend_from_slice(&chunk);
            }
            Ok(Fetched::Body(body))
        };
        let deadline = timeout * FEED_DEADLINE_FACTOR;
        match self.handle.block_on(tokio::time::timeout(deadline, fetch)) {
            Ok(Ok(fetched)) => fetched,
            Ok(Err(e)) => Fetched::Failed(e),
            Err(_) => Fetched::Failed("timed out".into()),
        }
    }
}

// ---------------------------------------------------------------------------
// CLIs
// ---------------------------------------------------------------------------

/// `providers.which` y `_run` del frente: los programas con el entorno de los
/// hijos del frente, esperados desde el pool de bloqueo.
struct FrontHost {
    handle: Handle,
    opts: NativeOptions,
}

impl Host for FrontHost {
    fn which(&self, name: &str) -> Option<PathBuf> {
        providers::which_in_dirs(
            name,
            self.opts.search_path.as_deref(),
            &self.opts.home,
            &self.opts.user_bin_dirs,
        )
    }

    fn run(
        &self,
        exe: &Path,
        args: &[&str],
        timeout: Duration,
        env: &[(&str, &OsStr)],
    ) -> Option<String> {
        let mut program = self.opts.program(exe);
        for (name, value) in env {
            program
                .env
                .push((OsString::from(name), value.to_os_string()));
        }
        let output = self
            .handle
            .block_on(run_program(&program, args, timeout))
            .ok()?;
        Some(output.stdout + &output.stderr)
    }
}

fn model_paths(opts: &NativeOptions) -> Paths {
    Paths {
        home: opts.home.clone(),
        codex_home: opts.codex_home.clone(),
        cwd: opts.cwd.clone(),
    }
}

fn signature(opts: &NativeOptions) -> Value {
    catalog_signature(&catalog_paths(
        &opts.home,
        &opts.cwd,
        opts.codex_home.as_deref(),
        opts.grok_home.as_deref(),
    ))
}

/// `int(time.time())`.
fn now_int(opts: &NativeOptions) -> i64 {
    let t = (opts.clock_seconds)().trunc();
    if t.is_finite() { t as i64 } else { 0 }
}

// ---------------------------------------------------------------------------
// Ciclo
// ---------------------------------------------------------------------------

/// `_model_watch_cycle(force)`: espera el candado y corre una vuelta.
pub async fn cycle(native: &Arc<Native>, force: bool) -> Option<Watch> {
    run_owned_cycle(native, force, None).await
}

/// `_force_model_watch_cycle()`: una vuelta completa salvo que otra termine
/// su escaneo mientras se espera el candado (coalesce por finalización).
pub async fn force_cycle(native: &Arc<Native>) {
    let started = (native.options().clock_seconds)();
    run_owned_cycle(native, true, Some(started)).await;
}

/// Esperar el candado no produce efectos. Una vez adquirido, la tarea
/// registrada conserva la guardia y termina escrituras y avisos aunque el
/// cliente se vaya o venza su plazo HTTP. No se crean tareas por los clientes
/// que cancelan mientras esperan otro ciclo.
async fn run_owned_cycle(native: &Arc<Native>, force: bool, started: Option<f64>) -> Option<Watch> {
    let mut state = Arc::clone(lock(native)).lock_owned().await;
    if started.is_some_and(|at| state.last_full >= at) {
        return None;
    }
    let job = native
        .tasks()
        .spawn_handle({
            let native = Arc::clone(native);
            async move { cycle_locked(&native, &mut state, force).await }
        })
        .ok()?;
    job.await.ok().flatten()
}

async fn blocking<T: Send + 'static>(job: impl FnOnce() -> T + Send + 'static) -> Option<T> {
    tokio::task::spawn_blocking(job).await.ok()
}

/// `_model_watch_cycle_locked(force)`.
async fn cycle_locked(native: &Arc<Native>, state: &mut WatchState, force: bool) -> Option<Watch> {
    let opts = native.options().clone();
    let handle = Handle::current();
    let host = FrontHost {
        handle: handle.clone(),
        opts: opts.clone(),
    };
    let (versions, catalog, host) = blocking({
        let opts = opts.clone();
        move || {
            let versions = model_watch::installed_versions(&host).ok();
            (versions, signature(&opts), host)
        }
    })
    .await?;
    let versions = versions?;
    let changed = Value::Object(versions.clone()) != Value::Object(state.versions.clone())
        || state.catalog.as_ref() != Some(&catalog);
    let due = (opts.clock_seconds)() - state.last_full > FULL_EVERY;
    if !(force || changed || due) {
        state.versions = versions.clone();
        heartbeat(&opts, Some(versions)).await;
        return None;
    }
    // Comandos presentes en cada binario y ayudas: la primera petición de la
    // barra de comandos ya no lee cientos de MB.
    catalog_cli::warm(native).await;
    let (watch, catalog) = blocking({
        let opts = opts.clone();
        move || {
            let paths = model_paths(&opts);
            let hooks = opts.hooks.clone();
            let registry = opts
                .repo_root
                .as_ref()
                .map(|r| r.join("config/providers.json"))
                .unwrap_or_default();
            // `grok_home=os.path.expanduser("~/.grok")`, nunca `GROK_HOME`.
            let grok = opts.home.join(".grok");
            let at = WatchPaths {
                paths: &paths,
                hooks: &hooks,
                registry: &registry,
                grok_home: Some(&grok),
            };
            let watch = model_watch::watch_models(&host, &at, now_int(&opts));
            (watch, signature(&opts))
        }
    })
    .await?;
    let watch = watch.ok()?;
    state.last_full = (opts.clock_seconds)();
    state.catalog = Some(catalog);
    if !watch.news.is_empty() {
        notify_models(native, &watch.news, &versions).await;
    }
    // Noticias de skills y MCPs (patrón Radar): en las mismas vueltas.
    news(native, &opts, handle).await;
    state.versions = versions;
    heartbeat(&opts, None).await;
    Some(watch)
}

/// `_model_watch_notify(news, versions)`.
async fn notify_models(
    native: &Native,
    news: &[(String, Vec<String>)],
    versions: &Map<String, Value>,
) {
    for (prov, models) in news {
        let mut body = models
            .iter()
            .take(4)
            .map(String::as_str)
            .collect::<Vec<_>>()
            .join(", ");
        if models.len() > 4 {
            body.push('…');
        }
        let detail = if prov == "claude" {
            "detectados en el binario; disponibilidad sin verificar"
        } else {
            "detectados en el CLI; los catálogos locales actualizan el selector"
        };
        let cli = versions
            .get(prov)
            .map(py::str_of)
            .unwrap_or_else(|| "?".to_owned());
        let title = format!("Modelos nuevos de {prov} detectados");
        let excerpt = format!("{body} (CLI {cli}) — {detail}");
        notice(native, &title, &excerpt, "modelos").await;
    }
}

/// `notice_emit("announcement", title, excerpt)` y el POST a cc-notifyd con
/// `{"title", "kind": "info", "project", "body"}`.
async fn notice(native: &Native, title: &str, excerpt: &str, project: &str) {
    crate::dash::native::usage::pane_models::notice_emit(
        native,
        "announcement",
        title,
        excerpt,
        None,
        None,
    )
    .await;
    let payload = json!({"title": title, "kind": "info", "project": project, "body": excerpt});
    if let Ok(body) = response_dumps(&payload) {
        native
            .options()
            .notifyd
            .post_checked(body, NOTIFY_TIMEOUT)
            .await;
    }
}

/// `watch_news(HOOKS)` y, si hay algo nuevo, su aviso (la primera noticia).
async fn news(native: &Native, opts: &NativeOptions, handle: Handle) {
    let fetch = FrontFetch {
        handle,
        feeds: Arc::clone(&opts.news_feeds),
    };
    let watched = blocking({
        let opts = opts.clone();
        move || {
            let seconds = Arc::clone(&opts.clock_seconds);
            let clock = move || seconds();
            let sources = Sources {
                fetch: &fetch,
                endpoints: fetch.feeds.endpoints(),
                clock: &clock,
            };
            news_watch::watch_news(&sources, &opts.hooks, now_int(&opts))
        }
    })
    .await;
    let Some(Ok(watched)) = watched else {
        return;
    };
    let Some(top) = watched.news.first() else {
        return;
    };
    let title = format!("Noticias de skills/MCPs ({})", watched.news.len());
    let excerpt: String = top
        .get("title")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .chars()
        .take(140)
        .collect();
    notice(native, &title, &excerpt, "noticias").await;
}

/// El latido: `snap = _read_json_quiet(MODEL_WATCH_FILE) or {}`,
/// `snap["heartbeatAt"] = int(time.time())` (y en la vuelta barata
/// `snap.setdefault("versions", versions)`), `write_json_file(indent=1)`.
/// Todo error se traga; lo que el port no lee con certeza, no se reescribe.
async fn heartbeat(opts: &NativeOptions, versions: Option<Map<String, Value>>) {
    let opts = opts.clone();
    let _ = blocking(move || {
        let path = opts.hooks.join("model-watch.json");
        let mut snap = match light::load(&path) {
            Ok(Some(Value::Object(map))) => map,
            Ok(Some(v)) if py::truthy(&v) => return,
            Ok(_) => Map::new(),
            Err(_) => return,
        };
        snap.insert("heartbeatAt".into(), now_int(&opts).into());
        if let Some(versions) = versions {
            snap.entry("versions").or_insert(Value::Object(versions));
        }
        if let Ok(text) = indent_dumps(&Value::Object(snap), 1, true) {
            let _ = write_text_atomic(&path, &text);
        }
    })
    .await;
}

// ---------------------------------------------------------------------------
// Bucle
// ---------------------------------------------------------------------------

/// `_model_watch_loop`: 90 s de espera, una vuelta, 600 s. Tarea registrada
/// del frente; se para con `stop`, con el frente apagado o soltado.
pub fn start(native: &Arc<Native>, stop: Arc<Stop>) -> bool {
    start_with(native, stop, FIRST_DELAY, PERIOD)
}

/// `start` con las esperas inyectadas (pruebas).
pub fn start_with(
    native: &Arc<Native>,
    stop: Arc<Stop>,
    first: Duration,
    period: Duration,
) -> bool {
    let weak = Arc::downgrade(native);
    native.tasks().spawn(run(weak, stop, first, period)).is_ok()
}

async fn run(weak: Weak<Native>, stop: Arc<Stop>, first: Duration, period: Duration) {
    let mut wait = first;
    loop {
        tokio::select! {
            () = tokio::time::sleep(wait) => {}
            () = stop.wait() => return,
        }
        if stop.is_set() {
            return;
        }
        match weak.upgrade() {
            Some(native) if native.enabled() => {
                cycle(&native, false).await;
            }
            _ => return,
        }
        wait = period;
    }
}
