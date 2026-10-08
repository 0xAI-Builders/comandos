//! Live MCP call policy for one exact tmux pane incarnation.
//! This does not unload tool descriptions or change skills in an agent's history.
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

const MAX_BYTES: u64 = 1_048_576;
fn invalid() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "política MCP por panel inválida",
    )
}
fn filename(identity: &str) -> io::Result<String> {
    if identity.is_empty() || identity.len() > 4096 {
        return Err(invalid());
    }
    Ok(format!("{:x}.json", Sha256::digest(identity.as_bytes())))
}
fn directory(home: &Path) -> PathBuf {
    home.join(".config/comandos/extensions/session-gates")
}
fn private_metadata(path: &Path, home: &Path, directory: bool) -> io::Result<()> {
    let meta = fs::symlink_metadata(path)?;
    if meta.file_type().is_symlink()
        || meta.uid() != fs::metadata(home)?.uid()
        || meta.permissions().mode() & 0o077 != 0
        || if directory {
            !meta.is_dir()
        } else {
            !meta.is_file()
        }
    {
        return Err(invalid());
    }
    Ok(())
}
fn validate(mcps: &Value) -> io::Result<()> {
    let rows = mcps.as_object().ok_or_else(invalid)?;
    if rows.len() > 3000
        || rows
            .iter()
            .any(|(name, enabled)| name.is_empty() || name.len() > 256 || !enabled.is_boolean())
    {
        return Err(invalid());
    }
    Ok(())
}

/// Missing policy and names inherit the global catalog. Malformed policy is an error.
pub fn selection(home: &Path, identity: &str) -> io::Result<Option<Value>> {
    let name = filename(identity)?;
    let dir = directory(home);
    match private_metadata(&dir, home, true) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        other => other?,
    }
    let path = dir.join(name);
    match private_metadata(&path, home, false) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        other => other?,
    }
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(nix::libc::O_NOFOLLOW)
        .open(&path)?;
    let mut bytes = Vec::new();
    file.take(MAX_BYTES + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err(invalid());
    }
    let data: Value = serde_json::from_slice(&bytes).map_err(|_| invalid())?;
    if data["version"] != 1 || data["identity"] != identity {
        return Err(invalid());
    }
    validate(&data["mcps"])?;
    Ok(Some(data["mcps"].clone()))
}

/// Callers must deny on errors; this gate never overrides a global catalog disable.
pub fn enabled(home: &Path, identity: &str, server: &str) -> io::Result<bool> {
    Ok(selection(home, identity)?.is_none_or(|mcps| mcps[server] != false))
}

/// Publishes the initial policy without replacing a newer selection already saved.
pub fn initialize(home: &Path, identity: &str, mcps: &Value) -> io::Result<()> {
    write_policy(home, identity, mcps, false)
}

/// Atomically replaces only this pane's policy, without writing provider configuration.
pub fn save(home: &Path, identity: &str, mcps: &Value) -> io::Result<()> {
    write_policy(home, identity, mcps, true)
}

fn write_policy(home: &Path, identity: &str, mcps: &Value, replace: bool) -> io::Result<()> {
    let name = filename(identity)?;
    validate(mcps)?;
    let dir = directory(home);
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(&dir)?;
    private_metadata(&dir, home, true)?;
    let data = serde_json::to_vec(&json!({"version":1,"identity":identity,"mcps":mcps}))?;
    if data.len() as u64 > MAX_BYTES {
        return Err(invalid());
    }
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let tmp = dir.join(format!(
        ".tmp-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&tmp)?;
        file.write_all(&data)?;
        file.sync_all()?;
        if replace {
            fs::rename(&tmp, dir.join(name))?;
        } else {
            match fs::hard_link(&tmp, dir.join(name)) {
                Ok(()) => {}
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
                    // Validate the winner, but never replace its live decision.
                    selection(home, identity)?.ok_or_else(invalid)?;
                }
                Err(e) => return Err(e),
            }
            fs::remove_file(&tmp)?;
        }
        File::open(&dir)?.sync_all()
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}
