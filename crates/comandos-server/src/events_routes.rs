//! Four dashboard routes, called synchronously by a connection-owning worker.
use crate::{HandlerError, Reply, Request};
use comandos_core::{dashboard_access, hook, json::truthy, turn};
use comandos_store::{
    Error,
    intake::{self, Reception},
    marks,
};
use http::{Method, StatusCode};
use rusqlite::Connection;
use serde_json::{Map, Value, json};
use std::path::PathBuf;

/// Native observations required by event intake and human-mark writes.
/// Implementations must not discover database or legacy timeline paths.
pub trait Facts {
    fn now_ms(&mut self) -> comandos_store::Result<u64>;
    fn fresh_id(&mut self, prefix: &str) -> comandos_store::Result<String>;
    fn process_start(&mut self, pid: &str) -> Option<String>;
}

pub struct NativeFacts;

impl Facts for NativeFacts {
    fn now_ms(&mut self) -> comandos_store::Result<u64> {
        comandos_runtime::now_ms()
    }
    fn fresh_id(&mut self, prefix: &str) -> comandos_store::Result<String> {
        comandos_runtime::fresh_id(prefix)
    }
    fn process_start(&mut self, pid: &str) -> Option<String> {
        comandos_runtime::process_start_time(pid).map(|start| start.to_string())
    }
}

/// Por qué `EventRoutes::handle_native` no respondió.
pub enum Unanswered {
    Failed(HandlerError),
    /// Una lectura falló (p. ej. una fila guardada que no se decodifica) y
    /// esta petición no escribió nada: se puede reenviar sin efectos.
    Unsure,
}

impl From<HandlerError> for Unanswered {
    fn from(error: HandlerError) -> Self {
        Unanswered::Failed(error)
    }
}

/// Una lectura de la base que no se pudo completar.
struct ReadFailed;

pub struct EventRoutes<F = NativeFacts> {
    legacy_path: PathBuf,
    facts: F,
    import_done: bool,
}

impl<F: Facts> EventRoutes<F> {
    pub fn new(legacy_path: PathBuf, facts: F) -> Self {
        Self {
            legacy_path,
            facts,
            import_done: false,
        }
    }

    /// Unknown routes return None for composition. Only admitted requests may
    /// reach this adapter; transport owns general authentication/body parsing.
    pub fn handle(
        &mut self,
        conn: &Connection,
        request: &Request,
    ) -> Result<Option<Reply>, HandlerError> {
        self.handle_native(conn, request)
            .map_err(|unanswered| match unanswered {
                Unanswered::Failed(error) => error,
                Unanswered::Unsure => HandlerError::Failure,
            })
    }

    /// Como `handle`, pero distingue `Unanswered::Unsure`: una lectura de la
    /// base falló sin que esta petición escribiera nada, y el frente nativo
    /// puede reenviarla al Python para que responda lo suyo.
    pub fn handle_native(
        &mut self,
        conn: &Connection,
        request: &Request,
    ) -> Result<Option<Reply>, Unanswered> {
        if request.method == Method::GET {
            return self.get(conn, request);
        }
        self.post(conn, request).map_err(Unanswered::Failed)
    }

    fn get(&mut self, conn: &Connection, request: &Request) -> Result<Option<Reply>, Unanswered> {
        let (path, query) =
            dashboard_access::request_target_parts(&request.target).ok_or(HandlerError::Failure)?;
        match path.as_str() {
            "/events/v2" => {
                let pairs = dashboard_access::query_pairs(&query, false);
                let after = match pagination(&pairs, "after", 0) {
                    Ok(value) => value,
                    Err(message) => return Ok(Some(error(400, &message)?)),
                };
                let limit = match pagination(&pairs, "limit", 100) {
                    Ok(value) => value,
                    Err(message) => return Ok(Some(error(400, &message)?)),
                };
                // Si la importación corre en esta petición ya pudo escribir:
                // desde ahí un fallo de lectura es un 500, nunca un reenvío.
                let fresh = !self.import_done;
                if let Err(problem) = self.import(conn) {
                    return Ok(Some(store_error(problem)?));
                }
                let with_turns = pairs
                    .iter()
                    .find(|(key, _)| key == "turns")
                    .is_some_and(|(_, value)| value == "1");
                let page = events_page(conn, after, limit, with_turns).map_err(|ReadFailed| {
                    if fresh {
                        Unanswered::Failed(HandlerError::Failure)
                    } else {
                        Unanswered::Unsure
                    }
                })?;
                Ok(Some(Reply::json(StatusCode::OK, &page)?))
            }
            // Solo lee: cualquier fallo de lectura se puede reenviar.
            "/work-marks" => {
                let payload = work_marks_payload(conn).map_err(|ReadFailed| Unanswered::Unsure)?;
                Ok(Some(Reply::json(StatusCode::OK, &payload)?))
            }
            _ => Ok(None),
        }
    }

