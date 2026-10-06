//! Reads and writes shared app state files under `hooks_dir`.
use crate::mode::RunMode;
use comandos_store::snapshot_files::{self, Backend, Policy};
use nix::fcntl::{Flock, FlockArg};
use serde_json::Value;
use std::path::{Path, PathBuf};

pub const MAX_BYTES: usize = 8 * 1024 * 1024;
pub trait StateConfig: Clone {
    fn mode(&self) -> RunMode;
    fn home(&self) -> &Path;
    fn hooks_dir(&self) -> &Path;
}
pub trait StateGuard: Clone {
    type Error: std::fmt::Debug;
    fn create_dir_all(&self, path: &Path, mode: u32) -> Result<(), Self::Error>;
    fn open_lock(&self, path: &Path) -> Result<std::fs::File, Self::Error>;
    fn write_atomic(&self, path: &Path, bytes: &[u8], prefix: &str) -> Result<(), Self::Error>;
    fn write_atomic_when(
        &self,
        path: &Path,
        bytes: &[u8],
        prefix: &str,
        allowed: impl Fn() -> bool,
    ) -> Result<(), Self::Error>;
    fn archive_once(&self, path: &Path, bytes: &[u8]) -> Result<bool, Self::Error>;
    fn remove_file(&self, path: &Path) -> Result<(), Self::Error>;
}

#[derive(Debug)]
pub enum StateError<E> {
    Name(String),
    Guard(E),
    Io(PathBuf, String),
    Json(PathBuf, String),
}

