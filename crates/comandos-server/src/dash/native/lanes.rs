//! Carriles de base adicionales: un `BackendWorker` por archivo SQLite que no
//! es `app-state` (la base de uso y el journal de operaciones de sesión).
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
    /// Se evalúa antes de cada trabajo. Solo `Refusal::Newer` e
    /// `Refusal::Incompatible` apagan el carril; cualquier otro rechazo
    /// declina esa petición.
    fn admit(&self) -> Result<(), Refusal>;
}

/// Espera antes de reintentar un trabajo que declinó por un fallo pasajero
/// de la puerta (`with_retry`).
pub const TRANSIENT_RETRY: std::time::Duration = std::time::Duration::from_millis(250);

pub struct Lane<B: LaneBackend> {
    path: PathBuf,
    enabled: AtomicBool,
    refusals: AtomicUsize,
    transients: AtomicUsize,
    caller: OnceCell<BackendCaller<B>>,
    worker: Mutex<Option<BackendWorker<B>>>,
}

impl<B: LaneBackend> Lane<B> {
    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            enabled: AtomicBool::new(true),
            refusals: AtomicUsize::new(0),
            transients: AtomicUsize::new(0),
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

    /// Abre la base (en un hilo de bloqueo) y arranca el worker. Una vez
    /// abierta no se reabre nunca. Un esquema más nuevo o desconocido, o un
    /// pánico al abrir, apagan el carril para siempre; cualquier otro fallo
    /// (p. ej. `SQLITE_BUSY` con el Python escribiendo) declina solo esta
    /// petición y la celda queda vacía para reintentar en la siguiente.
    async fn caller(&self) -> Option<&BackendCaller<B>> {
        let opened = self
            .caller
            .get_or_try_init(|| async {
                let path = self.path.clone();
                match tokio::task::spawn_blocking(move || B::open(&path)).await {
                    Ok(Ok(backend)) => match BackendWorker::start(WORKER_CAPACITY, backend) {
                        Ok(worker) => {
                            let caller = worker.caller();
                            *self.worker.lock().unwrap_or_else(|p| p.into_inner()) = Some(worker);
                            Ok(caller)
                        }
                        // Sin hilo para el worker: pasajero, se reintenta.
                        Err(error) => {
                            self.transient(&error.to_string());
                            Err(())
                        }
                    },
                    Ok(Err(refusal @ (Refusal::Newer { .. } | Refusal::Incompatible(_)))) => {
                        self.disable(&refusal);
                        Err(())
                    }
                    Ok(Err(Refusal::Unopened(error))) => {
                        self.transient(&error);
                        Err(())
                    }
                    Ok(Err(refusal @ Refusal::Retired)) => {
                        self.disable(&refusal);
                        Err(())
                    }
                    // `open` entró en pánico: no se vuelve a intentar.
                    Err(join) => {
                        self.disable(&Refusal::Unopened(join.to_string()));
                        Err(())
                    }
                }
            })
            .await;
        opened.ok().filter(|_| self.enabled())
    }

    /// Un fallo pasajero al abrir: una línea en stderr la primera vez (sin
    /// contar como apagado, porque el carril sigue) y esta petición se reenvía.
    fn transient(&self, error: &str) {
        if self.transients.fetch_add(1, Ordering::AcqRel) == 0 {
            eprintln!(
                "comandos dash: {}: apertura fallida ({error}); se reenvía esta petición \
                 de {} y se reintenta en la siguiente",
                self.path.display(),
                B::ROUTES
            );
        }
    }

