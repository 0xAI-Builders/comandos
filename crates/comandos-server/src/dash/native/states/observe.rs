//! `observe_pane` (6788) sin efectos: la evidencia llega recogida (Tarea 5) y
//! aquí se arma el resultado en el orden del Python; `reconcile_card_config`
//! (6884) sobre la tarjeta.
use super::{StateFault, is_text, or_default, py_str, str_or_empty};
use comandos_core::json::{python_eq, truthy};
use comandos_runtime::{
    hooks::py::float_value,
    providers::engine_for_model,
    tui_state::{Obs, StateTracker},
};
use serde_json::{Map, Value};

/// Lo que `observe_pane` devuelve dentro del `try` de `reconcile_card_config`.
#[derive(Debug, Clone, PartialEq)]
pub enum Observed {
    Seen(Obs),
    /// `OSError`/`ValueError`/`RuntimeError` capturados (el panel ya no existe,
    /// no pertenece a la sesión…): `{"confirmed": false, …, "source": "unconfirmed"}`.
    Unconfirmed,
}

/// Lo que la Tarea 5 recoge para `observe_pane`, en su orden.
#[derive(Debug, Clone)]
pub struct ObserveEvidence {
    pub agent: String,
    pub pid: i64,
    /// `_identity_key(identity)`.
    pub identity: String,
    /// `inspector({...})` de la Tarea 2.
    pub snap: Obs,
    /// `account_for_pid(pid, agent).get('account') or 'unknown'`.
    pub account: String,
    /// `capabilities.accounts` del arnés en el registro.
    pub harness_has_accounts: bool,
    pub cmdline: Vec<String>,
    /// codex: contexto del rollout raíz (`{}` si ninguno trae modelo); claude:
    /// transcript único (`{}` si no); grok/opencode/agy: `project_metadata`.
    /// Solo cuenta para esos arneses (y codex/claude con `conversationId`).
    pub conversation: Obs,
    /// acp: `acp_state_for_pane(pane)`; el filtro de pid y sesión se aplica aquí.
    pub acp: Obs,
    /// `pane_visible_config` (solo codex, claude y opencode lo usan).
    pub visible: Obs,
    /// `_process_start(pid)`.
    pub process_start: String,
    /// `ANTHROPIC_BASE_URL` del `environ` decodificado con `replace` (`""` si falta).
    pub base_url: String,
}

/// `snap.get('resume_id') or (snap.get('acp') or {}).get('sessionId') or ''`.
/// Un `acp` verdadero que no es objeto es el `AttributeError` del Python, que
/// `reconcile_card_config` no captura.
pub fn conversation_id(snap: &Obs) -> Result<Value, StateFault> {
    if let Some(id) = snap.get("resume_id").filter(|v| truthy(v)) {
        return Ok(id.clone());
    }
    let session = match snap.get("acp").filter(|v| truthy(v)) {
        None => None,
        Some(Value::Object(acp)) => acp.get("sessionId"),
        Some(_) => return Err(StateFault::Failure),
    };
    Ok(or_default(session, ""))
}

/// `dict(model=…, effort=…, revision=metadata.get(revision) or '')` si el
/// `sessionId` de los metadatos es la conversación del pane; si no, `{}`.
pub fn project_metadata(metadata: &Obs, conversation_id: &Value, revision: &str) -> Obs {
    let mut out = Map::new();
    if python_eq(
        metadata.get("sessionId").unwrap_or(&Value::Null),
        conversation_id,
    ) {
        out.insert("model".into(), or_default(metadata.get("model"), ""));
        out.insert("effort".into(), or_default(metadata.get("effort"), ""));
        out.insert("revision".into(), or_default(metadata.get(revision), ""));
    }
    out
}

/// `re.fullmatch(r'model_reasoning_effort=["\']?([a-z]+)["\']?', value)`.
fn reasoning_effort(value: &str) -> Option<&str> {
    let rest = value.strip_prefix("model_reasoning_effort=")?;
    let rest = rest.strip_prefix(['"', '\'']).unwrap_or(rest);
    let end = rest
        .bytes()
        .position(|b| !b.is_ascii_lowercase())
        .unwrap_or(rest.len());
    let (word, tail) = rest.split_at(end);
    (!word.is_empty() && matches!(tail, "" | "\"" | "'")).then_some(word)
}

