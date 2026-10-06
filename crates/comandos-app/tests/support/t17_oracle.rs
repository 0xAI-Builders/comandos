#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]
use comandos_app::proc::{ProcSpec, run};
use serde_json::Value;
pub fn original(input: Value) -> Value {
    let root = comandos_oracle::golden_root(std::path::Path::new(env!("CARGO_MANIFEST_DIR")));
    let bytes = comandos_oracle::oracle_at(&root, "gtk-t17", &input, || {
        let mut input = input.clone();
        input["source"] =
            Value::String(std::env::var("COMANDOS_CC_APP_ORACLE").unwrap_or_else(|_| {
                concat!(env!("CARGO_MANIFEST_DIR"), "/../../bin/cc-app").into()
            }));
        let out = run(&ProcSpec {
            program: "python3".into(),
            args: vec!["-c".into(), include_str!("t17_original.py").into()],
            stdin: Some(serde_json::to_vec(&input).unwrap()),
            env: vec![],
            clear_env: false,
            env_remove: vec![],
            cwd: None,
            timeout: std::time::Duration::from_secs(5),
        })
        .map_err(|e| format!("T17 reference: {e:?}"))?;
        if out.code != Some(0) || out.timed_out {
            return Err(format!(
                "T17 reference failed: {:?}: {}",
                out.code,
                String::from_utf8_lossy(&out.stderr)
            ));
        }
        Ok(out.stdout)
    });
    serde_json::from_slice(&bytes).unwrap()
}
