//! Dueño de la importación de uso (D1): `refresh_local_usage(False)`
//! (bin/cc-dash:235) y `_do_refresh_local_usage` (347). Intervalo de 60 s,
//! gracia al arrancar, `flock` entre frentes y generación del memo de GET
//! `/usage/state` al terminar. La importación entera corre en el hilo del carril
//! de importación (D2); `git rev-parse` sale por el puente de la 2c
//! (`Handle::block_on`) desde ese hilo, nunca en el runtime.
use super::super::{
    NativeOptions,
    files::FileLock,
    lanes::{Lane, UsageImportBackend},
    py,
};
use super::state::{self, ConfFile, UsageEngine};
use comandos_core::{json::truthy, usage_state::LocalZone};
use comandos_store::{
    usage_import::{self, GitRoots, ImportError, ImportPlan, ImportSeen},
    usage_read,
};
use rusqlite::Connection;
use serde_json::{Map, Value};
use std::{
    cell::RefCell,
    collections::{BTreeMap, HashMap},
    path::{Path, PathBuf},
    sync::{Arc, Mutex, MutexGuard},
};

/// `now - LOCAL_USAGE_REFRESH_AT < 60`.
pub const IMPORT_INTERVAL_S: i64 = 60;
/// `max_age_days=14` de `reconcile_orphan_interactions`.
const RECONCILE_DAYS: i64 = 14;
/// `int(env.get("COMANDOS_USAGE_LOCAL_DAYS") or 21)`.
const LOCAL_DAYS: i64 = 21;

pub struct ImportOwner {
    started_ms: i64,
    state: Mutex<ImportState>,
}

#[derive(Default)]
struct ImportState {
    /// `LOCAL_USAGE_REFRESH_AT`.
    last_at: i64,
    running: bool,
    /// El `_IMPORT_SEEN` del Python, acotado al corte de cada fuente.
    seen: Option<ImportSeen>,
}

/// Lo que la tarea de importación necesita, sin `&Native` (D13).
#[derive(Clone)]
pub struct ImportDeps {
    pub opts: NativeOptions,
    pub import_lane: Arc<Lane<UsageImportBackend>>,
    pub engine: Arc<UsageEngine>,
}

impl ImportOwner {
    pub fn new(started_ms: i64) -> Self {
        Self {
            started_ms,
            state: Mutex::new(ImportState::default()),
        }
    }

    fn lock(&self) -> MutexGuard<'_, ImportState> {
        self.state.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Si hay una importación en vuelo (para las pruebas).
    pub fn running(&self) -> bool {
        self.lock().running
    }

    /// Rutas recordadas por el `seen` (para la cota).
    pub fn seen_len(&self) -> usize {
        self.lock().seen.as_ref().map_or(0, ImportSeen::len)
    }

    /// `refresh_local_usage(False)`: decide y lanza; nunca espera. Con el carril
    /// de uso o el de importación apagados el Python es dueño de todo (D1, D10):
    /// `usage_state` reenvía la vuelta antes de llegar aquí, y esta guarda es
    /// la segunda línea. Sin efectos de uso (sombra) no importa. `cards` son las tarjetas de
    /// `/state` (`None` si declinó: se omite `ensure_observed_configs`).
    pub fn maybe_start(
        self: &Arc<Self>,
        deps: ImportDeps,
        usage_enabled: bool,
        cards: Option<Arc<Vec<Value>>>,
    ) {
        let opts = &deps.opts;
        if !opts.usage_effects || !usage_enabled || !deps.import_lane.enabled() {
            return;
        }
        let now_ms = (opts.clock)();
        let now = now_ms.div_euclid(1000);
        if now_ms.saturating_sub(self.started_ms) < opts.usage_import_grace_ms {
            return;
        }
        let seen = {
            let mut st = self.lock();
            if now.saturating_sub(st.last_at) < IMPORT_INTERVAL_S || st.running {
                return;
            }
            st.running = true;
            st.last_at = now;
            st.seen.take().unwrap_or_default()
        };
        tokio::spawn(run(self.clone(), deps, cards, seen, now));
    }
}

/// `finally: _local_usage_refreshing = False`, también si la tarea se cancela.
struct Running(Arc<ImportOwner>);

impl Drop for Running {
    fn drop(&mut self) {
        self.0.lock().running = false;
    }
}

