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

/// Execute the immutable server module only inside an explicit oracle closure.
pub fn run_dash_original(
    home: &super::TestHome,
    code: &str,
    opts: &super::oracle::OracleOpts,
) -> Result<String, String> {
    assert!(
        matches!(
            std::env::var("COMANDOS_ORACLE").as_deref(),
            Ok("record" | "check")
        ),
        "original source execution requires record/check"
    );
    let reference = reference(&home.root)?;
    let mut extra_env = opts.extra_env.clone();
    assert!(
        !extra_env
            .iter()
            .any(|(key, _)| key == "COMANDOS_ORACLE_REFERENCE_ROOT")
    );
    extra_env.push((
        "COMANDOS_ORACLE_REFERENCE_ROOT".into(),
        reference.display().to_string(),
    ));
    let options = super::oracle::OracleOpts {
        fakebin_extra: opts.fakebin_extra.clone(),
        python_prelude: opts.python_prelude.clone(),
        allow_ports: opts.allow_ports.clone(),
        keep_loops: opts.keep_loops.clone(),
        extra_env,
    };
    super::oracle::run_dash_with(home, code, &options)
        .ok_or_else(|| "explicit record/check requires Python".into())
}

/// Preserve standalone fixture setup while importing the immutable source root.
/// Invoke only from a golden record/check closure; replay never probes Python.
pub fn run_python_original(
    script: &str,
    args: &[&std::ffi::OsStr],
    home: &Path,
) -> Result<String, String> {
    assert!(
        matches!(
            std::env::var("COMANDOS_ORACLE").as_deref(),
            Ok("record" | "check")
        ),
        "original source execution requires record/check"
    );
    let reference = reference(home)?;
    let interpreter = std::env::var("COMANDOS_SERVER_ORACLE_PYTHON")
        .unwrap_or_else(|_| "/usr/bin/python3".into());
    super::oracle::run_python_at(script, args, home, &reference, &interpreter)
        .ok_or_else(|| "explicit record/check requires CPython 3.10.12".into())
}
