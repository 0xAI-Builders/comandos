//! H. GET /model/status (8325, `session_operation_status` 6217) sobre el
//! journal `H/session-operations.sqlite3`. Sin registro (el `MOTOR_RESULT`
//! vive en la memoria del Python) o con `awaiting_confirmation`
//! (`refresh_session_confirmation` observa el proceso y escribe) se declina.
//! `recover_abandoned` va antes, como en el Python: es idempotente, así que
//! declinar después equivale a declinar antes.
//!
//! El dueño de una operación en curso es el proceso que la reclamó (el Python
//! heredado o cc-app), nunca el frente: la recuperación pregunta por el pid
//! guardado en la fila, y solo la marca abandonada si ese pid ya no existe.
use super::{Answer, Entry, Fault, Key, Native, NativeRoute, Verb, light, py, query::Query};
use comandos_core::json::truthy;
use comandos_runtime::session_operations::{self as ops, OperationStore};
use http::StatusCode;
use rusqlite::{Connection, OptionalExtension};
use serde_json::{Map, Value};
use std::path::Path;

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
}

pub async fn answer(native: &Native, request: &crate::Request) -> Answer {
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
    let (session, pane) = (session.to_owned(), pane.to_owned());
    let clock = native.options().clock.clone();
    let status = native
        .journal
        .with(move |journal| {
            let now = move || clock() as f64 / 1000.0;
            status_in(&journal.conn, &session, &pane, &operation_id, &now)
        })
        .await?;
    match status {
        Status::Ok(mut result) => {
            result.insert("operationKey".into(), Value::String(operation));
            // Lo leído que el codificador portado no pueda escribir se reenvía.
            light::read_reply(&Value::Object(result))
        }
        Status::BadRequest(message) => light::error(StatusCode::BAD_REQUEST, message),
        Status::Decline => Err(Fault::Decline),
    }
}

/// `os.kill(pid, 0)`: `ProcessLookupError` → muerto; `PermissionError` → vivo.
/// En Linux equivale a que exista `/proc/<pid>` (también hilos y zombis). Solo
/// un `NotFound` cierto dice «muerto»; cualquier otra duda dice «vivo».
fn alive(pid: i64) -> ops::Result<bool> {
    // `os.kill(0, 0)` señala al propio grupo de procesos: siempre vivo.
    if pid == 0 {
        return Ok(true);
    }
    match std::fs::symlink_metadata(format!("/proc/{pid}")) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        _ => Ok(true),
    }
}

/// Antes de recuperar: `/proc` debe ser el de nuestro espacio de pids (aparece
/// el propio pid) y ningún dueño pendiente puede salirse de lo que `os.kill`
/// trata como un pid (negativo = grupo de procesos; fuera de `pid_t` =
/// `OverflowError`). Si no, se declina sin escribir nada.
fn recovery_is_certain(conn: &Connection) -> Result<bool, rusqlite::Error> {
    if !Path::new(&format!("/proc/{}", std::process::id())).exists() {
        return Ok(false);
    }
    let unusual: Option<i64> = conn
        .query_row(
            "SELECT 1 FROM session_operations WHERE state NOT IN \
             ('confirmed','failed','rolled_back','recovery_required','awaiting_confirmation') \
             AND (typeof(owner) != 'integer' OR owner < 0 OR owner > 2147483647) LIMIT 1",
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
        return Status::Decline;
    };
    let Some(state) = record.get("state").and_then(Value::as_str) else {
        return Status::Decline;
    };
    if state == "awaiting_confirmation" {
        return Status::Decline;
    }
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
