//! IPC files produced by the dashboard and consumed by the desktop app.
use crate::mode::RunMode;
pub trait IpcGuard: Clone {
    type Error;
    fn remove_file(&self, path: &Path) -> Result<(), Self::Error>;
}
use serde_json::Value;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq)]
pub struct IpcRequest {
    pub kind: IpcKind,
    pub payload: Value,
    pub path: PathBuf,
    pub stamp: (u64, u64, i64, i64),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IpcKind {
    Focus,
    TabClose,
    TabOpen,
    Command,
}

#[derive(Debug)]
pub enum IpcError {
    Unknown(PathBuf),
    Io(PathBuf, String),
    Json(PathBuf),
}

pub fn read_request(path: &Path) -> Result<IpcRequest, IpcError> {
    let kind = match path.file_name().and_then(|s| s.to_str()) {
        Some("app-focus.json") | Some("app-tab-active.json") => IpcKind::Focus,
        Some("app-tab-close.json") => IpcKind::TabClose,
        Some("app-tab-open.json") => IpcKind::TabOpen,
        Some("app-command.json") => IpcKind::Command,
        _ => return Err(IpcError::Unknown(path.to_path_buf())),
    };
    let before =
        std::fs::metadata(path).map_err(|e| IpcError::Io(path.to_path_buf(), e.to_string()))?;
    if before.len() > 1024 * 1024 {
        return Err(IpcError::Json(path.to_path_buf()));
    }
    let stamp = (
        before.dev(),
        before.ino(),
        before.mtime(),
        before.mtime_nsec(),
    );
    let bytes = std::fs::read(path).map_err(|e| IpcError::Io(path.to_path_buf(), e.to_string()))?;
    let after =
        std::fs::metadata(path).map_err(|e| IpcError::Io(path.to_path_buf(), e.to_string()))?;
    if stamp != (after.dev(), after.ino(), after.mtime(), after.mtime_nsec()) {
        return Err(IpcError::Json(path.to_path_buf()));
    }
    let payload = comandos_core::json::workspace_loads_bytes(&bytes)
        .ok_or_else(|| IpcError::Json(path.to_path_buf()))?;
    if !payload.is_object() {
        return Err(IpcError::Json(path.to_path_buf()));
    }
    Ok(IpcRequest {
        kind,
        payload,
        path: path.to_path_buf(),
        stamp,
    })
}

#[derive(Clone)]
pub struct IpcConsumer<G: IpcGuard> {
    mode: RunMode,
    guard: G,
}

impl<G: IpcGuard> IpcConsumer<G> {
    pub fn new(mode: RunMode, guard: G) -> Self {
        Self { mode, guard }
    }

    pub fn consume(&self, request: &IpcRequest) -> Result<(), G::Error> {
        if self.mode == RunMode::Shadow {
            return Ok(());
        }
        // An atomic replacement arriving after the read belongs to the next request.
        let Ok(current) = read_request(&request.path) else {
            return Ok(());
        };
        if current.stamp != request.stamp || current.payload != request.payload {
            return Ok(());
        }
        self.guard.remove_file(&request.path)
    }
}