#[derive(Clone)]
pub struct StateFiles<C: StateConfig, G: StateGuard> {
    config: C,
    guard: G,
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

impl<C: StateConfig, G: StateGuard> StateFiles<C, G> {
    /// These two pane UI documents have distinct catalog owners.
    pub fn read_pane_document(&self, name: &str) -> Result<Value, StateError<G::Error>> {
        self.read_ui_document(name)
    }
    pub fn read_ui_document(&self, name: &str) -> Result<Value, StateError<G::Error>> {
        let (domain, key) = match name {
            "app-tab-models.json" => ("tabs", "H/app-tab-models.json"),
            "app-extension-shelf.json" => ("app-ui", "H/app-extension-shelf.json"),
            "app-layout.json" => ("app-ui", "H/app-layout.json"),
            "app-pane-position.json" => ("app-ui", "H/app-pane-position.json"),
            _ => return Err(StateError::Name(name.into())),
        };
        let path = self.path(name)?;
        let doc = comandos_store::domains::DomainStore {
            home: self.config.home(),
        }
        .document(key, domain, path.clone());
        match doc
            .read_readonly()
            .map_err(|e| StateError::Io(path.clone(), e.to_string()))?
        {
            None => Ok(Value::Null),
            Some(bytes) if bytes.len() <= MAX_BYTES => {
                comandos_core::json::workspace_loads_bytes(&bytes)
                    .ok_or_else(|| StateError::Json(path, "invalid pane document JSON".into()))
            }
            Some(_) => Err(StateError::Name("pane document exceeds 8 MiB".into())),
        }
    }
    pub fn write_shelf_when(
        &self,
        expected: &Value,
        next: &Value,
        allowed: impl Fn() -> bool,
    ) -> Result<bool, StateError<G::Error>> {
        self.write_ui_document_when("app-extension-shelf.json", expected, next, allowed)
    }
    pub fn write_ui_document_when(
        &self,
        name: &str,
        expected: &Value,
        next: &Value,
        allowed: impl Fn() -> bool,
    ) -> Result<bool, StateError<G::Error>> {
        let key = match name {
            "app-extension-shelf.json" => "H/app-extension-shelf.json",
            "app-layout.json" => "H/app-layout.json",
            "app-pane-position.json" => "H/app-pane-position.json",
            _ => return Err(StateError::Name(name.into())),
        };
        let path = self.path(name)?;
        if !allowed() {
            return Ok(false);
        }
        let doc = comandos_store::domains::DomainStore {
            home: self.config.home(),
        }
        .document(key, "app-ui", path.clone());
        doc.with_legacy_authority(|| {
            if self.config.mode() == RunMode::Sandbox {
                self.guard
                    .create_dir_all(self.config.hooks_dir(), 0o700)
                    .map_err(|e| comandos_store::Error::Validation(format!("{e:?}")))?;
            }
            let lock_path = self.config.hooks_dir().join(format!("{name}.lock"));
            let lock = self
                .guard
                .open_lock(&lock_path)
                .map_err(|e| comandos_store::Error::Validation(format!("{e:?}")))?;
            let _lock = lock_exclusive::<G::Error>(lock, &lock_path)
                .map_err(|e| comandos_store::Error::Validation(format!("{e:?}")))?;
            let current = self
                .read(name)
                .map_err(|e| comandos_store::Error::Validation(format!("{e:?}")))?;
            if &current != expected || !allowed() {
                return Ok(false);
            }
            let text = comandos_core::json::response_dumps(next)
                .map_err(comandos_store::Error::Validation)?;
            if text.len() > MAX_BYTES {
                return Err(comandos_store::Error::Validation(
                    "shelf document exceeds 8 MiB".into(),
                ));
            }
            doc.with_legacy_authority(|| {
                self.guard
                    .write_atomic_when(&path, text.as_bytes(), "app-state.", || {
                        allowed() && doc.with_legacy_authority(|| Ok(true)).is_ok()
                    })
                    .map_err(|e| comandos_store::Error::Validation(format!("{e:?}")))?;
                Ok(true)
            })
        })
        .map_err(|e| StateError::Io(path, e.to_string()))
    }
    pub fn read_snippets(&self) -> Result<Vec<Value>, StateError<G::Error>> {
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
            Some(bytes) if bytes.len() <= MAX_BYTES => {
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
    ) -> Result<bool, StateError<G::Error>> {
        let path = self.path("snippets.json")?;
        if !allowed() {
            return Ok(false);
        }
        let doc = comandos_store::domains::DomainStore {
            home: self.config.home(),
        }
        .document("H/snippets.json", "ui-docs", path.clone());
        doc.with_legacy_authority(|| {
            if self.config.mode() == RunMode::Sandbox {
                self.guard
                    .create_dir_all(self.config.hooks_dir(), 0o700)
                    .map_err(|e| comandos_store::Error::Validation(format!("{e:?}")))?;
            }
            let lock_path = self.config.hooks_dir().join("snippets.json.lock");
            let lock = self
                .guard
                .open_lock(&lock_path)
                .map_err(|e| comandos_store::Error::Validation(format!("{e:?}")))?;
            let _lock = lock_exclusive::<G::Error>(lock, &lock_path)
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
            if text.len() > MAX_BYTES {
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
    pub fn new(config: C, guard: G) -> Self {
        Self { config, guard }
    }

    pub fn read(&self, name: &str) -> Result<Value, StateError<G::Error>> {
        let path = self.path(name)?;
        match std::fs::read(&path) {
            Ok(bytes) => comandos_core::json::workspace_loads_bytes(&bytes)
                .ok_or_else(|| StateError::Json(path, "invalid JSON".into())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Value::Null),
            Err(e) => Err(StateError::Io(path, e.to_string())),
        }
    }

    pub fn write_tabs(&self, value: &Value) -> Result<(), StateError<G::Error>> {
        self.write_tabs_when(value, || true)
    }

    pub fn write_tabs_when(
        &self,
        value: &Value,
        allowed: impl Fn() -> bool,
    ) -> Result<(), StateError<G::Error>> {
        let lock_path = self.config.hooks_dir().join("app-tabs.json.lock");
        let lock = self
            .guard
            .open_lock(&lock_path)
            .map_err(StateError::Guard)?;
        let _lock = lock_exclusive::<G::Error>(lock, &lock_path)?;
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
    ) -> Result<(), StateError<G::Error>> {
        if !matches!(name, "app-sessions-v2.json" | "app-tabs-snapshot.json") {
            return Err(StateError::Name(name.into()));
        }
        let lock_path = self.config.hooks_dir().join("app-tabs.json.lock");
        let lock = self
            .guard
            .open_lock(&lock_path)
            .map_err(StateError::Guard)?;
        let _lock = lock_exclusive::<G::Error>(lock, &lock_path)?;
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

    fn write_session_snapshot(&self, value: &Value) -> Result<(), StateError<G::Error>> {
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

    pub fn archive_tab(&self, entry: &Value) -> Result<(), StateError<G::Error>> {
        let key = entry
            .get("session")
            .and_then(Value::as_str)
            .filter(|k| !k.is_empty())
            .ok_or_else(|| StateError::Name("history requires session".into()))?;
        let path = self.config.hooks_dir().join("app-tabs-history.json.lock");
        let lock = self.guard.open_lock(&path).map_err(StateError::Guard)?;
        let _lock = lock_exclusive::<G::Error>(lock, &path)?;
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

    pub fn write(&self, name: &str, value: &Value) -> Result<(), StateError<G::Error>> {
        let path = self.path(name)?;
        let text = comandos_core::json::response_dumps(value).map_err(StateError::Name)?;
        self.guard
            .write_atomic(&path, text.as_bytes(), tmp_prefix::<G::Error>(name)?)
            .map_err(StateError::Guard)
    }

    fn path(&self, name: &str) -> Result<PathBuf, StateError<G::Error>> {
        if !ALLOWED.contains(&name) {
            return Err(StateError::Name(name.to_string()));
        }
        Ok(self.config.hooks_dir().join(name))
    }
}

struct SnapshotBackend<'a, G: StateGuard>(&'a G);
impl<G: StateGuard> Backend for SnapshotBackend<'_, G> {
    type Error = StateError<G::Error>;
    fn invalid(message: String) -> StateError<G::Error> {
        StateError::Name(message)
    }
    fn read(&self, path: &Path) -> Result<Option<Vec<u8>>, StateError<G::Error>> {
        match std::fs::read(path) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(StateError::Io(path.to_path_buf(), e.to_string())),
        }
    }
    fn prepare(&self, path: &Path) -> Result<(), StateError<G::Error>> {
        self.0
            .create_dir_all(path, 0o700)
            .map_err(StateError::Guard)
    }
    fn write(&self, path: &Path, bytes: &[u8]) -> Result<(), StateError<G::Error>> {
        self.0
            .write_atomic(path, bytes, "app-state.")
            .map_err(StateError::Guard)
    }
    fn archive(
        &self,
        _source: &Path,
        target: &Path,
        bytes: &[u8],
    ) -> Result<bool, StateError<G::Error>> {
        self.0
            .archive_once(target, bytes)
            .map_err(StateError::Guard)
    }
    fn backup(
        &self,
        _source: &Path,
        target: &Path,
        bytes: &[u8],
    ) -> Result<(), StateError<G::Error>> {
        self.0
            .write_atomic(target, bytes, "app-sessions-v2.")
            .map_err(StateError::Guard)
    }
    fn prune(&self, history: &Path, cutoff: i128) -> Result<(), StateError<G::Error>> {
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

fn lock_exclusive<E>(
    file: std::fs::File,
    path: &Path,
) -> Result<Flock<std::fs::File>, StateError<E>> {
    Flock::lock(file, FlockArg::LockExclusive)
        .map_err(|(_, e)| StateError::Io(path.to_path_buf(), e.to_string()))
}

fn tmp_prefix<E>(name: &str) -> Result<&'static str, StateError<E>> {
    match name {
        "app-tabs.json" => Ok("app-tabs."),
        "app-tabs-history.json" => Ok("app-tabs-history."),
        "app-tabs-snapshot.json" => Ok("app-tabs-snapshot."),
        "snippets.json" => Ok(".snippets.json."),
        _ => Ok("app-state."),
    }
}
