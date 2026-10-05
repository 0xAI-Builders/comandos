//! Motor de GET `/usage/state` (bin/cc-dash:8441-8462), latente: el cálculo
//! completo de la respuesta con `record_pane` y el memo de
//! `cached_usage_state` (269), sin la tabla nativa (Tarea 8), los bordes de
//! pane (Tarea 7) ni la importación (Tarea 8). En producción la ruta sigue
//! reenviada y el Python es dueño de sus efectos.
//!
//! Orden: el primer trabajo del carril de uso (`usage_settings`) es el único
//! punto de declinar (D1); desde ahí un trabajo del carril que declina se
//! reintenta una vez (la puerta pudo dar `SQLITE_BUSY`) y, si vuelve a
//! declinar, es un 500 (el carril se apagó a mitad y el Python habría leído).
//! `record_pane` va en un trabajo propio del carril que la respuesta no espera
//! (R3; la Tarea 8 lo pasa al carril de importación). Saltos de
//! bloqueo por cómputo: uno (lecturas de `/proc`, registro, pestañas y
//! archivos de entorno) y otro más solo cuando el memo no sirve (el ensamblado
//! de `build_usage_state`); el resto son procesos async (`tmux`, `git`) y
//! trabajos del carril, que tiene hilo propio.
use super::super::{
    Fault, Native, NativeOptions, light,
    states::{StateFault, gather},
    tmux::{self, Program},
};
use crate::HandlerError;
use comandos_core::{
    allocation,
    json::response_dumps,
    text,
    usage_state::{self, UsageError},
};
use comandos_runtime::{
    agent_procs::{AgentMaps, PANE_FORMAT, PaneRow, parse_pane_inventory},
    providers::{self, RegistryCache},
};
use comandos_store::usage_read::{self, ReadError};
use futures_util::{StreamExt, stream};
use serde_json::{Map, Value};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    io,
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

type Row = Map<String, Value>;

/// `git rev-parse` de `git_root_for_path` (cc_usage.py:423): 3 s.
const GIT_TIMEOUT: Duration = Duration::from_secs(3);
/// Cuántos `git rev-parse` corren a la vez (el Python los hace uno a uno).
const GIT_CONCURRENCY: usize = 8;
/// `USAGE_STATE_DAYS * 86400` (cc_usage.py:181).
const STATE_WINDOW_S: i64 = 14 * 86_400;
/// `list_alerts(db_path, limit=12)` (cc_usage.py:992).
const ALERTS_LIMIT: i64 = 12;

fn failure() -> Fault {
    Fault::Error(HandlerError::Failure)
}

/// D1: pasado el punto de declinar, un `Decline` (el carril se apagó a mitad,
/// o la puerta falló dos veces seguidas en `with_retry`) es el 500 que el
/// Python no daría pero el frente tampoco puede reenviar.
fn no_decline(fault: Fault) -> Fault {
    match fault {
        Fault::Decline => failure(),
        other => other,
    }
}

/// La respuesta calculada: el cuerpo serializado, el estado del memo (lo que
/// usa el escritor de bordes de la Tarea 7) y los paneles vivos.
pub struct UsageStateReply {
    pub body: bytes::Bytes,
    pub state: Arc<Value>,
    pub live_panes: Vec<Row>,
}

/// Memo de `build_usage_state` (`cached_usage_state` 269): una entrada.
#[derive(Default)]
pub struct UsageEngine {
    generation: AtomicU64,
    memo: tokio::sync::Mutex<Option<(MemoKey, Arc<Value>)>>,
}

#[derive(Clone, PartialEq, Eq)]
struct MemoKey {
    generation: u64,
    /// `sorted((tmux_session, tmux_pane, pane_pwd, agent))`.
    panes: Vec<(String, String, String, String)>,
}

