//! Bordes de pane (`@ccmodel`), `H/pane-models.txt` y avisos de nivel de
//! modelo: `write_pane_models` (bin/cc-dash:4641), `_reconcile_pane_models`
//! (4567), `_pane_model_snapshot` (4540), `_pane_model_live_ids` (4556),
//! `_maybe_tier_alert` (4440), `usage_alert_send` (455) y `notice_emit` (422).
//!
//! Latente (Tarea 7a): ninguna ruta lo llama todavía. Los valores de borde y
//! las alertas (`live_rows`, `pane_values`) llegan en la 7b y la Tarea 8 lo
//! engancha a `/usage/state`. Todo lo que toca tmux, el disco o cc-notifyd
//! corre en tareas o en `spawn_blocking`; nada bloquea el runtime.
use super::super::{
    Fault, Native, files,
    tmux::{Tmux, run_program},
};
use comandos_core::json::{response_dumps, truthy};
use comandos_runtime::{Unsure, providers::read_conf};
use futures_util::future::BoxFuture;
use serde_json::{Map, Value, json};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    net::SocketAddr,
    path::PathBuf,
    sync::{Arc, Mutex, MutexGuard},
    time::{Duration, Instant},
};

/// `PANE_MODEL_RETRY_SECONDS` (cc-dash:1364).
const RETRY: Duration = Duration::from_secs(5);
/// `tmux(..., timeout=2)` de la reconciliación.
const TMUX_PLAZO: Duration = Duration::from_secs(2);
/// `urlopen(..., timeout=2)` de `usage_alert_send`.
const NOTIFY_PLAZO: Duration = Duration::from_secs(2);
/// `TIER_ALERT_COOLDOWN_S` (cc-dash:4437): un aviso por pane y hora.
pub const TIER_ALERT_COOLDOWN_S: f64 = 3600.0;
/// El cc-notifyd de la sesión (`http://127.0.0.1:4778/notify`).
pub const NOTIFYD_ADDR: ([u8; 4], u16) = ([127, 0, 0, 1], 4778);

// ---------------------------------------------------------------------------
// cc-notifyd
// ---------------------------------------------------------------------------

pub type NotifyFuture = BoxFuture<'static, ()>;

/// POST de un cuerpo JSON ya serializado a cc-notifyd (`/notify`). Como el
/// `try/except: pass` del Python, nunca falla hacia fuera; las pruebas ponen
/// un doble que solo guarda los cuerpos.
pub trait NotifyPost: Send + Sync + 'static {
    fn post(&self, body: String) -> NotifyFuture;
}

/// Cliente HTTP/1 de hyper hacia cc-notifyd: `POST /notify` con
/// `Content-Type: application/json`, todo bajo un plazo de 2 s; la respuesta
/// no se lee (el Python tampoco la lee).
#[derive(Debug, Clone, Copy)]
pub struct HyperNotify {
    pub addr: SocketAddr,
}

impl Default for HyperNotify {
    fn default() -> Self {
        Self {
            addr: SocketAddr::from(NOTIFYD_ADDR),
        }
    }
}

impl NotifyPost for HyperNotify {
    fn post(&self, body: String) -> NotifyFuture {
        let addr = self.addr;
        Box::pin(async move {
            let _ = tokio::time::timeout(NOTIFY_PLAZO, send_notify(addr, body)).await;
        })
    }
}

async fn send_notify(addr: SocketAddr, body: String) -> Option<()> {
    use bytes::Bytes;
    use http_body_util::Full;
    use hyper_util::rt::TokioIo;
    let stream = tokio::net::TcpStream::connect(addr).await.ok()?;
    let (mut sender, conn) = hyper::client::conn::http1::handshake(TokioIo::new(stream))
        .await
        .ok()?;
    let request = hyper::Request::post("/notify")
        .header(hyper::header::HOST, addr.to_string())
        .header(hyper::header::CONTENT_TYPE, "application/json")
        .body(Full::new(Bytes::from(body)))
        .ok()?;
    // La conexión se mueve aquí mismo y muere con este futuro: nada queda
    // vivo después del plazo.
    tokio::pin!(conn);
    tokio::select! {
        response = sender.send_request(request) => {
            drop(response);
            Some(())
        }
        _ = &mut conn => None,
    }
}

// ---------------------------------------------------------------------------
// Avisos de nivel
// ---------------------------------------------------------------------------

/// Clave de `_TIER_LAST`/`_TIER_ALERTED`: `(sesión, pane or "")`.
pub type TierKey = (String, String);