/// `(model, effort)` pedidos en la línea de órdenes del proceso.
fn launch_args(args: &[String]) -> (String, String) {
    let (mut model, mut effort) = (String::new(), String::new());
    for pair in args.windows(2) {
        let [arg, value] = pair else { continue };
        if value.starts_with('-') {
            continue;
        }
        match arg.as_str() {
            "--model" | "-m" => model.clone_from(value),
            "--effort" => effort.clone_from(value),
            "-c" | "--config" => {
                if let Some(word) = reasoning_effort(value) {
                    effort = word.to_owned();
                }
            }
            _ => {}
        }
    }
    (model, effort)
}

fn text(s: &str) -> Value {
    Value::from(s)
}

/// `observe_pane` desde `result = {...}` hasta el final.
pub fn observe(
    evidence: &ObserveEvidence,
    tracker: &mut StateTracker,
    registry: &Value,
    now: f64,
) -> Result<Obs, StateFault> {
    let agent = evidence.agent.as_str();
    let conversation_id = conversation_id(&evidence.snap)?;
    let mut result = Map::new();
    result.insert("harness".into(), text(agent));
    result.insert("pid".into(), Value::from(evidence.pid));
    result.insert("conversationId".into(), conversation_id.clone());
    result.insert("model".into(), text(""));
    result.insert("effort".into(), text(""));
    result.insert("source".into(), text("unconfirmed"));
    result.insert("observedAt".into(), float_value(now));
    result.insert("identity".into(), text(&evidence.identity));
    let account = if evidence.harness_has_accounts {
        evidence.account.clone()
    } else {
        "main".to_owned()
    };
    result.insert("harnessAccount".into(), text(&account));
    let (mut launch_model, mut launch_effort) = launch_args(&evidence.cmdline);
    let has_conversation = truthy(&conversation_id);
    let conversation = match agent {
        "codex" | "claude" if has_conversation => evidence.conversation.clone(),
        "grok" | "opencode" | "agy" => evidence.conversation.clone(),
        "acp" => {
            // Los argumentos son peticiones: solo el protocolo del mismo proceso confirma.
            launch_model.clear();
            launch_effort.clear();
            let empty = Map::new();
            let same = python_eq(
                evidence.acp.get("pid").unwrap_or(&Value::Null),
                &Value::from(evidence.pid),
            ) && python_eq(
                evidence.acp.get("sessionId").unwrap_or(&Value::Null),
                &conversation_id,
            );
            let metadata = if same { &evidence.acp } else { &empty };
            let mut conversation = Map::new();
            conversation.insert(
                "model".into(),
                or_default(metadata.get("observedModel"), ""),
            );
            conversation.insert(
                "effort".into(),
                or_default(metadata.get("observedEffort"), ""),
            );
            let requested = |primary: &str, fallback: &str| {
                metadata
                    .get(primary)
                    .filter(|v| truthy(v))
                    .or_else(|| metadata.get(fallback).filter(|v| truthy(v)))
                    .cloned()
                    .unwrap_or_else(|| text(""))
            };
            result.insert("motor".into(), or_default(metadata.get("agent"), ""));
            result.insert(
                "motorAccount".into(),
                or_default(metadata.get("account"), "unknown"),
            );
            result.insert("harnessAccount".into(), text("main"));
            result.insert(
                "effortSource".into(),
                or_default(metadata.get("effortSource"), "unconfirmed"),
            );
            result.insert(
                "requestedModel".into(),
                requested("requestedModel", "model"),
            );
            result.insert(
                "requestedEffort".into(),
                requested("requestedEffort", "effort"),
            );
            conversation
        }
        _ => Map::new(),
    };
    let visible = if matches!(agent, "codex" | "claude" | "opencode") {
        evidence.visible.clone()
    } else {
        Map::new()
    };
    let mut launch = Map::new();
    launch.insert("model".into(), text(&launch_model));
    launch.insert("effort".into(), text(&launch_effort));
    // La clave del Python es la tupla (identidad, agente, pid, inicio, conversación).
    let key = format!(
        "{}\u{1f}{agent}\u{1f}{}\u{1f}{}\u{1f}{conversation_id}",
        evidence.identity, evidence.pid, evidence.process_start
    );
    for (field, value) in tracker.observe(&key, &launch, &conversation, &visible, now) {
        result.insert(field, value);
    }
    let motor = match result.get("motor").filter(|v| truthy(v)) {
        Some(motor) => motor.clone(),
        None if matches!(agent, "opencode" | "agy" | "gemini") => text(agent),
        None => {
            let model = str_or_empty(result.get("model"))?;
            let engine = engine_for_model(registry, &model)?;
            text(if engine.is_empty() { agent } else { &engine })
        }
    };
    result.insert("motor".into(), motor.clone());
    let motor_account = match result.get("motorAccount").filter(|v| truthy(v)) {
        Some(existing) => existing.clone(),
        None if python_eq(&motor, &text(agent)) => text(&account),
        None => text("unknown"),
    };
    result.insert("motorAccount".into(), motor_account);
    if agent == "claude"
        && (is_text(Some(&motor), "codex") || is_text(Some(&motor), "grok"))
        && (evidence.base_url.starts_with("http://127.0.0.1:")
            || evidence.base_url.starts_with("http://localhost:"))
    {
        result.insert("motorAccount".into(), text("main"));
    }
    let model_set = result.get("model").is_some_and(truthy);
    let source = result.get("source");
    let confirmed = model_set
        && !["process", "unconfirmed", "status-script"]
            .iter()
            .any(|s| is_text(source, s));
    result.insert("confirmed".into(), Value::Bool(confirmed));
    result.insert(
        "accountSource".into(),
        text(if agent == "acp" {
            "acp-state"
        } else {
            "process-environment"
        }),
    );
    let mut limitations = Vec::new();
    if matches!(agent, "opencode" | "agy" | "gemini") && visible.is_empty() {
        limitations.push(text("native-runtime-state-unavailable"));
    }
    if !result.get("effort").is_some_and(truthy) {
        limitations.push(text("effort-unobserved"));
    }
    result.insert("limitations".into(), Value::Array(limitations));
    Ok(result)
}

