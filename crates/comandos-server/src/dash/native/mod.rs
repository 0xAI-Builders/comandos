//! Rutas que el frente responde sin el Python (Fase 2b).
//!
//! Cada ruta reproduce byte a byte una rama de `bin/cc-dash`. Lo que toca la
//! base pasa por un único `BackendWorker<StateBackend>`; lo que toca tmux o
//! `fc-list`, por `tokio::process`. Si una entrada no se puede reproducir con
//! certeza, el manejador devuelve `Fault::Decline` ANTES de cualquier efecto
//! y el frente reenvía la petición original al heredado.
pub mod files;
pub mod py;
pub mod query;
pub mod state;
pub mod tmux;

use crate::{
    HandlerError, Reply, Request,
    blocking::{BackendCaller, BackendWorker},
    dash::router::path_of,
};
use http::Method;
use state::{Refusal, StateBackend};
use std::{
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
pub enum NativeRoute {}

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
const TABLES: &[&[Entry]] = &[];

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

/// Milisegundos Unix; las pruebas lo sustituyen.
pub type Clock = Arc<dyn Fn() -> i64 + Send + Sync>;

pub fn wall_clock_ms() -> i64 {
    comandos_runtime::now_ms().map_or(0, |ms| i64::try_from(ms).unwrap_or(i64::MAX))
}

#[derive(Clone)]
pub struct NativeOptions {
    pub state_db: PathBuf,
    /// `~/.claude/hooks` (el `HOOKS` del Python).
    pub hooks: PathBuf,
    pub clock: Clock,
    /// `tmux` como lo llama el Python (entorno heredado, plazo 5 s).
    pub tmux: tmux::Tmux,
    /// `fc-list` de `_installed_font_families` (7603).
    pub fc_list: tmux::Program,
}

impl NativeOptions {
    pub fn for_home(home: &Path, state_db: PathBuf) -> Self {
        Self {
            state_db,
            hooks: home.join(".claude/hooks"),
            clock: Arc::new(wall_clock_ms),
            tmux: tmux::Tmux::system(),
            fc_list: tmux::Program::named("fc-list"),
        }
    }
}

pub struct Native {
    opts: NativeOptions,
    enabled: AtomicBool,
    refusals: AtomicUsize,
    state: OnceCell<Option<BackendCaller<StateBackend>>>,
    worker: Mutex<Option<BackendWorker<StateBackend>>>,
}

impl Native {
    pub fn new(opts: NativeOptions) -> Self {
        Self {
            opts,
            enabled: AtomicBool::new(true),
            refusals: AtomicUsize::new(0),
            state: OnceCell::new(),
            worker: Mutex::new(None),
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
    /// retiró (y en esos dos casos lo apaga). Solo el trabajo que entró en
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
            Ok(Err(refusal)) => {
                self.disable(&refusal);
                Err(Fault::Decline)
            }
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

    pub async fn dispatch(
        &self,
        route: NativeRoute,
        request: &Request,
    ) -> Result<Outcome, HandlerError> {
        if !self.ready().await {
            return Ok(Outcome::Decline);
        }
        match self.answer(route, request).await {
            Ok(reply) => Ok(Outcome::Reply(reply)),
            Err(Fault::Decline) => Ok(Outcome::Decline),
            Err(Fault::Error(error)) => Err(error),
        }
    }

    async fn answer(&self, route: NativeRoute, _request: &Request) -> Answer {
        match route {}
    }

    /// Para el worker (si arrancó) y espera a que termine su trabajo en curso.
    pub async fn shutdown(&self) {
        // Apagado ordenado: sin línea en stderr; lo que llegue tarde se reenvía.
        self.enabled.store(false, Ordering::Release);
        let worker = self.worker.lock().unwrap_or_else(|p| p.into_inner()).take();
        if let Some(worker) = worker {
            let _ = worker.shutdown().await;
        }
    }
}