impl MemoKey {
    fn new(generation: u64, live: &[Row]) -> Self {
        let field = |row: &Row, key: &str| {
            row.get(key)
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned()
        };
        let mut panes: Vec<_> = live
            .iter()
            .map(|p| {
                (
                    field(p, "tmux_session"),
                    field(p, "tmux_pane"),
                    field(p, "pane_pwd"),
                    field(p, "agent"),
                )
            })
            .collect();
        panes.sort();
        Self { generation, panes }
    }
}

impl UsageEngine {
    pub fn generation(&self) -> u64 {
        self.generation.load(Ordering::Acquire)
    }

    /// Al terminar una importación completa (`_usage_state_generation += 1`).
    pub fn bump(&self) {
        self.generation.fetch_add(1, Ordering::AcqRel);
    }

    /// El estado del memo, o uno nuevo si la clave cambió. El candado se
    /// sostiene durante el cálculo: los que llegan esperan y reutilizan (el
    /// `with _usage_state_lock` del Python). Un cliente que se desconecta con
    /// el candado tomado suelta el futuro y el siguiente calcula (D4 de la 2d).
    async fn state(
        &self,
        native: &Native,
        live: &[Row],
        settings: Row,
    ) -> Result<Arc<Value>, Fault> {
        let mut memo = self.memo.lock().await;
        // La generación se lee con el candado tomado: una importación que
        // termina mientras otro calcula invalida lo que este guarde después.
        let key = MemoKey::new(self.generation(), live);
        if let Some((k, state)) = memo.as_ref()
            && *k == key
        {
            return Ok(state.clone());
        }
        // `int(time.time())` de `build_usage_state`.
        let now = (native.options().clock)().div_euclid(1000);
        let want_panes = live.is_empty();
        let rows = native
            .usage
            .with_retry(move |u| -> Result<_, ReadError> {
                let rows = usage_read::state_rows(&u.conn, now - STATE_WINDOW_S)?;
                let panes = if want_panes {
                    usage_read::list_panes(&u.conn)?
                } else {
                    Vec::new()
                };
                Ok((rows, panes))
            })
            .await
            .map_err(no_decline)?
            .map_err(|_| failure())?;
        let live = live.to_vec();
        let built = tokio::task::spawn_blocking(move || {
            let (rows, stored) = rows;
            let panes = if live.is_empty() { stored } else { live };
            usage_state::build_state(
                now,
                panes,
                &rows.turns,
                &rows.provider_usage,
                &rows.provider_costs,
                &settings,
            )
        })
        .await
        .map_err(|_| failure())?
        .map_err(|_| failure())?;
        let state = Arc::new(built);
        *memo = Some((key, state.clone()));
        Ok(state)
    }
}

/// Un archivo de configuración leído una vez: `cc-notify.conf` lo usan el
/// entorno (`_parse_env_file`, que ignora cualquier `OSError`) y `ui_lang`
/// (`read_conf`, que solo ignora `FileNotFoundError`).
enum ConfFile {
    Missing,
    Unreadable,
    Bytes(Vec<u8>),
}

impl ConfFile {
    fn read(path: &Path) -> Self {
        match std::fs::read(path) {
            Ok(bytes) => Self::Bytes(bytes),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Self::Missing,
            Err(_) => Self::Unreadable,
        }
    }

    /// `_parse_env_file` (cc_usage.py:2157): un `OSError` es `{}`; texto que no
    /// es UTF-8 es el `UnicodeDecodeError` no capturado (500).
    fn env_pairs(&self) -> Result<Vec<(String, String)>, Fault> {
        match self {
            Self::Missing | Self::Unreadable => Ok(Vec::new()),
            Self::Bytes(bytes) => {
                let text = std::str::from_utf8(bytes).map_err(|_| failure())?;
                Ok(usage_state::parse_env_text(text))
            }
        }
    }

