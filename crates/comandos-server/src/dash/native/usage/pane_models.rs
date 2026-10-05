//! Bordes de pane (`@ccmodel`), `H/pane-models.txt` y avisos de nivel de
//! modelo: `write_pane_models` (bin/cc-dash:4641), `_reconcile_pane_models`
//! (4567), `_pane_model_snapshot` (4540), `_pane_model_live_ids` (4556),
//! `_maybe_tier_alert` (4440), `usage_alert_send` (455) y `notice_emit` (422).
//!
//! También `_pane_models_for_live_state` (286) y `_pane_model_values` (4481):
//! `live_rows` y `pane_values`.
//!
//! Latente (Tareas 7a y 7b): ninguna ruta lo llama todavía; la Tarea 8 lo
//! engancha a `/usage/state`. Todo lo que toca tmux, el disco, `/proc` o
//! cc-notifyd corre en tareas o en `spawn_blocking`; nada bloquea el runtime.
use super::super::{
    Fault, Native, catalogs, files,
    states::{PyFloat, gather, py_float, py_str},
    tmux::{Tmux, run_program},
};
use comandos_core::json::{response_dumps, truthy};
use comandos_runtime::{
    Unsure,
    providers::{model_tier, read_conf},
};
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

/// Lo que una expresión del Python hace cuando no da un valor: lanza una
/// excepción que nadie captura (500 de `/usage/state`) o no se sabe con
/// certeza (esa vuelta no escribe nada).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PyFault {
    Raises,
    Unsure,
}

impl From<Unsure> for PyFault {
    fn from(_: Unsure) -> Self {
        Self::Unsure
    }
}

/// `str(x)` de un escalar; el `repr` de un contenedor no se reproduce.
fn text_of(value: &Value) -> Result<String, PyFault> {
    py_str(value).map_err(|_| PyFault::Unsure)
}

