//! Rutas que el frente responde sin el Python (Fase 2b).
//!
//! Cada ruta reproduce byte a byte una rama de `bin/cc-dash`. Lo que toca la
//! base pasa por un único `BackendWorker<StateBackend>`; lo que toca tmux o
//! `fc-list`, por `tokio::process`. Si una entrada no se puede reproducir con
//! certeza, el manejador devuelve `Fault::Decline` ANTES de cualquier efecto
//! y el frente reenvía la petición original al heredado.
pub mod catalogs;
pub mod events;
pub mod files;
pub mod lanes;
pub mod light;
pub mod notices;
pub mod operations;
pub mod pomodoro;
pub mod py;
pub mod query;
pub mod quick;
pub mod retired;
pub mod snippets;
pub mod state;
pub mod states;
pub mod subrequest;
pub mod terminal;
pub mod tmux;
pub mod typing;
pub mod ui_log;
pub mod workspace;

use crate::{
    HandlerError, Reply, Request,
    blocking::{BackendCaller, BackendWorker},
    dash::router::path_of,
};
use http::Method;
use state::{Refusal, StateBackend};
use std::{
    ffi::OsString,
    net::{Ipv4Addr, SocketAddr},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
};
use tokio::sync::OnceCell;

/// Trabajos en cola del worker de la base (sin contar el que corre).
pub const WORKER_CAPACITY: usize = 64;

/// Cada dominio añade su variante en su tarea.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeRoute {
    Light(light::LightRoute),
    Events,
    Notices(notices::NoticesRoute),
    Workspace(workspace::WorkspaceRoute),
    Snippets(snippets::SnippetsRoute),
    UiLog,
    Pomodoro,
    Catalog(catalogs::CatalogRoute),
    Terminal(terminal::TerminalRoute),
    PaneType,
    ModelStatus,
    State,
    QuickTerminal,
    Retired,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verb {
    Get,
    Post,
}

