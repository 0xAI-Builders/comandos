//! Immutable native payloads; installed links never depend on the checkout.
use comandos_store::files::FileLock;
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Write,
    os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
};

fn artifact(name: &str) -> Result<(), String> {
    if matches!(
        name,
        "comandos-notifyd" | "cc-model-proxy" | "comandos-app-mac" | "comandos-broker-mac"
    ) {
        Ok(())
    } else {
        Err("unknown native component".into())
    }
}
fn read_binary(path: &Path) -> Result<Vec<u8>, String> {
    super::release::check_app_parents(path)?;
    let before = path
        .symlink_metadata()
        .map_err(|e| format!("{}: {e}", path.display()))?;
    if !before.is_file() || before.permissions().mode() & 0o111 == 0 {
        return Err(format!("{} must be a regular executable", path.display()));
    }
    let bytes = fs::read(path).map_err(|e| e.to_string())?;
    let after = path.symlink_metadata().map_err(|e| e.to_string())?;
    if bytes.is_empty()
        || (
            before.dev(),
            before.ino(),
            before.len(),
            before.mtime(),
            before.mtime_nsec(),
            before.ctime(),
            before.ctime_nsec(),
        ) != (
            after.dev(),
            after.ino(),
            after.len(),
            after.mtime(),
            after.mtime_nsec(),
            after.ctime(),
            after.ctime_nsec(),
        )
        || before.len() != bytes.len() as u64
    {
        return Err("native component changed during preflight".into());
    }
    Ok(bytes)
}
fn manifest(name: &str, hash: &str) -> Vec<u8> {
    serde_json::json!({"artifact":name,"sha256":hash,"artifact_version":1,"state_protocol":super::STATE_PROTOCOL}).to_string().into_bytes()
}
fn verified(dir: &Path, name: &str, bytes: &[u8], expected: &[u8]) -> Result<(), String> {
    let path = dir.join(name);
    if read_binary(&path)? != bytes {
        return Err(format!(
            "native component hash mismatch: {}",
            path.display()
        ));
    }
    let meta = dir.join("manifest.json");
    if !meta.symlink_metadata().is_ok_and(|m| m.is_file())
        || fs::read(&meta).map_err(|e| e.to_string())? != expected
    {
        return Err(format!(
            "native component manifest mismatch: {}",
            meta.display()
        ));
    }
    Ok(())
}
struct Temporary(PathBuf);
impl Drop for Temporary {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Preview validates the source and any existing installation, without writes.
pub fn stage(home: &Path, name: &str, source: &Path, dry: bool) -> Result<PathBuf, String> {
    artifact(name)?;
    super::release::check_app_parents(&home.join("placeholder"))?;
    let bytes = read_binary(source)?;
    let hash = format!("{:x}", Sha256::digest(&bytes));
    let parent = home.join(".local/share/comandos/components").join(name);
    let dest = parent.join(&hash);
    let expected = manifest(name, &hash);
    super::release::check_app_parents(&dest.join(name))?;
    if dest.symlink_metadata().is_ok() {
        verified(&dest, name, &bytes, &expected)?;
        return Ok(dest.join(name));
    }
    if dry {
        return Ok(dest.join(name));
    }
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(&parent)
        .map_err(|e| e.to_string())?;
    let _lock = FileLock::exclusive(&parent.join("stage.lock")).map_err(|e| e.to_string())?;
    if dest.symlink_metadata().is_ok() {
        verified(&dest, name, &bytes, &expected)?;
        return Ok(dest.join(name));
    }
    let mut random = [0u8; 16];
    getrandom::fill(&mut random).map_err(|e| e.to_string())?;
    let temporary = Temporary(parent.join(format!(".stage-{:x}", u128::from_ne_bytes(random))));
    fs::DirBuilder::new()
        .mode(0o700)
        .create(&temporary.0)
        .map_err(|e| e.to_string())?;
    for (path, body, mode) in [
        (temporary.0.join(name), bytes.as_slice(), 0o755),
        (
            temporary.0.join("manifest.json"),
            expected.as_slice(),
            0o600,
        ),
    ] {
        let mut file = fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .mode(mode)
            .open(path)
            .map_err(|e| e.to_string())?;
        file.write_all(body)
            .and_then(|()| file.sync_all())
            .map_err(|e| e.to_string())?;
    }
    fs::File::open(&temporary.0)
        .and_then(|f| f.sync_all())
        .map_err(|e| e.to_string())?;
    fs::rename(&temporary.0, &dest).map_err(|e| e.to_string())?;
    fs::File::open(&parent)
        .and_then(|f| f.sync_all())
        .map_err(|e| e.to_string())?;
    Ok(dest.join(name))
}
