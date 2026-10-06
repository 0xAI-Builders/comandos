//! Build the vendored Rust gateway only when an available source is newer.
use std::{
    fs,
    path::{Path, PathBuf},
    time::SystemTime,
};
fn newer(path: &Path, stamp: SystemTime) -> Result<bool, String> {
    let meta = path
        .symlink_metadata()
        .map_err(|e| format!("{}: {e}", path.display()))?;
    if meta.is_file() {
        return Ok(meta.modified().map_err(|e| e.to_string())? > stamp);
    }
    if !meta.is_dir() {
        return Err(format!(
            "{} is not a regular vendored source",
            path.display()
        ));
    }
    for entry in fs::read_dir(path).map_err(|e| e.to_string())? {
        if newer(&entry.map_err(|e| e.to_string())?.path(), stamp)? {
            return Ok(true);
        }
    }
    Ok(false)
}
pub fn needs_build(source: &Path, binary: &Path) -> Result<bool, String> {
    if !source.join("Cargo.toml").is_file() {
        return Ok(false);
    }
    use std::os::unix::fs::PermissionsExt;
    let stamp = match binary.metadata() {
        Ok(meta) if meta.is_file() && meta.permissions().mode() & 0o111 != 0 => {
            meta.modified().map_err(|e| e.to_string())?
        }
        _ => return Ok(true),
    };
    for relative in ["src", "Cargo.toml", "Cargo.lock"] {
        let path = source.join(relative);
        if path.exists() && newer(&path, stamp)? {
            return Ok(true);
        }
    }
    Ok(false)
}
pub fn prepare(home: &Path, release: &Path, dry: bool) -> Result<Option<PathBuf>, String> {
    let prebuilt = release.join("cc-model-proxy");
    let source = release.join("vendor/claude-codex");
    if !source.join("Cargo.toml").is_file() {
        return Ok(prebuilt.is_file().then_some(prebuilt));
    }
    let target = home.join(".local/share/comandos/build/claude-codex");
    let compiled = target.join("release/claude-codex");
    let candidate = if prebuilt.is_file() {
        &prebuilt
    } else {
        &compiled
    };
    if !needs_build(&source, candidate)? {
        return Ok(Some(candidate.clone()));
    }
    if dry {
        println!(
            "dry-run: build locked Rust gateway {} into {}",
            source.display(),
            target.display()
        );
        return Ok(None);
    }
    super::release::check_app_parents(&target.join("placeholder"))?;
    let cargo = super::full::find("cargo")
        .ok_or("cargo unavailable; supply a native cc-model-proxy release artifact")?;
    super::full::command(
        home,
        &cargo,
        vec![
            "build".into(),
            "--release".into(),
            "--locked".into(),
            "-j2".into(),
            "--manifest-path".into(),
            source.join("Cargo.toml").to_string_lossy().into_owned(),
            "--target-dir".into(),
            target.to_string_lossy().into_owned(),
        ],
        std::time::Duration::from_secs(1200),
    )?;
    Ok(Some(compiled))
}