    /// `read_conf().get("CC_LANG")`: ausente → nada; otro `OSError` o texto
    /// que no es UTF-8 → la excepción no capturada (500).
    fn cc_lang(&self) -> Result<Option<String>, Fault> {
        let text = match self {
            Self::Missing => return Ok(None),
            Self::Unreadable => return Err(failure()),
            Self::Bytes(bytes) => std::str::from_utf8(bytes).map_err(|_| failure())?,
        };
        let conf = providers::parse_conf(text).map_err(|_| failure())?;
        Ok(conf
            .into_iter()
            .find(|(k, _)| k == "CC_LANG")
            .map(|(_, v)| v))
    }
}

fn lang_of(cc_lang: Option<&str>, usage_env: &BTreeMap<String, String>) -> &'static str {
    let lang = usage_env.get("LANG").map(String::as_str);
    if super::pane_models::ui_lang_es(cc_lang, lang) {
        "es"
    } else {
        "en"
    }
}

/// `ui_lang()` (cc-dash:7272) desde un hilo de bloqueo: `CC_LANG` de
/// `cc-notify.conf` si es `es`/`en`; si no, `es` cuando el `LANG` capturado
/// (D7) empieza por `es`. `Err`: `read_conf` lanzaría (500).
pub fn ui_lang(conf: &Path, usage_env: &BTreeMap<String, String>) -> Result<&'static str, Fault> {
    let cc_lang = ConfFile::read(conf).cc_lang()?;
    Ok(lang_of(cc_lang.as_deref(), usage_env))
}

/// `git_root_for_path` (cc_usage.py:423): `git rev-parse --show-toplevel` en
/// `path` con plazo de 3 s; cualquier fallo (plazo, directorio inexistente,
/// salida que no es UTF-8, salida vacía) devuelve `path`.
pub async fn git_root_for_path(path: &Path) -> String {
    let fallback = path.to_string_lossy().into_owned();
    if fallback.is_empty() {
        return fallback;
    }
    let git = Program::named("git");
    match tmux::run_program_in(&git, &["rev-parse", "--show-toplevel"], path, GIT_TIMEOUT).await {
        Ok(out) if out.ok => {
            let root = text::strip(&out.stdout);
            if root.is_empty() {
                fallback
            } else {
                root.to_owned()
            }
        }
        _ => fallback,
    }
}

/// Lo que el salto de bloqueo devuelve.
struct Scanned {
    conf: ConfFile,
    usage_file: ConfFile,
    /// Etiquetas y agentes; `Err` de la recolección de la 2d.
    agents: Result<(HashMap<String, String>, AgentMaps), StateFault>,
}

/// El único salto de bloqueo de todo cómputo: pestañas, registro (la caché
/// única del frente, B9), agentes de `/proc` y los dos archivos de entorno.
/// `live` son las sesiones de `tmux_sessions()`. Ningún candado que sostenga
/// el escaneo de `/state`: el del registro solo cubre su carga, así que este
/// hilo del pool no queda aparcado detrás de `/state` (I2 de la revisión).
fn scan(
    opts: &NativeOptions,
    panes: &[PaneRow],
    live: &HashSet<String>,
    registry: &Mutex<RegistryCache>,
) -> Scanned {
    let agents = (|| -> Result<_, StateFault> {
        // `session_labels()`: `tab_labels()` y `read_tab_history()`.
        let tabs = light::tab_labels(&opts.hooks)?;
        let history = light::read_tab_history(&opts.hooks)?;
        let labels = gather::session_labels(tabs, live, &history);
        // `agent_pane_maps(agent_procs())`.
        let registry = gather::load_registry(opts, registry)?;
        let (maps, _external) = gather::agent_maps(opts, &registry, panes)?;
        Ok((labels, maps))
    })();
    Scanned {
        conf: ConfFile::read(&opts.hooks.join("cc-notify.conf")),
        usage_file: ConfFile::read(&opts.hooks.join("usage.env")),
        agents,
    }
}

