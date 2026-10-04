//! Carriles de base adicionales: un `BackendWorker` por archivo SQLite que no
//! es `app-state` (la base de uso; el journal de operaciones en la Tarea 6).
//! Misma semántica que `Native::with_state`, pero el apagado es del carril:
//! una línea en stderr y solo las rutas que lo usan pasan a reenviarse.
use super::{Fault, WORKER_CAPACITY, state::Refusal};
use crate::blocking::{BackendCaller, BackendWorker};
use comandos_store::usage;
use rusqlite::Connection;
use std::{
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
};
use tokio::sync::OnceCell;

pub trait LaneBackend: Sized + Send + 'static {
    /// Rutas que dependen del carril, para la línea de stderr.
    const ROUTES: &'static str;
    /// Abre y migra; la puerta va ANTES de tocar una base más nueva.
    fn open(path: &Path) -> Result<Self, Refusal>;
    /// Se evalúa antes de cada trabajo.
    fn admit(&self) -> Result<(), Refusal>;
}

pub struct Lane<B: LaneBackend> {
    path: PathBuf,
    enabled: AtomicBool,
    refusals: AtomicUsize,
    caller: OnceCell<Option<BackendCaller<B>>>,
    worker: Mutex<Option<BackendWorker<B>>>,
}

impl<B: LaneBackend> Lane<B> {
    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            enabled: AtomicBool::new(true),
            refusals: AtomicUsize::new(0),
            caller: OnceCell::new(),
            worker: Mutex::new(None),
        }
    }

    pub fn enabled(&self) -> bool {
        self.enabled.load(Ordering::Acquire)
    }

    /// Cuántas veces se escribió la línea de apagado (0 o 1).
    pub fn refusals(&self) -> usize {
        self.refusals.load(Ordering::Acquire)
    }

    fn disable(&self, refusal: &Refusal) {
        if self.enabled.swap(false, Ordering::AcqRel) {
            self.refusals.fetch_add(1, Ordering::AcqRel);
            eprintln!("{}", refusal.lane_message(&self.path, B::ROUTES));
        }
    }

    /// Abre la base una sola vez (en un hilo de bloqueo) y arranca el worker;
    /// un carril apagado nunca se vuelve a abrir.
    async fn caller(&self) -> Option<&BackendCaller<B>> {
        let caller = self
            .caller
            .get_or_init(|| async {
                let path = self.path.clone();
                match tokio::task::spawn_blocking(move || B::open(&path)).await {
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
        caller.as_ref().filter(|_| self.enabled())
    }

    /// Un trabajo sobre la base del carril; `Err(Fault::Decline)` si el carril
    /// está apagado, si la puerta rechaza ahora o si el worker se retiró sin
    /// empezar el trabajo. Solo el trabajo que entró en pánico responde 500.
    pub async fn with<T, F>(&self, job: F) -> Result<T, Fault>
    where
        F: FnOnce(&mut B) -> T + Send + 'static,
        T: Send + 'static,
    {
        if !self.enabled() {
            return Err(Fault::Decline);
        }
        let Some(caller) = self.caller().await else {
            return Err(Fault::Decline);
        };
        if caller.stopped() {
            self.disable(&Refusal::Retired);
            return Err(Fault::Decline);
        }
        // Marca de que el trabajo empezó: si no empezó, no hubo efectos.
        let started = Arc::new(AtomicBool::new(false));
        let mark = started.clone();
        let called = caller
            .call(move |backend: &mut B| {
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
            Err(error) if started.load(Ordering::Acquire) => {
                self.disable(&Refusal::Retired);
                Err(Fault::Error(error))
            }
            Err(_) => {
                if caller.stopped() {
                    self.disable(&Refusal::Retired);
                }
                Err(Fault::Decline)
            }
        }
    }

    /// Apagado ordenado: sin línea en stderr; lo que llegue tarde se reenvía.
    pub async fn shutdown(&self) {
        self.enabled.store(false, Ordering::Release);
        let worker = self.worker.lock().unwrap_or_else(|p| p.into_inner()).take();
        if let Some(worker) = worker {
            let _ = worker.shutdown().await;
        }
    }
}

/// `~/.claude/hooks/comandos-usage.sqlite` (`USAGE_DB` de `bin/cc-dash:172`,
/// que no honra `COMANDOS_USAGE_DB`). Puerta: `user_version` <= 11.
pub struct UsageBackend {
    pub conn: Connection,
}

impl LaneBackend for UsageBackend {
    const ROUTES: &'static str = "GET /pomodoro y GET /sovereignty";

    fn open(path: &Path) -> Result<Self, Refusal> {
        // Sondeo sin PRAGMAs: `open_usage_db_at` pide `journal_mode=wal`, y una
        // base más nueva no se toca ni para eso.
        if path.exists() {
            let probe = Connection::open_with_flags(
                path,
                rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY
                    | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
            )
            .map_err(|e| Refusal::Unopened(e.to_string()))?;
            gate(&probe)?;
        }
        let conn = usage::open_usage_db_at(path).map_err(|e| Refusal::Unopened(e.to_string()))?;
        let backend = Self { conn };
        backend.admit()?;
        usage::ensure_schema(&backend.conn).map_err(|e| Refusal::Unopened(e.to_string()))?;
        Ok(backend)
    }

    fn admit(&self) -> Result<(), Refusal> {
        gate(&self.conn)
    }
}

/// `user_version` mayor que el que conoce este binario: se rechaza sin migrar.
fn gate(conn: &Connection) -> Result<(), Refusal> {
    let found = usage::schema_version(conn).map_err(|e| Refusal::Unopened(e.to_string()))?;
    if found > usage::SCHEMA_VERSION {
        return Err(Refusal::Newer {
            found,
            known: usage::SCHEMA_VERSION,
        });
    }
    Ok(())
}
