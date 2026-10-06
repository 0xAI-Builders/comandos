pub use comandos_web_view::work_marks::{MARKS, ai_state};
use serde_json::{Value, json};
pub fn channels(mark: &str, activity: &str) -> Value {
    let mark = if MARKS.contains(&mark) { mark } else { "none" };
    let ai = match activity {
        "working" => "work",
        "awaiting_permission" | "awaiting_input" | "waiting" => "need",
        "completed" | "done" => "done",
        "failed" | "error" => "error",
        _ => "idle",
    };
    let sticker = match mark {
        "resolved" => Some("Hecho"),
        "frozen" => Some("Aparcado"),
        "awaiting_reply" => Some("Esperando"),
        _ => None,
    };
    json!({"ai":ai,"sticker":sticker,"mark":mark,"suggest":ai=="done"&&mark=="none"})
}
pub fn display(mark: &str, activity: &str, english: bool) -> Value {
    let mark = if MARKS.contains(&mark) { mark } else { "none" };
    let icon = if mark != "none" {
        mark
    } else if activity == "working" {
        "working"
    } else {
        "none"
    };
    json!({"icon":icon,"label":comandos_web_view::work_marks::label(icon,english),"animated":icon!="none","mark":mark})
}
pub fn target_for_row(row: &str, panes: &Value) -> Value {
    let mut split = row.split('|');
    let session = split.next().unwrap_or_default();
    let pane = split.next().filter(|s| !s.is_empty());
    if session.is_empty() {
        return Value::Null;
    }
    if let Some(pane) = pane {
        let hits = panes
            .as_array()
            .into_iter()
            .flatten()
            .filter(|p| {
                p.get("session").and_then(Value::as_str) == Some(session)
                    && p.get("paneId").and_then(Value::as_str) == Some(pane)
            })
            .collect::<Vec<_>>();
        if hits.len() == 1 {
            return json!({"scope":"pane","key":hits.first().and_then(|p|p.get("paneKey")),"session":session,"paneId":pane});
        }
    }
    json!({"scope":"session","key":session,"session":session,"paneId":pane})
}
pub fn activity_for(target: &Value, activity: &Value) -> Value {
    if target.is_null() {
        return Value::Null;
    }
    let key = target
        .get("key")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if target.get("scope").and_then(Value::as_str) == Some("pane") {
        let direct = activity.get(format!("pane:{key}"));
        let session = target
            .get("session")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let pane = target
            .get("paneId")
            .and_then(Value::as_str)
            .unwrap_or_default();
        return direct
            .or_else(|| activity.get(format!("tmux:{session}:{pane}")))
            .and_then(|v| v.get("state"))
            .cloned()
            .unwrap_or(Value::Null);
    }
    let states = activity
        .as_object()
        .into_iter()
        .flat_map(|a| a.values())
        .filter(|v| v.get("session").and_then(Value::as_str) == Some(key))
        .filter_map(|v| v.get("state").and_then(Value::as_str))
        .collect::<Vec<_>>();
    [
        "awaiting_permission",
        "awaiting_input",
        "failed",
        "working",
        "completed",
    ]
    .iter()
    .find(|s| states.contains(s))
    .map(|s| json!(s))
    .unwrap_or(Value::Null)
}
pub fn next_index(key: &str, index: i32, count: i32) -> i32 {
    if count == 0 {
        return -1;
    }
    match key {
        "ArrowDown" => (index + 1) % count,
        "ArrowUp" => (index - 1 + count) % count,
        "Home" => 0,
        "End" => count - 1,
        _ => index,
    }
}
#[cfg(target_arch = "wasm32")]
#[path = "work_marks_web.rs"]
mod web;
#[cfg(target_arch = "wasm32")]
pub use web::{attach, mount};
#[cfg(not(target_arch = "wasm32"))]
pub fn mount() -> Result<(), wasm_bindgen::JsValue> {
    Ok(())
}
#[cfg(not(target_arch = "wasm32"))]
pub fn attach() -> Result<(), wasm_bindgen::JsValue> {
    Ok(())
}