/// `tier_style(tier)` (cc-dash:3743) sobre el `model-tiers.json` ya leído (un
/// no objeto es el `{}` de `load_model_tiers`). `tiers` verdadero que no es
/// objeto: ningún tipo de JSON tiene `.get`, el Python lanza → `Raises`.
pub fn tier_style(tiers: &Value, tier: &str) -> Result<Map<String, Value>, PyFault> {
    let all = match tiers.get("tiers") {
        None => return Ok(Map::new()),
        Some(v) if !truthy(v) => return Ok(Map::new()),
        Some(Value::Object(all)) => all,
        Some(_) => return Err(PyFault::Raises),
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
    /// Un símbolo o etiqueta verdaderos que no son texto pasan por el `str()`
    /// del f-string (`1` → `"1"`, `true` → `"True"`); un contenedor (su
    /// `repr`) o un `tiers` que no es objeto → `Unsure`: quien llama no avisa.
    pub fn styled(
        session: &str,
        agent: &str,
        model: &str,
        tier: &str,
        tiers: &Value,
    ) -> Result<Self, Unsure> {
        let style = tier_style(tiers, tier).map_err(|_| Unsure)?;
        let pick = |name: &str, default: &str| match style.get(name) {
            Some(v) if truthy(v) => text_of(v).map_err(|_| Unsure),
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
// Valores de borde
// ---------------------------------------------------------------------------

type Row = Map<String, Value>;

/// `PANE_MODEL_COLORS` (cc-dash:1360): color del borde por agente.
const PANE_MODEL_COLORS: &[(&str, &str)] = &[
    ("claude", "141"),
    ("codex", "43"),
    ("grok", "208"),
    ("opencode", "215"),
    ("gemini", "111"),
    ("agy", "183"),
];

/// `_pane_models_for_live_state` (cc-dash:286): las filas del memo cuyo pane
/// sigue vivo, con el modelo, el agente y el esfuerzo de la tarjeta de
/// `/state` del mismo pane si la tarjeta tiene modelo. `cards` son las
/// tarjetas de `Native::states_cached`; `None` (declinó) → `None`: esa vuelta
/// no escribe bordes (D1; el Python usaría la memoria de su propio `/state`).
pub fn live_rows(live_panes: &[Row], state: &Value, cards: Option<&[Value]>) -> Option<Vec<Row>> {
    let cards = cards?;
    let live: BTreeSet<&str> = live_panes
        .iter()
        .filter_map(|p| p.get("tmux_pane").and_then(Value::as_str))
        .filter(|id| id.starts_with('%'))
        .collect();
    let reconciled = reconciled_cards(cards);
    let memo: &[Value] = match state.get("panes") {
        Some(Value::Array(panes)) => panes,
        _ => &[],
    };
    let mut out = Vec::new();
    for pane in memo {
        // El memo sale de `build_usage_state`: filas objeto con `tmux_pane`
        // de texto. Otra cosa no puede estar entre los vivos.
        let Some(row) = pane.as_object() else {
            continue;
        };
        let Some(id) = row.get("tmux_pane").and_then(Value::as_str) else {
            continue;
        };
        if !live.contains(id) {
            continue;
        }
        let mut row = row.clone();
        if let Some(card) = reconciled.get(id)
            && let Some(model) = card.get("model").filter(|v| truthy(v))
        {
            row.insert("model".into(), model.clone());
            let agent = card
                .get("agent")
                .filter(|v| truthy(v))
                .or_else(|| row.get("agent"))
                .cloned()
                .unwrap_or(Value::Null);
            row.insert("agent".into(), agent.clone());
            row.insert("provider".into(), agent);
            if let Some(effort) = card.get("effort").filter(|v| truthy(v)) {
                row.insert("reasoning_effort".into(), effort.clone());
            }
        }
        out.push(row);
    }
    Some(out)
}

/// `{i.get("pane"): i for i in read_states_cached() if i.get("pane")}` dentro
/// del `try/except Exception: reconciled = {}`: una tarjeta que no es objeto o
/// un `pane` verdadero que no se puede usar de clave (lista, objeto) lanzan y
/// el mapa queda vacío. Las claves que no son texto nunca coinciden con un
/// `tmux_pane` de texto; la última tarjeta de cada pane gana.
fn reconciled_cards(cards: &[Value]) -> HashMap<&str, &Map<String, Value>> {
    let mut out = HashMap::new();
    for card in cards {
        let Some(card) = card.as_object() else {
            return HashMap::new();
        };
        match card.get("pane").filter(|v| truthy(v)) {
            None => {}
            Some(Value::String(pane)) => {
                out.insert(pane.as_str(), card);
            }
            Some(Value::Array(_) | Value::Object(_)) => return HashMap::new(),
            Some(_) => {}
        }
    }
    out
}

/// Valores de borde de una vuelta: `values` por pane (`None` = quitar la
/// opción), el texto de `pane-models.txt` y los avisos de nivel decididos.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PaneValues {
    pub values: BTreeMap<String, Option<String>>,
    pub file_text: String,
    pub alerts: Vec<TierAlert>,
}

/// Lo que `pane_values` deja hacer a quien llama.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PaneOutcome {
    /// Escribir los bordes y enviar los avisos.
    Ready(PaneValues),
    /// Incierto (un archivo de entrada dudoso, un `repr` que no se reproduce):
    /// esta vuelta no escribe nada ni anota avisos; la siguiente, sí.
    Skip,
    /// `_pane_model_values` lanzaría: `/usage/state` es un 500 sin bordes.
    /// Los avisos de los panes anteriores a la excepción ya salieron en sus
    /// hilos en el Python: aquí se devuelven para enviarlos igual.
    Raises(Vec<TierAlert>),
}

/// Un `_maybe_tier_alert` pendiente de aplicar a `TierAlerts`.
#[derive(Debug)]
struct Observation {
    key: TierKey,
    tier: String,
    alert_tier: String,
    /// El aviso con su estilo; `Err` si el `str()` de algún campo no se
    /// reproduce (se anota igual, como el Python, pero no se avisa).
    alert: Result<TierAlert, Unsure>,
}

/// El cálculo sin efectos: valores, texto y observaciones en el orden de los
/// panes, y dónde se paró si se paró.
#[derive(Debug, Default)]
struct Computed {
    values: BTreeMap<String, Option<String>>,
    plain: BTreeMap<String, String>,
    observations: Vec<Observation>,
    halt: Option<PyFault>,
}

impl Computed {
    /// `"\n".join(plain[id] for id in sorted(plain)) + "\n"` si hay alguna
    /// (el orden de `str` del Python es el de los bytes UTF-8).
    fn file_text(&self) -> String {
        let mut text = String::new();
        for line in self.plain.values() {
            text.push_str(line);
            text.push('\n');
        }
        text
    }
}

/// `x or y or ""` sobre claves de la fila: el primer valor verdadero.
fn first_truthy<'a>(row: &'a Row, keys: &[&str]) -> Option<&'a Value> {
    keys.iter().find_map(|k| row.get(*k).filter(|v| truthy(v)))
}