async fn run(
    owner: Arc<ImportOwner>,
    deps: ImportDeps,
    cards: Option<Arc<Vec<Value>>>,
    seen: ImportSeen,
    now: i64,
) {
    let _running = Running(owner.clone());
    // Dos frentes sobre el mismo HOME: el que no tiene el candado salta la vuelta.
    let lock_path = deps.opts.hooks.join("comandos-usage-import.lock");
    let lock = match tokio::task::spawn_blocking(move || FileLock::try_acquire(&lock_path)).await {
        Ok(Ok(Some(lock))) => lock,
        _ => {
            owner.lock().seen = Some(seen);
            return;
        }
    };
    let cycle = Cycle {
        handle: tokio::runtime::Handle::current(),
        home: deps.opts.home.clone(),
        hooks: deps.opts.hooks.clone(),
        usage_env: deps.opts.usage_env.clone(),
        zone: deps.opts.zone.clone(),
        lane: deps.import_lane.clone(),
    };
    // Si el frente rehúsa la vuelta (carril apagado o esquema que no admite),
    // lo leído no se escribió: vuelve el `seen` de antes para releerlo.
    let before = seen.clone();
    let outcome = deps
        .import_lane
        .with(move |b| {
            let mut seen = seen;
            let result = cycle.run(&b.conn, cards.as_deref().map(Vec::as_slice), &mut seen, now);
            (seen, result)
        })
        .await;
    drop(lock);
    let (seen, settled) = settle(before, outcome);
    match settled {
        // `_usage_state_generation += 1`, solo si todo terminó.
        Settled::Done => deps.engine.bump(),
        Settled::Refused => {}
        // El hilo del Python moría con su traza en stderr.
        Settled::Failed(error) => eprintln!("comandos dash: importación de uso: {error}"),
    }
    owner.lock().seen = Some(seen);
}

/// Cómo terminó una vuelta.
#[derive(Debug)]
enum Settled {
    Done,
    Refused,
    Failed(ImportError),
}

/// El `seen` que queda tras una vuelta. Como el `_IMPORT_SEEN` del Python, que
/// se actualiza antes de leer, el nuevo se queda también tras un fallo a mitad;
/// pero si el frente rehusó (carril apagado, esquema que no admite o una vuelta
/// que no corrió) lo leído no se escribió y vuelve el de antes.
fn settle<S, E>(before: S, outcome: Result<(S, Result<(), ImportError>), E>) -> (S, Settled) {
    match outcome {
        Ok((seen, Ok(()))) => (seen, Settled::Done),
        Ok((_, Err(ImportError::Refused))) | Err(_) => (before, Settled::Refused),
        Ok((seen, Err(error))) => (seen, Settled::Failed(error)),
    }
}

/// Lo que la vuelta necesita dentro del hilo del carril.
struct Cycle {
    handle: tokio::runtime::Handle,
    home: PathBuf,
    hooks: PathBuf,
    usage_env: Arc<BTreeMap<String, String>>,
    zone: Arc<dyn LocalZone + Send + Sync>,
    lane: Arc<Lane<UsageImportBackend>>,
}

/// `git_root_for_path` con una caché por vuelta (`roots` del Python es por
/// importador; el resultado es el mismo).
struct CycleRoots<'a> {
    handle: &'a tokio::runtime::Handle,
    cache: RefCell<HashMap<String, String>>,
}

impl GitRoots for CycleRoots<'_> {
    fn root(&self, path: &str) -> String {
        if let Some(root) = self.cache.borrow().get(path) {
            return root.clone();
        }
        let root = self
            .handle
            .block_on(state::git_root_for_path(Path::new(path)));
        self.cache
            .borrow_mut()
            .insert(path.to_owned(), root.clone());
        root
    }
}

/// `int(env.get(key) or default)`; lo que `int()` rechaza lanza (el hilo del
/// Python moría ahí).
fn env_int(env: &Map<String, Value>, key: &str, default: i64) -> Result<i64, ImportError> {
    match env.get(key) {
        Some(v) if truthy(v) => py::int_of(v).map_err(|_| ImportError::Raises),
        _ => Ok(default),
    }
}

/// `int(env or 0) or None`.
fn max_files(env: &Map<String, Value>, key: &str) -> Result<Option<i64>, ImportError> {
    Ok(match env_int(env, key, 0)? {
        0 => None,
        n => Some(n),
    })
}

