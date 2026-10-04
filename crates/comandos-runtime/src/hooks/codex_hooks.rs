//! `comandos hook codex-hooks`: transcripción de `adapters/codex-hooks.sh` (los
//! lifecycle hooks de Codex). Lee el JSON del hook por stdin y entrega al pipeline
//! de `hook claude` un `working`, un `done` (sin repetir el que ya avisó el
//! `notify`) o un `waiting` de permiso con herramienta, comando y motivo.
use super::adapter::{
    Jq, arg, codex_state_file, first_of, jq_get, jq_values, notify, pwd, recent_codex_done, strings,
};
use super::input::{clock, env_bytes};
use super::text::strip_nl;
use serde_json::Value;
use std::ffi::OsStr;
use std::io::Read;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

/// `.a?.b?...`: `None` cuando jq no produce nada (un `?` calló el error).
fn opt_path<'a>(value: &'a Value, keys: &[&str]) -> Option<&'a Value> {
    static NULL: Value = Value::Null;
    let mut current = value;
    for key in keys {
        current = match current {
            Value::Object(map) => map.get(*key).unwrap_or(&NULL),
            Value::Null => &NULL,
            _ => return None,
        };
    }
    Some(current)
}

/// `[RUTAS] | (map(objeto → .name // empty)) | map(select(cadena no vacía)) | first // ""`.
fn first_string(value: &Value, paths: &[&[&str]], object_name: bool) -> Jq<Option<Value>> {
    for path in paths {
        let Some(mut found) = opt_path(value, path) else {
            continue;
        };
        if object_name && found.is_object() {
            match found.get("name") {
                Some(name) if super::adapter::truthy(name) => found = name,
                _ => continue,
            }
        }
        if found.as_str().is_some_and(|s| !s.is_empty()) {
            return Ok(Some(found.clone()));
        }
    }
    Ok(Some(Value::String(String::new())))
}

pub fn run(_args: &[String]) -> i32 {
    let mut raw = Vec::new();
    if std::io::stdin().read_to_end(&mut raw).is_err() {
        return 0;
    }
    let payload = strip_nl(&raw);
    if payload.is_empty() {
        return 0;
    }
    let home = PathBuf::from(OsStr::from_bytes(&env_bytes("HOME")));
    let executable = std::fs::metadata(home.join(".claude/hooks/cc-notify.sh"))
        .is_ok_and(|m| m.permissions().mode() & 0o111 != 0);
    if !executable {
        return 0;
    }
    let values = jq_values(payload);
    let event = jq_get(&values, |v| {
        first_of(v, &["hook_event_name", "event"], Some(""))
    });
    let mut cwd = jq_get(&values, |v| first_of(v, &["cwd", "workspace-path"], None));
    if cwd.is_empty() {
        cwd = pwd();
    }
    let state = codex_state_file(&cwd);
    // Identidad que Codex suministra: conversación (session_id) y turno (turn_id).
    let session_id = jq_get(&values, |v| {
        strings(first_of(
            v,
            &["session_id", "sessionId", "thread_id"],
            Some(""),
        ))
    });
    let turn_id = jq_get(&values, |v| {
        strings(first_of(v, &["turn_id", "turnId"], Some("")))
    });
    let ids = [arg("--session-id"), session_id, arg("--turn-id"), turn_id];
    let base = |event: &str| {
        vec![
            arg("--agent"),
            arg("codex"),
            arg("--event"),
            arg(event),
            arg("--cwd"),
            cwd.clone(),
        ]
    };
    match event.as_slice() {
        b"UserPromptSubmit" => {
            let mut args = base("working");
            args.extend([arg("--hook-event"), arg("UserPromptSubmit")]);
            args.extend(ids);
            notify(&args)
        }
        b"Stop" => {
            if state.is_file() && recent_codex_done(&state, clock().0) {
                return 0;
            }
            let full = jq_get(&values, |v| {
                first_of(
                    v,
                    &[
                        "last_assistant_message",
                        "last-assistant-message",
                        "message",
                    ],
                    Some(""),
                )
            });
            let mut args = base("done");
            args.extend([arg("--full"), full, arg("--hook-event"), arg("Stop")]);
            args.extend(ids);
            notify(&args)
        }
        b"PermissionRequest" => {
            let pick = |paths: &[&[&str]], object_name: bool| {
                jq_get(&values, |v| first_string(v, paths, object_name))
            };
            let tool = pick(
                &[
                    &["tool_name"],
                    &["tool"],
                    &["tool_call", "name"],
                    &["request", "tool_name"],
                    &["permission", "tool_name"],
                ],
                true,
            );
            let command = pick(
                &[
                    &["command"],
                    &["tool_call", "command"],
                    &["input", "command"],
                    &["tool_input", "command"],
                    &["arguments", "command"],
                    &["params", "command"],
                ],
                false,
            );
            let reason = pick(
                &[
                    &["reason"],
                    &["description"],
                    &["message"],
                    &["approval_request", "reason"],
                    &["request", "reason"],
                ],
                false,
            );
            let request_id = pick(
                &[
                    &["request_id"],
                    &["requestId"],
                    &["approval_id"],
                    &["call_id"],
                    &["tool_use_id"],
                    &["tool_call", "id"],
                ],
                false,
            );
            let mut msg = b"Codex necesita permiso".to_vec();
            if !tool.is_empty() {
                msg.extend_from_slice(b": ");
                msg.extend_from_slice(&tool);
            }
            let mut full = msg.clone();
            if !command.is_empty() {
                full.extend_from_slice(b"\nCommand: ");
                full.extend_from_slice(&command);
            }
            if !reason.is_empty() {
                full.extend_from_slice(b"\nReason: ");
                full.extend_from_slice(&reason);
            }
            let mut args = base("waiting");
            args.extend([
                arg("--msg"),
                msg,
                arg("--full"),
                full,
                arg("--hook-event"),
                arg("PermissionRequest"),
                arg("--request-id"),
                request_id,
            ]);
            args.extend(ids);
            notify(&args)
        }
        _ => 0,
    }
}
