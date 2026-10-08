use comandos_browser::{
    broker::{Broker, write_status},
    config::BrokerConfig,
};
use serde_json::json;
use std::{io::Read, os::unix::fs::PermissionsExt, path::PathBuf, time::Duration};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWriteExt, BufReader};

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

async fn assert_socket_closed<R: AsyncRead + Unpin>(reader: &mut R) {
    let mut byte = [0_u8; 1];
    match tokio::time::timeout(Duration::from_millis(200), reader.read(&mut byte)).await {
        Ok(Ok(0)) => {}
        Ok(Err(error)) if error.kind() == std::io::ErrorKind::ConnectionReset => {}
        other => panic!("socket remained readable/open: {other:?}"),
    }
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

#[tokio::test(flavor = "current_thread")]
async fn broker_shutdown_closes_idle_and_inflight_client_sockets() {
    let dir = temp_dir("shutdown-clients");
    let port = free_port();
    let cfg = BrokerConfig::from_value(
        json!({
            "command":[env!("CARGO_BIN_EXE_fake-mcp-worker")],
            "state_dir":dir,
            "catalog":{"protocolVersion":"2025-11-25","tools":[{"name":"navigate_page"}]},
            "port":port,
            "tool_timeout":5.0,
            "stop_grace":0.1
        }),
        &std::env::temp_dir(),
    )
    .unwrap();
    let (tx, rx) = tokio::sync::oneshot::channel::<()>();
    let server = tokio::spawn(Broker::serve(cfg, async {
        let _ = rx.await;
    }));
    tokio::time::sleep(Duration::from_millis(50)).await;

    let idle = tokio::net::TcpStream::connect(("127.0.0.1", port))
        .await
        .unwrap();
    let (idle_read_half, mut idle_write) = tokio::io::split(idle);
    let mut idle_reader = BufReader::new(idle_read_half);
    idle_write
        .write_all(br#"{"jsonrpc":"2.0","id":9,"method":"initialize","params":{}}"#)
        .await
        .unwrap();
    idle_write.write_all(b"\n").await.unwrap();
    let mut idle_line = String::new();
    idle_reader.read_line(&mut idle_line).await.unwrap();
    assert!(idle_line.contains(r#""id":9"#));
    let inflight = tokio::net::TcpStream::connect(("127.0.0.1", port))
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;
    let (inflight_read, mut inflight_write) = tokio::io::split(inflight);
    let mut inflight_reader = BufReader::new(inflight_read);
    inflight_write
        .write_all(br#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#)
        .await
        .unwrap();
    inflight_write.write_all(b"\n").await.unwrap();
    let mut line = String::new();
    inflight_reader.read_line(&mut line).await.unwrap();
    assert!(line.contains(r#""id":1"#));
    inflight_write
        .write_all(br#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"navigate_page","arguments":{"sleep_ms":5000}}}"#)
        .await
        .unwrap();
    inflight_write.write_all(b"\n").await.unwrap();

    let _ = tx.send(());
    tokio::time::timeout(Duration::from_secs(2), server)
        .await
        .unwrap()
        .unwrap()
        .unwrap();

    assert_socket_closed(&mut idle_reader).await;
    assert_socket_closed(&mut inflight_reader).await;
}