/// Un texto verdadero del entorno como ruta.
fn env_path(env: &Map<String, Value>, key: &str) -> Option<PathBuf> {
    match env.get(key) {
        Some(Value::String(s)) if !s.is_empty() => Some(PathBuf::from(s)),
        _ => None,
    }
}

impl Cycle {
    /// `_do_refresh_local_usage` (cc-dash:347), en orden.
    fn run(
        &self,
        conn: &Connection,
        cards: Option<&[Value]>,
        seen: &mut ImportSeen,
        now: i64,
    ) -> Result<(), ImportError> {
        // `usage_runtime_env()`: un error aquí mataba el hilo.
        let conf = ConfFile::read(&self.hooks.join("cc-notify.conf"));
        let usage_file = ConfFile::read(&self.hooks.join("usage.env"));
        let settings = usage_read::usage_settings(conn).map_err(|_| ImportError::Raises)?;
        let env = state::runtime_env(&conf, &usage_file, &self.usage_env, settings)
            .map_err(|_| ImportError::Raises)?;
        // 1. Atribución: `try: ensure_observed_configs(); reconcile(...) except: pass`.
        // Sin tarjetas (`/state` declinó) se omite la primera parte.
        let _ = (|| -> Result<(), ImportError> {
            if let Some(cards) = cards {
                usage_import::ensure_observed_configs(conn, cards, now)?;
            }
            usage_import::reconcile_orphan_interactions(conn, now, RECONCILE_DAYS)
        })();
        // 2.–4. Ventana, poda (errores ignorados) y topes.
        let max_age = env_int(&env, "COMANDOS_USAGE_LOCAL_DAYS", LOCAL_DAYS)?;
        let _ = usage_import::prune_old_turns(conn, now, max_age);
        let claude_max_files = max_files(&env, "COMANDOS_USAGE_CLAUDE_MAX_FILES")?;
        let codex_max_files = max_files(&env, "COMANDOS_USAGE_CODEX_MAX_FILES")?;
        let admit = |c: &Connection| UsageImportBackend::admits(c);
        let lane = self.lane.clone();
        let cancelled = move || !lane.enabled();
        let plan = ImportPlan {
            now,
            max_age_days: max_age,
            claude_max_files,
            codex_max_files,
            home: &self.home,
            claude_projects_main: env_path(&env, "COMANDOS_CLAUDE_PROJECTS_DIR"),
            opencode_db: env_path(&env, "COMANDOS_OPENCODE_DB")
                .unwrap_or_else(|| self.home.join(".local/share/opencode/opencode.db")),
            zone: self.zone.as_ref(),
            admit: &admit,
            cancelled: &cancelled,
        };
        let roots = CycleRoots {
            handle: &self.handle,
            cache: RefCell::default(),
        };
        // 5. Codex → Grok → Claude por cuenta → OpenCode.
        usage_import::record_local_codex_rollouts(conn, &plan, seen, &roots)?;
        let grok: Vec<PathBuf> = comandos_runtime::limits::grok_account_homes(&self.home)
            .into_iter()
            .map(|(_, home)| home)
            .collect();
        usage_import::record_local_grok_updates(conn, &plan, &grok, &roots)?;
        for (alias, home) in usage_import::account_homes(&self.home, ".claude", ".claude-accounts")
        {
            let projects = match (&plan.claude_projects_main, alias.as_str()) {
                (Some(dir), "main") => dir.clone(),
                _ => home.join("projects"),
            };
            usage_import::record_local_claude_jsonl(conn, &plan, &projects, &alias, seen, &roots)?;
        }
        usage_import::record_local_opencode_db(conn, &plan, &roots)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{ImportError, Settled, settle};

    #[test]
    fn refused_cycles_keep_the_previous_seen() {
        assert!(matches!(
            settle::<_, ()>(1, Ok((2, Ok(())))),
            (2, Settled::Done)
        ));
        assert!(matches!(
            settle::<_, ()>(1, Ok((2, Err(ImportError::Refused)))),
            (1, Settled::Refused)
        ));
        assert!(matches!(settle(1, Err(())), (1, Settled::Refused)));
        assert!(matches!(
            settle::<_, ()>(1, Ok((2, Err(ImportError::Raises)))),
            (2, Settled::Failed(ImportError::Raises))
        ));
    }
}
