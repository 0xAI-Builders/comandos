//! Reads and writes shared app state files under `hooks_dir`.
use crate::config::AppConfig;
use crate::guard::{GuardError, WriteGuard};
use nix::fcntl::{Flock, FlockArg};
use serde_json::Value;
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub enum StateError {
    Name(String),
    Guard(GuardError),
    Io(PathBuf, String),
    Json(PathBuf, String),
}

#[derive(Clone)]
pub struct StateFiles {
    config: AppConfig,
    guard: WriteGuard,
}

const ALLOWED: &[&str] = &[
    "app-tabs.json",
    "app-tabs-history.json",
    "app-tabs-snapshot.json",
    "app-sessions-v2.json",
    "app-tab-active.json",
    "app-tab-models.json",
    "app-extension-shelf.json",
    "app-layout.json",
    "app-pane-position.json",
    "app-focus.json",
    "app-tab-close.json",
    "app-tab-open.json",
    "app-command.json",
    "snippets.json",
    "acp-panes.json",
];

impl StateFiles {
    pub fn new(config: AppConfig, guard: WriteGuard) -> Self {
        Self { config, guard }
    }

    pub fn read(&self, name: &str) -> Result<Value, StateError> {
        let path = self.path(name)?;
        match std::fs::read(&path) {
            Ok(bytes) => comandos_core::json::workspace_loads_bytes(&bytes)
                .ok_or_else(|| StateError::Json(path, "invalid JSON".into())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Value::Null),
            Err(e) => Err(StateError::Io(path, e.to_string())),
        }
    }

    pub fn write_tabs(&self, value: &Value) -> Result<(), StateError> {
        self.write_tabs_when(value, || true)
    }

    pub fn write_tabs_when(
        &self,
        value: &Value,
        allowed: impl FnOnce() -> bool,
    ) -> Result<(), StateError> {
        let lock_path = self.config.hooks_dir().join("app-tabs.json.lock");
        let lock = self
            .guard
            .open_lock(&lock_path)
            .map_err(StateError::Guard)?;
        let _lock = lock_exclusive(lock, &lock_path)?;
        if allowed() {
            self.write("app-tabs.json", value)
        } else {
            Ok(())
        }
    }

    pub fn write_snapshot_when(
        &self,
        name: &str,
        value: &Value,
        allowed: impl FnOnce() -> bool,
    ) -> Result<(), StateError> {
        if !matches!(name, "app-sessions-v2.json" | "app-tabs-snapshot.json") {
            return Err(StateError::Name(name.into()));
        }
        let lock_path = self.config.hooks_dir().join("app-tabs.json.lock");
        let lock = self
            .guard
            .open_lock(&lock_path)
            .map_err(StateError::Guard)?;
        let _lock = lock_exclusive(lock, &lock_path)?;
        if allowed() {
            if name == "app-sessions-v2.json" {
                self.write_session_snapshot(value)
            } else {
                self.write(name, value)
            }
        } else {
            Ok(())
        }
    }

    pub fn read_session_snapshot(&self) -> Value {
        for name in ["app-sessions-v2.json", "app-sessions-v2.json.bak"] {
            if let Ok(bytes) = std::fs::read(self.config.hooks_dir().join(name))
                && let Some(value) = comandos_core::json::workspace_loads_bytes(&bytes)
                && comandos_core::workspace::snapshot::check_snapshot(&value)
                    == comandos_core::workspace::snapshot::Snapshot::Valid
            {
                return value;
            }
        }
        serde_json::json!({"version":2,"sessions":{}})
    }

    fn write_session_snapshot(&self, value: &Value) -> Result<(), StateError> {
        if comandos_core::workspace::snapshot::check_snapshot(value)
            != comandos_core::workspace::snapshot::Snapshot::Valid
        {
            return Err(StateError::Name(
                "Refusing incomplete session layout snapshot".into(),
            ));
        }
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        if let Ok(old) = self.read("app-sessions-v2.json")
            && comandos_core::workspace::snapshot::check_snapshot(&old)
                == comandos_core::workspace::snapshot::Snapshot::Valid
        {
            let encoded = comandos_core::json::response_dumps(&old).map_err(StateError::Name)?;
            let history = self.config.hooks_dir().join("app-sessions-v2.json.history");
            self.guard
                .create_dir_all(&history, 0o700)
                .map_err(StateError::Guard)?;
            let stamp = old.get("saved_at").and_then(Value::as_u64).unwrap_or(now) / 60 * 60;
            self.guard
                .archive_once(
                    &history.join(format!("{stamp:012}.json")),
                    encoded.as_bytes(),
                )
                .map_err(StateError::Guard)?;
            if let Ok(entries) = std::fs::read_dir(&history) {
                for entry in entries.filter_map(Result::ok) {
                    if entry
                        .file_name()
                        .to_str()
                        .and_then(|n| n.strip_suffix(".json"))
                        .and_then(|n| n.parse::<u64>().ok())
                        .is_some_and(|stamp| stamp < now.saturating_sub(7 * 86400))
                    {
                        self.guard
                            .remove_file(&entry.path())
                            .map_err(StateError::Guard)?;
                    }
                }
            }
            let backup = self.config.hooks_dir().join("app-sessions-v2.json.bak");
            self.guard
                .write_atomic(&backup, encoded.as_bytes(), "app-sessions-v2.")
                .map_err(StateError::Guard)?;
        }
        self.write("app-sessions-v2.json", value)
    }

    pub fn archive_tab(&self, entry: &Value) -> Result<(), StateError> {
        let key = entry
            .get("session")
            .and_then(Value::as_str)
            .filter(|k| !k.is_empty())
            .ok_or_else(|| StateError::Name("history requires session".into()))?;
        let path = self.config.hooks_dir().join("app-tabs-history.json.lock");
        let lock = self.guard.open_lock(&path).map_err(StateError::Guard)?;
        let _lock = lock_exclusive(lock, &path)?;
        let mut history = self
            .read("app-tabs-history.json")
            .ok()
            .and_then(|v| v.as_array().cloned())
            .unwrap_or_default();
        history.retain(|item| item.get("session").and_then(Value::as_str) != Some(key));
        history.truncate(79);
        history.insert(0, entry.clone());
        self.write("app-tabs-history.json", &Value::Array(history))
    }

    pub fn write(&self, name: &str, value: &Value) -> Result<(), StateError> {
        let path = self.path(name)?;
        let text = comandos_core::json::response_dumps(value).map_err(StateError::Name)?;
        self.guard
            .write_atomic(&path, text.as_bytes(), tmp_prefix(name)?)
            .map_err(StateError::Guard)
    }

    fn path(&self, name: &str) -> Result<PathBuf, StateError> {
        if !ALLOWED.contains(&name) {
            return Err(StateError::Name(name.to_string()));
        }
        Ok(self.config.hooks_dir().join(name))
    }
}

fn lock_exclusive(file: std::fs::File, path: &Path) -> Result<Flock<std::fs::File>, StateError> {
    Flock::lock(file, FlockArg::LockExclusive)
        .map_err(|(_, e)| StateError::Io(path.to_path_buf(), e.to_string()))
}

fn tmp_prefix(name: &str) -> Result<&'static str, StateError> {
    match name {
        "app-tabs.json" => Ok("app-tabs."),
        "app-tabs-history.json" => Ok("app-tabs-history."),
        "app-tabs-snapshot.json" => Ok("app-tabs-snapshot."),
        _ => Ok("app-state."),
    }
}
