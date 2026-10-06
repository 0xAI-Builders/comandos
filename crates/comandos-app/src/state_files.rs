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
        let lock_path = self.config.hooks_dir().join("app-tabs.json.lock");
        let lock = self
            .guard
            .open_lock(&lock_path)
            .map_err(StateError::Guard)?;
        let _lock = lock_exclusive(lock, &lock_path)?;
        self.write("app-tabs.json", value)
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
