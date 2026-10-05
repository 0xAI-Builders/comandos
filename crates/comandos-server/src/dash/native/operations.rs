//! H. GET /model/status (8325, `session_operation_status` 6217) sobre el
//! journal y MotorResults. Los casos sin fila o pendientes de confirmación
//! pertenecen al corte Ops; con ese corte apagado se conservan sus Decline.
//! `recover_abandoned` va antes, como en el Python: es idempotente, así que
//! declinar después equivale a declinar antes.
//!
//! El dueño de una operación en curso es el proceso que la reclamó (el Python
//! heredado o cc-app), nunca el frente: la recuperación pregunta por el pid
//! guardado en la fila, y solo la marca abandonada si ese pid ya no existe.
use super::{Answer, Entry, Fault, Key, Native, NativeRoute, Verb, light, py, query::Query};
use super::{
    Cut,
    ops::{configure, results::MotorResults},
};
use comandos_core::json::truthy;
use comandos_runtime::session_operations::{self as ops, OperationStore};
use http::StatusCode;
use rusqlite::{Connection, OptionalExtension};
use serde_json::{Map, Value, json};
use std::sync::Arc;

pub const ROUTES: &[Entry] = &[Entry {
    verb: Verb::Get,
    key: Key::Path("/model/status"),
    route: NativeRoute::ModelStatus,
}];

const TERMINAL: [&str; 4] = ["confirmed", "failed", "rolled_back", "recovery_required"];
const PANEL_MISMATCH: &str = "La operación no pertenece a este panel";