/// `(model or "").replace("claude-", "")` y el último segmento tras `/`.
fn short_model(model: &str) -> String {
    let model = model.replace("claude-", "");
    match model.rsplit_once('/') {
        Some((_, last)) => last.to_owned(),
        None => model,
    }
}

/// `account_for_pid(agent_pid or pid or 0, agent).get("account") or ""`.
fn account_of(
    row: &Row,
    agent: Option<&Value>,
    account: &mut dyn FnMut(i64, &str) -> Result<String, Unsure>,
) -> Result<String, PyFault> {
    // `AGENT_ACCOUNT_ENV.get(agent or '')`: una clave que no se puede hashear
    // lanza `TypeError`; otro tipo no está en el mapa.
    let agent = match agent {
        Some(Value::Array(_) | Value::Object(_)) => return Err(PyFault::Raises),
        Some(Value::String(agent)) if matches!(agent.as_str(), "claude" | "codex" | "grok") => {
            agent.as_str()
        }
        _ => return Ok(String::new()),
    };
    let pid = match first_truthy(row, &["agent_pid", "pid"]) {
        None => return Ok(String::new()),
        // `int(pid)` de un `int`; otro tipo (texto con dígitos, `float`,
        // `True`) no se reproduce.
        Some(Value::Number(n)) => n.as_i64().ok_or(PyFault::Unsure)?,
        Some(_) => return Err(PyFault::Unsure),
    };
    Ok(account(pid, agent)?)
}

