#![cfg(not(target_os = "macos"))]
#![allow(clippy::unwrap_used)]
#[test]
fn linux_binary_warns_and_exits_without_initializing_a_gui() {
    let out = comandos_desktop::proc::run(&comandos_desktop::proc::ProcSpec {
        program: env!("CARGO_BIN_EXE_comandos-app-mac").into(),
        args: vec![],
        stdin: None,
        env: vec![],
        clear_env: false,
        env_remove: vec![],
        cwd: Some(std::env::temp_dir()),
        timeout: std::time::Duration::from_secs(2),
    })
    .unwrap();
    assert_eq!(out.code, Some(2));
    assert_eq!(out.stderr, b"comandos-app-mac requiere macOS\n");
    assert!(out.stdout.is_empty());
}
