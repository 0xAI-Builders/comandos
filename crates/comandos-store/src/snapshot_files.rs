//! Shared snapshot generation policy with caller-owned filesystem guards.
use comandos_core::{
    json,
    workspace::snapshot::{Snapshot, check_snapshot},
};
use serde_json::Value;
use std::{
    fs,
    io::{self, Write},
    os::unix::fs::{DirBuilderExt, OpenOptionsExt},
    path::{Path, PathBuf},
};

#[derive(Clone, Copy)]
pub enum Policy {
    Bridge,
    App,
}

pub trait Backend {
    type Error;
    fn invalid(message: String) -> Self::Error;
    fn read(&self, path: &Path) -> Result<Option<Vec<u8>>, Self::Error>;
    fn prepare(&self, path: &Path) -> Result<(), Self::Error>;
    fn write(&self, path: &Path, bytes: &[u8]) -> Result<(), Self::Error>;
    fn archive(&self, source: &Path, target: &Path, bytes: &[u8]) -> Result<bool, Self::Error>;
    fn backup(&self, source: &Path, target: &Path, bytes: &[u8]) -> Result<(), Self::Error>;
    fn prune(&self, history: &Path, cutoff: i128) -> Result<(), Self::Error>;
}

pub fn suffix(path: &Path, tail: &str) -> PathBuf {
    let mut p = path.as_os_str().to_os_string();
    p.push(tail);
    p.into()
}
pub fn valid(bytes: &[u8]) -> Option<Value> {
    json::workspace_loads_bytes(bytes).filter(|v| check_snapshot(v) == Snapshot::Valid)
}
pub fn read_with(backend: &impl Backend, path: &Path) -> Value {
    for file in [path.to_owned(), suffix(path, ".bak")] {
        if let Ok(Some(bytes)) = backend.read(&file)
            && let Some(value) = valid(&bytes)
        {
            return value;
        }
    }
    serde_json::json!({"version":2,"sessions":{}})
}

pub struct Plan {
    pub current: Vec<u8>,
    pub previous: Option<Vec<u8>>,
    pub minute: Option<i128>,
    pub cutoff: i128,
}
fn stamp(value: Option<&Value>, now: i64, policy: Policy) -> Result<i128, String> {
    if matches!(policy, Policy::App) {
        return Ok(value
            .and_then(Value::as_u64)
            .map(i128::from)
            .unwrap_or(i128::from(now)));
    }
    let Some(value) = value else {
        return Ok(i128::from(now));
    };
    match value {
        Value::Bool(v) => Ok(i128::from(*v)),
        Value::Number(n) => n
            .as_str()
            .parse::<i128>()
            .ok()
            .or_else(|| {
                n.as_f64()
                    .filter(|f| f.is_finite() && *f >= i128::MIN as f64 && *f < i128::MAX as f64)
                    .map(|f| f.trunc() as i128)
            })
            .ok_or("snapshot timestamp outside supported integer range".into()),
        Value::String(s) => s
            .trim()
            .parse()
            .map_err(|_| "invalid snapshot saved_at integer".into()),
        _ => Err("invalid snapshot saved_at integer".into()),
    }
}
pub fn plan(value: &Value, old: Option<&[u8]>, now: i64, policy: Policy) -> Result<Plan, String> {
    if check_snapshot(value) != Snapshot::Valid {
        return Err("Refusing incomplete session layout snapshot".into());
    }
    let current = json::response_dumps(value)?.into_bytes();
    let (previous, minute) =
        if let Some((bytes, value)) = old.and_then(|b| valid(b).map(|v| (b, v))) {
            let minute = stamp(value.get("saved_at"), now, policy)?.div_euclid(60) * 60;
            let bytes = if matches!(policy, Policy::App) {
                json::response_dumps(&value)?.into_bytes()
            } else {
                bytes.to_vec()
            };
            (Some(bytes), Some(minute))
        } else {
            (None, None)
        };
    Ok(Plan {
        current,
        previous,
        minute,
        cutoff: if matches!(policy, Policy::App) {
            (i128::from(now) - 7 * 86400).max(0)
        } else {
            i128::from(now) - 7 * 86400
        },
    })
}
pub fn apply_with<B: Backend>(
    backend: &B,
    path: &Path,
    plan: &Plan,
    policy: Policy,
) -> Result<(), B::Error> {
    if let (Some(previous), Some(minute)) = (&plan.previous, plan.minute) {
        let history = suffix(path, ".history");
        backend.prepare(&history)?;
        let archived =
            backend.archive(path, &history.join(format!("{minute:012}.json")), previous)?;
        if archived || matches!(policy, Policy::App) {
            backend.prune(&history, plan.cutoff)?;
        }
        backend.backup(path, &suffix(path, ".bak"), previous)?;
    }
    backend.write(path, &plan.current)
}
pub fn write_with<B: Backend>(
    backend: &B,
    path: &Path,
    value: &Value,
    now: i64,
    policy: Policy,
) -> Result<(), B::Error> {
    if matches!(policy, Policy::Bridge)
        && let Some(parent) = path.parent()
    {
        backend.prepare(parent)?;
    }
    let old = match backend.read(path) {
        Ok(old) => old,
        Err(_) if matches!(policy, Policy::App) => None,
        Err(e) => return Err(e),
    };
    let plan = plan(value, old.as_deref(), now, policy).map_err(B::invalid)?;
    apply_with(backend, path, &plan, policy)
}