/// `_pane_model_values` (cc-dash:4481) para un pane, con los `try` que no
/// tiene: lo que lanzaría (antes o después de anotar el aviso) para la vuelta.
fn one_pane(
    row: &Row,
    tiers: &Value,
    motor: &Map<String, Value>,
    now: f64,
    account: &mut dyn FnMut(i64, &str) -> Result<String, Unsure>,
    out: &mut Computed,
) -> Result<(), PyFault> {
    let pane_id = match row.get("tmux_pane").filter(|v| truthy(v)) {
        None => return Ok(()),
        Some(Value::String(id)) => id.as_str(),
        // `.startswith` de algo que no es texto.
        Some(_) => return Err(PyFault::Raises),
    };
    if !pane_id.starts_with('%') {
        return Ok(());
    }
    let agent = first_truthy(row, &["agent", "provider"]);
    let model = match row.get("model").filter(|v| truthy(v)) {
        None => String::new(),
        Some(Value::String(model)) => short_model(model),
        // `.replace` de algo que no es texto.
        Some(_) => return Err(PyFault::Raises),
    };
    let acct = account_of(row, agent, account)?;
    // `str(agent)`: un contenedor ya lanzó en `account_of`.
    let agent_text = agent.map_or(Ok(String::new()), text_of)?;
    let (head, head_truthy) = if !acct.is_empty() && acct != "main" {
        (format!("{acct} · {agent_text}"), true)
    } else {
        (agent_text.clone(), agent.is_some())
    };
    let session = first_truthy(row, &["tmux_session", "session"]);
    let session_text = session.map_or(Ok(String::new()), text_of)?;
    let pending = match motor.get(&format!("{session_text}|{pane_id}")) {
        Some(Value::Object(pending)) => Some(pending),
        _ => None,
    };
    let switching = match pending {
        Some(p) if p.contains_key("stage") && !p.contains_key("ok") => {
            let ts = match p.get("ts").filter(|v| truthy(v)) {
                None => 0.0,
                Some(ts) => match py_float(ts) {
                    PyFloat::Value(ts) => ts,
                    PyFloat::Raises => return Err(PyFault::Raises),
                    PyFloat::Unsure => return Err(PyFault::Unsure),
                },
            };
            now - ts < 300.0
        }
        _ => false,
    };
    let tier = model_tier(tiers, &model)?;
    let style = tier_style(tiers, &tier)?;
    if !model.is_empty() {
        observe_pane(session, &agent_text, &model, &tier, pane_id, tiers, out)?;
    }
    let color = match agent {
        Some(Value::String(agent)) => PANE_MODEL_COLORS
            .iter()
            .find(|(name, _)| name == agent)
            .map_or("244", |(_, color)| color),
        _ => "244",
    };
    let sym = match style.get("symbol").filter(|v| truthy(v)) {
        Some(sym) => text_of(sym)?,
        None => String::new(),
    };
    let suffix = if sym.is_empty() {
        String::new()
    } else {
        format!(" {sym}")
    };
    let id = pane_id.to_owned();
    if switching {
        let requested = match pending.and_then(|p| p.get("model")).filter(|v| truthy(v)) {
            Some(model) => text_of(model)?,
            None => String::new(),
        };
        let target = short_model(&requested);
        let target = if target.is_empty() { "…" } else { &target };
        out.values.insert(
            id.clone(),
            Some(format!(
                "#[fg=colour{color},bold]▸ {head}#[default]\
                 #[fg=colour214,bold] · cambiando → {target}#[default]"
            )),
        );
        out.plain
            .insert(id, format!("{pane_id} {head} · cambiando → {target}"));
    } else if !model.is_empty() {
        let mcolor = match style.get("tmux").filter(|v| truthy(v)) {
            Some(tmux) => text_of(tmux)?,
            None => color.to_owned(),
        };
        out.values.insert(
            id.clone(),
            Some(format!(
                "#[fg=colour{color},bold]▸ {head}#[default]\
                 #[fg=colour{mcolor},bold] · {model}{suffix}#[default]"
            )),
        );
        out.plain
            .insert(id, format!("{pane_id} {head} · {model}{suffix}"));
    } else if agent
        .and_then(Value::as_str)
        .is_some_and(|a| matches!(a, "claude" | "codex" | "grok"))
    {
        out.values.insert(
            id.clone(),
            Some(format!(
                "#[fg=colour{color},bold]▸ {head}#[default]\
                 #[fg=colour244] · detectando…#[default]"
            )),
        );
        out.plain
            .insert(id, format!("{pane_id} {head} · detectando…"));
    } else if head_truthy {
        out.values.insert(
            id.clone(),
            Some(format!("#[fg=colour{color},bold]▸ {head}#[default]")),
        );
        out.plain.insert(id, format!("{pane_id} {head}"));
    } else {
        // Sin `plain`: una línea anterior del mismo pane se queda, como en
        // el Python.
        out.values.insert(id, None);
    }
    Ok(())
}

/// La parte de `_maybe_tier_alert` (cc-dash:4440) que se decide al calcular:
/// sin sesión o sin nivel no se anota nada; si no, una observación con el
/// `alertTier` vigente y el aviso ya estilado.
fn observe_pane(
    session: Option<&Value>,
    agent: &str,
    model: &str,
    tier: &str,
    pane_id: &str,
    tiers: &Value,
    out: &mut Computed,
) -> Result<(), PyFault> {
    let Some(session) = session else {
        return Ok(());
    };
    if tier.is_empty() {
        return Ok(());
    }
    // Clave de `dict`: una sesión que no es texto no se reproduce.
    let session = session.as_str().ok_or(PyFault::Unsure)?;
    // `cfg.get("alertTier") or "high"`; un valor que no es texto nunca es
    // igual al nivel (texto no vacío): `""` hace lo mismo en `observe`.
    let alert_tier = match tiers.get("alertTier").filter(|v| truthy(v)) {
        None => "high".to_owned(),
        Some(Value::String(alert)) => alert.clone(),
        Some(_) => String::new(),
    };
    out.observations.push(Observation {
        key: (session.to_owned(), pane_id.to_owned()),
        tier: tier.to_owned(),
        alert_tier,
        alert: TierAlert::styled(session, agent, model, tier, tiers),
    });
    Ok(())
}

