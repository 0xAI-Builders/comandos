//! `reconcile_card_config` (6884) sobre la tarjeta. `observe_pane` (6788) sin
//! efectos —la evidencia, el resultado en el orden del Python y sus lectores—
//! vive en `comandos_runtime::pane_observe` (movido allí sin cambios en la
//! 2f-2/T2 para el adaptador de operaciones de sesión); aquí se reexporta.
use super::{StateFault, or_default, py_str, str_or_empty};
use comandos_core::json::truthy;
pub use comandos_runtime::pane_observe::{
    ObserveEvidence, ObserveFault, conversation_id, observe, project_metadata,
};
use comandos_runtime::tui_state::Obs;
use serde_json::{Map, Value};

impl From<ObserveFault> for StateFault {
    fn from(fault: ObserveFault) -> Self {
        match fault {
            ObserveFault::Decline => StateFault::Decline,
            ObserveFault::Failure => StateFault::Failure,
        }
    }
}

/// Lo que `observe_pane` devuelve dentro del `try` de `reconcile_card_config`.
#[derive(Debug, Clone, PartialEq)]
pub enum Observed {
    Seen(Obs),
    /// `OSError`/`ValueError`/`RuntimeError` capturados (el panel ya no existe,
    /// no pertenece a la sesión…): `{"confirmed": false, …, "source": "unconfirmed"}`.
    Unconfirmed,
}

fn text(s: &str) -> Value {
    Value::from(s)
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
