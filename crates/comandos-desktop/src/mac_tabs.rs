//! Data contracts extracted from the macOS desktop, independent of its UI toolkit.
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TabKind {
    Project,
    Scratch,
    Shell,
    Ssh,
    #[serde(rename = "ssh-tab")]
    SshTab,
    Xterm,
}
impl TabKind {
    fn metadata(raw: &str) -> Option<Self> {
        match raw {
            "project" => Some(Self::Project),
            "scratch" => Some(Self::Scratch),
            "shell" => Some(Self::Shell),
            "ssh" => Some(Self::Ssh),
            "ssh-tab" => Some(Self::SshTab),
            _ => None,
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TabMeta {
    pub kind: TabKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RestoreSpec {
    pub session: String,
    pub kind: TabKind,
    pub host: String,
    pub cwd: String,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MetadataError {
    UnhashableKind(&'static str),
}
fn load_text_json(raw: &[u8]) -> Option<Value> {
    // The macOS original uses json.load(open(path)) with text UTF-8 input.
    // Preserve its rejection of BOM and UTF-16/32 rather than byte autodetection.
    comandos_core::json::workspace_loads(std::str::from_utf8(raw).ok()?).ok()
}
pub fn load_tab_metadata(raw: &[u8]) -> Result<BTreeMap<String, TabMeta>, MetadataError> {
    let mut result = BTreeMap::new();
    let data = load_text_json(raw).unwrap_or(Value::Null);
    if let Some(items) = data.as_object() {
        for (session, item) in items {
            if !item.is_object() {
                continue;
            }
            if let Some(value) = item.get("kind") {
                if value.is_array() {
                    return Err(MetadataError::UnhashableKind("list"));
                }
                if value.is_object() {
                    return Err(MetadataError::UnhashableKind("dict"));
                }
            }
            let Some(kind) = item
                .get("kind")
                .and_then(Value::as_str)
                .and_then(TabKind::metadata)
            else {
                continue;
            };
            result.insert(
                session.clone(),
                TabMeta {
                    kind,
                    host: item
                        .get("host")
                        .and_then(Value::as_str)
                        .filter(|s| !s.is_empty())
                        .map(str::to_string),
                    cwd: item
                        .get("cwd")
                        .and_then(Value::as_str)
                        .filter(|s| s.starts_with('/'))
                        .map(str::to_string),
                },
            );
        }
    }
    Ok(result)
}
pub type SavedTabs = Vec<(String, Value)>;
pub fn load_saved_tabs(raw: &[u8]) -> SavedTabs {
    match load_text_json(raw) {
        Some(Value::Array(items)) => items
            .into_iter()
            .filter_map(|v| {
                v.as_str()
                    .filter(|s| !s.is_empty())
                    .map(|s| (s.to_string(), Value::Null))
            })
            .collect(),
        Some(Value::Object(items)) => items.into_iter().filter(|(s, _)| !s.is_empty()).collect(),
        _ => vec![],
    }
}
pub fn merge_tab_labels(
    saved: &[(String, Value)],
    current: &[(String, String)],
) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = vec![];
    for (key, label) in saved {
        if key.is_empty() {
            continue;
        }
        let label = label.as_str().filter(|s| !s.is_empty()).unwrap_or(key);
        put_label(&mut out, key, label);
    }
    for (key, label) in current {
        put_label(&mut out, key, label);
    }
    out
}
fn put_label(out: &mut Vec<(String, String)>, key: &str, label: &str) {
    if let Some((_, old)) = out.iter_mut().find(|(k, _)| k == key) {
        *old = label.into();
    } else {
        out.push((key.into(), label.into()));
    }
}
pub fn cancel_restore_snapshot(saved: &[(String, Value)], session: &str) -> (SavedTabs, bool) {
    let mut removed = false;
    let remaining = saved
        .iter()
        .filter(|(key, _)| {
            let keep = key.split(':').next() != Some(session);
            removed |= !keep;
            keep
        })
        .cloned()
        .collect();
    (remaining, removed)
}
pub fn ssh_host_from_session(session: &str) -> Option<String> {
    if let Some(host) = session.strip_prefix("ssh-") {
        return (!host.is_empty()).then(|| host.to_string());
    }
    let (host, index) = session.strip_prefix("sshtab-")?.rsplit_once('-')?;
    (!host.is_empty() && !index.is_empty() && index.chars().all(crate::python_digit::is_digit))
        .then(|| host.to_string())
}
pub fn restore_tab_spec(
    key: &str,
    metadata: &BTreeMap<String, TabMeta>,
    project_dir: &str,
) -> RestoreSpec {
    let session = key.split(':').next().unwrap_or("").to_string();
    if let Some(meta) = metadata.get(&session).filter(|m| m.kind != TabKind::Xterm) {
        return RestoreSpec {
            session,
            kind: meta.kind,
            host: meta.host.clone().unwrap_or_default(),
            cwd: meta.cwd.clone().unwrap_or_default(),
        };
    }
    if !project_dir.is_empty() {
        return RestoreSpec {
            session,
            kind: TabKind::Project,
            host: String::new(),
            cwd: project_dir.into(),
        };
    }
    let host = ssh_host_from_session(&session);
    let kind = if session.starts_with("xterm-") {
        TabKind::Xterm
    } else if host.is_some() && session.starts_with("ssh-") {
        TabKind::Ssh
    } else if host.is_some() {
        TabKind::SshTab
    } else if session.starts_with("term-") {
        TabKind::Scratch
    } else {
        TabKind::Project
    };
    RestoreSpec {
        session,
        kind,
        host: host.unwrap_or_default(),
        cwd: String::new(),
    }
}
pub fn find_project_dir(codebase: &Path, session: &str) -> Option<PathBuf> {
    let first: Vec<_> = std::fs::read_dir(codebase)
        .ok()?
        .filter_map(Result::ok)
        .filter(|e| e.path().is_dir())
        .collect();
    let mut candidates: Vec<PathBuf> = first.iter().map(|e| e.path()).collect();
    for parent in first {
        if let Ok(entries) = std::fs::read_dir(parent.path()) {
            candidates.extend(
                entries
                    .filter_map(Result::ok)
                    .filter(|e| e.path().is_dir() && e.file_name() != "node_modules")
                    .map(|e| e.path()),
            );
        }
    }
    let want = session.to_lowercase();
    candidates.into_iter().find(|path| {
        path.file_name().and_then(|n| n.to_str()).is_some_and(|n| {
            n.replace(['.', ':'], "-")
                .chars()
                .take(80)
                .collect::<String>()
                .to_lowercase()
                == want
        })
    })
}
pub fn agent_from_command(command: &str) -> &'static str {
    match comandos_core::text::strip(command) {
        "node" => "codex",
        "claude" => "claude",
        "grok" => "grok",
        "opencode" => "opencode",
        "gemini" => "gemini",
        "agy" => "agy",
        "aider" => "aider",
        _ => "claude",
    }
}
pub fn history_item(
    key: &str,
    label: &str,
    cwd: &str,
    command: &str,
    reason: &str,
    ts: i64,
) -> Value {
    json!({"session":key,"label":if label.is_empty(){key}else{label},"cwd":comandos_core::text::strip(cwd),"agent":agent_from_command(command),"reason":reason,"ts":ts})
}
pub fn archive_into(history: Option<Value>, item: Value) -> Value {
    let key = item.get("session").cloned().unwrap_or(Value::Null);
    let mut result = vec![item];
    result.extend(
        history
            .and_then(|v| v.as_array().cloned())
            .unwrap_or_default()
            .into_iter()
            .filter(|h| !h.is_object() || h.get("session") != Some(&key))
            .take(79),
    );
    Value::Array(result)
}