/// `_pane_model_values` sin efectos: se para en el primer pane que lanzaría
/// o es incierto.
fn compute_values(
    rows: &[Row],
    tiers: &Value,
    motor: &Map<String, Value>,
    now: f64,
    account: &mut dyn FnMut(i64, &str) -> Result<String, Unsure>,
) -> Computed {
    let mut out = Computed::default();
    for row in rows {
        if let Err(fault) = one_pane(row, tiers, motor, now, account, &mut out) {
            out.halt = Some(fault);
            break;
        }
    }
    out
}

/// `_pane_model_values(panes)` (cc-dash:4481) sobre las filas de `live_rows`:
/// cuenta por `account_for_pid` (caché propia del escritor, B9: nunca la del
/// escaneo de `/state`), `switching` con `H/motor-results.json` (D1: archivo
/// incierto → `Skip`), nivel y estilo con `config/model-tiers.json` (ausente o
/// incierto → `Skip`) y `_maybe_tier_alert` en el orden de los panes. Todo lo
/// que lee el disco o `/proc` va en un salto de bloqueo; las observaciones se
/// aplican después bajo el candado de `TierAlerts`, sin `await` en medio.
/// `tmux_panes` son TODOS los panes vivos de tmux de la vuelta (`None` si no
/// se pudieron listar): con ellos se acota `TierAlerts` (regla 6).
pub async fn pane_values(
    native: &Native,
    rows: &[Row],
    tmux_panes: Option<&BTreeSet<String>>,
) -> PaneOutcome {
    let opts = native.options().clone();
    // `time.time()` del Python, en segundos con fracción.
    let now = (opts.clock)() as f64 / 1000.0;
    let rows = rows.to_vec();
    let accounts = native.pane_accounts.clone();
    let computed = tokio::task::spawn_blocking(move || {
        let motor = gather::motor_results(&opts.hooks).ok()?;
        let tiers = catalogs::read_model_tiers(&opts).ok()?;
        let mut cache = accounts.lock().unwrap_or_else(|p| p.into_inner());
        let mut account = |pid: i64, agent: &str| -> Result<String, Unsure> {
            let obs = cache.account_for_pid(&opts.home, &opts.proc_root, pid, agent)?;
            Ok(match obs.get("account") {
                Some(Value::String(alias)) => alias.clone(),
                _ => String::new(),
            })
        };
        Some(compute_values(&rows, &tiers, &motor, now, &mut account))
    })
    .await;
    let Ok(Some(computed)) = computed else {
        return PaneOutcome::Skip;
    };
    if computed.halt == Some(PyFault::Unsure) {
        return PaneOutcome::Skip;
    }
    let file_text = computed.file_text();
    let mut alerts = Vec::new();
    {
        let mut memory = native.tier_alerts();
        for obs in computed.observations {
            if memory.observe(&obs.key, &obs.tier, &obs.alert_tier, now)
                && let Ok(alert) = obs.alert
            {
                alerts.push(alert);
            }
        }
        if computed.halt.is_none()
            && let Some(live) = tmux_panes
        {
            memory.prune(live, now);
        }
    }
    if computed.halt == Some(PyFault::Raises) {
        return PaneOutcome::Raises(alerts);
    }
    PaneOutcome::Ready(PaneValues {
        values: computed.values,
        file_text,
        alerts,
    })
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
    /// tarea. Nunca espera a tmux. El candado del archivo se sostiene hasta
    /// haber anotado `desired` (el `_pane_model_lock` único del Python): dos
    /// llamadas a la vez no pueden dejar el archivo de una y los valores de
    /// la otra.
    pub async fn apply(
        self: &Arc<Self>,
        values: BTreeMap<String, Option<String>>,
        file_text: String,
        tmux: Tmux,
        hooks: PathBuf,
    ) {
        let file_guard = self.file.lock().await;
        let changed = self.lock().file_text.as_deref() != Some(file_text.as_str());
        if changed {
            let path = hooks.join("pane-models.txt");
            let text = file_text.clone();
            let written =
                tokio::task::spawn_blocking(move || files::write_text_atomic(&path, &text)).await;
            if matches!(written, Ok(Ok(()))) {
                self.lock().file_text = Some(file_text);
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
        drop(file_guard);
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
