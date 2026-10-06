use crate::tmux::TmuxCtl;
use comandos_runtime::pane_snapshot::PaneInspector;
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SnapshotError {
    Read(String),
    Invalid(String),
}

pub fn capture_session(
    tmux: &TmuxCtl,
    name: &str,
    inspector: &PaneInspector,
) -> Result<Value, SnapshotError> {
    capture_with(tmux, name, |pane| {
        let id = pane
            .get("id")
            .unwrap_or(&Value::Null)
            .as_str()
            .ok_or("missing pane id")?;
        let pid = pane
            .get("pid")
            .unwrap_or(&Value::Null)
            .as_i64()
            .ok_or("missing pane pid")?;
        let command = pane
            .get("command")
            .unwrap_or(&Value::Null)
            .as_str()
            .ok_or("missing command")?;
        let mut metadata = inspector
            .inspect(&comandos_runtime::pane_snapshot::PaneRef { id, pid, command })
            .map_err(|_| "ambiguous process metadata")?;
        metadata.insert(
            "start".into(),
            process_start_time(std::path::Path::new("/proc"), pid),
        );
        Ok(metadata)
    })
}

pub fn process_start_time(proc_root: &std::path::Path, pid: i64) -> Value {
    std::fs::read_to_string(proc_root.join(pid.to_string()).join("stat"))
        .ok()
        .and_then(|s| {
            s.rsplit_once(')')
                .and_then(|(_, fields)| fields.split_whitespace().nth(19))
                .and_then(|tick| tick.parse::<u64>().ok())
        })
        .map(Value::from)
        .unwrap_or(Value::Null)
}

/// All tmux reads and process metadata are injectable. No partial tree is returned.
pub fn capture_with<T: crate::restore::RestoreTmux>(
    tmux: &T,
    name: &str,
    mut metadata: impl FnMut(&Value) -> Result<serde_json::Map<String, Value>, &'static str>,
) -> Result<Value, SnapshotError> {
    let read = |args: &[&str]| tmux.read(args).map_err(SnapshotError::Read);
    let mut nonce = [0u8; 12];
    getrandom::fill(&mut nonce).map_err(|e| SnapshotError::Invalid(e.to_string()))?;
    let separator = format!(
        "<cc-{}>",
        nonce.iter().map(|b| format!("{b:02x}")).collect::<String>()
    );
    let window_format = [
        "window_id",
        "window_index",
        "window_name",
        "window_layout",
        "window_width",
        "window_height",
        "window_active",
        "window_zoomed_flag",
        "pane-border-status",
        "automatic-rename",
        "window-size",
    ]
    .iter()
    .map(|key| format!("#{{{key}}}"))
    .collect::<Vec<_>>()
    .join(&separator);
    let pane_format = [
        "pane_id",
        "pane_index",
        "pane_current_path",
        "pane_pid",
        "pane_current_command",
        "pane_active",
        "@comandos-pane-key",
    ]
    .iter()
    .map(|key| format!("#{{{key}}}"))
    .collect::<Vec<_>>()
    .join(&separator);
    let raw = read(&[
        "list-windows",
        "-t",
        &format!("={name}"),
        "-F",
        &window_format,
    ])?;
    let integer = |s: &str| {
        s.parse::<i64>()
            .map_err(|_| SnapshotError::Invalid(format!("invalid integer {s}")))
    };
    let mut windows = Vec::new();
    for line in raw.lines() {
        let fields: Vec<_> = line.split(&separator).collect();
        let [
            wid,
            index,
            name,
            layout,
            width,
            height,
            active,
            zoomed,
            border,
            rename,
            size,
        ] = fields.as_slice()
        else {
            return Err(SnapshotError::Invalid(format!(
                "incomplete window row: {line:?}"
            )));
        };
        let mut window = serde_json::json!({"id":wid,"index":integer(index)?,"name":name,"layout":layout,
            "width":integer(width)?,"height":integer(height)?,"active":*active == "1","zoomed":*zoomed == "1","panes":[]});
        // Window options are expanded in this same list-windows response. Flag
        // formats use 0/1 while show-options printed off/on in the snapshot.
        let rename = match *rename {
            "" | "0" | "off" => "off",
            "1" | "on" => "on",
            value => {
                return Err(SnapshotError::Invalid(format!(
                    "invalid automatic-rename {value:?}"
                )));
            }
        };
        for (value, field, default) in [
            (*border, "border_status", "off"),
            (rename, "automatic_rename", "off"),
            (*size, "window_size", "latest"),
        ] {
            if let Some(object) = window.as_object_mut() {
                object.insert(
                    field.into(),
                    Value::String(if value.is_empty() {
                        default.into()
                    } else {
                        value.to_owned()
                    }),
                );
            }
        }
        let raw = read(&["list-panes", "-t", wid, "-F", &pane_format])?;
        let mut panes = Vec::new();
        for row in raw.lines() {
            let fields: Vec<_> = row.split(&separator).collect();
            let [id, index, cwd, pid, command, active, tagged] = fields.as_slice() else {
                return Err(SnapshotError::Invalid(format!(
                    "incomplete pane row: {row:?}"
                )));
            };
            let mut pane = serde_json::json!({"id":id,"index":integer(index)?,"cwd":cwd,"pid":integer(pid)?,"command":command,"active":*active == "1","start":null});
            if !tagged.is_empty()
                && let Some(object) = pane.as_object_mut()
            {
                object.insert("tagged_key".into(), Value::from(*tagged));
            }
            let extra = metadata(&pane).map_err(|e| SnapshotError::Invalid(e.into()))?;
            if let Some(object) = pane.as_object_mut() {
                object.extend(extra);
            }
            panes.push(pane);
        }
        if read(&["display-message", "-p", "-t", wid, "#{window_layout}"])? != *layout {
            return Err(SnapshotError::Invalid(
                "Window resized during capture".into(),
            ));
        }
        if let Some(object) = window.as_object_mut() {
            object.insert("panes".into(), Value::Array(panes));
        }
        windows.push(window);
    }
    let captured = serde_json::json!({"windows":windows,"captured_at":std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs())});
    if comandos_core::workspace::snapshot::check_snapshot(
        &serde_json::json!({"version":2,"sessions":{name:&captured}}),
    ) != comandos_core::workspace::snapshot::Snapshot::Valid
    {
        return Err(SnapshotError::Invalid(
            "incomplete or changed session layout".into(),
        ));
    }
    Ok(captured)
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
            let mut bytes = [0u8; 16];
            if getrandom::fill(&mut bytes).is_err() {
                // Refuse a deterministic cross-generation key on entropy failure.
                return captured;
            }
            key = Some(format!(
                "pane-{}",
                bytes.iter().map(|b| format!("{b:02x}")).collect::<String>()
            ));
        }
        let key = key.unwrap_or_else(|| "pane-1".to_string());
        used.insert(key.clone());
        if let Some(object) = pane.as_object_mut() {
            object.insert("key".into(), Value::String(key));
        }
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
