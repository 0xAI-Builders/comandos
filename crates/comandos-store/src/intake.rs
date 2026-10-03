//! One reception and its human-mark effects share the same transaction.
use crate::{Error, Result, append_event, marks, with_transaction};
use comandos_core::{hook, legacy_truthy};
use rusqlite::{Connection, OptionalExtension};
use serde_json::Value;

pub struct Reception<'a> {
    pub now_ms: u64,
    pub event_id: &'a str,
    pub receipt_id: &'a str,
    pub process_start: Option<&'a str>,
}

pub fn record(
    conn: &Connection,
    payload: &Value,
    reception: &Reception<'_>,
) -> Result<Option<Value>> {
    let mut event = if payload.get("hookEvent").is_some() {
        let Some(event) = hook::normalize_hook(payload, reception.process_start) else {
            return Ok(None);
        };
        Value::Object(event)
    } else {
        if !payload.is_object() {
            return Err(Error::Validation("Evento inválido".into()));
        }
        payload.clone()
    };
    with_transaction(conn, || {
        if !legacy_truthy(&event["paneKey"]) {
            let pane = if legacy_truthy(&event["sessionKey"]) && legacy_truthy(&event["paneId"]) {
                let document: Option<String> = conn
                    .query_row(
                        "SELECT document FROM workspace_current WHERE id = 1",
                        [],
                        |r| r.get(0),
                    )
                    .optional()?;
                document
                    .and_then(|s| comandos_core::json::parse_value(&s).ok())
                    .and_then(|document| hook::resolve_pane_key(&document, &event))
            } else {
                None
            };
            event["paneKey"] = pane.into();
        }
        let stored = append_event(
            conn,
            &event,
            reception.now_ms,
            reception.event_id,
            reception.receipt_id,
        )?;
        if stored["duplicate"] != true {
            let now = i64::try_from(reception.now_ms)
                .map_err(|_| Error::Validation("entero fuera de rango SQLite".into()))?;
            marks::apply_turn_event(conn, &stored, now)?;
        }
        Ok(Some(stored))
    })
}
