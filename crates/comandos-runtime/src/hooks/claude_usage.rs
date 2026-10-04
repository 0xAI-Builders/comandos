//! `comandos hook claude-usage`: transcripción de `hooks/cc-usage-tool.sh`, la
//! telemetría de herramientas de cada `PreToolUse`/`PostToolUse`. Solo nombre,
//! fase, momento y estado (nunca argumentos ni resultados), registrados con
//! `comandos_store::usage::tool_event` (en el proceso de entrega desacoplado) en
//! lugar de `cc_usage.py tool-event &`.
use super::adapter::{get, jq_values, truthy};
use super::input::{clock, env_bytes};
use super::jq::jq_tostring;
use super::text::strip_nl;
use super::which;
use serde_json::{Value, json};
use std::ffi::OsStr;
use std::os::unix::ffi::OsStrExt;
use std::path::PathBuf;
use std::process::{Command, Stdio};

/// `.campo // "" | if type == "string" then . else tojson end`.
fn text_or_json(value: &Value, key: &str) -> Result<String, ()> {
    let found = get(value, key)?;
    Ok(if !truthy(found) {
        String::new()
    } else {
        jq_tostring(found)
    })
}

/// `\A[A-Za-z0-9_][A-Za-z0-9_.:@/-]{0,159}\z`.
fn skill_ok(s: &str) -> bool {
    let mut chars = s.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphanumeric() || c == '_')
        && s.chars().count() <= 160
        && chars.all(|c| c.is_ascii_alphanumeric() || "_.:@/-".contains(c))
}

/// El programa jq del script para una entrada; `Ok(None)` = `empty`.
fn tool_event(value: &Value, session: &str, pane: &str, at: i64) -> Result<Option<Value>, ()> {
    let ev = get(value, "hook_event_name")?;
    let phase = match ev.as_str().filter(|_| truthy(ev)) {
        Some("PreToolUse") => "start",
        Some("PostToolUse") => {
            let response = get(value, "tool_response")?;
            let failed = response.is_object() && {
                let is_error = get(response, "is_error")?;
                is_error == &Value::Bool(true) || response.get("error").is_some()
            };
            if failed { "failed" } else { "success" }
        }
        _ => return Ok(None),
    };
    let tool = text_or_json(value, "tool_name")?;
    if tool.is_empty() {
        return Ok(None);
    }
    let raw_skill = if tool == "Skill" {
        get(value, "tool_input")
            .and_then(|input| get(input, "skill"))
            .cloned()
            .unwrap_or(Value::String(String::new()))
    } else {
        Value::String(String::new())
    };
    let skill = raw_skill.as_str().filter(|s| skill_ok(s)).unwrap_or("");
    Ok(Some(json!({
        "phase": phase,
        "tmux_session": session,
        "tmux_pane": pane,
        "tool_name": tool,
        "skill_name": skill,
        "tool_use_id": text_or_json(value, "tool_use_id")?,
        "at_ms": at,
        "confidence": "exact",
    })))
}

pub fn run(_args: &[String]) -> i32 {
    let pane = env_bytes("TMUX_PANE");
    if !(pane.len() > 1 && pane[0] == b'%' && pane[1..].iter().all(u8::is_ascii_digit)) {
        return 0;
    }
    let Some(tmux) = which("tmux") else { return 0 };
    let pane = String::from_utf8_lossy(&pane).into_owned();
    let session = Command::new(tmux)
        .args(["display-message", "-p", "-t", &pane, "#S"])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .map(|o| strip_nl(&o.stdout).to_vec())
        .unwrap_or_default();
    let session_ok = (1..=80).contains(&session.len())
        && session
            .iter()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(b));
    if !session_ok {
        return 0;
    }
    let session = String::from_utf8_lossy(&session).into_owned();
    let at = clock().1;
    let mut raw = Vec::new();
    let _ = std::io::Read::read_to_end(&mut std::io::stdin(), &mut raw);
    let events: Vec<Value> = jq_values(&raw)
        .iter()
        .filter_map(|v| tool_event(v, &session, &pane, at).ok().flatten())
        .collect();
    // `python3 cc_usage.py tool-event` lee UN documento: con varios, `json.load` falla.
    let [event] = events.as_slice() else {
        return 0;
    };
    // Como el `&` del bash: el hook regresa ya; SQLite (compartida, a veces
    // bloqueada hasta 10 s) la escribe el proceso de entrega desacoplado.
    super::notify_http::spawn(super::notify_http::Job {
        home: PathBuf::from(OsStr::from_bytes(&env_bytes("HOME"))),
        usage: vec![super::usage_hook::tool_step(event.clone())],
        volume: 60,
        voice: None,
        desktop: None,
    });
    0
}