    fn post(
        &mut self,
        conn: &Connection,
        request: &Request,
    ) -> Result<Option<Reply>, HandlerError> {
        if request.method != Method::POST
            || !matches!(request.target.as_str(), "/events/v2" | "/work-marks")
        {
            return Ok(None);
        }
        if request.target == "/events/v2" && !request.internal_producer {
            return error(403, "Solo productores internos de este equipo").map(Some);
        }
        let data = request
            .data
            .as_ref()
            .filter(|value| value.is_object())
            .ok_or(HandlerError::Failure)?;
        if request.target == "/work-marks" {
            let scope = data["scope"].as_str().unwrap_or("");
            let key = data["key"].as_str().unwrap_or("");
            let revision = &data["expectedRevision"];
            // SQLite revisions fit i64. Every larger positive Python integer
            // must conflict, preserving the current row rather than narrowing it.
            let huge = revision.as_number().is_some_and(|number| {
                let raw = number.as_str();
                !raw.starts_with('-')
                    && !raw.contains(['.', 'e', 'E'])
                    && raw.bytes().all(|byte| byte.is_ascii_digit())
                    && revision.as_u64().is_none()
            });
            let expected = if huge { &json!(u64::MAX) } else { revision };
            let now = self.facts.now_ms().map_err(|_| HandlerError::Failure)?;
            let now = i64::try_from(now).map_err(|_| HandlerError::Failure)?;
            return match marks::set_mark(conn, scope, key, &data["value"], expected, now) {
                Ok(mark) => Reply::json(StatusCode::OK, &json!({"mark":mark})).map(Some),
                Err(problem) => store_error(problem).map(Some),
            };
        }
        if let Err(problem) = self.import(conn) {
            return store_error(problem).map(Some);
        }
        if data.get("hookEvent").is_some() {
            if let Some(kind) = unhashable(&data["hookEvent"]) {
                return error(400, &format!("unhashable type: '{kind}'")).map(Some);
            }
            if data["hookEvent"] == "Notification"
                && let Some(kind) = unhashable(&data["notificationType"])
            {
                return error(400, &format!("unhashable type: '{kind}'")).map(Some);
            }
            if hook::normalize_hook(data, None).is_none() {
                return Reply::json(StatusCode::ACCEPTED, &json!({"ignored":true})).map(Some);
            }
            // Python retains nonnegative integer hook timestamps before the
            // SQLite write; core's u64 representation cannot retain this one.
            if positive_integer_overflow(&data["occurredAtMs"]) {
                return Err(HandlerError::Failure);
            }
        }
        let started = if data.get("hookEvent").is_some() {
            data["panePid"]
                .as_str()
                .and_then(|pid| self.facts.process_start(pid))
        } else {
            None
        };
        let now = self.facts.now_ms().map_err(|_| HandlerError::Failure)?;
        let event_id = if data.get("hookEvent").is_none() && truthy(&data["eventId"]) {
            String::new()
        } else {
            self.facts
                .fresh_id("event")
                .map_err(|_| HandlerError::Failure)?
        };
        let receipt_id = self
            .facts
            .fresh_id("receipt")
            .map_err(|_| HandlerError::Failure)?;
        match intake::record(
            conn,
            data,
            &Reception {
                now_ms: now,
                event_id: &event_id,
                receipt_id: &receipt_id,
                process_start: started.as_deref(),
            },
        ) {
            Ok(Some(event)) => Reply::json(StatusCode::OK, &json!({"event":event})).map(Some),
            Ok(None) => Reply::json(StatusCode::ACCEPTED, &json!({"ignored":true})).map(Some),
            Err(Error::Validation(message))
                if ["occurredAtMs", "receivedAtMs"].iter().any(|field| {
                    message == format!("{field} inválido")
                        && (positive_integer_overflow(&data[*field])
                            || data[*field]
                                .as_number()
                                .is_some_and(|number| number.as_str() == "Infinity")
                            || data[*field]
                                .as_f64()
                                .is_some_and(|number| number >= u64::MAX as f64))
                }) =>
            {
                Err(HandlerError::Failure)
            }
            Err(problem) => store_error(problem).map(Some),
        }
    }