/// Cómo compara el Python la ruta (sin decodificar `%XX`).
#[derive(Debug, Clone, Copy)]
pub enum Key {
    /// `urlsplit(self.path).path == p` o `self.path.startswith(p)`: se
    /// reclama solo la ruta exacta, con consulta opcional.
    Path(&'static str),
    /// `self.path == p or self.path.startswith(p + "?")`.
    ExactOrQuery(&'static str),
    /// `self.path == p`: sin consulta.
    Raw(&'static str),
}

impl Key {
    pub fn matches(self, target: &str) -> bool {
        match self {
            Key::Path(p) => path_of(target) == p,
            Key::ExactOrQuery(p) => {
                target == p
                    || target
                        .strip_prefix(p)
                        .is_some_and(|rest| rest.starts_with('?'))
            }
            Key::Raw(p) => target == p,
        }
    }
}

pub struct Entry {
    pub verb: Verb,
    pub key: Key,
    pub route: NativeRoute,
}

/// Una tabla por dominio; las tareas 3–7 añaden la suya.
const TABLES: &[&[Entry]] = &[
    light::ROUTES,
    events::ROUTES,
    notices::ROUTES,
    workspace::ROUTES,
    snippets::ROUTES,
    ui_log::ROUTES,
    pomodoro::ROUTES,
    catalogs::ROUTES,
    terminal::ROUTES,
    typing::ROUTES,
    operations::ROUTES,
    states::ROUTES,
    quick::ROUTES,
    retired::ROUTES,
];

pub fn route(method: &Method, target: &str) -> Option<NativeRoute> {
    let verb = if *method == Method::GET {
        Verb::Get
    } else if *method == Method::POST {
        Verb::Post
    } else {
        return None;
    };
    TABLES
        .iter()
        .flat_map(|table| table.iter())
        .find(|entry| entry.verb == verb && entry.key.matches(target))
        .map(|entry| entry.route)
}

pub enum Outcome {
    Reply(Reply),
    /// Nada se escribió: el frente reenvía la petición original.
    Decline,
}

pub enum Fault {
    Decline,
    Error(HandlerError),
}

impl From<HandlerError> for Fault {
    fn from(error: HandlerError) -> Self {
        Fault::Error(error)
    }
}

pub type Answer = Result<Reply, Fault>;

/// `self._json(status, value)`: cuerpo con `json.dumps` del Python.
pub fn reply(status: http::StatusCode, value: &serde_json::Value) -> Answer {
    Reply::json(status, value).map_err(Fault::from)
}

/// Milisegundos Unix; las pruebas lo sustituyen.
pub type Clock = Arc<dyn Fn() -> i64 + Send + Sync>;

pub fn wall_clock_ms() -> i64 {
    comandos_runtime::now_ms().map_or(0, |ms| i64::try_from(ms).unwrap_or(i64::MAX))
}

/// Segundos Unix con la precisión de `time.time()`; las pruebas lo sustituyen.
pub type SecondsClock = Arc<dyn Fn() -> f64 + Send + Sync>;

/// `time.time()` del Python: los nanosegundos de `CLOCK_REALTIME` (el de
/// `SystemTime`) convertidos como `_PyTime_AsSecondsDouble`.
pub fn python_time() -> f64 {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_nanos()).unwrap_or(i64::MAX));
    python_seconds(nanos)
}

/// `_PyTime_AsSecondsDouble`: segundos exactos si no hay fracción; si no,
/// `(double)ns / 1e9`. Mismo `double`, bit a bit, que escribe el Python.
pub fn python_seconds(nanos: i64) -> f64 {
    const NS: i64 = 1_000_000_000;
    if nanos % NS == 0 {
        (nanos / NS) as f64
    } else {
        nanos as f64 / 1e9
    }
}

/// `"desktop-" + re.sub(r"[^A-Za-z0-9_.-]", "-", os.uname().nodename or "local")[:60]`.
/// `/proc/sys/kernel/hostname` es el `nodename` de `uname`.
pub fn desktop_device() -> String {
    let node = std::fs::read_to_string("/proc/sys/kernel/hostname")
        .map(|s| s.trim_end_matches('\n').to_owned())
        .unwrap_or_default();
    let node = if node.is_empty() {
        "local".to_owned()
    } else {
        node
    };
    let clean: String = node
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-') {
                c
            } else {
                '-'
            }
        })
        .take(60)
        .collect();
    format!("desktop-{clean}")
}

#[derive(Clone)]
pub struct NativeOptions {
    pub state_db: PathBuf,
    /// `~/.claude/hooks` (el `HOOKS` del Python).
    pub hooks: PathBuf,
    pub clock: Clock,
    /// `time.time()` de las escrituras del journal (`recover_abandoned`).
    pub clock_seconds: SecondsClock,
    /// `tmux` como lo llama el Python (entorno heredado, plazo 5 s).
    pub tmux: tmux::Tmux,
    /// `fc-list` de `_installed_font_families` (7603).
    pub fc_list: tmux::Program,
    /// `~/.claude/hooks/comandos-usage.sqlite`.
    pub usage_db: PathBuf,
    /// `~/.claude/hooks/session-operations.sqlite3` (journal de operaciones).
    pub journal_db: PathBuf,
    /// `DESKTOP_DEVICE` del Python (999).
    pub desktop_device: String,
    /// Checkout del heredado (`REPO_ROOT`): de él sale `config/model-tiers.json`.
    pub repo_root: Option<PathBuf>,
    /// `HOME` del frente (`os.path.expanduser("~")` del Python).
    pub home: PathBuf,
    /// Raíz de `/proc` (las pruebas de capa usan una falsa; el frente, la real).
    pub proc_root: PathBuf,
    /// Heredado para el contexto de sugerencias (D1, D11). `build` lo fija.
    pub legacy: SocketAddr,
    pub legacy_token: Vec<u8>,
    /// `PATH` del frente al arrancar, para `providers::which` (D9).
    pub search_path: Option<OsString>,
    /// Directorio de trabajo del frente (rutas relativas del catálogo y cuentas).
    pub cwd: PathBuf,
    /// `CODEX_HOME` / `GROK_HOME` del frente (catálogos de modelos).
    pub codex_home: Option<PathBuf>,
    pub grok_home: Option<PathBuf>,
    /// `ssh` de `ssh_state` (7708).
    pub ssh: tmux::Program,
    /// `systemd-run --user --scope --collect --quiet` de `scope_cmd` (5420) para
    /// lanzar la terminal rápida; `None` (sin `systemd-run` en el `PATH`) declina
    /// POST `/terminal/quick`.
    pub scope: Option<tmux::Program>,
    /// `quick_terminal_lib.default_base()` (carpetas de la terminal rápida).
    pub quick_base: PathBuf,
}

