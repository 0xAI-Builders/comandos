#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]
use comandos_app::proc::{ProcSpec, run};
use serde_json::Value;
pub fn original(mut input: Value) -> Value {
    input["source"] = Value::String(
        std::env::var("COMANDOS_CC_APP_ORACLE")
            .unwrap_or_else(|_| concat!(env!("CARGO_MANIFEST_DIR"), "/../../bin/cc-app").into()),
    );
    let out = run(&ProcSpec {
        program: "python3".into(),
        args: vec!["-c".into(), include_str!("t18_original.py").into()],
        stdin: Some(serde_json::to_vec(&input).unwrap()),
        env: vec![],
        clear_env: false,
        env_remove: vec![],
        cwd: None,
        timeout: std::time::Duration::from_secs(5),
    })
    .unwrap();
    assert_eq!(
        out.code,
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).unwrap()
}