/// Paso 2 sin `record_pane` (`usage_live_panes` 7230): `None` si la
/// recolección de la 2d declinó (lista vacía esta vuelta, sin `record_pane`).
async fn live_panes(
    native: &Native,
    now: i64,
) -> Result<(Option<Vec<Row>>, ConfFile, ConfFile), Fault> {
    let opts = native.options();
    // `tmux_sessions()` y `tmux_pane_inventory()`: excepciones sin capturar
    // (plazo → 504, el resto → 500); un código de salida distinto de 0 → vacío.
    let (sessions, listed) = tokio::join!(
        light::tmux_sessions(&opts.tmux),
        opts.tmux.run(&["list-panes", "-a", "-F", PANE_FORMAT])
    );
    let sessions = sessions?;
    let listed = listed.map_err(|e| Fault::Error(e.uncaught()))?;
    let panes = if listed.ok {
        parse_pane_inventory(&listed.stdout)
    } else {
        Vec::new()
    };
    let scan_opts = opts.clone();
    let registry = native.registry.clone();
    let scanned =
        tokio::task::spawn_blocking(move || scan(&scan_opts, &panes, &sessions, &registry))
            .await
            .map_err(|_| failure())?;
    let (labels, maps) = match scanned.agents {
        Ok(found) => found,
        Err(StateFault::Decline) => return Ok((None, scanned.conf, scanned.usage_file)),
        Err(fault) => return Err(fault.into()),
    };
    // Deduplicado por `(session, pane, agent)` en el orden de `by_cwd`.
    let mut seen = HashSet::new();
    let mut infos = Vec::new();
    for (_, rows) in maps.by_cwd {
        for info in rows {
            if seen.insert((info.session.clone(), info.pane.clone(), info.agent.clone())) {
                infos.push(info);
            }
        }
    }
    // Los `git rev-parse` corren a la vez (acotados); el resultado no depende
    // del orden y uno por carpeta basta.
    let cwds: HashSet<String> = infos.iter().map(|i| i.cwd.clone()).collect();
    let roots: HashMap<String, String> = stream::iter(cwds)
        .map(|cwd| async move {
            let root = git_root_for_path(Path::new(&cwd)).await;
            (cwd, root)
        })
        .buffer_unordered(GIT_CONCURRENCY)
        .collect()
        .await;
    let mut out = Vec::with_capacity(infos.len());
    for info in infos {
        let git_root = roots.get(&info.cwd).cloned().unwrap_or_default();
        let agent = if info.agent.is_empty() {
            "claude".to_owned()
        } else {
            info.agent
        };
        let mut raw = Row::new();
        raw.insert("session".into(), info.session.into());
        raw.insert("pane".into(), info.pane.into());
        raw.insert("cwd".into(), info.cwd.into());
        raw.insert("git_root".into(), git_root.into());
        raw.insert("agent".into(), agent.into());
        raw.insert("pid".into(), info.pid.into());
        match usage_state::normalize_pane_identity(&Value::Object(raw), &labels, now) {
            Ok(pane) => out.push(pane),
            Err(UsageError::Unsure) => return Ok((None, scanned.conf, scanned.usage_file)),
            Err(UsageError::Raises | UsageError::Overflow) => return Err(failure()),
        }
    }
    Ok((Some(out), scanned.conf, scanned.usage_file))
}

/// `usage_runtime_env()` (cc-dash:179): `cc-notify.conf`, `usage.env`, el
/// entorno capturado al arrancar (D7) y `read_usage_settings`, en ese orden.
fn runtime_env(
    conf: &ConfFile,
    usage_file: &ConfFile,
    process: &BTreeMap<String, String>,
    settings: Vec<(String, Value)>,
) -> Result<Row, Fault> {
    let mut env = Row::new();
    for (key, value) in conf.env_pairs()?.into_iter().chain(usage_file.env_pairs()?) {
        env.insert(key, value.into());
    }
    for (key, value) in process {
        env.insert(key.clone(), value.clone().into());
    }
    for (key, value) in settings {
        env.insert(key, value);
    }
    Ok(env)
}