    fn import(&mut self, conn: &Connection) -> comandos_store::Result<()> {
        if !self.import_done {
            comandos_runtime::legacy::import_legacy(conn, &self.legacy_path)?;
            self.import_done = true;
        }
        Ok(())
    }
}

fn pagination(pairs: &[(String, String)], name: &str, default: i64) -> Result<i64, String> {
    let Some((_, raw)) = pairs.iter().find(|(key, _)| key == name) else {
        return Ok(default);
    };
    if !(1..=15).contains(&raw.len()) || !raw.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(format!("{name} inválido"));
    }
    raw.parse().map_err(|_| format!("{name} inválido"))
}

/// `{"events","nextAfter","latest"}` (+ `turns`) de `events_v2_list`.
fn events_page(
    conn: &Connection,
    after: i64,
    limit: i64,
    with_turns: bool,
) -> Result<Value, ReadFailed> {
    let events = comandos_store::list_events(conn, after, limit).map_err(|_| ReadFailed)?;
    let next_after = match events.last() {
        Some(event) => sequence(event)?,
        None => after,
    };
    let latest = comandos_store::latest_sequence(conn).map_err(|_| ReadFailed)?;
    let mut value = json!({"events":events,"nextAfter":next_after,"latest":latest});
    if with_turns {
        value["turns"] = Value::Object(turns(conn)?);
    }
    Ok(value)
}

/// `sequence` es la clave entera de la tabla; otra cosa es una fila ilegible.
fn sequence(event: &Value) -> Result<i64, ReadFailed> {
    event["sequence"].as_i64().ok_or(ReadFailed)
}

fn turns(conn: &Connection) -> Result<Map<String, Value>, ReadFailed> {
    let latest = comandos_store::latest_sequence(conn).map_err(|_| ReadFailed)?;
    let mut after = (latest - 2000).max(0);
    let mut recent = Vec::new();
    loop {
        let page = comandos_store::list_events(conn, after, 500).map_err(|_| ReadFailed)?;
        let Some(last) = page.last() else {
            break;
        };
        after = sequence(last)?;
        recent.extend(page);
    }
    Ok(turn::turns_from_events(&recent))
}

fn work_marks_payload(conn: &Connection) -> Result<Value, ReadFailed> {
    let current = comandos_store::workspace::WorkspaceStore::new(conn)
        .current()
        .map_err(|_| ReadFailed)?;
    let document = current.map_or_else(comandos_core::workspace::empty_document, |state| {
        state.document
    });
    let panes: Vec<_> = document["bindings"].as_object().into_iter().flat_map(|bindings| bindings.iter()).filter(|(_, binding)| binding.is_object()).map(|(key, binding)| {
        json!({"paneKey":key,"session":binding["session"],"paneId":binding["paneId"]})
    }).collect();
    let mut activity = Map::new();
    for (key, turn) in turns(conn)? {
        activity.insert(key, json!({"state":turn["state"],"paneKey":turn["paneKey"],"session":turn["sessionKey"],"paneId":turn["paneId"]}));
    }
    let marks = marks::list_marks(conn).map_err(|_| ReadFailed)?;
    Ok(json!({"marks":marks,"panes":panes,"activity":activity}))
}

fn error(status: u16, message: &str) -> Result<Reply, HandlerError> {
    let status = StatusCode::from_u16(status).map_err(|_| HandlerError::Failure)?;
    Reply::json(status, &json!({"error":message}))
}

fn store_error(problem: Error) -> Result<Reply, HandlerError> {
    match problem {
        // Python's sqlite overflow is not caught by the route's ValueError/
        // TypeError handlers. SQL/I/O/system errors never expose their text.
        Error::Validation(message)
            if !message.ends_with("fuera de rango SQLite")
                && message != "timestamp histórico fuera de rango" =>
        {
            error(400, &message)
        }
        Error::Conflict(current) => Reply::json(
            StatusCode::CONFLICT,
            &json!({"error":"Revisión desactualizada","current":current}),
        ),
        _ => Err(HandlerError::Failure),
    }
}

fn positive_integer_overflow(value: &Value) -> bool {
    value.as_number().is_some_and(|number| {
        let raw = number.as_str();
        !raw.starts_with('-')
            && !raw.contains(['.', 'e', 'E'])
            && raw.bytes().all(|byte| byte.is_ascii_digit())
            && value.as_u64().is_none()
    })
}

fn unhashable(value: &Value) -> Option<&'static str> {
    match value {
        Value::Object(_) => Some("dict"),
        Value::Array(_) => Some("list"),
        _ => None,
    }
}
