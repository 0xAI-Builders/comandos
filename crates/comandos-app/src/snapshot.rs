use crate::tmux::TmuxCtl;
use comandos_runtime::pane_snapshot::PaneInspector;
use serde_json::Value;

#[derive(Debug)]
pub enum SnapshotError {
    Unsupported,
}

pub fn capture_session(
    _tmux: &TmuxCtl,
    _name: &str,
    _inspector: &PaneInspector,
) -> Result<Value, SnapshotError> {
    Err(SnapshotError::Unsupported)
}

pub fn carry_resume_ids(mut captured: Value, previous: &Value) -> Value {
    let old = panes(previous);
    for pane in panes_mut(&mut captured) {
        let agent = pane.get("agent").and_then(Value::as_str).unwrap_or("");
        if !matches!(agent, "claude" | "codex" | "grok")
            || pane.get("resume_id").and_then(Value::as_str).is_some()
        {
            continue;
        }
        let Some(prev) = old.iter().find(|p| p.get("id") == pane.get("id")) else {
            continue;
        };
        let same_start = prev.get("start").is_none()
            || pane.get("start").is_none()
            || prev.get("start") == pane.get("start");
        if same_start
            && prev.get("pid") == pane.get("pid")
            && prev.get("command") == pane.get("command")
            && prev.get("agent") == pane.get("agent")
            && let Some(object) = pane.as_object_mut()
            && let Some(prev_object) = prev.as_object()
        {
            for (key, value) in prev_object {
                if !matches!(
                    key.as_str(),
                    "id" | "index" | "cwd" | "pid" | "start" | "command" | "active" | "tagged_key"
                ) {
                    object.entry(key.clone()).or_insert_with(|| value.clone());
                }
            }
        }
    }
    captured
}

pub fn carry_pane_keys(mut captured: Value, previous: &Value) -> Value {
    let old = panes(previous);
    let mut used = std::collections::BTreeSet::new();
    for pane in panes_mut(&mut captured) {
        let mut key = pane
            .get("tagged_key")
            .and_then(Value::as_str)
            .map(ToString::to_string)
            .or_else(|| {
                old.iter()
                    .find(|p| {
                        p.get("id") == pane.get("id")
                            && p.get("pid") == pane.get("pid")
                            && p.get("start") == pane.get("start")
                    })
                    .and_then(|p| p.get("key"))
                    .and_then(Value::as_str)
                    .map(ToString::to_string)
            })
            .or_else(|| {
                let agent = pane.get("agent")?;
                let resume = pane.get("resume_id")?;
                old.iter()
                    .find(|p| p.get("agent") == Some(agent) && p.get("resume_id") == Some(resume))
                    .and_then(|p| p.get("key"))
                    .and_then(Value::as_str)
                    .map(ToString::to_string)
            });
        if key.as_ref().is_none_or(|k| used.contains(k)) {
            key = Some(format!("pane-{}", used.len() + 1));
        }
        let key = key.unwrap_or_else(|| "pane-1".to_string());
        used.insert(key.clone());
        pane["key"] = Value::String(key);
    }
    captured
}

fn panes(value: &Value) -> Vec<&Value> {
    value
        .get("windows")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .flat_map(|w| {
            w.get("panes")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
        })
        .collect()
}

fn panes_mut(value: &mut Value) -> Vec<&mut Value> {
    value
        .get_mut("windows")
        .and_then(Value::as_array_mut)
        .into_iter()
        .flatten()
        .flat_map(|w| {
            w.get_mut("panes")
                .and_then(Value::as_array_mut)
                .into_iter()
                .flatten()
        })
        .collect()
}
