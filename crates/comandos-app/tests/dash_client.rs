#![allow(clippy::unwrap_used, clippy::expect_used)]
use comandos_app::{
    config::RunMode,
    dash_client::{DashClient, DashError},
};
use std::{
    io::{Read, Write},
    net::TcpListener,
    time::Duration,
};
#[test]
fn disconnected_shadow_and_http_contracts_use_only_private_fixture() {
    let none = DashClient::new(None, RunMode::Sandbox).unwrap();
    assert_eq!(
        none.get("/prefs", Duration::from_millis(50)),
        Err(DashError::Disconnected)
    );
    let listener = TcpListener::bind("127.0.0.1:7311").expect("private fixture7311 must be free");
    listener.set_nonblocking(true).unwrap();
    let shadow = DashClient::new(Some("http://127.0.0.1:7311"), RunMode::Shadow).unwrap();
    assert_eq!(
        shadow.post("/prefs", &serde_json::json!({}), Duration::from_millis(50)),
        Err(DashError::ShadowRefused)
    );
    assert!(listener.accept().is_err(), "Shadow opened a socket");
    listener.set_nonblocking(false).unwrap();
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let mut bytes = Vec::new();
        let mut buf = [0u8; 4096];
        loop {
            let n = stream.read(&mut buf).unwrap();
            assert!(n > 0);
            bytes.extend_from_slice(buf.get(..n).unwrap());
            if let Some(end) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
                let header = String::from_utf8_lossy(bytes.get(..end).unwrap());
                let length: usize = header
                    .lines()
                    .find_map(|l| {
                        l.to_ascii_lowercase()
                            .strip_prefix("content-length:")
                            .map(|v| v.trim().parse().unwrap())
                    })
                    .unwrap();
                if bytes.len() >= end + 4 + length {
                    break;
                }
            }
        }
        let request = String::from_utf8(bytes).unwrap();
        assert!(request.starts_with("POST /prefs HTTP/1."));
        assert!(
            request
                .to_ascii_lowercase()
                .contains("host: 127.0.0.1:7311")
        );
        assert!(
            request.ends_with(r#"{"z": "\u00f1\ud83d\ude80", "a": 1}"#),
            "{request}"
        );
        let body = r#"{"error": "conflict", "revision": 2}"#;
        write!(
            stream,
            "HTTP/1.0 409 Conflict\r\nContent-Length: {}\r\n\r\n{}",
            body.len(),
            body
        )
        .unwrap();
    });
    let client = DashClient::new(Some("http://127.0.0.1:7311"), RunMode::Sandbox).unwrap();
    let mut payload = serde_json::Map::new();
    payload.insert("z".into(), serde_json::json!("ñ🚀"));
    payload.insert("a".into(), serde_json::json!(1));
    let (status, value) = client
        .post("/prefs", &payload.into(), Duration::from_secs(2))
        .unwrap();
    assert_eq!(status, 409);
    assert_eq!(value["error"], "conflict");
    server.join().unwrap();
    assert!(DashClient::new(Some("http://example.org:7311"), RunMode::Sandbox).is_err());
    assert!(DashClient::new(Some("http://127.0.0.1:4777"), RunMode::Sandbox).is_err());
}