/// `reconcile_card_config` (6884) con la observación ya hecha y la última
/// configuración confirmada (`None` o `{}` = `None`). `routeId` formatea el
/// agente y el motor con `str()`: un contenedor declina.
pub fn reconcile(
    item: &mut Map<String, Value>,
    observed: &Observed,
    config: Option<Obs>,
) -> Result<(), StateFault> {
    let observed = match observed {
        Observed::Seen(seen) => seen.clone(),
        Observed::Unconfirmed => {
            let mut fallback = Map::new();
            fallback.insert("confirmed".into(), Value::Bool(false));
            fallback.insert("model".into(), text(""));
            fallback.insert("effort".into(), text(""));
            fallback.insert("source".into(), text("unconfirmed"));
            fallback
        }
    };
    let get =
        |key: &str, default: &str| observed.get(key).cloned().unwrap_or_else(|| text(default));
    item.insert("observedConfig".into(), Value::Object(observed.clone()));
    item.insert(
        "lastConfirmedConfig".into(),
        config
            .filter(|c| !c.is_empty())
            .map_or(Value::Null, Value::Object),
    );
    item.insert("model".into(), get("model", ""));
    item.insert("effort".into(), get("effort", ""));
    item.insert("modelSource".into(), get("source", "unconfirmed"));
    item.insert(
        "configConfirmed".into(),
        Value::Bool(observed.get("confirmed").is_some_and(truthy)),
    );
    item.insert("agentSessionId".into(), get("conversationId", ""));
    if let Some(harness) = observed.get("harness").filter(|v| truthy(v)) {
        item.insert("agent".into(), harness.clone());
    }
    for key in ["motor", "harnessAccount", "motorAccount"] {
        item.insert(key.into(), or_default(observed.get(key), "unknown"));
    }
    let agent = str_or_empty(item.get("agent"))?;
    let motor = py_str(item.get("motor").unwrap_or(&Value::Null))?;
    item.insert("routeId".into(), Value::from(format!("{agent}:{motor}")));
    let harness_account = item.get("harnessAccount").cloned().unwrap_or(Value::Null);
    item.insert("account".into(), harness_account);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reasoning_effort_fullmatch() {
        assert_eq!(
            reasoning_effort("model_reasoning_effort=high"),
            Some("high")
        );
        assert_eq!(
            reasoning_effort("model_reasoning_effort=\"low\""),
            Some("low")
        );
        assert_eq!(
            reasoning_effort("model_reasoning_effort='max\""),
            Some("max")
        );
        assert_eq!(reasoning_effort("model_reasoning_effort=x'"), Some("x"));
        assert_eq!(reasoning_effort("model_reasoning_effort=\""), None);
        assert_eq!(reasoning_effort("model_reasoning_effort=High"), None);
        assert_eq!(reasoning_effort("model_reasoning_effort=a''"), None);
    }
}
