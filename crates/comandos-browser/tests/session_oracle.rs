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
    let rust_port = free_port();
    let rust_dir = temp_dir("rust");
    let cfg = BrokerConfig::from_value(
        json!({
            "command":[env!("CARGO_BIN_EXE_fake-mcp-worker")],
            "state_dir":&rust_dir,
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
    let py = tokio::task::spawn_blocking(move || {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let bytes = comandos_oracle::oracle_at(
            &root.join("tests/golden"),
            "browser-session",
            &json!({"source_ref":"2674f36",
                "source_sha256":"184ef7da4b7c017d477c85570e33ebcb6e298d4722d67ecc0a41cef2190fa338",
                "catalog":{"protocolVersion":"2025-11-25","tools":[{"name":"navigate_page"}]},
                "worker":include_str!("support/fake_worker.rs"), "sequence":py_sequence}),
            || {
                let py_port = free_port();
                let py_dir = temp_dir("python");
                let config_path = py_dir.join("config.json");
                std::fs::write(&config_path, json!({
                    "command":[env!("CARGO_BIN_EXE_fake-mcp-worker")], "state_dir":py_dir,
                    "catalog":{"protocolVersion":"2025-11-25","tools":[{"name":"navigate_page"}]},
                    "port":py_port
                }).to_string()).map_err(|e| e.to_string())?;
                let python = Command::new(
                    std::env::var("COMANDOS_BROWSER_ORACLE_PYTHON")
                        .unwrap_or_else(|_| "python3".into()),
                )
                .arg(root.join("tests/oracle-src/broker.py"))
                .arg("--config")
                .arg(&config_path)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .map_err(|e| e.to_string())?;
                let python = ChildGuard(python);
                wait_port(py_port);
                let responses = exchange("python", py_port, &py_sequence);
                drop(python);
                std::fs::remove_dir_all(py_dir).map_err(|e| e.to_string())?;
                serde_json::to_vec(&responses).map_err(|e| e.to_string())
            },
        );
        serde_json::from_slice::<Vec<String>>(&bytes).unwrap()
    })
    .await
    .unwrap();
    let rust_sequence = sequence;
    let rust = tokio::task::spawn_blocking(move || exchange("rust", rust_port, &rust_sequence))
        .await
        .unwrap();
    let _ = tx.send(());
    server.await.unwrap().unwrap();
    assert_eq!(rust, py);
    std::fs::remove_dir_all(rust_dir).unwrap();
}
