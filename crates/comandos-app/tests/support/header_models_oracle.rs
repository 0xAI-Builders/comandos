//! Pinned original header CSS, raster, tween and wizard projection.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::disallowed_methods)]
use comandos_app::proc::{ProcSpec, run};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    os::unix::fs::DirBuilderExt,
    path::{Path, PathBuf},
    process::Command,
};

pub fn oracle(script: &str) -> Value {
    let provenance: Value =
        serde_json::from_str(include_str!("../golden/app-header-models-provenance.json")).unwrap();
    let mut nonce = [0u8; 12];
    getrandom::fill(&mut nonce).unwrap();
    let root = std::env::temp_dir().join(format!(
        "comandos-header-oracle-{}",
        nonce.iter().map(|b| format!("{b:02x}")).collect::<String>()
    ));
    let home = root.join("home");
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(&home)
        .unwrap();
    let golden_root = std::env::var_os("COMANDOS_APP_HEADER_GOLDEN_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| comandos_oracle::golden_root(Path::new(env!("CARGO_MANIFEST_DIR"))));
    let input = json!({"source_commit":provenance["source_commit"],"source_git_path":provenance["source_git_path"],"source_sha256":provenance["source_sha256"],"icons":provenance["icons"],"script_sha256":format!("{:x}",Sha256::digest(script.as_bytes()))});
    let result = comandos_oracle::text_with_tree_at(
        &golden_root,
        "app-header-models",
        &input,
        &home,
        &[("$HEADER_HOME", &home), ("$HEADER_ROOT", &root)],
        || {
            let reference = root.join("reference");
            std::fs::create_dir_all(reference.join("bin")).map_err(|e| e.to_string())?;
            std::fs::create_dir_all(reference.join("dash/icons")).map_err(|e| e.to_string())?;
            let commit = provenance["source_commit"]
                .as_str()
                .ok_or("header source commit missing")?;
            let source_path = provenance["source_git_path"]
                .as_str()
                .ok_or("header source path missing")?;
            frozen(
                &format!("{commit}:{source_path}"),
                provenance["source_sha256"]
                    .as_str()
                    .ok_or("header source SHA missing")?,
                &reference.join(source_path),
            )?;
            for icon in provenance["icons"]
                .as_array()
                .ok_or("header icons missing")?
            {
                let fields = icon.as_array().ok_or("header icon malformed")?;
                let name = fields
                    .first()
                    .and_then(Value::as_str)
                    .ok_or("header icon name missing")?;
                let hash = fields
                    .get(1)
                    .and_then(Value::as_str)
                    .ok_or("header icon SHA missing")?;
                frozen(
                    &format!("{commit}:dash/icons/{name}.svg"),
                    hash,
                    &reference.join(format!("dash/icons/{name}.svg")),
                )?;
            }
            let out = run(&ProcSpec {
                program: std::env::var("COMANDOS_APP_ORACLE_PYTHON")
                    .unwrap_or_else(|_| "/usr/bin/python3".into()),
                args: vec![
                    "-c".into(),
                    script.into(),
                    reference.join(source_path).into_os_string(),
                ],
                stdin: None,
                env: vec![("HOME".into(), home.clone().into_os_string())],
                clear_env: false,
                env_remove: vec![
                    "DISPLAY".into(),
                    "WAYLAND_DISPLAY".into(),
                    "DBUS_SESSION_BUS_ADDRESS".into(),
                ],
                cwd: None,
                timeout: std::time::Duration::from_secs(15),
            })
            .map_err(|e| format!("header reference: {e:?}"))?;
            if out.code != Some(0) || out.timed_out {
                return Err(format!(
                    "header original failed: {}",
                    String::from_utf8_lossy(&out.stderr)
                ));
            }
            String::from_utf8(out.stdout).map_err(|e| e.to_string())
        },
    );
    std::fs::remove_dir_all(root).unwrap();
    serde_json::from_str(&result.unwrap()).unwrap()
}
fn frozen(reference: &str, expected: &str, destination: &Path) -> Result<(), String> {
    let out = Command::new("git")
        .args(["show", reference])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(format!("header original unavailable: {reference}"));
    }
    if format!("{:x}", Sha256::digest(&out.stdout)) != expected {
        return Err(format!("header original checksum differs: {reference}"));
    }
    std::fs::write(destination, out.stdout).map_err(|e| e.to_string())
}
