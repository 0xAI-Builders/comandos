//! `comandos hook grok [--accept RUTA]`: transcripción de `adapters/grok-hooks.py`.
//! Sin argumentos normaliza el JSON de un hook de Grok Build (lista blanca de
//! campos, credenciales tachadas) y lo imprime; con `--accept RUTA` decide si un
//! evento ya normalizado puede reemplazar al vigente del pane (lo registra bajo
//! `flock`) o es tardío (`REJECTED_EXIT`).
use super::grok_redact::redact;
use super::py;
use serde_json::{Map, Value};
use std::io::{Read, Seek, SeekFrom, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;

pub const REJECTED_EXIT: i32 = 3;

/// `TypeError` de Python (clave no *hashable*): el script muere con 1.
pub struct Unhashable;

const EVENTS: [(&str, &str, &str, i32); 6] = [
    // (evento, alias, estado, fase)
    ("UserPromptSubmit", "user_prompt_submit", "working", 10),
    ("Stop", "stop", "done", 30),
    ("StopFailure", "stop_failure", "error", 30),
    ("StopCancelled", "stop_cancelled", "cancelled", 30),
    ("Notification", "notification", "waiting", 20),
    ("SessionEnd", "session_end", "end", 40),
];

fn hashable(value: &Value) -> Result<(), Unhashable> {
    if value.is_array() || value.is_object() {
        Err(Unhashable)
    } else {
        Ok(())
    }
}

/// `_PHASE.get(evento, -1)`.
fn phase(value: Option<&Value>) -> Result<Option<i32>, Unhashable> {
    let Some(value) = value else { return Ok(None) };
    hashable(value)?;
    Ok(value
        .as_str()
        .and_then(|e| EVENTS.iter().find(|x| x.0 == e))
        .map(|x| x.3))
}

/// `_text`: escalar estable y tachado, o vacío.
pub fn text(value: Option<&Value>) -> String {
    let raw = match value {
        Some(Value::Bool(b)) => if *b { "true" } else { "false" }.to_string(),
        Some(Value::String(s)) => s.clone(),
        Some(Value::Number(n)) => py::number_str(n),
        _ => return String::new(),
    };
    redact(&raw)
}

/// `_nested_detail`.
fn nested(value: Option<&Value>) -> String {
    let Some(Value::Object(map)) = value else {
        return text(value);
    };
    [
        "message",
        "detail",
        "reason",
        "description",
        "error",
        "code",
    ]
    .iter()
    .map(|k| text(map.get(*k)))
    .find(|d| !d.is_empty())
    .unwrap_or_default()
}

fn first_detail(payload: &Map<String, Value>, keys: &[&str]) -> String {
    keys.iter()
        .map(|k| nested(payload.get(*k)))
        .find(|d| !d.is_empty())
        .unwrap_or_default()
}

/// `normalize`: `Ok(None)` para lo que no se reenvía.
pub fn normalize(payload: &Value) -> Result<Option<Map<String, Value>>, Unhashable> {
    let Some(payload) = payload.as_object() else {
        return Ok(None);
    };
    let event = match payload.get("hookEventName").and_then(Value::as_str) {
        Some(e) => EVENTS
            .iter()
            .find(|x| x.1 == e)
            .map_or(e, |x| x.0)
            .to_string(),
        None => return Ok(None),
    };
    let Some(&(_, _, base_status, _)) = EVENTS.iter().find(|x| x.0 == event) else {
        return Ok(None);
    };
    if payload.get("subagentType").is_some_and(py::truthy) {
        return Ok(None);
    }
    let mut status = base_status.to_string();
    let detail = match event.as_str() {
        "Notification" => {
            let mut kind = payload.get("notificationType");
            if !kind.is_some_and(py::truthy) {
                kind = payload.get("notification_type");
            }
            let kind = kind.unwrap_or(&Value::Null);
            hashable(kind)?;
            let kind = match kind.as_str() {
                Some(k @ ("permission_prompt" | "idle_prompt")) => k,
                _ => return Ok(None),
            };
            if kind == "idle_prompt" {
                status = "idle".into();
            }
            let detail = first_detail(payload, &["message", "detail", "reason", "title"]);
            if detail.is_empty() {
                kind.to_string()
            } else {
                detail
            }
        }
        "UserPromptSubmit" => String::new(),
        "Stop" => first_detail(payload, &["lastAssistantMessage", "reason", "message"]),
        "StopFailure" => first_detail(payload, &["errorDetails", "error", "reason", "message"]),
        "StopCancelled" => {
            status = "idle".into();
            first_detail(payload, &["reason", "message", "lastAssistantMessage"])
        }
        _ => first_detail(payload, &["reason", "message"]),
    };
    let mut out = Map::new();
    out.insert("event".into(), Value::String(event));
    out.insert("status".into(), Value::String(status));
    out.insert("detail".into(), Value::String(detail));
    for key in ["sessionId", "promptId", "model", "effort", "pane"] {
        let value = text(payload.get(key));
        if !value.is_empty() {
            out.insert(key.into(), Value::String(value));
        }
    }
    Ok(Some(out))
}

/// `should_accept_event`.
pub fn should_accept(current: Option<&Value>, candidate: &Value) -> Result<bool, Unhashable> {
    let Some(candidate_phase) = phase(candidate.get("event"))? else {
        return Ok(false);
    };
    let current = match current.filter(|c| c.is_object()) {
        Some(c) => c,
        None => return Ok(true),
    };
    let Some(current_phase) = phase(current.get("event"))? else {
        return Ok(true);
    };
    let (cur_session, cand_session) = (
        text(current.get("sessionId")),
        text(candidate.get("sessionId")),
    );
    if !cur_session.is_empty() && !cand_session.is_empty() && cur_session != cand_session {
        return Ok(true);
    }
    let cand_event = candidate.get("event").and_then(Value::as_str);
    let cur_event = current.get("event").and_then(Value::as_str);
    if cand_event == Some("SessionEnd") {
        return Ok(true);
    }
    if cur_event == Some("SessionEnd") {
        return Ok(false);
    }
    let submit = cand_event == Some("UserPromptSubmit");
    let (cur_prompt, cand_prompt) = (
        text(current.get("promptId")),
        text(candidate.get("promptId")),
    );
    if !cur_prompt.is_empty() && !cand_prompt.is_empty() && cur_prompt != cand_prompt {
        return Ok(submit);
    }
    if !cur_prompt.is_empty() && cand_prompt.is_empty() && cur_event == Some("UserPromptSubmit") {
        return Ok(submit);
    }
    if cur_prompt == cand_prompt {
        return Ok(candidate_phase >= current_phase);
    }
    Ok(submit)
}

/// `accept_and_record`: los fallos de E/S dejan pasar el evento.
pub fn accept_and_record(path: &Path, candidate: &Value) -> Result<bool, Unhashable> {
    let Ok(mut file) = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .open(path)
    else {
        return Ok(true);
    };
    if file.lock().is_err() {
        return Ok(true);
    }
    let mut raw = Vec::new();
    if file.read_to_end(&mut raw).is_err() {
        return Ok(true);
    }
    let Ok(raw) = String::from_utf8(raw) else {
        return Ok(true);
    };
    let current = if raw.trim().is_empty() {
        None
    } else {
        py::json_load(raw.as_bytes())
    };
    if !should_accept(current.as_ref(), candidate)? {
        return Ok(false);
    }
    let mut record = Map::new();
    for key in ["event", "sessionId", "promptId"] {
        if let Some(v) = candidate.get(key).filter(|v| py::truthy(v)) {
            record.insert(key.into(), v.clone());
        }
    }
    let mut text = String::new();
    py::dump(&Value::Object(record), true, (",", ":"), &mut text);
    let _ = file
        .seek(SeekFrom::Start(0))
        .and_then(|_| file.set_len(0))
        .and_then(|_| file.write_all(text.as_bytes()));
    Ok(true)
}

fn crash() -> i32 {
    eprintln!("hook grok: TypeError: unhashable type");
    1
}

pub fn run(args: &[String]) -> i32 {
    let mut raw = Vec::new();
    let read = std::io::stdin().read_to_end(&mut raw).is_ok();
    let payload = if read { py::json_load(&raw) } else { None };
    if args.len() == 2 && args[0] == "--accept" {
        let Some(candidate) = payload.filter(Value::is_object) else {
            return 0;
        };
        return match accept_and_record(Path::new(&args[1]), &candidate) {
            Ok(true) => 0,
            Ok(false) => REJECTED_EXIT,
            Err(Unhashable) => crash(),
        };
    }
    let Some(payload) = payload else { return 0 };
    match normalize(&payload) {
        Ok(Some(out)) => {
            let mut text = String::new();
            py::dump(&Value::Object(out), false, (",", ":"), &mut text);
            text.push('\n');
            let _ = std::io::stdout().write_all(text.as_bytes());
            0
        }
        Ok(None) => 0,
        Err(Unhashable) => crash(),
    }
}
