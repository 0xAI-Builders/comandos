use comandos_browser::{broker::Broker, config::BrokerConfig};
use serde_json::json;
use std::{
    io::{BufRead, BufReader, Write},
    net::TcpStream,
    path::PathBuf,
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

fn temp_dir(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "cb-oracle-{label}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn free_port() -> u16 {
    std::net::TcpListener::bind(("127.0.0.1", 0))
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn wait_port(port: u16) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if TcpStream::connect(("127.0.0.1", port)).is_ok() {
            return;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    panic!("port {port} did not open");
}

fn exchange(label: &str, port: u16, lines: &[&str]) -> Vec<String> {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    stream
        .set_write_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    let mut out = Vec::new();
    for line in lines {
        stream.write_all(line.as_bytes()).unwrap();
        stream.write_all(b"\n").unwrap();
        stream.flush().unwrap();
        if line.contains(r#""id""#) || !line.starts_with(r#"{"#) {
            let mut response = String::new();
            reader.read_line(&mut response).unwrap_or_else(|error| {
                panic!("{label}: timeout/failed reading from port {port} after {line}: {error}")
            });
            assert!(
                !response.is_empty(),
                "{label}: EOF reading from port {port} after {line}"
            );
            out.push(response.trim_end().to_owned());
        }
    }
    out
}

struct ChildGuard(Child);

impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[tokio::test(flavor = "current_thread")]
async fn rust_session_matches_python_for_basic_dispatch_sequence() {
    if Command::new("python3")
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_err()
    {
        eprintln!("SKIP: python3 no disponible para session_oracle");
        return;
    }
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .unwrap()
        .to_path_buf();
    let py_port = free_port();
    let rust_port = free_port();
    let py_dir = temp_dir("python");
    let rust_dir = temp_dir("rust");
    let config = json!({
        "command":[env!("CARGO_BIN_EXE_fake-mcp-worker")],
        "state_dir":py_dir,
        "catalog":{"protocolVersion":"2025-11-25","tools":[{"name":"navigate_page"}]},
        "port":py_port
    });
    let config_path = py_dir.join("config.json");
    std::fs::write(&config_path, config.to_string()).unwrap();
    let python = Command::new("python3")
        .arg(root.join("services/browser/broker.py"))
        .arg("--config")
        .arg(&config_path)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let _python = ChildGuard(python);
    wait_port(py_port);

    let cfg = BrokerConfig::from_value(
        json!({
            "command":[env!("CARGO_BIN_EXE_fake-mcp-worker")],
            "state_dir":rust_dir,
            "catalog":{"protocolVersion":"2025-11-25","tools":[{"name":"navigate_page"}]},
            "port":rust_port
        }),
        &std::env::temp_dir(),
    )
    .unwrap();
    let (tx, rx) = tokio::sync::oneshot::channel::<()>();
    let server = tokio::spawn(Broker::serve(cfg, async {
        let _ = rx.await;
    }));
    tokio::time::sleep(Duration::from_millis(50)).await;

    let sequence = [
        "not json",
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"clientInfo":{"name":"oracle"}}}"#,
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#,
        r#"{"jsonrpc":"2.0","id":3,"method":"ping"}"#,
        r#"{"jsonrpc":"2.0","method":"notifications/cancelled","params":{}}"#,
        r#"{"jsonrpc":"2.0","id":4,"method":"unknown"}"#,
    ];
    let py_sequence = sequence;
    let py = tokio::task::spawn_blocking(move || exchange("python", py_port, &py_sequence))
        .await
        .unwrap();
    let rust_sequence = sequence;
    let rust = tokio::task::spawn_blocking(move || exchange("rust", rust_port, &rust_sequence))
        .await
        .unwrap();
    let _ = tx.send(());
    server.await.unwrap().unwrap();
    assert_eq!(rust, py);
}