impl NativeOptions {
    pub fn for_home(home: &Path, state_db: PathBuf) -> Self {
        Self {
            state_db,
            hooks: home.join(".claude/hooks"),
            clock: Arc::new(wall_clock_ms),
            clock_seconds: Arc::new(python_time),
            tmux: tmux::Tmux::system(),
            fc_list: tmux::Program::named("fc-list"),
            usage_db: home.join(".claude/hooks/comandos-usage.sqlite"),
            journal_db: home.join(".claude/hooks/session-operations.sqlite3"),
            desktop_device: desktop_device(),
            repo_root: None,
            home: home.to_path_buf(),
            proc_root: PathBuf::from("/proc"),
            legacy: SocketAddr::from((Ipv4Addr::LOCALHOST, crate::dash::DEFAULT_LEGACY_PORT)),
            legacy_token: Vec::new(),
            search_path: std::env::var_os("PATH"),
            cwd: std::env::current_dir().unwrap_or_else(|_| home.to_path_buf()),
            codex_home: env_path("CODEX_HOME"),
            grok_home: env_path("GROK_HOME"),
            ssh: tmux::Program::named("ssh"),
            scope: quick::find_scope(std::env::var_os("PATH").as_deref()),
            quick_base: quick::default_base(home),
        }
    }
}

/// Variable de entorno no vacía como ruta.
fn env_path(name: &str) -> Option<PathBuf> {
    std::env::var_os(name)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
}

pub struct Native {
    opts: NativeOptions,
    enabled: AtomicBool,
    refusals: AtomicUsize,
    state: OnceCell<Option<BackendCaller<StateBackend>>>,
    worker: Mutex<Option<BackendWorker<StateBackend>>>,
    /// `_PREFS_LOCK` del Python.
    pub(crate) prefs_lock: tokio::sync::Mutex<()>,
    /// `_FONT_CACHE`: familias de `fc-list` y cuándo se leyeron.
    pub(crate) fonts: Mutex<Option<(std::time::Instant, std::collections::HashSet<String>)>>,
    /// La revisión de avisos que comparten las esperas de `/notices/watch`.
    pub(crate) notice_feed: notices::RevisionFeed,
    /// Carril de la base de uso (`GET /pomodoro`, `GET /sovereignty`).
    pub(crate) usage: lanes::Lane<lanes::UsageBackend>,
    /// Carril del journal de operaciones (`GET /model/status`).
    pub(crate) journal: lanes::Lane<lanes::JournalBackend>,
    /// Caché por `requestId` y candados de `POST /pane/type`.
    pub(crate) typing: Arc<typing::TypingState>,
    /// GET `/state`: caché de 1,2 s, vuelo único y cachés de los lectores.
    pub(crate) states: states::Engine,
}

impl Native {
    pub fn new(opts: NativeOptions) -> Self {
        Self {
            usage: lanes::Lane::new(opts.usage_db.clone()),
            journal: lanes::Lane::new(opts.journal_db.clone()),
            opts,
            enabled: AtomicBool::new(true),
            refusals: AtomicUsize::new(0),
            state: OnceCell::new(),
            worker: Mutex::new(None),
            prefs_lock: tokio::sync::Mutex::new(()),
            fonts: Mutex::new(None),
            notice_feed: notices::RevisionFeed::default(),
            typing: Arc::default(),
            states: states::Engine::default(),
        }
    }