pub struct Files;
fn atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let parent = path.parent().unwrap_or(Path::new("."));
    fs::create_dir_all(parent)?;
    let mut nonce = [0; 12];
    getrandom::fill(&mut nonce).map_err(|e| io::Error::other(e.to_string()))?;
    let tmp = parent.join(format!(
        "{}.{}",
        path.file_name().unwrap_or_default().to_string_lossy(),
        nonce.iter().map(|b| format!("{b:02x}")).collect::<String>()
    ));
    let result = (|| {
        let mut f = fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .mode(0o600)
            .open(&tmp)?;
        f.write_all(bytes)?;
        f.flush()?;
        f.sync_all()?;
        fs::rename(&tmp, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(tmp);
    }
    result
}
impl Backend for Files {
    type Error = crate::Error;
    fn invalid(message: String) -> Self::Error {
        crate::Error::Validation(message)
    }
    fn read(&self, path: &Path) -> crate::Result<Option<Vec<u8>>> {
        match fs::read(path) {
            Ok(b) => Ok(Some(b)),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        }
    }
    fn prepare(&self, path: &Path) -> crate::Result<()> {
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(path)?;
        Ok(())
    }
    fn write(&self, path: &Path, bytes: &[u8]) -> crate::Result<()> {
        Ok(atomic(path, bytes)?)
    }
    fn archive(&self, source: &Path, target: &Path, bytes: &[u8]) -> crate::Result<bool> {
        let result = if fs::read(source).ok().as_deref() == Some(bytes) {
            fs::hard_link(source, target)
        } else {
            (|| {
                let mut f = fs::OpenOptions::new()
                    .create_new(true)
                    .write(true)
                    .mode(0o600)
                    .open(target)?;
                f.write_all(bytes)?;
                f.sync_all()
            })()
        };
        match result {
            Ok(()) => Ok(true),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => Ok(false),
            Err(e) => Err(e.into()),
        }
    }
    fn backup(&self, source: &Path, target: &Path, bytes: &[u8]) -> crate::Result<()> {
        if fs::read(source).ok().as_deref() != Some(bytes) {
            return self.write(target, bytes);
        }
        let mut nonce = [0; 12];
        getrandom::fill(&mut nonce).map_err(|e| io::Error::other(e.to_string()))?;
        let stage = suffix(
            target,
            &format!(
                ".{}",
                nonce.iter().map(|b| format!("{b:02x}")).collect::<String>()
            ),
        );
        fs::hard_link(source, &stage)?;
        if let Err(error) = fs::rename(&stage, target) {
            let _ = fs::remove_file(stage);
            return Err(error.into());
        }
        Ok(())
    }
    fn prune(&self, history: &Path, cutoff: i128) -> crate::Result<()> {
        for entry in fs::read_dir(history)? {
            let entry = entry?;
            if entry
                .file_name()
                .to_str()
                .and_then(|n| n.strip_suffix(".json"))
                .filter(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
                .and_then(|n| n.parse::<i128>().ok())
                .is_some_and(|n| n < cutoff)
            {
                fs::remove_file(entry.path())?;
            }
        }
        Ok(())
    }
}
