//! Terminal admission and lifecycle against disposable HOME and tmux only.
#[path = "support/private_tmux.rs"]
mod private_tmux;
use comandos_server::dash::{self, native::NativeOptions, term::TermMode};
use futures_util::{SinkExt, StreamExt};
use private_tmux::{PrivateTmux, TestHome};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::watch,
    time::{Duration, timeout},
};
use tokio_tungstenite::tungstenite::{
    Error, Message, client::IntoClientRequest, protocol::frame::coding::CloseCode,
};
struct Front {
    address: std::net::SocketAddr,
    stop: watch::Sender<bool>,
    task: tokio::task::JoinHandle<std::io::Result<()>>,
}
impl Front {
    async fn start(home: &TestHome, mode: TermMode, shadow: bool) -> Self {
        let own = home.root.join("home");
        std::fs::create_dir_all(own.join(".claude/hooks/dash")).unwrap();
        std::fs::write(dash::token_path(&own), b"  t0k\n").unwrap();
        std::fs::write(own.join(".claude/hooks/webterm-enabled"), b"").unwrap();
        std::fs::write(
            own.join(".claude/hooks/dash/term.html"),
            b"fixture-terminal-exact",
        )
        .unwrap();
        std::fs::create_dir_all(home.root.join("replay")).unwrap();
        std::fs::write(home.root.join("replay/default.bin"), b"shadow-output").unwrap();
        let mut cfg = dash::parse_args(&[], &own, None).unwrap();
        cfg.native = false;
        cfg.token = b"t0k".to_vec();
        cfg.term = mode;
        cfg.shadow_readonly = shadow;
        cfg.term_replay_dir = Some(home.root.join("replay"));
        let mut opts = NativeOptions::for_home(&own, cfg.state_db.clone());
        opts.term_target =
            comandos_server::dash::term::attach::TmuxTarget::Private(home.root.clone());
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (stop, rx) = watch::channel(false);
        let task = tokio::spawn(dash::serve_with(listener, cfg, Some(opts), rx));
        Self {
            address,
            stop,
            task,
        }
    }
    fn request(&self, token: &str) -> http::Request<()> {
        let mut r = format!("ws://{}/term/ws?arg={token}&arg=t1", self.address)
            .into_client_request()
            .unwrap();
        r.headers_mut()
            .insert("Sec-WebSocket-Protocol", "tty".parse().unwrap());
        r.headers_mut()
            .insert("X-Forwarded-For", "203.0.113.2".parse().unwrap());
        r.headers_mut().insert(
            "Origin",
            format!("http://{}", self.address).parse().unwrap(),
        );
        r
    }
    async fn http(&self, method: &str, path: &str) -> String {
        let mut s = TcpStream::connect(self.address).await.unwrap();
        let body = if method == "POST" { "{}" } else { "" };
        s.write_all(format!("{method} {path} HTTP/1.1\r\nHost: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",self.address,body.len()).as_bytes()).await.unwrap();
        let mut b = Vec::new();
        timeout(Duration::from_secs(3), s.read_to_end(&mut b))
            .await
            .unwrap()
            .unwrap();
        String::from_utf8(b).unwrap()
    }
    async fn stop(self) {
        self.stop.send(true).unwrap();
        timeout(Duration::from_secs(3), self.task)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
    }
}
#[tokio::test]
async fn http_exact_legacy_page_and_empty_token_opt_in() {
    let home = TestHome::new();
    let f = Front::start(&home, TermMode::Ttyd, false).await;
    let token = f.http("GET", "/term/token").await;
    assert!(token.starts_with("HTTP/1.1 200"));
    assert!(token.contains("x-comandos-term: ttyd"));
    assert!(token.ends_with(r#"{"token":""}"#));
    let page = f.http("GET", "/term/?auth=t0k&arg=t1").await;
    assert!(page.ends_with("fixture-terminal-exact"));
    assert!(
        f.http("GET", "/term/unknown")
            .await
            .starts_with("HTTP/1.1 404")
    );
    f.stop().await;
}
#[tokio::test]
async fn bad_token_denied_before_init_and_private_pty() {
    let Some(tmux) = PrivateTmux::start(&["t1"]) else {
        return;
    };
    let f = Front::start(&tmux.home, TermMode::Ttyd, false).await;
    let (mut ws, _) = tokio_tungstenite::connect_async(f.request("bad"))
        .await
        .unwrap();
    assert_eq!(
        ws.next().await.unwrap().unwrap().into_data(),
        b"0Acceso denegado al terminal de ComandOS.\r\n"[..]
    );
    match ws.next().await.unwrap().unwrap() {
        Message::Close(Some(c)) => assert_eq!(c.code, CloseCode::Policy),
        x => panic!("{x:?}"),
    };
    assert_eq!(tmux.clients(), 0);
    f.stop().await;
}
#[tokio::test]
async fn valid_token_never_bypasses_host_or_origin() {
    let home = TestHome::new();
    let f = Front::start(&home, TermMode::Ttyd, false).await;
    for (header, value) in [
        ("Origin", "https://foreign.example"),
        ("Host", "foreign.example"),
    ] {
        let mut r = f.request("t0k");
        r.headers_mut().insert(header, value.parse().unwrap());
        match tokio_tungstenite::connect_async(r).await.unwrap_err() {
            Error::Http(r) => assert_eq!(r.status(), 403),
            x => panic!("{x:?}"),
        };
    }
    f.stop().await;
}
#[tokio::test]
async fn tty_no_cookie_valid_token_attach_input_oversized_discard_and_shutdown_reaps_client() {
    let Some(tmux) = PrivateTmux::start(&["t1"]) else {
        return;
    };
    let f = Front::start(&tmux.home, TermMode::Ttyd, false).await;
    let (mut ws, _) = tokio_tungstenite::connect_async(f.request("%20t0k%20"))
        .await
        .unwrap();
    ws.send(Message::text(r#"{"columns":80,"rows":24}"#))
        .await
        .unwrap();
    assert_eq!(ws.next().await.unwrap().unwrap().into_data()[0], b'1');
    assert_eq!(ws.next().await.unwrap().unwrap().into_data()[0], b'2');
    let first = timeout(Duration::from_secs(3), ws.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(first.into_data()[0], b'0');
    let mut oversized = vec![b'x'; 2 * 1024 * 1024];
    oversized[0] = b'0';
    ws.send(Message::binary(oversized)).await.unwrap();
    ws.send(Message::binary(b"0echo route-ok\r".to_vec()))
        .await
        .unwrap();
    timeout(Duration::from_secs(3), async {
        loop {
            if tmux.capture().contains("route-ok") {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    f.stop().await;
    assert_eq!(tmux.clients(), 0);
    assert!(tmux.tmux(&["has-session", "-t", "=t1"]).status.success());
}
#[tokio::test]
async fn shadow_replay_never_attaches_and_post_is_simulated() {
    let Some(tmux) = PrivateTmux::start(&["t1"]) else {
        return;
    };
    let f = Front::start(&tmux.home, TermMode::Ttyd, true).await;
    let post = f.http("POST", "/session/configure").await;
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(post.split_once("\r\n\r\n").unwrap().1).unwrap(),
        serde_json::json!({"ok":true,"shadow":true})
    );
    let (mut ws, _) = tokio_tungstenite::connect_async(f.request("t0k").map(|_| ()))
        .await
        .unwrap();
    // No session recording: use default, including a traversal-shaped session.
    ws.close(None).await.unwrap();
    let mut request = f.request("t0k");
    *request.uri_mut() = format!("ws://{}/term/ws?arg=t0k&arg=..%2Foutside", f.address)
        .parse()
        .unwrap();
    let (mut ws, _) = tokio_tungstenite::connect_async(request).await.unwrap();
    ws.send(Message::text(r#"{"columns":80,"rows":24}"#))
        .await
        .unwrap();
    ws.next().await.unwrap().unwrap();
    ws.next().await.unwrap().unwrap();
    assert_eq!(
        ws.next().await.unwrap().unwrap().into_data(),
        b"0shadow-output"[..]
    );
    assert_eq!(tmux.clients(), 0);
    drop(ws);
    f.stop().await;
}
#[test]
fn flags_default_off_and_parse_explicit_replay() {
    let home = TestHome::new();
    assert_eq!(
        dash::parse_args(&[], &home.root, None).unwrap().term,
        TermMode::Off
    );
    let args = [
        "--term=native",
        "--webterm-compat=4780,4779",
        "--shadow-readonly",
        "--no-usage-effects",
        "--term-replay-dir=/tmp/replays",
    ]
    .map(String::from);
    let c = dash::parse_args(&args, &home.root, None).unwrap();
    assert_eq!(c.term, TermMode::Native);
    assert_eq!(c.webterm_compat, [4780, 4779]);
    assert!(c.shadow_readonly && c.no_usage_effects);
    assert!(dash::parse_args(&["--term=invalid".into()], &home.root, None).is_err());
}

#[tokio::test]
async fn native_v1_retains_dashboard_gate_and_off_forwards_legacy() {
    let home = TestHome::new();
    let f = Front::start(&home, TermMode::Native, false).await;
    let mut request = f.request("t0k");
    request.headers_mut().insert(
        "Sec-WebSocket-Protocol",
        "comandos.term.v1".parse().unwrap(),
    );
    match tokio_tungstenite::connect_async(request).await.unwrap_err() {
        Error::Http(r) => assert_eq!(r.status(), 401),
        x => panic!("{x:?}"),
    };
    assert!(f.http("GET", "/term/").await.contains("<html"));
    f.stop().await;
    let own = home.root.join("home");
    let legacy = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let legacy_port = legacy.local_addr().unwrap().port();
    let legacy_task = tokio::spawn(async move {
        let (mut s, _) = legacy.accept().await.unwrap();
        let mut b = Vec::new();
        loop {
            let mut chunk = [0; 2048];
            let n = s.read(&mut chunk).await.unwrap();
            assert!(n > 0);
            b.extend_from_slice(&chunk[..n]);
            if b.windows(4).any(|w| w == b"\r\n\r\n") {
                break;
            }
        }
        assert!(String::from_utf8_lossy(&b).starts_with("GET /term/token "));
        s.write_all(
            b"HTTP/1.1 404 Not Found\r\nContent-Length: 6\r\nConnection: close\r\n\r\nlegacy",
        )
        .await
        .unwrap();
    });
    let mut cfg = dash::parse_args(&[], &own, Some(&legacy_port.to_string())).unwrap();
    cfg.native = false;
    cfg.token = b"t0k".to_vec();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (stop, rx) = watch::channel(false);
    let task = tokio::spawn(dash::serve_with(listener, cfg, None, rx));
    let f = Front {
        address,
        stop,
        task,
    };
    let response = f.http("GET", "/term/token").await;
    assert!(response.starts_with("HTTP/1.1 404"));
    assert!(response.ends_with("legacy"));
    f.stop().await;
    legacy_task.await.unwrap();
}
#[tokio::test]
async fn terminal_requires_origin_and_v1_requires_token_even_direct_loopback() {
    let Some(tmux) = PrivateTmux::start(&["t1"]) else {
        return;
    };
    let f = Front::start(&tmux.home, TermMode::Native, false).await;
    let mut request = f.request("t0k");
    request.headers_mut().remove("Origin");
    match tokio_tungstenite::connect_async(request).await.unwrap_err() {
        Error::Http(r) => assert_eq!(r.status(), 403),
        x => panic!("{x:?}"),
    }
    let mut request = f.request("t0k");
    request.headers_mut().remove("X-Forwarded-For");
    request.headers_mut().insert(
        "Sec-WebSocket-Protocol",
        "comandos.term.v1".parse().unwrap(),
    );
    match tokio_tungstenite::connect_async(request).await.unwrap_err() {
        Error::Http(r) => assert_eq!(r.status(), 401),
        x => panic!("{x:?}"),
    }
    let mut request = f.request("t0k");
    request.headers_mut().remove("X-Forwarded-For");
    request.headers_mut().insert(
        "Sec-WebSocket-Protocol",
        "comandos.term.v1".parse().unwrap(),
    );
    request
        .headers_mut()
        .insert("Cookie", "cc_token=t0k".parse().unwrap());
    let (mut ws, _) = tokio_tungstenite::connect_async(request).await.unwrap();
    ws.close(None).await.unwrap();
    assert_eq!(tmux.clients(), 0);
    f.stop().await;
}

#[tokio::test]
async fn enabled_file_disables_front_ws_and_reaps_active_private_client() {
    let Some(tmux) = PrivateTmux::start(&["t1"]) else {
        return;
    };
    let f = Front::start(&tmux.home, TermMode::Ttyd, false).await;
    let (mut ws, _) = tokio_tungstenite::connect_async(f.request("t0k"))
        .await
        .unwrap();
    ws.send(Message::text(r#"{"columns":80,"rows":24}"#))
        .await
        .unwrap();
    ws.next().await.unwrap().unwrap();
    ws.next().await.unwrap().unwrap();
    timeout(Duration::from_secs(3), ws.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    let (mut pending, _) = tokio_tungstenite::connect_async(f.request("t0k"))
        .await
        .unwrap();
    std::fs::remove_file(tmux.home.root.join("home/.claude/hooks/webterm-enabled")).unwrap();
    assert!(
        f.http("GET", "/term/token")
            .await
            .starts_with("HTTP/1.1 404")
    );
    timeout(Duration::from_secs(3), pending.next())
        .await
        .unwrap();
    timeout(Duration::from_secs(3), async {
        loop {
            match ws.next().await {
                Some(Ok(Message::Close(_))) | None => break,
                Some(Err(_)) => break,
                _ => {}
            }
        }
    })
    .await
    .unwrap();
    timeout(Duration::from_secs(3), async {
        loop {
            if tmux.clients() == 0 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    assert!(tmux.tmux(&["has-session", "-t", "=t1"]).status.success());
    f.stop().await;
}
