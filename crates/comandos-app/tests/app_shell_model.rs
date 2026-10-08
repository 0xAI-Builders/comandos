#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::disallowed_methods
)]
use comandos_app::ui::webview::dashboard_uri;

#[test]
fn disconnected_dashboard_has_no_live_url() {
    assert!(dashboard_uri(None, "fixture-v1").is_none());
    let uri = dashboard_uri(Some("http://127.0.0.1:7311"), "fixture-v1").expect("fixture");
    assert!(uri.contains("app=1") && uri.contains("anwin=1") && uri.contains("fixture-v1"));
}

#[test]
fn web_terminal_uri_encodes_auth_and_requires_configured_native_server() {
    let uri = comandos_app::ui::webview::terminal_uri(
        "http://127.0.0.1:4777/",
        "term-one",
        "tok en&secret",
        "日本",
    )
    .unwrap();
    assert_eq!(
        uri,
        "http://127.0.0.1:4777/term/?web=native&auth=tok%20en%26secret&arg=term-one&theme=%E6%97%A5%E6%9C%AC"
    );
    for base in [
        "file:///private/assets/term.html",
        "http://example.org:4777",
        "http://127.0.0.1:4777/redirect",
        "http://127.0.0.1:0",
    ] {
        assert!(comandos_app::ui::webview::terminal_uri(base, "local", "token", "noche").is_err());
    }
}

#[test]
fn display_lock_contends_and_activation_targets_only_its_mode_class() {
    use std::os::unix::fs::DirBuilderExt;
    // Locking and injected activation need no display connection.
    let root = std::env::temp_dir().join(format!("comandos-instance-test-{}", std::process::id()));
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(root.join("home"))
        .unwrap();
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(root.join("run"))
        .unwrap();
    let lookup = |key: &str| match key {
        "HOME" => Some(root.join("home").display().to_string()),
        "XDG_RUNTIME_DIR" => Some(root.join("run").display().to_string()),
        _ => None,
    };
    let cfg = comandos_app::config::parse_args(
        &[
            "--mode".into(),
            "sandbox".into(),
            "--tmux-socket".into(),
            "test-instance".into(),
        ],
        false,
        &lookup,
    )
    .unwrap();
    let guard = comandos_app::guard::WriteGuard::from_config(&cfg, ":fixture");
    let lock = comandos_app::ui::window::instance_lock(&cfg, &guard, ":fixture").unwrap();
    assert!(matches!(
        comandos_app::ui::window::instance_lock(&cfg, &guard, ":fixture"),
        Err(comandos_app::ui::window::InstanceError::Contended)
    ));
    assert!(comandos_app::ui::window::instance_lock(&cfg, &guard, ":other").is_ok());
    let called = std::cell::Cell::new(false);
    assert!(comandos_app::ui::window::activate_existing(&cfg, &|spec| {
        called.set(true);
        assert_eq!(spec.program, "wmctrl");
        assert_eq!(
            spec.args,
            vec![
                std::ffi::OsString::from("-x"),
                "-a".into(),
                "sandbox-app-rs.sandbox-app-rs".into()
            ]
        );
        Ok(comandos_app::proc::ProcOutput {
            code: Some(0),
            ..Default::default()
        })
    }));
    assert!(called.get());
    drop(lock);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn native_terminal_admission_rejects_isolated_modes_before_network_io() {
    for mode in [
        comandos_app::config::RunMode::Sandbox,
        comandos_app::config::RunMode::Shadow,
    ] {
        let error =
            comandos_app::ui::webview::native_terminal_backend(mode, "invalid").unwrap_err();
        assert!(error.contains("isolated mode"));
    }
    assert!(
        comandos_app::ui::webview::terminal_uri("http://127.0.0.1:4777", "local", "", "noche")
            .unwrap_err()
            .contains("access token unavailable")
    );
}
#[test]
fn native_terminal_admission_uses_only_configured_private_native_endpoint() {
    use std::io::{Read, Write};
    for (status, header, body, accepted) in [
        ("200 OK", "native", r#"{"token":""}"#, true),
        ("200 OK", "ttyd", r#"{"token":""}"#, false),
        ("200 OK", "", r#"{"token":""}"#, false),
        ("200 OK", "native", r#"{"token":"foreign"}"#, false),
        ("200 OK", "native", "broken", false),
        ("404 Not Found", "native", r#"{"error":"disabled"}"#, false),
        ("302 Found", "native", r#"{"token":""}"#, false),
    ] {
        let listener = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            socket
                .set_read_timeout(Some(std::time::Duration::from_secs(3)))
                .unwrap();
            let mut wire = Vec::new();
            let mut bytes = [0u8; 1024];
            while !wire.windows(4).any(|p| p == b"\r\n\r\n") {
                let n = socket.read(&mut bytes).unwrap();
                assert!(n > 0);
                wire.extend_from_slice(&bytes[..n]);
            }
            let wire = String::from_utf8(wire).unwrap();
            assert!(wire.starts_with("GET /term/token HTTP/1.1\r\n"), "{wire}");
            write!(socket,"HTTP/1.1 {status}\r\nContent-Length: {}\r\nX-Comandos-Term: {header}\r\nLocation: http://127.0.0.1:1/never\r\nConnection: close\r\n\r\n{body}",body.len()).unwrap();
        });
        let result = comandos_app::ui::webview::native_terminal_backend(
            comandos_app::config::RunMode::Live,
            &format!("http://{address}"),
        );
        assert_eq!(
            result.is_ok(),
            accepted,
            "status={status} header={header} result={result:?}"
        );
        server.join().unwrap();
    }
}