    pub fn options(&self) -> &NativeOptions {
        &self.opts
    }

    pub fn enabled(&self) -> bool {
        self.enabled.load(Ordering::Acquire)
    }

    /// Cuántas veces se escribió la línea de desactivación (0 o 1).
    pub fn refusals(&self) -> usize {
        self.refusals.load(Ordering::Acquire)
    }

    /// El carril de la base de uso (estado y líneas de apagado, para las pruebas).
    pub fn usage_lane(&self) -> &lanes::Lane<lanes::UsageBackend> {
        &self.usage
    }

    /// El carril del journal de operaciones (estado y líneas de apagado).
    pub fn journal_lane(&self) -> &lanes::Lane<lanes::JournalBackend> {
        &self.journal
    }

    fn disable(&self, refusal: &Refusal) {
        if self.enabled.swap(false, Ordering::AcqRel) {
            self.refusals.fetch_add(1, Ordering::AcqRel);
            eprintln!("{}", refusal.message(&self.opts.state_db));
        }
    }

    /// Abre la base una sola vez (en un hilo de bloqueo) y arranca el worker.
    /// Devuelve si el conjunto nativo está activo.
    pub async fn ready(&self) -> bool {
        let caller = self
            .state
            .get_or_init(|| async {
                let path = self.opts.state_db.clone();
                let now_seconds = (self.opts.clock)() as f64 / 1000.0;
                let opened =
                    tokio::task::spawn_blocking(move || StateBackend::open(&path, now_seconds))
                        .await;
                match opened {
                    Ok(Ok(backend)) => match BackendWorker::start(WORKER_CAPACITY, backend) {
                        Ok(worker) => {
                            let caller = worker.caller();
                            *self.worker.lock().unwrap_or_else(|p| p.into_inner()) = Some(worker);
                            Some(caller)
                        }
                        Err(error) => {
                            self.disable(&Refusal::Unopened(error.to_string()));
                            None
                        }
                    },
                    Ok(Err(refusal)) => {
                        self.disable(&refusal);
                        None
                    }
                    Err(join) => {
                        self.disable(&Refusal::Unopened(join.to_string()));
                        None
                    }
                }
            })
            .await;
        caller.is_some() && self.enabled()
    }

    /// Un trabajo sobre la base. `Err(Fault::Decline)` si el conjunto está
    /// apagado, si la puerta de esquema rechaza ahora o si el worker ya se
    /// retiró. Un esquema más nuevo o desconocido y un worker retirado lo
    /// apagan; un fallo pasajero de la puerta declina solo esta petición. Solo el trabajo que entró en
    /// pánico responde `HandlerError::Failure`; los demás se reenvían.
    pub async fn with_state<T, F>(&self, job: F) -> Result<T, Fault>
    where
        F: FnOnce(&mut StateBackend) -> T + Send + 'static,
        T: Send + 'static,
    {
        if !self.ready().await {
            return Err(Fault::Decline);
        }
        let Some(Some(caller)) = self.state.get() else {
            return Err(Fault::Decline);
        };
        if caller.stopped() {
            self.disable(&Refusal::Retired);
            return Err(Fault::Decline);
        }
        // Marca de que el trabajo empezó: si no empezó, no hubo efectos y
        // se puede declinar con certeza.
        let started = Arc::new(AtomicBool::new(false));
        let mark = started.clone();
        let called = caller
            .call(move |backend: &mut StateBackend| {
                mark.store(true, Ordering::Release);
                backend.admit().map(|()| job(backend))
            })
            .await;
        match called {
            Ok(Ok(value)) => Ok(value),
            // Solo un esquema más nuevo o desconocido apaga el conjunto.
            Ok(Err(
                refusal @ (Refusal::Newer { .. } | Refusal::Incompatible(_) | Refusal::Retired),
            )) => {
                self.disable(&refusal);
                Err(Fault::Decline)
            }
            // Un fallo al consultar las versiones (p. ej. `SQLITE_BUSY`) no
            // dice nada del esquema: se reenvía esta petición y el conjunto
            // sigue (`admit` corre antes del trabajo, que no llegó a empezar).
            Ok(Err(Refusal::Unopened(_))) => Err(Fault::Decline),
            // El trabajo corrió y no respondió: entró en pánico y el worker
            // se retira. Esta petición es la única que recibe el 500.
            Err(error) if started.load(Ordering::Acquire) => {
                self.disable(&Refusal::Retired);
                Err(Fault::Error(error))
            }
            // Nunca empezó (worker parado o retirado en la carrera): sin
            // efectos, se reenvía al heredado.
            Err(_) => {
                if caller.stopped() {
                    self.disable(&Refusal::Retired);
                }
                Err(Fault::Decline)
            }
        }
    }