/// `_TIER_LAST` y `_TIER_ALERTED`. La decisión pura de `_maybe_tier_alert`
/// (sin la lectura de `NOTIFY_MODEL_TIER`, que hace `send_alerts`).
#[derive(Debug, Default)]
pub struct TierAlerts {
    last: HashMap<TierKey, String>,
    alerted: HashMap<TierKey, f64>,
}

impl TierAlerts {
    /// Si este nivel avisa ahora. Sin sesión o sin nivel no se anota nada;
    /// la primera vista, el mismo nivel o un nivel distinto de `alert_tier`
    /// solo se anotan; dentro de la hora del último aviso tampoco avisa.
    pub fn observe(&mut self, key: &TierKey, tier: &str, alert_tier: &str, now: f64) -> bool {
        if key.0.is_empty() || tier.is_empty() {
            return false;
        }
        let prev = self.last.insert(key.clone(), tier.to_owned());
        match prev {
            None => return false,
            Some(prev) if prev == tier => return false,
            Some(_) if tier != alert_tier => return false,
            Some(_) => {}
        }
        let last = self.alerted.get(key).copied().unwrap_or(0.0);
        if now - last < TIER_ALERT_COOLDOWN_S {
            return false;
        }
        self.alerted.insert(key.clone(), now);
        true
    }

    /// Cota (regla 6): quita las claves de panes que ya no viven y cuyo último
    /// aviso venció (ya no frenaría otro). La deduplicación por pane vivo no
    /// cambia y los panes muertos no se acumulan. Las claves sin pane (`""`)
    /// se quedan: no hay pane que mirar y su número lo acotan las sesiones.
    pub fn prune(&mut self, live_panes: &BTreeSet<String>, now: f64) {
        let alerted = &self.alerted;
        let dead = |key: &TierKey| {
            !key.1.is_empty()
                && !live_panes.contains(&key.1)
                && now - alerted.get(key).copied().unwrap_or(0.0) >= TIER_ALERT_COOLDOWN_S
        };
        let gone: Vec<TierKey> = self
            .last
            .keys()
            .chain(self.alerted.keys())
            .filter(|k| dead(k))
            .cloned()
            .collect();
        for key in gone {
            self.last.remove(&key);
            self.alerted.remove(&key);
        }
    }

    /// Claves recordadas (para las pruebas de la cota).
    pub fn len(&self) -> usize {
        let mut keys: BTreeSet<&TierKey> = self.last.keys().collect();
        keys.extend(self.alerted.keys());
        keys.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// `tier_style(tier)` (cc-dash:3743) sobre el `model-tiers.json` ya leído (un
/// no objeto es el `{}` de `load_model_tiers`). `tiers` verdadero que no es
/// objeto: el `.get` del Python lanza → `Unsure`.
pub fn tier_style(tiers: &Value, tier: &str) -> Result<Map<String, Value>, Unsure> {
    let all = match tiers.get("tiers") {
        None => return Ok(Map::new()),
        Some(v) if !truthy(v) => return Ok(Map::new()),
        Some(Value::Object(all)) => all,
        Some(_) => return Err(Unsure),
    };
    Ok(all
        .get(tier)
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default())
}

/// Un aviso que `_maybe_tier_alert` decidió dar, con el estilo del nivel ya
/// resuelto (el Python lo lee en la misma llamada, de la misma caché).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TierAlert {
    pub session: String,
    pub agent: String,
    pub model: String,
    pub tier: String,
    /// `tier_style(tier).get("symbol") or ""`.
    pub symbol: String,
    /// `tier_style(tier).get("label") or tier`.
    pub label: String,
}

impl TierAlert {
    /// Un símbolo o etiqueta verdaderos que no son texto (el `str()` del
    /// f-string) → `Unsure`: quien llama no avisa.
    pub fn styled(
        session: &str,
        agent: &str,
        model: &str,
        tier: &str,
        tiers: &Value,
    ) -> Result<Self, Unsure> {
        let style = tier_style(tiers, tier)?;
        let pick = |name: &str, default: &str| match style.get(name) {
            Some(v) if truthy(v) => v.as_str().map(str::to_owned).ok_or(Unsure),
            _ => Ok(default.to_owned()),
        };
        Ok(Self {
            session: session.to_owned(),
            agent: agent.to_owned(),
            model: model.to_owned(),
            tier: tier.to_owned(),
            symbol: pick("symbol", "")?,
            label: pick("label", tier)?,
        })
    }

