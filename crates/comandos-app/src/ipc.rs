//! IPC files produced by the dashboard and consumed by the desktop app.
use crate::config::RunMode;
use crate::guard::{GuardError, WriteGuard};
use serde_json::Value;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq)]
pub struct IpcRequest {
    pub kind: IpcKind,
    pub payload: Value,
    pub path: PathBuf,
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
    let bytes = std::fs::read(path).map_err(|e| IpcError::Io(path.to_path_buf(), e.to_string()))?;
    let payload = comandos_core::json::workspace_loads_bytes(&bytes)
        .ok_or_else(|| IpcError::Json(path.to_path_buf()))?;
    Ok(IpcRequest {
        kind,
        payload,
        path: path.to_path_buf(),
    })
}

#[derive(Clone)]
pub struct IpcConsumer {
    mode: RunMode,
    guard: WriteGuard,
}

impl IpcConsumer {
    pub fn new(mode: RunMode, guard: WriteGuard) -> Self {
        Self { mode, guard }
    }

    pub fn consume(&self, request: &IpcRequest) -> Result<(), GuardError> {
        if self.mode == RunMode::Shadow {
            return Ok(());
        }
        self.guard.remove_file(&request.path)
    }
}