    /// Con el conjunto apagado todo se reenvía, salvo el reintento de un
    /// `POST /pane/type` que el frente ya tecleó: ese sale de su caché, porque
    /// el Python no lo conoce y volvería a teclear.
    pub fn typing_retry(&self, request: &Request) -> Option<Reply> {
        if route(&request.method, &request.target) != Some(NativeRoute::PaneType) {
            return None;
        }
        typing::retry_reply(&self.typing, request)
    }

    pub async fn dispatch(
        &self,
        route: NativeRoute,
        request: &Request,
    ) -> Result<Outcome, HandlerError> {
        if !self.ready().await {
            if route == NativeRoute::PaneType
                && let Some(reply) = typing::retry_reply(&self.typing, request)
            {
                return Ok(Outcome::Reply(reply));
            }
            return Ok(Outcome::Decline);
        }
        match self.answer(route, request).await {
            Ok(reply) => Ok(Outcome::Reply(reply)),
            Err(Fault::Decline) => Ok(Outcome::Decline),
            Err(Fault::Error(error)) => Err(error),
        }
    }

    async fn answer(&self, route: NativeRoute, request: &Request) -> Answer {
        match route {
            NativeRoute::Light(route) => light::answer(self, route, request).await,
            NativeRoute::Events => events::answer(self, request).await,
            NativeRoute::Notices(route) => notices::answer(self, route, request).await,
            NativeRoute::Workspace(route) => workspace::answer(self, route, request).await,
            NativeRoute::Snippets(route) => snippets::answer(self, route, request).await,
            NativeRoute::UiLog => ui_log::answer(self, request).await,
            NativeRoute::Pomodoro => pomodoro::answer(self).await,
            NativeRoute::Catalog(route) => catalogs::answer(self, route).await,
            NativeRoute::Terminal(route) => terminal::answer(self, route, request).await,
            NativeRoute::PaneType => typing::answer(self, request).await,
            NativeRoute::ModelStatus => operations::answer(self, request).await,
            NativeRoute::State => states::answer(self).await,
            NativeRoute::QuickTerminal => quick::answer(self, request).await,
            NativeRoute::Retired => {
                let path = request
                    .target
                    .split_once('?')
                    .map_or(request.target.as_str(), |(path, _)| path);
                retired::answer(&request.method, path)
            }
        }
    }

    /// GET `/state` con la caché del Python (`read_states_cached`); también la
    /// usará `/workspace/sort` en modo `by`.
    pub async fn states_cached(&self) -> Result<Arc<states::States>, Fault> {
        let clock = self.opts.clock.clone();
        self.states
            .cache
            .get(&*clock, || states::gather::compute(self))
            .await
            .map_err(Fault::from)
    }

    /// Para el worker (si arrancó) y espera a que termine su trabajo en curso.
    pub async fn shutdown(&self) {
        // Apagado ordenado: sin línea en stderr; lo que llegue tarde se reenvía.
        self.enabled.store(false, Ordering::Release);
        let worker = self.worker.lock().unwrap_or_else(|p| p.into_inner()).take();
        if let Some(worker) = worker {
            let _ = worker.shutdown().await;
        }
        self.usage.shutdown().await;
        self.journal.shutdown().await;
    }
}
