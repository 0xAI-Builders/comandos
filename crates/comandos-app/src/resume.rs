use serde_json::Value;

const RESUME_SKIP_FLAGS: &[&str] = &["--resume", "--continue", "-r", "-c", "--last"];
const VALUE_FLAGS: &[&str] = &[
    "--model",
    "--effort",
    "--permission-mode",
    "--add-dir",
    "--settings",
    "--session-id",
    "--agents",
    "--mcp-config",
];
const CODEX_VALUE_FLAGS: &[&str] = &[
    "--model",
    "-m",
    "--config",
    "-c",
    "--sandbox",
    "-s",
    "--ask-for-approval",
    "-a",
    "--profile",
    "-p",
    "--cd",
    "-C",
    "--add-dir",
    "--image",
    "-i",
    "--enable",
    "--disable",
    "--local-provider",
];

pub fn sane_flags(raw: &Value, agent: &str) -> Vec<String> {
    let Some(raw) = raw.as_array() else {
        return Vec::new();
    };
    let args: Vec<&str> = raw.iter().filter_map(Value::as_str).collect();
    let skip_flags: Vec<&str> = if agent == "codex" {
        RESUME_SKIP_FLAGS
            .iter()
            .copied()
            .filter(|f| *f != "-c")
            .collect()
    } else {
        RESUME_SKIP_FLAGS.to_vec()
    };
    let value_flags = if agent == "codex" {
        CODEX_VALUE_FLAGS
    } else {
        VALUE_FLAGS
    };
    let mut out = Vec::new();
    let mut i = 0;
    while i < args.len() {
        let Some(arg) = args.get(i).copied() else {
            break;
        };
        if !arg.starts_with('-') {
            i += 1;
            continue;
        }
        if skip_flags.contains(&arg) {
            i += 1;
            if matches!(arg, "--resume" | "-r")
                && args.get(i).is_some_and(|next| !next.starts_with('-'))
            {
                i += 1;
            }
            continue;
        }
        if value_flags.contains(&arg) {
            if let Some(value) = args.get(i + 1).filter(|value| !value.starts_with('-')) {
                out.push(arg.to_string());
                out.push((*value).to_string());
                i += 2;
            } else {
                i += 1;
            }
            continue;
        }
        out.push(arg.to_string());
        i += 1;
    }
    out
}

pub fn resume_command(snapshot: &Value) -> Option<String> {
    let agent = snapshot.get("agent").and_then(Value::as_str).unwrap_or("");
    let flags = sane_flags(snapshot.get("flags").unwrap_or(&Value::Null), agent);
    let fl = flags
        .iter()
        .map(|f| shell_quote(f))
        .collect::<Vec<_>>()
        .join(" ");
    let suffix = if fl.is_empty() {
        String::new()
    } else {
        format!(" {fl}")
    };
    let sid = snapshot
        .get("resume_id")
        .and_then(Value::as_str)
        .filter(|s| is_uuidish(s))
        .unwrap_or("");
    match agent {
        "claude" if !sid.is_empty() => Some(format!("claude --resume {sid}{suffix}")),
        "codex" if !sid.is_empty() => Some(format!("codex resume {sid}{suffix}")),
        "acp" => {
            let st = snapshot.get("acp").and_then(Value::as_object)?;
            let mut cmd = format!(
                "cc-acp --agent {}",
                shell_quote(st.get("agent").and_then(Value::as_str).unwrap_or("claude"))
            );
            for (flag, key) in [
                ("--model", "model"),
                ("--effort", "effort"),
                ("--account", "account"),
                ("--resume", "sessionId"),
            ] {
                let value = st.get(key).and_then(Value::as_str).unwrap_or("");
                if !(value.is_empty() || key == "account" && value == "main") {
                    cmd.push_str(&format!(" {flag} {}", shell_quote(value)));
                }
            }
            Some(format!("{cmd}{suffix}"))
        }
        "grok" if !sid.is_empty() => Some(format!("grok --resume {sid}{suffix}")),
        _ => None,
    }
}

pub fn exact_resume_command(pane: &Value) -> Option<String> {
    if pane.get("resume_id").and_then(Value::as_str).is_none()
        && pane
            .pointer("/acp/sessionId")
            .and_then(Value::as_str)
            .is_none()
    {
        return None;
    }
    let cmd = resume_command(pane)?;
    let words: std::collections::BTreeSet<_> = cmd.split_whitespace().collect();
    (!words.contains("--last") && !words.contains("--continue")).then_some(cmd)
}

fn is_uuidish(raw: &str) -> bool {
    raw.len() == 36 && raw.chars().all(|c| c.is_ascii_hexdigit() || c == '-')
}

fn shell_quote(raw: &str) -> String {
    if raw
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | '/' | ':' | '='))
    {
        raw.to_string()
    } else {
        format!("'{}'", raw.replace('\'', "'\"'\"'"))
    }
}
