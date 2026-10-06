#![allow(dead_code)]
#[path = "../../../comandos-app/src/proc.rs"]
mod proc;
use serde_json::Value;
pub fn original(mut input: Value) -> Value {
    input["source"] = std::env::var("COMANDOS_MAC_APP_ORACLE")
        .unwrap_or_else(|_| concat!(env!("CARGO_MANIFEST_DIR"), "/../../bin/cc-app-mac").into())
        .into();
    let out = proc::run(&proc::ProcSpec {
        program: "python3".into(),
        args: vec!["-c".into(), include_str!("mac_original.py").into()],
        stdin: Some(serde_json::to_vec(&input).unwrap()),
        env: vec![],
        clear_env: false,
        env_remove: vec![],
        cwd: Some(std::env::temp_dir()),
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
