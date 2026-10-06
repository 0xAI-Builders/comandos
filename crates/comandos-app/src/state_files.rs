//! Reads and writes shared app state files under `hooks_dir`.
use crate::config::AppConfig;
use crate::guard::{GuardError, WriteGuard};
use comandos_store::snapshot_files::{self, Backend, Policy};
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
    pub fn read_snippets(&self) -> Result<Vec<Value>, StateError> {
        let path = self.path("snippets.json")?;
        let doc = comandos_store::domains::DomainStore {
            home: self.config.home(),
        }
        .document("H/snippets.json", "ui-docs", path.clone());
        match doc
            .read_readonly()
            .map_err(|e| StateError::Io(path.clone(), e.to_string()))?
        {
            None => Ok(Vec::new()),
            Some(bytes) if bytes.len() <= crate::clipboard_bridge::MAX_BYTES => {
                let value = comandos_core::json::workspace_loads_bytes(&bytes)
                    .ok_or_else(|| StateError::Json(path, "invalid snippets JSON".into()))?;
                Ok(value.as_array().cloned().unwrap_or_default())
            }
            Some(_) => Err(StateError::Name("snippets document exceeds 8 MiB".into())),
        }
    }
    pub fn write_snippets_when(
        &self,
        expected: &[Value],
        next: &[Value],
        allowed: impl Fn() -> bool,
    ) -> Result<bool, StateError> {
        let path = self.path("snippets.json")?;
        if !allowed() {
            return Ok(false);
        }
        let doc = comandos_store::domains::DomainStore {
            home: self.config.home(),
        }
        .document("H/snippets.json", "ui-docs", path.clone());
        doc.with_legacy_authority(|| {
            if self.config.mode() == crate::config::RunMode::Sandbox {
                self.guard
                    .create_dir_all(self.config.hooks_dir(), 0o700)
                    .map_err(|e| comandos_store::Error::Validation(format!("{e:?}")))?;
            }
            let lock_path = self.config.hooks_dir().join("snippets.json.lock");
            let lock = self
                .guard
                .open_lock(&lock_path)
                .map_err(|e| comandos_store::Error::Validation(format!("{e:?}")))?;
            let _lock = lock_exclusive(lock, &lock_path)
                .map_err(|e| comandos_store::Error::Validation(format!("{e:?}")))?;
            let current = self
                .read("snippets.json")
                .map_err(|e| comandos_store::Error::Validation(format!("{e:?}")))?;
            if current.as_array().cloned().unwrap_or_default() != expected {
                return Ok(false);
            }
            if !allowed() {
                return Ok(false);
            }
            let document = Value::Array(next.to_vec());
            let text = comandos_core::json::response_dumps(&document)
                .map_err(comandos_store::Error::Validation)?;
            if text.len() > crate::clipboard_bridge::MAX_BYTES {
                return Err(comandos_store::Error::Validation(
                    "snippets exceeds 8 MiB".into(),
                ));
            }
            doc.with_legacy_authority(|| {
                self.guard
                    .write_atomic_when(&path, text.as_bytes(), ".snippets.json.", || {
                        allowed() && doc.with_legacy_authority(|| Ok(true)).is_ok()
                    })
                    .map_err(|e| comandos_store::Error::Validation(format!("{e:?}")))?;
                Ok(true)
            })
        })
        .map_err(|e| StateError::Io(path, e.to_string()))
    }
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
        allowed: impl Fn() -> bool,
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
        allowed: impl Fn() -> bool,
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
        snapshot_files::read_with(
            &SnapshotBackend(&self.guard),
            &self.config.hooks_dir().join("app-sessions-v2.json"),
        )
    }

    fn write_session_snapshot(&self, value: &Value) -> Result<(), StateError> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        snapshot_files::write_with(
            &SnapshotBackend(&self.guard),
            &self.config.hooks_dir().join("app-sessions-v2.json"),
            value,
            i64::try_from(now).unwrap_or(i64::MAX),
            Policy::App,
        )
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

struct SnapshotBackend<'a>(&'a WriteGuard);
impl Backend for SnapshotBackend<'_> {
    type Error = StateError;
    fn invalid(message: String) -> StateError {
        StateError::Name(message)
    }
    fn read(&self, path: &Path) -> Result<Option<Vec<u8>>, StateError> {
        match std::fs::read(path) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(StateError::Io(path.to_path_buf(), e.to_string())),
        }
    }
    fn prepare(&self, path: &Path) -> Result<(), StateError> {
        self.0
            .create_dir_all(path, 0o700)
            .map_err(StateError::Guard)
    }
    fn write(&self, path: &Path, bytes: &[u8]) -> Result<(), StateError> {
        self.0
            .write_atomic(path, bytes, "app-state.")
            .map_err(StateError::Guard)
    }
    fn archive(&self, _source: &Path, target: &Path, bytes: &[u8]) -> Result<bool, StateError> {
        self.0
            .archive_once(target, bytes)
            .map_err(StateError::Guard)
    }
    fn backup(&self, _source: &Path, target: &Path, bytes: &[u8]) -> Result<(), StateError> {
        self.0
            .write_atomic(target, bytes, "app-sessions-v2.")
            .map_err(StateError::Guard)
    }
    fn prune(&self, history: &Path, cutoff: i128) -> Result<(), StateError> {
        if let Ok(entries) = std::fs::read_dir(history) {
            for entry in entries.filter_map(Result::ok) {
                if entry
                    .file_name()
                    .to_str()
                    .and_then(|n| n.strip_suffix(".json"))
                    .and_then(|n| n.parse::<u64>().ok())
                    .is_some_and(|n| i128::from(n) < cutoff)
                {
                    self.0
                        .remove_file(&entry.path())
                        .map_err(StateError::Guard)?;
                }
            }
        }
        Ok(())
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
        "snippets.json" => Ok(".snippets.json."),
        _ => Ok("app-state."),
    }
}
