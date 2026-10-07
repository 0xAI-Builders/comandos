//! Explicit record/check reference. Replay never resolves Git or executes Python.
#![allow(dead_code)]
use std::{
    path::{Path, PathBuf},
    process::Command,
};
pub const SOURCE_COMMIT: &str = "2674f366bb01b9db42f6f64b728929b3acfe8b83";
pub fn reference(home: &Path) -> Result<PathBuf, String> {
    let target = home.join(".oracle/reference");
    std::fs::create_dir_all(&target).map_err(|e| e.to_string())?;
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let archive = Command::new("git")
        .arg("-C")
        .arg(&repo)
        .args([
            "archive",
            "--format=tar",
            SOURCE_COMMIT,
            "bin/cc-dash",
            "bin/cc_usage.py",
            "lib",
            "config",
            "dash/index.html",
            "dash/term.html",
            "tests/fixtures/analytics",
        ])
        .output()
        .map_err(|e| e.to_string())?;
    if !archive.status.success() {
        return Err(String::from_utf8_lossy(&archive.stderr).into_owned());
    }
    let archive_path = home.join(".oracle/reference.tar");
    std::fs::write(&archive_path, archive.stdout).map_err(|e| e.to_string())?;
    let extracted = Command::new("tar")
        .arg("-xf")
        .arg(&archive_path)
        .arg("-C")
        .arg(&target)
        .output()
        .map_err(|e| e.to_string())?;
    if !extracted.status.success() {
        return Err(String::from_utf8_lossy(&extracted.stderr).into_owned());
    }
    Ok(target)
}
