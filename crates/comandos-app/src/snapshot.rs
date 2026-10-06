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
    capture_with(tmux, name, |pane| inspect_pane(inspector, pane))
}

fn inspect_pane(
    inspector: &PaneInspector,
    pane: &Value,
) -> Result<serde_json::Map<String, Value>, &'static str> {
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
    let separator = separator()?;
    capture_from_reads(
        name,
        &separator,
        |args| tmux.read(args).map_err(SnapshotError::Read),
        &mut metadata,
        true,
    )
}

fn separator() -> Result<String, SnapshotError> {
    let mut nonce = [0u8; 12];
    getrandom::fill(&mut nonce).map_err(|e| SnapshotError::Invalid(e.to_string()))?;
    Ok(format!(
        "<cc-{}>",
        nonce.iter().map(|b| format!("{b:02x}")).collect::<String>()
    ))
}

fn formats(separator: &str) -> (String, String) {
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
    .join(separator);
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
    .join(separator);
    (window_format, pane_format)
}

fn capture_from_reads(
    name: &str,
    separator: &str,
    mut read: impl FnMut(&[&str]) -> Result<String, SnapshotError>,
    metadata: &mut impl FnMut(&Value) -> Result<serde_json::Map<String, Value>, &'static str>,
    check_layout: bool,
) -> Result<Value, SnapshotError> {
    let (window_format, pane_format) = formats(separator);
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
        let fields: Vec<_> = line.split(separator).collect();
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
            let fields: Vec<_> = row.split(separator).collect();
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
        if check_layout
            && read(&["display-message", "-p", "-t", wid, "#{window_layout}"])? != *layout
        {
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

/// Capture all requested sessions with three bounded tmux processes. The final
/// layout check runs after every pane's process metadata has been inspected.
/// Any failed command discards the entire batch; the caller owns fallback.
pub fn capture_sessions_when(
    tmux: &TmuxCtl,
    names: &[String],
    inspector: &PaneInspector,
    allowed: &dyn Fn() -> bool,
) -> Result<serde_json::Map<String, Value>, SnapshotError> {
    capture_sessions_with(
        names,
        |commands| {
            tmux.read_batch_when(commands, crate::tmux::TMUX_TIMEOUT, allowed)
                .map_err(|e| SnapshotError::Read(format!("{e:?}")))
        },
        |pane| inspect_pane(inspector, pane),
        allowed,
    )
}

/// Injectable equivalent of the real batch transport and process inspection.
pub fn capture_sessions_with(
    names: &[String],
    mut read_batch: impl FnMut(&[Vec<String>]) -> Result<crate::tmux::TmuxOut, SnapshotError>,
    mut metadata: impl FnMut(&Value) -> Result<serde_json::Map<String, Value>, &'static str>,
    allowed: &dyn Fn() -> bool,
) -> Result<serde_json::Map<String, Value>, SnapshotError> {
    if names.is_empty() {
        return Ok(serde_json::Map::new());
    }
    if names.len() > 256 || !allowed() {
        return Err(SnapshotError::Invalid("snapshot batch unavailable".into()));
    }
    let separator = separator()?;
    let (window_format, pane_format) = formats(&separator);
    let commands: Vec<_> = names
        .iter()
        .map(|name| {
            vec![
                "list-windows".into(),
                "-t".into(),
                format!("={name}"),
                "-F".into(),
                window_format.clone(),
            ]
        })
        .collect();
    let raw_windows = read_framed(&commands, &mut read_batch, allowed)?;
    let mut windows = std::collections::BTreeMap::new();
    for raw in &raw_windows {
        for line in raw.lines() {
            let fields: Vec<_> = line.split(&separator).collect();
            let [wid, _, _, layout, ..] = fields.as_slice() else {
                return Err(SnapshotError::Invalid("incomplete batch window".into()));
            };
            if fields.len() != 11 || !wid.starts_with('@') {
                return Err(SnapshotError::Invalid("invalid batch window".into()));
            }
            if let Some(old) = windows.insert((*wid).to_owned(), (*layout).to_owned())
                && old != *layout
            {
                return Err(SnapshotError::Invalid(
                    "Window resized during capture".into(),
                ));
            }
            if windows.len() > 1024 {
                return Err(SnapshotError::Invalid("snapshot batch too large".into()));
            }
        }
    }
    let commands: Vec<_> = windows
        .keys()
        .map(|wid| {
            vec![
                "list-panes".into(),
                "-t".into(),
                wid.clone(),
                "-F".into(),
                pane_format.clone(),
            ]
        })
        .collect();
    let raw_panes = read_framed(&commands, &mut read_batch, allowed)?;
    let panes: std::collections::BTreeMap<_, _> = windows.keys().zip(&raw_panes).collect();
    let mut captured = serde_json::Map::new();
    for (name, raw) in names.iter().zip(&raw_windows) {
        if !allowed() {
            return Err(SnapshotError::Invalid("snapshot cancelled".into()));
        }
        let value = capture_from_reads(
            name,
            &separator,
            |args| match args {
                ["list-windows", "-t", _, "-F", _] => Ok(raw.clone()),
                ["list-panes", "-t", wid, "-F", _] => panes
                    .get(&wid.to_string())
                    .map(|s| (*s).clone())
                    .ok_or_else(|| SnapshotError::Invalid("missing batch pane data".into())),
                _ => Err(SnapshotError::Invalid("unexpected snapshot read".into())),
            },
            &mut metadata,
            false,
        )?;
        if captured.insert(name.clone(), value).is_some() {
            return Err(SnapshotError::Invalid("duplicate snapshot session".into()));
        }
    }
    let commands: Vec<_> = windows
        .keys()
        .map(|wid| {
            vec![
                "display-message".into(),
                "-p".into(),
                "-t".into(),
                wid.clone(),
                "#{window_layout}".into(),
            ]
        })
        .collect();
    let layouts = read_framed(&commands, &mut read_batch, allowed)?;
    if windows
        .values()
        .zip(&layouts)
        .any(|(expected, actual)| expected != actual)
    {
        return Err(SnapshotError::Invalid(
            "Window resized during capture".into(),
        ));
    }
    if !allowed() {
        return Err(SnapshotError::Invalid("snapshot cancelled".into()));
    }
    Ok(captured)
}

fn read_framed(
    commands: &[Vec<String>],
    read_batch: &mut impl FnMut(&[Vec<String>]) -> Result<crate::tmux::TmuxOut, SnapshotError>,
    allowed: &dyn Fn() -> bool,
) -> Result<Vec<String>, SnapshotError> {
    if commands.is_empty() || !allowed() {
        return Err(SnapshotError::Invalid(
            "empty or cancelled snapshot batch".into(),
        ));
    }
    let delimiter = separator()?;
    let mut framed = Vec::new();
    for (index, command) in commands.iter().enumerate() {
        for (suffix, query) in [("begin", false), ("end", true)] {
            if query {
                framed.push(command.clone());
            }
            framed.push(vec![
                "display-message".into(),
                "-p".into(),
                format!("{delimiter}{index}:{suffix}"),
            ]);
        }
    }
    // Stay well below argv limits. Oversized captures use the existing fallback.
    if framed
        .iter()
        .flatten()
        .map(|arg| arg.len() + 1)
        .sum::<usize>()
        > 64 * 1024
    {
        return Err(SnapshotError::Invalid(
            "snapshot batch arguments too large".into(),
        ));
    }
    let output = read_batch(&framed)?;
    // tmux can return the final command's status even if an earlier command
    // failed. Refuse stderr and require a nonempty, complete frame for each one.
    if !output.ok() || !output.stderr.is_empty() || !allowed() {
        return Err(SnapshotError::Read(
            "incomplete or cancelled tmux batch".into(),
        ));
    }
    let mut lines = output.stdout.lines();
    let mut values = Vec::new();
    for index in 0..commands.len() {
        let begin = format!("{delimiter}{index}:begin");
        let end = format!("{delimiter}{index}:end");
        if lines.next() != Some(begin.as_str()) {
            return Err(SnapshotError::Invalid("missing snapshot frame".into()));
        }
        let mut body = Vec::new();
        loop {
            match lines.next() {
                Some(line) if line == end => break,
                Some(line) if !line.contains(&delimiter) => body.push(line),
                _ => return Err(SnapshotError::Invalid("incomplete snapshot frame".into())),
            }
        }
        if body.is_empty() {
            return Err(SnapshotError::Invalid("empty snapshot frame".into()));
        }
        values.push(body.join("\n"));
    }
    if lines.next().is_some() {
        return Err(SnapshotError::Invalid("extra snapshot output".into()));
    }
    Ok(values)
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
