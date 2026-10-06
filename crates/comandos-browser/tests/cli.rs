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

#[test]
fn sigterm_closes_a_confirmed_running_fake_worker_and_requested_socket() {
    use std::{
        io::Write,
        net::{TcpListener, TcpStream},
        time::{Duration, Instant},
    };
    let dir = temp_dir("sigterm");
    let marker = dir.join("call.started");
    let pid_file = dir.join("worker.pid");
    let available = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = available.local_addr().unwrap().port();
    drop(available);
    let config = dir.join("config.json");
    std::fs::write(&config,serde_json::to_vec(&serde_json::json!({"command":[env!("CARGO_BIN_EXE_fake-mcp-worker")],"env":{"FAKE_ROOT_PID_FILE":pid_file,"FAKE_CALL_MARKER":marker},"state_dir":dir.join("state"),"port":1,"catalog":{"tools":[{"name":"navigate_page"}]},"stop_grace":0.1})).unwrap()).unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_comandos-broker-mac"))
        .args(["serve", "--config"])
        .arg(config)
        .args(["--port", &port.to_string()])
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut line = String::new();
    BufReader::new(child.stdout.take().unwrap())
        .read_line(&mut line)
        .unwrap();
    assert_eq!(line, "ComandOS browser broker ready on loopback\n");
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    stream
        .write_all(b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{}}\n")
        .unwrap();
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    let mut response = String::new();
    reader.read_line(&mut response).unwrap();
    stream.write_all(b"{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"tools/call\",\"params\":{\"name\":\"navigate_page\",\"arguments\":{\"sleep_ms\":5000}}}\n").unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    while !marker.exists() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    let confirmed = marker.exists();
    nix::sys::signal::kill(
        nix::unistd::Pid::from_raw(child.id() as i32),
        nix::sys::signal::Signal::SIGTERM,
    )
    .unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break Some(status);
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            break None;
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    let pid = std::fs::read_to_string(pid_file)
        .unwrap()
        .parse::<i32>()
        .unwrap();
    let live = std::fs::read_to_string(format!("/proc/{pid}/stat"))
        .ok()
        .is_some_and(|stat| {
            stat[stat.rfind(')').unwrap() + 2..]
                .split_whitespace()
                .next()
                != Some("Z")
        });
    if live {
        let _ = nix::sys::signal::kill(
            nix::unistd::Pid::from_raw(pid),
            nix::sys::signal::Signal::SIGKILL,
        );
    }
    assert!(confirmed);
    assert!(status.is_some_and(|s| s.success()));
    assert!(!live);
    assert!(std::fs::read_dir(dir.join("state")).unwrap().all(|entry| {
        !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with("session-")
    }));
    assert!(TcpStream::connect(("127.0.0.1", port)).is_err());
}

#[test]
fn occupied_port_never_prints_ready() {
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_comandos-broker-mac"))
        .args(["serve", "--config"])
        .arg(write_fake_config())
        .args(["--port", &listener.local_addr().unwrap().port().to_string()])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(out.stdout.is_empty());
}