    /// Cuántos fallos pasajeros de apertura hubo (para las pruebas).
    pub fn transients(&self) -> usize {
        self.transients.load(Ordering::Acquire)
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
            // Solo un esquema más nuevo o desconocido apaga el carril para siempre.
            Ok(Err(refusal @ (Refusal::Newer { .. } | Refusal::Incompatible(_)))) => {
                self.disable(&refusal);
                Err(Fault::Decline)
            }
            // Un fallo al consultar la versión (p. ej. la base ocupada) no
            // dice nada del esquema: se reenvía esta petición y el carril sigue.
            Ok(Err(_)) => Err(Fault::Decline),
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

    /// Como `with`, para los trabajos que van después del punto de declinar
    /// (D1 de GET `/usage/state`): un `Decline` con el carril aún encendido es
    /// pasajero (la puerta dio `SQLITE_BUSY`, el trabajo no empezó y no dejó
    /// efectos) y se reintenta una vez pasados `TRANSIENT_RETRY`. Lo que
    /// devuelva el reintento (o el primer intento con el carril apagado) es la
    /// respuesta: el que llama convierte ese `Decline` en 500.
    pub async fn with_retry<T, F>(&self, job: F) -> Result<T, Fault>
    where
        F: FnOnce(&mut B) -> T + Clone + Send + 'static,
        T: Send + 'static,
    {
        match self.with(job.clone()).await {
            Err(Fault::Decline) if self.enabled() => {
                tokio::time::sleep(TRANSIENT_RETRY).await;
                self.with(job).await
            }
            other => other,
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
    const ROUTES: &'static str = "GET /pomodoro, GET /sovereignty, GET /state, GET /analytics/week, GET /accounts, GET /extension-usage y GET /usage/state";

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
            // Como `sqlite3.connect(db_path, timeout=10)` (`bin/cc_usage.py:38`):
            // un `SQLITE_BUSY` pasajero al primer abrir espera antes de fallar.
            probe
                .busy_timeout(std::time::Duration::from_secs(10))
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

/// La misma base en un segundo carril para la importación de uso de GET
/// `/usage/state` (D2): escribe durante segundos y en el carril de uso pararía
/// las rutas que leen. Misma puerta y misma apertura que `UsageBackend`; la base
/// está en WAL y las lecturas del otro carril no esperan.
pub struct UsageImportBackend {
    pub conn: Connection,
}

impl UsageImportBackend {
    /// La puerta de esquema, para comprobarla antes de cada lote de escritura
    /// dentro de una importación larga.
    pub fn admits(conn: &Connection) -> bool {
        gate(conn).is_ok()
    }
}

impl LaneBackend for UsageImportBackend {
    const ROUTES: &'static str = "GET /usage/state y su importación de uso";

    fn open(path: &Path) -> Result<Self, Refusal> {
        UsageBackend::open(path).map(|b| Self { conn: b.conn })
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

/// `~/.claude/hooks/session-operations.sqlite3` (`session_operation_store`,
/// `bin/cc-dash:2132`). El Python no versiona su esquema: la puerta exige las
/// 9 columnas de `session_operations` en su orden (`lib/session_operations.py:20`).
pub struct JournalBackend {
    pub conn: Connection,
}

const JOURNAL_COLUMNS: [&str; 9] = [
    "id",
    "pane_key",
    "fingerprint",
    "request",
    "state",
    "owner",
    "snapshot",
    "result",
    "updated",
];

impl LaneBackend for JournalBackend {
    const ROUTES: &'static str = "GET /model/status";

    fn open(path: &Path) -> Result<Self, Refusal> {
        // Sondeo de solo lectura: `open_journal` crea tablas e índices con
        // `IF NOT EXISTS`, y una tabla desconocida no se toca ni para eso.
        if path.exists() {
            let probe = Connection::open_with_flags(
                path,
                rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY
                    | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
            )
            .map_err(|e| Refusal::Unopened(e.to_string()))?;
            // Como `sqlite3.connect(…, timeout=15)` (`lib/session_operations.py:39`):
            // un `SQLITE_BUSY` pasajero al primer abrir no retira el carril.
            probe
                .busy_timeout(std::time::Duration::from_secs(15))
                .map_err(|e| Refusal::Unopened(e.to_string()))?;
            let names = journal_columns(&probe)?;
            // Sin tabla (archivo vacío o recién creado): la crea `open_journal`.
            if !names.is_empty() {
                known_journal(&names)?;
            }
        }
        let conn = comandos_runtime::session_operations::open_journal(path)
            .map_err(|e| Refusal::Unopened(e.to_string()))?;
        let backend = Self { conn };
        backend.admit()?;
        Ok(backend)
    }

    fn admit(&self) -> Result<(), Refusal> {
        known_journal(&journal_columns(&self.conn)?)
    }
}

/// Nombres de columna de `session_operations` (vacío si la tabla no existe).
fn journal_columns(conn: &Connection) -> Result<Vec<String>, Refusal> {
    conn.prepare("PRAGMA table_info(session_operations)")
        .and_then(|mut stmt| {
            stmt.query_map([], |r| r.get::<_, String>(1))?
                .collect::<rusqlite::Result<Vec<_>>>()
        })
        .map_err(|e| Refusal::Unopened(e.to_string()))
}

/// Otras columnas (un Python más nuevo o una base ajena): se rechaza para siempre.
fn known_journal(names: &[String]) -> Result<(), Refusal> {
    if names.iter().map(String::as_str).eq(JOURNAL_COLUMNS) {
        Ok(())
    } else {
        Err(Refusal::Incompatible(format!(
            "columnas de session_operations: {}",
            names.join(",")
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize as Counter;

    /// Doble de base: la puerta responde lo que diga `script`, en orden.
    struct Scripted {
        calls: Arc<Counter>,
        script: Vec<Result<(), Refusal>>,
    }

    static CALLS: std::sync::OnceLock<Arc<Counter>> = std::sync::OnceLock::new();

    impl LaneBackend for Scripted {
        const ROUTES: &'static str = "GET /prueba";

        fn open(_: &Path) -> Result<Self, Refusal> {
            Ok(Self {
                calls: CALLS.get_or_init(Arc::default).clone(),
                script: vec![
                    Err(Refusal::Unopened("database is locked".into())),
                    Ok(()),
                    Err(Refusal::Newer {
                        found: 12,
                        known: 11,
                    }),
                ],
            })
        }

        fn admit(&self) -> Result<(), Refusal> {
            let at = self.calls.fetch_add(1, Ordering::AcqRel);
            self.script.get(at).cloned().unwrap_or(Ok(()))
        }
    }

    #[tokio::test]
    async fn transient_gate_error_declines_once_and_newer_disables() {
        let lane = Lane::<Scripted>::new(PathBuf::from("/no-existe/prueba.sqlite"));
        assert!(matches!(lane.with(|_| 1).await, Err(Fault::Decline)));
        assert!(lane.enabled(), "un fallo pasajero no apaga el carril");
        assert_eq!(lane.refusals(), 0);
        assert!(matches!(lane.with(|_| 2).await, Ok(2)));
        assert!(matches!(lane.with(|_| 3).await, Err(Fault::Decline)));
        assert!(!lane.enabled(), "una base más nueva sí lo apaga");
        assert_eq!(lane.refusals(), 1);
        assert!(matches!(lane.with(|_| 4).await, Err(Fault::Decline)));
        assert_eq!(lane.refusals(), 1);
        lane.shutdown().await;
    }

    /// Doble de puerta para `with_retry`: el guion de `admit`, en orden.
    struct Busy {
        calls: Arc<Counter>,
        script: Vec<Result<(), Refusal>>,
    }

    static BUSY_SCRIPT: Mutex<Vec<Result<(), Refusal>>> = Mutex::new(Vec::new());
    static BUSY_CALLS: std::sync::OnceLock<Arc<Counter>> = std::sync::OnceLock::new();

    impl LaneBackend for Busy {
        const ROUTES: &'static str = "GET /prueba";

        fn open(_: &Path) -> Result<Self, Refusal> {
            Ok(Self {
                calls: BUSY_CALLS.get_or_init(Arc::default).clone(),
                script: BUSY_SCRIPT
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .clone(),
            })
        }

        fn admit(&self) -> Result<(), Refusal> {
            let at = self.calls.fetch_add(1, Ordering::AcqRel);
            self.script.get(at).cloned().unwrap_or(Ok(()))
        }
    }

    #[tokio::test]
    async fn with_retry_retries_a_transient_decline_once() {
        let busy = || Err(Refusal::Unopened("database is locked".into()));
        *BUSY_SCRIPT.lock().unwrap() = vec![busy(), Ok(()), busy(), busy()];
        let lane = Lane::<Busy>::new(PathBuf::from("/no-existe/reintento.sqlite"));
        let runs = Arc::new(Counter::new(0));
        let job = {
            let runs = runs.clone();
            move |_: &mut Busy| runs.fetch_add(1, Ordering::AcqRel) + 1
        };
        // Un `SQLITE_BUSY` en la puerta: el reintento corre el trabajo una vez.
        let started = std::time::Instant::now();
        assert!(matches!(lane.with_retry(job.clone()).await, Ok(1)));
        assert!(started.elapsed() >= TRANSIENT_RETRY);
        // Dos seguidos: el segundo es la respuesta (`Decline`) y no hubo efectos.
        assert!(matches!(
            lane.with_retry(job.clone()).await,
            Err(Fault::Decline)
        ));
        assert_eq!(runs.load(Ordering::Acquire), 1);
        assert!(lane.enabled(), "un fallo pasajero no apaga el carril");
        // Carril apagado: sin reintento ni espera.
        lane.shutdown().await;
        let started = std::time::Instant::now();
        assert!(matches!(lane.with_retry(job).await, Err(Fault::Decline)));
        assert!(started.elapsed() < TRANSIENT_RETRY);
    }

    /// Doble de apertura: la primera falla como `SQLITE_BUSY`, las demás abren.
    struct Flaky;

    static FLAKY_OPENS: Counter = Counter::new(0);

    impl LaneBackend for Flaky {
        const ROUTES: &'static str = "GET /prueba";

        fn open(_: &Path) -> Result<Self, Refusal> {
            match FLAKY_OPENS.fetch_add(1, Ordering::AcqRel) {
                0 => Err(Refusal::Unopened("database is locked".into())),
                _ => Ok(Self),
            }
        }

        fn admit(&self) -> Result<(), Refusal> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn transient_open_error_declines_once_and_retries() {
        let lane = Lane::<Flaky>::new(PathBuf::from("/no-existe/flaky.sqlite"));
        assert!(matches!(lane.with(|_| 1).await, Err(Fault::Decline)));
        assert!(
            lane.enabled(),
            "un fallo pasajero al abrir no apaga el carril"
        );
        assert_eq!(lane.refusals(), 0);
        assert_eq!(lane.transients(), 1);
        assert!(matches!(lane.with(|_| 2).await, Ok(2)));
        assert!(matches!(lane.with(|_| 3).await, Ok(3)));
        assert_eq!(
            FLAKY_OPENS.load(Ordering::Acquire),
            2,
            "abierta una sola vez"
        );
        lane.shutdown().await;
    }

    /// Doble de apertura: siempre una base más nueva.
    struct Ahead;

    static AHEAD_OPENS: Counter = Counter::new(0);

    impl LaneBackend for Ahead {
        const ROUTES: &'static str = "GET /prueba";

        fn open(_: &Path) -> Result<Self, Refusal> {
            AHEAD_OPENS.fetch_add(1, Ordering::AcqRel);
            Err(Refusal::Newer {
                found: 12,
                known: 11,
            })
        }

        fn admit(&self) -> Result<(), Refusal> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn newer_schema_at_open_disables_without_reopening() {
        let lane = Lane::<Ahead>::new(PathBuf::from("/no-existe/future.sqlite"));
        for n in 0..3 {
            assert!(matches!(lane.with(move |_| n).await, Err(Fault::Decline)));
        }
        assert!(!lane.enabled());
        assert_eq!(lane.refusals(), 1);
        assert_eq!(lane.transients(), 0);
        assert_eq!(AHEAD_OPENS.load(Ordering::Acquire), 1, "nunca se reabre");
    }
}
