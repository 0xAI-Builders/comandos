use crate::json::truthy;
use serde_json::{Map, Value};

pub const MAX_ID: usize = 200;
pub const KINDS: &[&str] = &[
    "prompt_accepted",
    "turn_started",
    "input_requested",
    "permission_requested",
    "turn_completed",
    "turn_cancelled",
    "turn_failed",
    "pane_closed",
    "session_ended",
    "focus_completed",
    "news_edition",
    "announcement",
    "usage_alert",
];
const IDENTS: &[&str] = &[
    "sourceEventId",
    "harness",
    "projectKey",
    "sessionKey",
    "paneKey",
    "paneId",
    "processKey",
    "conversationId",
    "turnId",
    "requestId",
];

pub(crate) fn valid_ident(value: &str) -> bool {
    value.chars().count() <= MAX_ID && !value.chars().any(|c| c < '\u{20}')
}

fn opt_ident(event: &Value, name: &str) -> Result<Value, String> {
    match &event[name] {
        Value::Null => Ok(Value::Null),
        Value::String(s) if s.is_empty() => Ok(Value::Null),
        Value::String(s) if valid_ident(s) => Ok(Value::String(s.clone())),
        _ => Err(format!("{name} inválido")),
    }
}

fn ms(value: &Value, name: &str) -> Result<Value, String> {
    if let Some(n) = value.as_u64() {
        return Ok(n.into());
    }
    if let Some(n) = value.as_f64().filter(|n| *n >= 0.0 && *n < u64::MAX as f64) {
        return Ok((n as u64).into());
    }
    Err(format!("{name} inválido"))
}

fn text(value: &Value, limit: usize) -> Value {
    value
        .as_str()
        .unwrap_or("")
        .chars()
        .take(limit)
        .collect::<String>()
        .into()
}

/// Normalize without reading a clock or generating an id. The caller supplies
/// both, so replay and WASM use exactly the same domain rules as native intake.
pub fn normalize(
    event: &Value,
    now_ms: u64,
    new_event_id: &str,
) -> Result<Map<String, Value>, String> {
    if !event.is_object() {
        return Err("Evento inválido".into());
    }
    let kind = event["kind"]
        .as_str()
        .filter(|s| KINDS.contains(s))
        .ok_or("kind desconocido")?;
    let source = event["source"]
        .as_str()
        .filter(|s| !s.is_empty() && s.chars().count() <= MAX_ID)
        .ok_or("source inválido")?;
    let evidence = if truthy(&event["evidence"]) {
        &event["evidence"]
    } else {
        &Value::Null
    };
    let evidence = if evidence.is_null() {
        "unknown"
    } else {
        evidence
            .as_str()
            .filter(|s| ["confirmed", "inferred", "historical", "unknown"].contains(s))
            .ok_or("evidence inválida")?
    };
    let correlation = if truthy(&event["correlation"]) {
        &event["correlation"]
    } else {
        &Value::Null
    };
    let correlation = if correlation.is_null() {
        "unknown"
    } else {
        correlation
            .as_str()
            .filter(|s| ["source", "local", "unknown"].contains(s))
            .ok_or("correlation inválida")?
    };
    let mut out = Map::new();
    for &name in IDENTS {
        out.insert(name.into(), opt_ident(event, name)?);
    }
    let received = ms(
        event.get("receivedAtMs").unwrap_or(&Value::from(now_ms)),
        "receivedAtMs",
    )?;
    let occurred = if event["occurredAtMs"].is_null() {
        received.clone()
    } else {
        ms(&event["occurredAtMs"], "occurredAtMs")?
    };
    out.insert("kind".into(), kind.into());
    out.insert("source".into(), source.into());
    out.insert("evidence".into(), evidence.into());
    out.insert("correlation".into(), correlation.into());
    out.insert("receivedAtMs".into(), received);
    out.insert("occurredAtMs".into(), occurred);
    out.insert("title".into(), text(&event["title"], 200));
    out.insert("excerpt".into(), text(&event["excerpt"], 500));
    let id = opt_ident(event, "eventId")?;
    out.insert(
        "eventId".into(),
        if id.is_null() {
            new_event_id.into()
        } else {
            id
        },
    );
    Ok(out)
}

/// Use normalized events. Local reception ids never prove provider identity.
pub fn dedupe_key(event: &Value) -> Option<String> {
    if let Some(source) = event["sourceEventId"].as_str().filter(|s| !s.is_empty()) {
        return Some(format!("src:{source}"));
    }
    if event["correlation"] == "source"
        && event["evidence"] == "confirmed"
        && (truthy(&event["turnId"]) || truthy(&event["requestId"]))
    {
        return Some(format!(
            "cycle:{}",
            ["harness", "conversationId", "turnId", "requestId", "kind"]
                .map(|k| event[k].as_str().unwrap_or(""))
                .join("|")
        ));
    }
    None
}

pub fn destination(event: &Value) -> &'static str {
    if truthy(&event["paneKey"]) {
        "pane"
    } else if truthy(&event["sessionKey"]) {
        "session"
    } else {
        "none"
    }
}
