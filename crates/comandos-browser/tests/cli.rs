use std::{
    io::{BufRead, BufReader},
    path::PathBuf,
    process::{Command, Stdio},
};

fn temp_dir(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "cb-cli-{label}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn write_fake_config() -> PathBuf {
    let dir = temp_dir("config");
    let config = dir.join("config.json");
    std::fs::write(
        &config,
        format!(
            r#"{{
              "command":["{}"],
              "state_dir":"{}",
              "catalog":{{"protocolVersion":"2025-11-25","tools":[{{"name":"navigate_page"}}]}},
              "port":1,
              "stop_grace":0.1
            }}"#,
            env!("CARGO_BIN_EXE_fake-mcp-worker"),
            dir.join("state").display()
        ),
    )
    .unwrap();
    config
}

fn spawn_and_read_first_line(args: &[&str]) -> String {
    let mut child = Command::new(env!("CARGO_BIN_EXE_comandos-broker-mac"))
        .args(args)
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut reader = BufReader::new(child.stdout.take().unwrap());
    let mut line = String::new();
    reader.read_line(&mut line).unwrap();
    let _ = child.kill();
    let _ = child.wait();
    line.trim_end().to_owned()
}

#[test]
fn apparmor_print_is_byte_identical_to_repo_profile() {
    let out = Command::new(env!("CARGO_BIN_EXE_comandos-broker-mac"))
        .args(["apparmor", "print"])
        .output();
    let repo = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../services/browser/apparmor/comandos-browser-chrome"
    ));
    assert_eq!(out.map(|o| o.stdout).ok(), repo.ok());
}

#[test]
fn port_flag_overrides_config_and_ready_line_matches_python() {
    let config = write_fake_config();
    let line = spawn_and_read_first_line(&[
        "serve",
        "--config",
        config.to_str().unwrap(),
        "--port",
        "7341",
    ]);
    assert_eq!(line, "ComandOS browser broker ready on loopback");
}

#[test]
fn status_fails_when_stale() {
    let dir = temp_dir("stale-status");
    let status = dir.join("status.json");
    std::fs::write(&status, r#"{"workers":0}"#).unwrap();
    let out = Command::new("touch")
        .args(["-d", "20 seconds ago", status.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(out.status.success());
    let out = Command::new(env!("CARGO_BIN_EXE_comandos-broker-mac"))
        .args(["status", "--state-dir", dir.to_str().unwrap()])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
}
