use comandos_browser::{
    broker::{Broker, write_status},
    config::BrokerConfig,
};
use serde_json::json;
use std::{io::Read, os::unix::fs::PermissionsExt, path::PathBuf, time::Duration};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

fn temp_dir(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "cb-broker-{label}-{}-{}",
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

#[test]
fn write_status_is_atomic_and_0600() {
    let dir = temp_dir("status");
    write_status(&dir, &json!({"connections":0,"sessions":[],"stuck":0})).unwrap();
    let status = dir.join("status.json");
    assert_eq!(
        std::fs::metadata(&status).unwrap().permissions().mode() & 0o777,
        0o600
    );
    let mut text = String::new();
    std::fs::File::open(&status)
        .unwrap()
        .read_to_string(&mut text)
        .unwrap();
    assert_eq!(text, r#"{"connections": 0, "sessions": [], "stuck": 0}"#);
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test(flavor = "current_thread")]
async fn broker_serves_initialize_on_loopback_and_stops_on_shutdown() {
    let dir = temp_dir("serve");
    let port = free_port();
    let cfg = BrokerConfig::from_value(
        json!({
            "command":[env!("CARGO_BIN_EXE_fake-mcp-worker")],
            "state_dir":dir,
            "catalog":{"protocolVersion":"2025-11-25","tools":[{"name":"navigate_page"}]},
            "port":port,
            "stop_grace":0.2
        }),
        &std::env::temp_dir(),
    )
    .unwrap();
    let (tx, rx) = tokio::sync::oneshot::channel::<()>();
    let server = tokio::spawn(Broker::serve(cfg, async {
        let _ = rx.await;
    }));
    tokio::time::sleep(Duration::from_millis(50)).await;
    let stream = tokio::net::TcpStream::connect(("127.0.0.1", port))
        .await
        .unwrap();
    let (read, mut write) = tokio::io::split(stream);
    write
        .write_all(br#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#)
        .await
        .unwrap();
    write.write_all(b"\n").await.unwrap();
    let mut line = String::new();
    BufReader::new(read).read_line(&mut line).await.unwrap();
    assert!(line.contains(r#""serverInfo":{"name":"comandos-browser-macmini""#));
    let _ = tx.send(());
    server.await.unwrap().unwrap();
}
