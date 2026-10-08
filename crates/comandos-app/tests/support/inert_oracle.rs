//! Frozen, inert AST projections. Reference source and Python are record/check only.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::disallowed_methods)]
use comandos_app::proc::{ProcSpec, run};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    os::unix::fs::DirBuilderExt,
    path::{Path, PathBuf},
    process::Command,
};

pub fn oracle(name: &str, script: &str, source: &str, cases: &Value, mode: Option<&str>) -> Value {
    let provenance: Value =
        serde_json::from_str(include_str!("../golden/app-inert-provenance.json")).unwrap();
    assert!(
        provenance["files"].get(source).is_some(),
        "unfrozen original source"
    );
    let mut nonce = [0u8; 12];
    getrandom::fill(&mut nonce).unwrap();
    let root = std::env::temp_dir().join(format!(
        "comandos-inert-oracle-{}",
        nonce.iter().map(|b| format!("{b:02x}")).collect::<String>()
    ));
    let home = root.join("home");
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(&home)
        .unwrap();
    let golden_root = std::env::var_os("COMANDOS_APP_INERT_GOLDEN_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| comandos_oracle::golden_root(Path::new(env!("CARGO_MANIFEST_DIR"))));
    let input = json!({"reference":provenance,"source":source,"script_sha256":format!("{:x}",Sha256::digest(script.as_bytes())),"cases":cases,"mode":mode});
    let result = comandos_oracle::text_with_tree_at(
        &golden_root,
        name,
        &input,
        &home,
        &[("$INERT_HOME", &home), ("$INERT_ROOT", &root)],
        || {
            let reference = root.join("reference");
            let commit = provenance["source_commit"]
                .as_str()
                .ok_or("original commit missing")?;
            for (file, checksum) in provenance["files"]
                .as_object()
                .ok_or("original files missing")?
            {
                let destination = reference.join(file);
                std::fs::create_dir_all(destination.parent().ok_or("original parent missing")?)
                    .map_err(|e| e.to_string())?;
                let revision = format!("{commit}:{file}");
                let out = Command::new("git")
                    .args(["show", &revision])
                    .current_dir(env!("CARGO_MANIFEST_DIR"))
                    .output()
                    .map_err(|e| e.to_string())?;
                if !out.status.success() {
                    return Err(format!("original unavailable: {revision}"));
                }
                if format!("{:x}", Sha256::digest(&out.stdout))
                    != checksum.as_str().ok_or("original checksum missing")?
                {
                    return Err(format!("original checksum differs: {revision}"));
                }
                std::fs::write(destination, out.stdout).map_err(|e| e.to_string())?;
            }
            let mut args = vec![
                "-c".into(),
                script.into(),
                reference.join(source).into_os_string(),
            ];
            if let Some(mode) = mode {
                args.push(mode.into());
            }
            let out = run(&ProcSpec {
                program: std::env::var("COMANDOS_APP_ORACLE_PYTHON")
                    .unwrap_or_else(|_| "/usr/bin/python3".into()),
                args,
                stdin: Some(serde_json::to_vec(cases).map_err(|e| e.to_string())?),
                env: vec![
                    ("HOME".into(), home.clone().into_os_string()),
                    ("PATH".into(), "/usr/bin:/bin".into()),
                ],
                clear_env: true,
                env_remove: vec![],
                cwd: Some(home.clone()),
                timeout: std::time::Duration::from_secs(30),
            })
            .map_err(|e| format!("inert reference: {e:?}"))?;
            if out.code != Some(0) || out.timed_out {
                return Err(format!(
                    "original failed: {}",
                    String::from_utf8_lossy(&out.stderr)
                ));
            }
            String::from_utf8(out.stdout).map_err(|e| e.to_string())
        },
    );
    std::fs::remove_dir_all(root).unwrap();
    serde_json::from_str(&result.unwrap()).unwrap()
}
