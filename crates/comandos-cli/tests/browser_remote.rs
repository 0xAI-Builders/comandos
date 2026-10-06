use comandos_cli::browser::remote::{RemoteTarget, load_target, remote_argv};
use std::path::PathBuf;

fn tempdir() -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "cli-remote-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn default_target_matches_shell_script() {
    let home = tempdir();
    let t = load_target(&home).ok();
    assert_eq!(
        t.as_ref().map(|t| (t.host.as_str(), t.port)),
        Some(("macmini", 19441))
    );
    let argv = remote_argv(&RemoteTarget {
        host: "macmini".into(),
        port: 19441,
    });
    assert_eq!(
        argv.join(" "),
        "ssh -T -o BatchMode=yes -o ExitOnForwardFailure=yes -o ConnectTimeout=8 -W 127.0.0.1:19441 macmini"
    );
}

#[test]
fn config_file_selects_rust_broker_port() {
    let home = tempdir();
    std::fs::create_dir_all(home.join(".config/comandos")).ok();
    std::fs::write(
        home.join(".config/comandos/browser.json"),
        r#"{"host":"macmini","port":19442}"#,
    )
    .ok();
    assert_eq!(load_target(&home).map(|t| t.port).ok(), Some(19442));
}

#[test]
fn host_starting_with_dash_is_rejected() {
    let home = tempdir();
    std::fs::create_dir_all(home.join(".config/comandos")).ok();
    std::fs::write(
        home.join(".config/comandos/browser.json"),
        r#"{"host":"-oProxyCommand=x","port":1}"#,
    )
    .ok();
    assert!(load_target(&home).is_err());
}
