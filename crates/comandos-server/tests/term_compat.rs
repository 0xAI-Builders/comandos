#[path = "support/private_tmux.rs"]
mod private_tmux;
use comandos_server::dash::{
    self, DashState,
    term::{
        TermMode,
        attach::TmuxTarget,
        compat::{self, CompatProfile},
    },
};
use futures_util::{SinkExt, StreamExt};
use private_tmux::TestHome;
use std::sync::Arc;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::watch,
};
use tokio_tungstenite::tungstenite::{Message, client::IntoClientRequest};
#[tokio::test]
async fn profiles_confine_routes_rewrite_ws_and_hold_port() {
    for profile in [CompatProfile::Path, CompatProfile::Plain] {
        let home = TestHome::new();
        let own = home.root.join("home");
        std::fs::create_dir_all(own.join(".claude/hooks/dash")).unwrap();
        std::fs::write(dash::token_path(&own), b"t0k").unwrap();
        std::fs::write(own.join(".claude/hooks/webterm-enabled"), b"").unwrap();
        std::fs::write(own.join(".claude/hooks/dash/term.html"), b"exact-page").unwrap();
        std::fs::write(home.root.join("default.bin"), b"compat-replay").unwrap();
        let mut cfg = dash::parse_args(&[], &own, None).unwrap();
        cfg.term = TermMode::Ttyd;
        cfg.token = b"t0k".to_vec();
        cfg.shadow_readonly = true;
        cfg.term_replay_dir = Some(home.root.clone());
        let state = Arc::new(DashState {
            config: cfg,
            asset_exists: Arc::new(|_| false),
            native: None,
            term_target: TmuxTarget::Private(home.root.clone()),
            term_control: Arc::new(dash::term::lifecycle::Control::new(&own)),
        });
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        assert_eq!(
            TcpListener::bind(addr).await.unwrap_err().kind(),
            std::io::ErrorKind::AddrInUse
        );
        let (stop, rx) = watch::channel(false);
        let task = tokio::spawn(compat::serve_listener(listener, profile, state, rx));
        for (path, status) in if profile == CompatProfile::Path {
            vec![("/term/token", 200), ("/", 404), ("/assets/x.js", 404)]
        } else {
            vec![("/token", 200), ("/term/token", 404), ("/assets/x.js", 404)]
        } {
            let mut s = TcpStream::connect(addr).await.unwrap();
            s.write_all(format!("GET {path} HTTP/1.1\r\nHost: phone.tail.ts.net\r\nX-Comandos-Token: t0k\r\nConnection: close\r\n\r\n").as_bytes()).await.unwrap();
            let mut b = Vec::new();
            s.read_to_end(&mut b).await.unwrap();
            assert!(String::from_utf8_lossy(&b).starts_with(&format!("HTTP/1.1 {status}")));
        }
        let path = if profile == CompatProfile::Path {
            "/term/ws"
        } else {
            "/ws"
        };
        let mut request = format!("ws://{addr}{path}?arg=t0k")
            .into_client_request()
            .unwrap();
        request
            .headers_mut()
            .insert("Sec-WebSocket-Protocol", "tty".parse().unwrap());
        request
            .headers_mut()
            .insert("Origin", format!("http://{addr}").parse().unwrap());
        let (mut ws, _) = tokio_tungstenite::connect_async(request).await.unwrap();
        ws.send(Message::text(r#"{"columns":80,"rows":24}"#))
            .await
            .unwrap();
        ws.next().await.unwrap().unwrap();
        ws.next().await.unwrap().unwrap();
        assert_eq!(
            ws.next().await.unwrap().unwrap().into_data(),
            b"0compat-replay"[..]
        );
        drop(ws);
        stop.send(true).unwrap();
        task.await.unwrap().unwrap();
        assert!(TcpStream::connect(addr).await.is_err());
    }
}
#[tokio::test]
async fn compat_bind_failure_keeps_front_live_and_enablement_releases_and_rebinds_port() {
    let home = TestHome::new();
    let own = home.root.join("home");
    std::fs::create_dir_all(own.join(".claude/hooks")).unwrap();
    std::fs::write(own.join(".claude/hooks/webterm-enabled"), b"").unwrap();
    let held = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = held.local_addr().unwrap().port();
    let front = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = front.local_addr().unwrap();
    let mut cfg = dash::parse_args(&[], &own, None).unwrap();
    cfg.token = b"t0k".to_vec();
    cfg.native = false;
    cfg.term = TermMode::Ttyd;
    cfg.webterm_compat = vec![port];
    cfg.port = address.port();
    let (stop, rx) = watch::channel(false);
    let task = tokio::spawn(dash::serve_with(front, cfg, None, rx));
    let http = || async {
        let mut s = TcpStream::connect(address).await.unwrap();
        s.write_all(
            format!("GET /web/status HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\n\r\n")
                .as_bytes(),
        )
        .await
        .unwrap();
        let mut b = Vec::new();
        s.read_to_end(&mut b).await.unwrap();
        String::from_utf8(b).unwrap()
    };
    tokio::time::timeout(std::time::Duration::from_secs(3), async {
        loop {
            let response = http().await;
            if response.contains("address in use") {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    assert!(!task.is_finished());
    drop(held);
    tokio::time::timeout(std::time::Duration::from_secs(3), async {
        loop {
            if TcpStream::connect((std::net::Ipv4Addr::LOCALHOST, port))
                .await
                .is_ok()
            {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    std::fs::remove_file(own.join(".claude/hooks/webterm-enabled")).unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(3), async {
        loop {
            if TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port))
                .await
                .is_ok()
            {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    std::fs::write(own.join(".claude/hooks/webterm-enabled"), b"").unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(3), async {
        loop {
            if TcpStream::connect((std::net::Ipv4Addr::LOCALHOST, port))
                .await
                .is_ok()
            {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    stop.send(true).unwrap();
    task.await.unwrap().unwrap();
}