    /// `(título, mensaje)` de cc-dash:4459-4463 en el idioma de la interfaz.
    pub fn texts(&self, es: bool) -> (String, String) {
        let (agent, sym, model, lbl) = (&self.agent, &self.symbol, &self.model, &self.label);
        if es {
            let agent = if agent.is_empty() { "El agente" } else { agent };
            (
                format!("Modelo {lbl} en uso"),
                format!(
                    "{agent} de esta sesión cambió a {model} {sym}: gasta tu cuota más rápido."
                ),
            )
        } else {
            let agent = if agent.is_empty() { "The agent" } else { agent };
            (
                format!("{lbl} model in use"),
                format!("{agent} in this session switched to {model} {sym}: it uses quota faster."),
            )
        }
    }
}

/// `ui_lang(conf) == "es"` (cc-dash:7272): `CC_LANG` explícito o, si no, el
/// `LANG` del proceso (el del heredado; aquí el de `usage_env`, D7).
pub fn ui_lang_es(cc_lang: Option<&str>, lang: Option<&str>) -> bool {
    match cc_lang {
        Some("es") => true,
        Some("en") => false,
        _ => lang.is_some_and(|l| l.to_lowercase().starts_with("es")),
    }
}

/// Los avisos que `_maybe_tier_alert` lanza en hilos (`usage_alert_send`),
/// en orden y en la tarea de quien llama: evento `usage_alert` en app-state
/// y, con `DESKTOP_NOTIFY` (por omisión `"1"`), el popup de cc-notifyd con el
/// texto completo. `NOTIFY_MODEL_TIER == "0"` → nada (el `_TIER_ALERTED` ya se
/// actualizó en `observe`, como en el Python). Sin efectos de uso (sombra)
/// no hace nada. Un `cc-notify.conf` ilegible (el Python lanzaría dentro de
/// `_maybe_tier_alert`) → ningún aviso y una línea en stderr.
pub async fn send_alerts(native: &Native, alerts: Vec<TierAlert>) {
    let opts = native.options();
    if alerts.is_empty() || !opts.usage_effects {
        return;
    }
    let path = opts.hooks.join("cc-notify.conf");
    let conf = match tokio::task::spawn_blocking(move || read_conf(&path)).await {
        Ok(Ok(conf)) => conf,
        _ => {
            eprintln!("notice_emit usage_alert: cc-notify.conf ilegible");
            return;
        }
    };
    let get = |name: &str| {
        conf.iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    };
    if get("NOTIFY_MODEL_TIER") == Some("0") {
        return;
    }
    let es = ui_lang_es(
        get("CC_LANG"),
        opts.usage_env.get("LANG").map(String::as_str),
    );
    let desktop = get("DESKTOP_NOTIFY").unwrap_or("1") == "1";
    for alert in alerts {
        let (title, message) = alert.texts(es);
        notice_emit(native, &title, &message, &alert.session).await;
        if !desktop {
            continue;
        }
        let payload = json!({
            "title": title,
            "body": message,
            "session": "",
            "kind": "done",
            "project": alert.session,
            "options": "",
            "full": message,
        });
        if let Ok(body) = response_dumps(&payload) {
            opts.notifyd.post(body).await;
        }
    }
}

/// `str(x)[:n]`: los primeros `n` puntos de código.
fn take_chars(text: &str, n: usize) -> String {
    text.chars().take(n).collect()
}

/// `notice_emit("usage_alert", title, excerpt, project)`: un evento en el
/// registro N1 de app-state por el worker de la base, en `BEGIN IMMEDIATE`.
/// Un error se escribe en stderr y no se propaga.
async fn notice_emit(native: &Native, title: &str, excerpt: &str, project: &str) {
    let now = (native.options().clock)();
    let mut random = [0u8; 4];
    if let Err(error) = getrandom::fill(&mut random) {
        eprintln!("notice_emit usage_alert: {error}");
        return;
    }
    let hex: String = random.iter().map(|b| format!("{b:02x}")).collect();
    let event_id = format!("usage_alert:{now}:{hex}");
    let receipt_id = match comandos_runtime::fresh_id("receipt") {
        Ok(id) => id,
        Err(error) => {
            eprintln!("notice_emit usage_alert: {error}");
            return;
        }
    };
    let event = json!({
        "eventId": event_id,
        "source": "comandos",
        "kind": "usage_alert",
        "evidence": "confirmed",
        "correlation": "unknown",
        "occurredAtMs": now,
        "receivedAtMs": now,
        "projectKey": project,
        "title": take_chars(title, 200),
        "excerpt": take_chars(excerpt, 500),
        "sourceEventId": Value::Null,
    });
    let now_ms = u64::try_from(now).unwrap_or(0);
    let done = native
        .with_state(move |backend| -> Result<(), String> {
            let tx = rusqlite::Transaction::new_unchecked(
                &backend.conn,
                rusqlite::TransactionBehavior::Immediate,
            )
            .map_err(|e| e.to_string())?;
            comandos_store::append_event(&backend.conn, &event, now_ms, &event_id, &receipt_id)
                .map_err(|e| e.to_string())?;
            tx.commit().map_err(|e| e.to_string())
        })
        .await;
    match done {
        Ok(Ok(())) => {}
        Ok(Err(error)) => eprintln!("notice_emit usage_alert: {error}"),
        Err(Fault::Decline) => eprintln!("notice_emit usage_alert: base de estado no disponible"),
        Err(Fault::Error(_)) => eprintln!("notice_emit usage_alert: fallo del worker de la base"),
    }
}

// ---------------------------------------------------------------------------
// Escritor de bordes
// ---------------------------------------------------------------------------

/// `_pane_model_state` (cc-dash:4467). En `desired`/`applied`, `None` es la
/// opción quitada (`set-option -u`), el `None` del Python.
#[derive(Debug, Default)]
struct WriterState {
    desired: BTreeMap<String, Option<String>>,
    applied: BTreeMap<String, Option<String>>,
    file_text: Option<String>,
    generation: u64,
    running: bool,
    discovered: bool,
    retry_after: Option<Instant>,
}

/// Escritor de `@ccmodel` y `pane-models.txt`. La escritura del archivo va
/// bajo su propio candado async; el estado, bajo un `Mutex` que nunca se
/// sostiene a través de un `await`. Una sola reconciliación en vuelo.
#[derive(Debug, Default)]
pub struct PaneModelWriter {
    file: tokio::sync::Mutex<()>,
    state: Mutex<WriterState>,
}

/// `_pane_models_need_reconcile` (cc-dash:4533).
fn need_reconcile(
    desired: &BTreeMap<String, Option<String>>,
    applied: &BTreeMap<String, Option<String>>,
) -> bool {
    desired
        .iter()
        .any(|(id, value)| applied.get(id) != Some(value))
        || applied.keys().any(|id| !desired.contains_key(id))
}

impl PaneModelWriter {
    fn lock(&self) -> MutexGuard<'_, WriterState> {
        self.state.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Lo que tmux tiene según la última reconciliación.
    pub fn applied(&self) -> BTreeMap<String, Option<String>> {
        self.lock().applied.clone()
    }