/// GET `/usage/state` sin bordes ni importación (pasos 1–9 de la tarea).
pub async fn compute(native: &Native) -> Result<UsageStateReply, Fault> {
    let opts = native.options();
    // 1. Punto de declinar: `read_usage_settings` por el carril de uso.
    let settings = native
        .usage
        .with(|u| usage_read::usage_settings(&u.conn))
        .await?
        .map_err(|e| match e {
            ReadError::Raises => failure(),
            ReadError::Sql(_) | ReadError::Undecodable | ReadError::Unsure => Fault::Decline,
        })?;
    // Desde aquí nunca `Fault::Decline` (D1).
    let now = (opts.clock)().div_euclid(1000);
    // 2. Paneles vivos (y los dos archivos de entorno, en el mismo salto).
    let (live, conf, usage_file) = live_panes(native, now).await.map_err(no_decline)?;
    let record = live.is_some();
    let live = live.unwrap_or_default();
    // 4. Entorno.
    let env = runtime_env(&conf, &usage_file, &opts.usage_env, settings)?;
    // 5. Límites (efecto: puede lanzar el refresco).
    let limits = native.limits.get(&native.refresh_deps());
    // 8. `record_pane` de cada panel vivo (errores ignorados, el `except
    // Exception: pass` de 7253): todos en una transacción, en un trabajo
    // propio del carril que la respuesta no espera (R3). El Python registra
    // antes del memo, pero el memo no lee `usage_panes` cuando hay vivos: el
    // orden no cambia la respuesta, y un cómputo posterior sin vivos encola
    // sus lecturas detrás (el carril es FIFO).
    if record && opts.usage_effects && !live.is_empty() {
        let usage = native.usage.clone();
        let to_record = live.clone();
        tokio::spawn(async move {
            let _ = usage
                .with(move |u| usage_read::record_panes(&u.conn, &to_record))
                .await;
        });
    }
    let (alerts, recent) = native
        .usage
        .with_retry(|u| -> Result<_, ReadError> {
            let alerts = usage_read::list_alerts(&u.conn, ALERTS_LIMIT)?;
            let recent = usage_read::recent_interactions(&u.conn, 1)?;
            Ok((alerts, recent))
        })
        .await
        .map_err(no_decline)?
        .map_err(|_| failure())?;
    // 6. Memo.
    let memo = native
        .usage_engine
        .state(native, &live, env.clone())
        .await?;
    // 7. La respuesta parte de una copia superficial del memo.
    let mut state = memo.as_object().cloned().ok_or_else(failure)?;
    let rows: Vec<Value> = limits.rows.into_iter().map(Value::Object).collect();
    state.insert("limits".into(), Value::Array(rows));
    let mut health = match usage_state::credential_health(&env) {
        Value::Object(map) => map,
        _ => return Err(failure()),
    };
    for (key, value) in limits.health {
        health.insert(key, value);
    }
    state.insert("credential_health".into(), Value::Object(health));
    state.insert(
        "alerts".into(),
        Value::Array(alerts.into_iter().map(Value::Object).collect()),
    );
    // D5: `attach_token_counts` sobre las filas cacheadas, y `enrich_limits`.
    let windows = state.get("windows").cloned().unwrap_or(Value::Null);
    let attached: Vec<Value> = native
        .limits
        .attach_tokens(&windows)
        .into_iter()
        .map(Value::Object)
        .collect();
    let lang = lang_of(conf.cc_lang()?.as_deref(), &opts.usage_env);
    let enriched = allocation::enrich_limits(&attached, (opts.clock)() as f64 / 1000.0, lang)
        .map_err(|_| failure())?;
    state.insert("limits".into(), Value::Array(enriched));
    let last = recent.into_iter().next().map_or(Value::Null, Value::Object);
    state.insert("lastInteraction".into(), last);
    // 9. Cuerpo.
    let body = response_dumps(&Value::Object(state)).map_err(|_| failure())?;
    Ok(UsageStateReply {
        body: bytes::Bytes::from(body),
        state: memo,
        live_panes: live,
    })
}
