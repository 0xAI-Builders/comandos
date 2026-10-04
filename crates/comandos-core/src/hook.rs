use crate::{
    event::valid_ident,
    json::{legacy_id, truthy},
};
use serde_json::{Map, Value};

fn ident(value: &Value) -> Option<&str> {
    value.as_str().filter(|s| !s.is_empty() && valid_ident(s))
}

fn matched(value: &Value, min: usize, max: usize, valid: impl Fn(u8) -> bool) -> Option<&str> {
    value
        .as_str()
        .filter(|s| s.len() >= min && s.len() <= max && s.bytes().all(valid))
}

/// `process_start` is the observation for payload.panePid, supplied by the
/// native process adapter. This pure function never opens /proc or a socket.
pub fn normalize_hook(payload: &Value, process_start: Option<&str>) -> Option<Map<String, Value>> {
    let mut kind = match payload["hookEvent"].as_str()? {
        "UserPromptSubmit" => "prompt_accepted",
        "Stop" => "turn_completed",
        "Notification" => "input_requested",
        "PermissionRequest" => "permission_requested",
        "GrokError" | "StopFailure" => "turn_failed",
        "GrokCancelled" | "StopCancelled" => "turn_cancelled",
        "SessionEnd" => "session_ended",
        _ => return None,
    };
    if kind == "input_requested" {
        match payload["notificationType"].as_str() {
            Some("idle_prompt" | "auth_success") => return None,
            Some("permission_prompt") => kind = "permission_requested",
            _ => {}
        }
    }
    let agent = matched(&payload["agent"], 1, 32, |b| {
        b.is_ascii_lowercase() || b.is_ascii_digit() || b"_-".contains(&b)
    })
    .unwrap_or("unknown");
    let session = matched(&payload["session"], 1, 80, |b| {
        b.is_ascii_alphanumeric() || b"._-".contains(&b)
    });
    let pane = payload["pane"].as_str().filter(|s| {
        s.starts_with('%')
            && (2..=10).contains(&s.len())
            && s.as_bytes()[1..].iter().all(u8::is_ascii_digit)
    });
    let pid = matched(&payload["panePid"], 1, 10, |b| b.is_ascii_digit());
    let turn = ident(&payload["turnId"]).or_else(|| ident(&payload["promptId"]));
    let request = ident(&payload["requestId"]);
    let correlation = if turn.is_some() || (request.is_some() && kind == "permission_requested") {
        "source"
    } else if session.is_some() && pane.is_some() {
        "local"
    } else {
        "unknown"
    };
    let process = pid.map(|p| {
        process_start
            .map(|s| format!("{p}-{s}"))
            .unwrap_or_else(|| p.into())
    });
    let mut out = Map::new();
    out.insert("source".into(), format!("hook:{agent}").into());
    out.insert("harness".into(), agent.into());
    out.insert("sessionKey".into(), session.into());
    out.insert("paneId".into(), pane.into());
    out.insert("paneKey".into(), Value::Null);
    out.insert("processKey".into(), process.into());
    out.insert("turnId".into(), turn.into());
    out.insert("requestId".into(), request.into());
    for (key, input) in [
        ("projectKey", "project"),
        ("conversationId", "conversationId"),
        ("sourceEventId", "sourceEventId"),
    ] {
        out.insert(key.into(), ident(&payload[input]).into());
    }
    out.insert("kind".into(), kind.into());
    out.insert("evidence".into(), "confirmed".into());
    out.insert("correlation".into(), correlation.into());
    out.insert(
        "occurredAtMs".into(),
        payload["occurredAtMs"].as_u64().into(),
    );
    for key in ["title", "excerpt"] {
        out.insert(key.into(), payload[key].as_str().unwrap_or("").into());
    }
    Some(out)
}

/// Resolve only a unique session/pane binding with matching process evidence.
/// Missing pid/start observations retain the legacy compatibility behavior.
pub fn resolve_pane_key(document: &Value, event: &Value) -> Option<String> {
    if !truthy(&event["sessionKey"]) || !truthy(&event["paneId"]) {
        return None;
    }
    let process = event["processKey"].as_str().unwrap_or("");
    let (pid, start) = process.split_once('-').unwrap_or((process, ""));
    let mut found = None;
    for (key, binding) in document["bindings"].as_object()? {
        if !binding.is_object()
            || binding["session"] != event["sessionKey"]
            || binding["paneId"] != event["paneId"]
        {
            continue;
        }
        if !pid.is_empty() && !binding["pid"].is_null() && legacy_id(&binding["pid"]) != pid {
            continue;
        }
        if !start.is_empty()
            && !binding["startTime"].is_null()
            && legacy_id(&binding["startTime"]) != start
        {
            continue;
        }
        if found.is_some() {
            return None;
        }
        found = Some(key.clone());
    }
    found
}