    /// Si hay una reconciliación en vuelo.
    pub fn running(&self) -> bool {
        self.lock().running
    }

    /// Si ya se leyeron las opciones físicas de tmux.
    pub fn discovered(&self) -> bool {
        self.lock().discovered
    }

    /// Si hay un reintento pendiente (`retry_after` en el futuro).
    pub fn retry_pending(&self) -> bool {
        self.lock()
            .retry_after
            .is_some_and(|at| Instant::now() < at)
    }

    /// R1 (b) del preflight: si el heredado pudo escribir bordes (un declinar de
    /// `/usage/state` con el carril encendido), la próxima reconciliación vuelve
    /// a leer las opciones físicas en vez de fiarse de `applied`.
    pub fn forget_discovery(&self) {
        let mut st = self.lock();
        st.discovered = false;
        st.retry_after = None;
    }

    /// `write_pane_models` (cc-dash:4641) con los valores ya calculados: el
    /// archivo si cambió (si la escritura falla, no se recuerda y se reintenta
    /// en la siguiente llamada) y, si hace falta, una reconciliación en una
    /// tarea. Nunca espera a tmux.
    pub async fn apply(
        self: &Arc<Self>,
        values: BTreeMap<String, Option<String>>,
        file_text: String,
        tmux: Tmux,
        hooks: PathBuf,
    ) {
        {
            let _file = self.file.lock().await;
            let changed = self.lock().file_text.as_deref() != Some(file_text.as_str());
            if changed {
                let path = hooks.join("pane-models.txt");
                let text = file_text.clone();
                let written =
                    tokio::task::spawn_blocking(move || files::write_text_atomic(&path, &text))
                        .await;
                if matches!(written, Ok(Ok(()))) {
                    self.lock().file_text = Some(file_text);
                }
            }
        }
        let start = {
            let mut st = self.lock();
            if values != st.desired {
                st.desired = values;
                st.generation = st.generation.wrapping_add(1);
                st.retry_after = None;
            }
            let due = st.retry_after.is_none_or(|at| Instant::now() >= at);
            let start =
                !st.running && due && (!st.discovered || need_reconcile(&st.desired, &st.applied));
            if start {
                st.running = true;
            }
            start
        };
        if start {
            tokio::spawn(reconcile(self.clone(), tmux));
        }
    }
}

/// Deja `running` en falso si la tarea se cancela o entra en pánico antes
/// de terminar; las salidas normales lo ponen ellas mismas bajo el candado.
struct RunningGuard {
    writer: Arc<PaneModelWriter>,
    done: bool,
}

impl Drop for RunningGuard {
    fn drop(&mut self) {
        if !self.done {
            self.writer.lock().running = false;
        }
    }
}

/// `tmux list-panes -a -F <format>` con éxito → su salida; si no, `None`.
async fn list_panes(tmux: &Tmux, format: &str) -> Option<String> {
    match run_program(
        &tmux.program,
        &["list-panes", "-a", "-F", format],
        TMUX_PLAZO,
    )
    .await
    {
        Ok(out) if out.ok => Some(out.stdout),
        _ => None,
    }
}

/// `set-option` con `returncode == 0`.
async fn set_option(tmux: &Tmux, args: &[&str]) -> bool {
    matches!(run_program(&tmux.program, args, TMUX_PLAZO).await, Ok(out) if out.ok)
}

/// `_reconcile_pane_models` (cc-dash:4567).
async fn reconcile(writer: Arc<PaneModelWriter>, tmux: Tmux) {
    let mut guard = RunningGuard {
        writer: writer.clone(),
        done: false,
    };
    loop {
        let (generation, desired, applied, discovered) = {
            let st = writer.lock();
            (
                st.generation,
                st.desired.clone(),
                st.applied.clone(),
                st.discovered,
            )
        };
        if !discovered {
            // `_pane_model_snapshot`: una lectura de las opciones físicas.
            let physical = list_panes(&tmux, "#{pane_id}\t#{@ccmodel}")
                .await
                .map(|out| {
                    out.lines()
                        .filter_map(|line| line.split_once('\t'))
                        .filter(|(id, value)| id.starts_with('%') && !value.is_empty())
                        .map(|(id, value)| (id.to_owned(), Some(value.to_owned())))
                        .collect::<BTreeMap<_, _>>()
                });
            let mut st = writer.lock();
            match physical {
                None => {
                    st.retry_after = Some(Instant::now() + RETRY);
                    st.running = false;
                    guard.done = true;
                    return;
                }
                Some(physical) => {
                    st.applied = physical;
                    st.discovered = true;
                    st.retry_after = None;
                }
            }
            continue;
        }
        let ids: BTreeSet<String> = desired.keys().chain(applied.keys()).cloned().collect();
        let mut live: Option<BTreeSet<String>> = None;
        let mut retry_failed = false;
        for id in ids {
            let value = match desired.get(&id) {
                Some(v) if applied.get(&id) == Some(v) => continue,
                Some(v) => v.clone(),
                None if applied.get(&id) == Some(&None) => continue,
                None => None,
            };
            let ok = match &value {
                None => set_option(&tmux, &["set-option", "-p", "-u", "-t", &id, "@ccmodel"]).await,
                Some(v) => set_option(&tmux, &["set-option", "-p", "-t", &id, "@ccmodel", v]).await,
            };
            if ok {
                writer.lock().applied.insert(id, value);
                continue;
            }
            let mut needs_retry = true;
            if value.is_none() {
                if live.is_none() {
                    live = list_panes(&tmux, "#{pane_id}").await.map(|out| {
                        out.lines()
                            .filter(|line| line.starts_with('%'))
                            .map(str::to_owned)
                            .collect()
                    });
                }
                if live.as_ref().is_some_and(|l| !l.contains(&id)) {
                    writer.lock().applied.insert(id, None);
                    needs_retry = false;
                }
            }
            retry_failed |= needs_retry;
        }
        let mut st = writer.lock();
        if generation != st.generation {
            // Llegaron valores nuevos durante la vuelta.
            continue;
        }
        let stale: Vec<String> = st
            .applied
            .iter()
            .filter(|(id, value)| value.is_none() && !st.desired.contains_key(*id))
            .map(|(id, _)| id.clone())
            .collect();
        for id in stale {
            st.applied.remove(&id);
        }
        st.retry_after = retry_failed.then(|| Instant::now() + RETRY);
        st.running = false;
        guard.done = true;
        return;
    }
}