enum Status {
    Ok(Map<String, Value>),
    BadRequest(&'static str),
    Decline,
    Missing,
    Pending(Value),
}

pub async fn answer(native: &Arc<Native>, request: &crate::Request) -> Answer {
    let query = Query::parse(&request.target)?;
    // `str((query.get(k) or [""])[0])`.
    let operation = query.first("operationKey").unwrap_or("").to_owned();
    let operation_id = query.first("operationId").unwrap_or("").to_owned();
    // `str(operation).partition('|')`.
    let (session, pane) = operation
        .split_once('|')
        .unwrap_or((operation.as_str(), ""));
    if !py::is_session(session) || (!pane.is_empty() && !py::is_pane(pane).ok_or(Fault::Decline)?) {
        return light::error(StatusCode::BAD_REQUEST, "operationKey inválido");
    }
    // `re.fullmatch(r'[A-Za-z0-9_-]{1,128}', operation_id)`: clase ASCII explícita.
    let id_ok = (1..=128).contains(&operation_id.len())
        && operation_id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-');
    if !operation_id.is_empty() && !id_ok {
        return light::error(StatusCode::BAD_REQUEST, "operationId inválido");
    }
    let enabled = !native.options().cuts_off.contains(&Cut::Ops);
    let results = MotorResults::shared(native.options());
    // MotorResults debe ser legible ANTES del barrido que puede escribir.
    let cached = if enabled {
        let results = Arc::clone(&results);
        Some(
            tokio::task::spawn_blocking(move || {
                if !results.certain() {
                    return Err(Fault::Decline);
                }
                Ok(results.all())
            })
            .await
            .map_err(|_| Fault::Error(crate::HandlerError::Failure))??,
        )
    } else {
        None
    };
    let (session, pane) = (session.to_owned(), pane.to_owned());
    let clock = native.options().clock_seconds.clone();
    let status = native
        .journal
        .with(move |journal| status_in(&journal.conn, &session, &pane, &operation_id, &*clock))
        .await?;
    let status = match status {
        Status::Missing if enabled => {
            let all = cached.as_ref().expect("checked above");
            let mut result = object_or_empty(all.get(&operation)).ok_or(Fault::Decline)?;
            if result.is_empty() {
                result = json!({"stage":"preparando cambio", "stageCode":"prepare", "ts":0})
                    .as_object()
                    .expect("object")
                    .clone();
            }
            Status::Ok(result)
        }
        Status::Pending(row) if enabled => {
            let row = configure::refresh_session_confirmation(native, row).await?;
            result_from_record(row)
        }
        Status::Missing | Status::Pending(_) => Status::Decline,
        other => other,
    };
    match status {
        Status::Ok(mut result) => {
            result.insert("operationKey".into(), Value::String(operation));
            // Lo leído que el codificador portado no pueda escribir se reenvía.
            light::read_reply(&Value::Object(result))
        }
        Status::BadRequest(message) => light::error(StatusCode::BAD_REQUEST, message),
        Status::Decline | Status::Missing | Status::Pending(_) => Err(Fault::Decline),
    }
}

/// `os.kill(pid, 0)` de `recover_abandoned` (`lib/session_operations.py:169`),
/// la misma llamada: `ESRCH` (`ProcessLookupError`) → muerto; `EPERM`
/// (`PermissionError`) o éxito → vivo. 0 es el propio grupo de procesos y un
/// negativo es un grupo, igual que en el Python. Cualquier otro error haría
/// subir una excepción en el Python (500): se reenvía sin escribir.
pub(crate) fn alive(pid: i64) -> ops::Result<bool> {
    use nix::{errno::Errno, sys::signal::kill, unistd::Pid};
    // Fuera de `pid_t` el Python lanza `OverflowError`: `recovery_is_certain`
    // ya lo declinó; si una fila nueva cuela entre medias, «vivo» no escribe.
    let Ok(raw) = i32::try_from(pid) else {
        return Ok(true);
    };
    match kill(Pid::from_raw(raw), None) {
        Ok(()) | Err(Errno::EPERM) => Ok(true),
        Err(Errno::ESRCH) => Ok(false),
        Err(other) => Err(ops::Error::Callback(other.to_string())),
    }
}

/// Antes de recuperar: ningún dueño pendiente puede salirse de lo que
/// `os.kill` acepta como pid (un no entero da `TypeError`; fuera de `pid_t`,
/// `OverflowError`: el Python respondería 500). Si no, se declina sin escribir.
pub(crate) fn recovery_is_certain(conn: &Connection) -> Result<bool, rusqlite::Error> {
    let unusual: Option<i64> = conn
        .query_row(
            "SELECT 1 FROM session_operations WHERE state NOT IN \
             ('confirmed','failed','rolled_back','recovery_required','awaiting_confirmation') \
             AND (typeof(owner) != 'integer' OR owner < -2147483648 OR owner > 2147483647) LIMIT 1",
            [],
            |r| r.get(0),
        )
        .optional()?;
    Ok(unusual.is_none())
}

/// `x or {}` donde `x` debe admitir `.get`/`update`: un no-dict verdadero haría
/// otra cosa en el Python (500 o un 400 con el texto de la excepción) → `None`.
fn object_or_empty(value: Option<&Value>) -> Option<Map<String, Value>> {
    match value {
        Some(Value::Object(map)) => Some(map.clone()),
        Some(v) if truthy(v) => None,
        _ => Some(Map::new()),
    }
}

/// `x or ''`.
fn or_empty(value: Option<&Value>) -> Value {
    match value {
        Some(v) if truthy(v) => v.clone(),
        _ => Value::String(String::new()),
    }
}

fn status_in(
    conn: &Connection,
    session: &str,
    pane: &str,
    operation_id: &str,
    now: &dyn Fn() -> f64,
) -> Status {
    // El dueño solo lo usan `claim`/`claim_recovery`, que aquí no se llaman.
    let owner = || 0i64;
    let store = OperationStore {
        connection: conn,
        owner: &owner,
        clock: now,
    };
    // Un fallo de la base (p. ej. ocupada más de 15 s) también se reenvía: lo
    // que se recuperó ya es lo que el Python recuperaría, y él repite el barrido.
    match recovery_is_certain(conn) {
        Ok(true) => {}
        Ok(false) | Err(_) => return Status::Decline,
    }
    if store.recover_abandoned(alive).is_err() {
        return Status::Decline;
    }
    let record = if operation_id.is_empty() {
        store.latest_for_target(session, pane)
    } else {
        store.get(operation_id)
    };
    // JSON ilegible en el registro: el Python responde 400 con el texto de
    // `json.loads`; se reenvía.
    let Ok(record) = record else {
        return Status::Decline;
    };
    if !operation_id.is_empty() {
        let Some(found) = &record else {
            return Status::BadRequest(PANEL_MISMATCH);
        };
        // `record['request'].get(...)` sobre un no-dict: 500 en el Python.
        let Some(Value::Object(req)) = found.get("request") else {
            return Status::Decline;
        };
        let same_session = req.get("session") == Some(&Value::String(session.to_owned()));
        // `request.get('pane') or ''`.
        let same_pane = match req.get("pane") {
            Some(v) if truthy(v) => v == &Value::String(pane.to_owned()),
            _ => pane.is_empty(),
        };
        if !same_session || !same_pane {
            return Status::BadRequest(PANEL_MISMATCH);
        }
    }
    let Some(record) = record else {
        return Status::Missing;
    };
    if record.get("state").and_then(Value::as_str) == Some("awaiting_confirmation") {
        return Status::Pending(record);
    }
    result_from_record(record)
}

fn result_from_record(record: Value) -> Status {
    let Some(state) = record.get("state").and_then(Value::as_str) else {
        return Status::Decline;
    };
    let (Some(id), Some(updated)) = (record.get("id"), record.get("updated")) else {
        return Status::Decline;
    };
    let mut result = Map::new();
    result.insert("operationId".into(), id.clone());
    result.insert("state".into(), Value::String(state.to_owned()));
    result.insert("ts".into(), updated.clone());
    let Some(durable) = object_or_empty(record.get("result")) else {
        return Status::Decline;
    };
    // `dict.update`: una clave existente conserva su posición.
    for (k, v) in &durable {
        result.insert(k.clone(), v.clone());
    }
    if TERMINAL.contains(&state) {
        let detail = match durable.get("error") {
            Some(v) if truthy(v) => v.clone(),
            _ => Value::String("configuración confirmada".into()),
        };
        result.insert("detail".into(), detail);
        let Some(observed) = object_or_empty(durable.get("observed")) else {
            return Status::Decline;
        };
        for (k, v) in observed {
            result.insert(k, v);
        }
    } else {
        // `result.pop('ok', None)`: las demás claves conservan su orden.
        result.shift_remove("ok");
        result.insert("stage".into(), Value::String(state.to_owned()));
        result.insert("stageCode".into(), Value::String(state.to_owned()));
    }
    let Some(snapshot) = object_or_empty(record.get("snapshot")) else {
        return Status::Decline;
    };
    let Some(origin) = object_or_empty(snapshot.get("origin")) else {
        return Status::Decline;
    };
    let Some(observed) = object_or_empty(origin.get("observed")) else {
        return Status::Decline;
    };
    result.insert("sourceHarness".into(), or_empty(origin.get("agent")));
    result.insert(
        "sourceConversationId".into(),
        or_empty(observed.get("conversationId")),
    );
    result.insert("handoffPath".into(), or_empty(origin.get("handoffPath")));
    Status::Ok(result)
}
