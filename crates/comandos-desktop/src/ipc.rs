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

/// The home is explicit because sandbox hooks may be outside its standard path.
pub fn read_request_domain(home: &Path, path: &Path) -> Result<IpcRequest, IpcError> {
    let name = path
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or_else(|| IpcError::Unknown(path.into()))?;
    // app-tab-active is a document, never an IPC queue.
    if name == "app-tab-active.json" {
        return read_request(path);
    }
    let (mode, row) = comandos_store::domains::commands::peek(home, name)
        .map_err(|e| IpcError::Io(path.into(), e.to_string()))?;
    if !matches!(
        mode,
        comandos_store::unified::Mode::Unified | comandos_store::unified::Mode::Sealed
    ) {
        return read_request(path);
    }
    let (seq, body) = row.ok_or_else(|| IpcError::Io(path.into(), "no pending command".into()))?;
    if body.len() > 1 << 20 {
        return Err(IpcError::Json(path.into()));
    }
    let payload = comandos_core::json::workspace_loads_bytes(&body)
        .filter(Value::is_object)
        .ok_or_else(|| IpcError::Json(path.into()))?;
    let kind = match name {
        "app-focus.json" => IpcKind::Focus,
        "app-tab-open.json" => IpcKind::TabOpen,
        "app-tab-close.json" => IpcKind::TabClose,
        "app-command.json" => IpcKind::Command,
        _ => return Err(IpcError::Unknown(path.into())),
    };
    Ok(IpcRequest {
        kind,
        payload,
        path: path.into(),
        stamp: (u64::MAX, seq as u64, 0, 0),
    })
}

/// Best-effort polling, preserving path order and one authority lease per batch.
/// Missing or invalid individual requests are skipped just as in the app poller.
pub fn read_requests_domain(home: &Path, paths: &[PathBuf]) -> Result<Vec<IpcRequest>, IpcError> {
    read_requests_domain_with(home, paths, false)
}
/// Runtime polling permits payload commits while retaining the domain lease.
pub fn read_requests_domain_live(
    home: &Path,
    paths: &[PathBuf],
) -> Result<Vec<IpcRequest>, IpcError> {
    read_requests_domain_with(home, paths, true)
}
fn read_requests_domain_with(
    home: &Path,
    paths: &[PathBuf],
    live: bool,
) -> Result<Vec<IpcRequest>, IpcError> {
    let queued: Vec<_> = paths
        .iter()
        .filter_map(|path| {
            let name = path.file_name()?.to_str()?;
            comandos_store::domains::commands::kind(name)
                .ok()
                .map(|_| (path, name))
        })
        .collect();
    if queued.is_empty() {
        return Ok(paths
            .iter()
            .filter_map(|path| read_request(path).ok())
            .collect());
    }
    let names: Vec<_> = queued.iter().map(|(_, name)| *name).collect();
    let consume = |mode,
                   rows: Vec<
        comandos_store::Result<Option<comandos_store::domains::commands::PendingCommand>>,
    >| {
        let mut queued = queued.into_iter().zip(rows).peekable();
        paths
            .iter()
            .filter_map(|path| {
                let Some(((queued_path, name), row)) = queued.peek() else {
                    return (path.file_name()?.to_str()? == "app-tab-active.json")
                        .then(|| read_request(path).ok())
                        .flatten();
                };
                if *queued_path != path {
                    return (path.file_name()?.to_str()? == "app-tab-active.json")
                        .then(|| read_request(path).ok())
                        .flatten();
                }
                let name = *name;
                let row = row.as_ref().ok().and_then(Option::as_ref);
                let request = if matches!(
                    mode,
                    comandos_store::unified::Mode::Unified | comandos_store::unified::Mode::Sealed
                ) {
                    row.and_then(|(seq, body)| {
                        if body.len() > 1 << 20 {
                            return None;
                        }
                        let payload = comandos_core::json::workspace_loads_bytes(body)
                            .filter(Value::is_object)?;
                        let kind = match name {
                            "app-focus.json" => IpcKind::Focus,
                            "app-tab-open.json" => IpcKind::TabOpen,
                            "app-tab-close.json" => IpcKind::TabClose,
                            "app-command.json" => IpcKind::Command,
                            _ => return None,
                        };
                        Some(IpcRequest {
                            kind,
                            payload,
                            path: path.clone(),
                            stamp: (u64::MAX, *seq as u64, 0, 0),
                        })
                    })
                } else {
                    read_request(path).ok()
                };
                queued.next();
                request
            })
            .collect()
    };
    (if live {
        comandos_store::domains::commands::with_peeked_live(home, &names, consume)
    } else {
        comandos_store::domains::commands::with_peeked(home, &names, consume)
    })
    .map_err(|e| IpcError::Io(home.into(), e.to_string()))
}
impl<G: IpcGuard> IpcConsumer<G> {
    pub fn consume_domain(&self, home: &Path, request: &IpcRequest) -> Result<(), IpcError> {
        if self.mode == RunMode::Shadow {
            return Ok(());
        }
        let Some(name) = request.path.file_name().and_then(|s| s.to_str()) else {
            return Err(IpcError::Unknown(request.path.clone()));
        };
        if name == "app-tab-active.json" {
            return self
                .consume(request)
                .map_err(|_| IpcError::Io(request.path.clone(), "legacy consume failed".into()));
        }
        let body = comandos_core::json::response_dumps(&request.payload)
            .map_err(|_| IpcError::Json(request.path.clone()))?;
        let seq = (request.stamp.0 == u64::MAX).then_some(request.stamp.1 as i64);
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX));
        comandos_store::domains::commands::acknowledge(
            home,
            name,
            seq,
            body.as_bytes(),
            now,
            || {
                // Preserve an atomic replacement arriving after the read.
                if let Ok(current) = read_request(&request.path)
                    && current.payload == request.payload
                    && (seq.is_some() || current.stamp == request.stamp)
                {
                    self.guard.remove_file(&request.path).map_err(|_| {
                        comandos_store::Error::Validation("legacy consume failed".into())
                    })?;
                }
                Ok(())
            },
        )
        .map_err(|e| IpcError::Io(request.path.clone(), e.to_string()))
    }
}
