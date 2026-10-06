//! Real HTTP upgrade, admission, independent capacity and shutdown lifecycle.
use comandos_server::{Config, Reply, WsAdmission, WsHandler, WsRoute};
use futures_util::{SinkExt, StreamExt};
use http::StatusCode;
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{net::TcpListener, sync::watch, task::JoinHandle, time::timeout};
use tokio_tungstenite::tungstenite::{Message, client::IntoClientRequest};
const WAIT: Duration = Duration::from_secs(3);
struct Fixture {
    address: std::net::SocketAddr,
    stop: watch::Sender<bool>,
    task: Option<JoinHandle<std::io::Result<()>>>,
    requests: Arc<Mutex<Vec<String>>>,
}
impl Fixture {
    async fn start(cap: usize, handler_token: bool) -> Self {
        Self::start_with_token(cap, handler_token, None).await
    }
    async fn start_with_token(
        cap: usize,
        handler_token: bool,
        token_file: Option<std::path::PathBuf>,
    ) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (stop, rx) = watch::channel(false);
        let requests = Arc::new(Mutex::new(Vec::new()));
        let recorded = requests.clone();
        let handler: WsHandler = Arc::new(move |request, mut socket| {
            recorded.lock().unwrap().push(request.target.clone());
            Box::pin(async move {
                let mut shutdown = request.shutdown;
                loop {
                    tokio::select! {
                        _=shutdown.changed()=>{let _=socket.close(None).await;break;},
                        msg=socket.next()=>{match msg {Some(Ok(msg)) if msg.is_text()||msg.is_binary()=>{if socket.send(msg).await.is_err(){break}},Some(Ok(Message::Ping(_)|Message::Pong(_)))=>{},_=>break}},
                    }
                }
            })
        });
        let mut route = WsRoute::new(
            Arc::new(|path| match path {
                "/echo" => Some(&["tty", "comandos.term.v1"][..]),
                "/plain" => Some(&[][..]),
                _ => None,
            }),
            handler,
        );
        if handler_token {
            route = route.with_admission(Arc::new(|protocol| {
                if protocol == Some("tty") {
                    WsAdmission::HandlerToken
                } else {
                    WsAdmission::Dashboard
                }
            }));
        }
        if token_file.is_some() {
            route = route.with_admission(Arc::new(|_| WsAdmission::DashboardTokenOrigin));
        }
        let mut limits = comandos_server::dash::limits();
        limits.connections = 2;
        limits.websockets = cap;
        limits.shutdown_grace = Duration::from_millis(200);
        let config = Config {
            token: b"fixture-token".to_vec(),
            token_file,
            asset_exists: Arc::new(|_| false),
            handler: Arc::new(|_| {
                Box::pin(async { Ok(Reply::bytes(StatusCode::NOT_FOUND, "text/plain", "missing")) })
            }),
            websocket: Some(route),
            limits,
        };
        let task = tokio::spawn(comandos_server::serve(listener, config, rx));
        Self {
            address,
            stop,
            task: Some(task),
            requests,
        }
    }
    fn request(&self, path: &str) -> http::Request<()> {
        let mut request = format!("ws://{}{path}", self.address)
            .into_client_request()
            .unwrap();
        request.headers_mut().insert(
            "Sec-WebSocket-Protocol",
            "tty, comandos.term.v1".parse().unwrap(),
        );
        request
    }
    async fn stop(mut self) {
        self.stop.send(true).unwrap();
        timeout(WAIT, self.task.take().unwrap())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = self.stop.send(true);
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}
#[tokio::test]
async fn upgrade_echo_protocol_order_and_target() {
    let fixture = Fixture::start(2, false).await;
    let mut request = fixture.request("/echo?arg=a&arg=b");
    request.headers_mut().insert(
        "Origin",
        format!("http://{}", fixture.address).parse().unwrap(),
    );
    let (mut socket, response) = tokio_tungstenite::connect_async(request).await.unwrap();
    assert_eq!(response.headers()["sec-websocket-protocol"], "tty");
    socket
        .send(Message::binary(vec![b'0', 255, 0]))
        .await
        .unwrap();
    assert_eq!(
        socket.next().await.unwrap().unwrap(),
        Message::binary(vec![b'0', 255, 0])
    );
    assert_eq!(*fixture.requests.lock().unwrap(), vec!["/echo?arg=a&arg=b"]);
    fixture.stop().await;
}
#[tokio::test]
async fn default_dashboard_route_never_skips_host_origin_or_token_gate() {
    let fixture = Fixture::start(2, false).await;
    for (header, value) in [
        ("Origin", "https://evil.example"),
        ("Host", "evil.example"),
        ("X-Forwarded-For", "198.51.100.9"),
    ] {
        let mut request = fixture.request("/echo");
        request.headers_mut().insert(header, value.parse().unwrap());
        let error = tokio_tungstenite::connect_async(request).await.unwrap_err();
        assert!(
            error.to_string().contains(if header == "X-Forwarded-For" {
                "401"
            } else {
                "403"
            }),
            "{error}"
        );
    }
    assert!(fixture.requests.lock().unwrap().is_empty());
    fixture.stop().await;
}
#[tokio::test]
async fn handler_token_is_explicit_and_still_checks_host_origin_v1_uses_dashboard() {
    let fixture = Fixture::start(2, true).await;
    let mut request = fixture.request("/echo");
    request
        .headers_mut()
        .insert("X-Forwarded-For", "198.51.100.9".parse().unwrap());
    let (socket, _) = tokio_tungstenite::connect_async(request).await.unwrap();
    drop(socket);
    for (header, value) in [("Origin", "https://evil.example"), ("Host", "evil.example")] {
        let mut request = fixture.request("/echo");
        request.headers_mut().insert(header, value.parse().unwrap());
        assert!(
            tokio_tungstenite::connect_async(request)
                .await
                .unwrap_err()
                .to_string()
                .contains("403")
        );
    }
    let mut request = fixture.request("/echo");
    request.headers_mut().insert(
        "Sec-WebSocket-Protocol",
        "comandos.term.v1".parse().unwrap(),
    );
    request
        .headers_mut()
        .insert("X-Forwarded-For", "198.51.100.9".parse().unwrap());
    assert!(
        tokio_tungstenite::connect_async(request)
            .await
            .unwrap_err()
            .to_string()
            .contains("401")
    );
    fixture.stop().await;
}
#[tokio::test]
async fn cap_is_independent_http_survives_and_disconnection_releases_it() {
    let fixture = Fixture::start(1, false).await;
    let (first, _) = tokio_tungstenite::connect_async(fixture.request("/echo"))
        .await
        .unwrap();
    assert!(
        tokio_tungstenite::connect_async(fixture.request("/echo"))
            .await
            .unwrap_err()
            .to_string()
            .contains("503")
    );
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let mut http = tokio::net::TcpStream::connect(fixture.address)
        .await
        .unwrap();
    http.write_all(
        format!(
            "GET / HTTP/1.1\r\nHost: {}\r\nConnection: close\r\n\r\n",
            fixture.address
        )
        .as_bytes(),
    )
    .await
    .unwrap();
    let mut response = Vec::new();
    http.read_to_end(&mut response).await.unwrap();
    assert!(String::from_utf8_lossy(&response).starts_with("HTTP/1.1 404"));
    drop(first);
    let (socket, _) = timeout(WAIT, async {
        loop {
            if let Ok(pair) = tokio_tungstenite::connect_async(fixture.request("/echo")).await {
                break pair;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    drop(socket);
    fixture.stop().await;
}
#[tokio::test]
async fn protocol_handshake_validation_and_empty_protocol_route() {
    let fixture = Fixture::start(2, false).await;
    for protocol in [Some("chat"), None] {
        let mut request = fixture.request("/echo");
        request.headers_mut().remove("Sec-WebSocket-Protocol");
        if let Some(protocol) = protocol {
            request
                .headers_mut()
                .insert("Sec-WebSocket-Protocol", protocol.parse().unwrap());
        }
        assert!(tokio_tungstenite::connect_async(request).await.is_err());
    }
    assert!(
        tokio_tungstenite::connect_async(fixture.request("/unknown"))
            .await
            .is_err()
    );
    let mut request = fixture.request("/echo");
    request.headers_mut().insert(
        "Sec-WebSocket-Key",
        "not-a-16-byte-base64-key!".parse().unwrap(),
    );
    assert!(
        tokio_tungstenite::connect_async(request)
            .await
            .unwrap_err()
            .to_string()
            .contains("400")
    );
    let (mut socket, response) =
        tokio_tungstenite::connect_async(format!("ws://{}/plain", fixture.address))
            .await
            .unwrap();
    assert!(response.headers().get("sec-websocket-protocol").is_none());
    socket.send(Message::text("plain")).await.unwrap();
    assert_eq!(
        socket.next().await.unwrap().unwrap(),
        Message::text("plain")
    );
    fixture.stop().await;
}
#[tokio::test]
async fn transport_allows_two_mib_for_application_to_discard_without_disconnect() {
    let fixture = Fixture::start(2, false).await;
    let (mut socket, _) = tokio_tungstenite::connect_async(fixture.request("/echo"))
        .await
        .unwrap();
    let data = vec![b'x'; 2 * 1024 * 1024 + 1];
    socket.send(Message::binary(data.clone())).await.unwrap();
    assert_eq!(
        timeout(WAIT, socket.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap(),
        Message::binary(data)
    );
    socket.send(Message::text("still alive")).await.unwrap();
    assert_eq!(
        socket.next().await.unwrap().unwrap(),
        Message::text("still alive")
    );
    fixture.stop().await;
}
#[tokio::test]
async fn shutdown_aborts_and_drops_noncooperative_websocket_handler() {
    use std::sync::atomic::{AtomicBool, Ordering};
    struct Dropped(Arc<AtomicBool>);
    impl Drop for Dropped {
        fn drop(&mut self) {
            self.0.store(true, Ordering::SeqCst);
        }
    }
    let started = Arc::new(AtomicBool::new(false));
    let dropped = Arc::new(AtomicBool::new(false));
    let enter = started.clone();
    let end = dropped.clone();
    let handler: WsHandler = Arc::new(move |_, socket| {
        let enter = enter.clone();
        let end = end.clone();
        Box::pin(async move {
            let _socket = socket;
            let _guard = Dropped(end);
            enter.store(true, Ordering::SeqCst);
            std::future::pending::<()>().await;
        })
    });
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (stop, rx) = watch::channel(false);
    let mut limits = comandos_server::dash::limits();
    limits.shutdown_grace = Duration::from_millis(100);
    let config = Config {
        token: b"fixture-token".to_vec(),
        token_file: None,
        asset_exists: Arc::new(|_| false),
        handler: Arc::new(|_| {
            Box::pin(async { Ok(Reply::bytes(StatusCode::NOT_FOUND, "text/plain", "missing")) })
        }),
        websocket: Some(WsRoute::new(
            Arc::new(|path| (path == "/plain").then_some(&[][..])),
            handler,
        )),
        limits,
    };
    let task = tokio::spawn(comandos_server::serve(listener, config, rx));
    let (client, _) = tokio_tungstenite::connect_async(format!("ws://{address}/plain"))
        .await
        .unwrap();
    timeout(WAIT, async {
        while !started.load(Ordering::SeqCst) {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    stop.send(true).unwrap();
    timeout(WAIT, task).await.unwrap().unwrap().unwrap();
    assert!(dropped.load(Ordering::SeqCst));
    drop(client);
}

#[tokio::test]
async fn websocket_uses_rotated_dashboard_token_without_restart() {
    let dir = std::env::temp_dir().join(format!(
        "comandos-ws-token-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("dash-token");
    std::fs::write(&path, b"fixture-token").unwrap();
    let fixture = Fixture::start_with_token(2, false, Some(path.clone())).await;
    let mut request = fixture.request("/echo");
    request.headers_mut().insert(
        "Origin",
        format!("http://{}", fixture.address).parse().unwrap(),
    );
    request
        .headers_mut()
        .insert("X-Comandos-Token", "fixture-token".parse().unwrap());
    let (mut old, _) = tokio_tungstenite::connect_async(request).await.unwrap();
    old.close(None).await.unwrap();
    drop(old);
    std::fs::write(&path, b"rotated-fixture-token-longer").unwrap();
    let mut request = fixture.request("/echo");
    request.headers_mut().insert(
        "Origin",
        format!("http://{}", fixture.address).parse().unwrap(),
    );
    request.headers_mut().insert(
        "X-Comandos-Token",
        "rotated-fixture-token-longer".parse().unwrap(),
    );
    let result = tokio_tungstenite::connect_async(request).await;
    assert!(
        result.is_ok(),
        "token nuevo rechazado después de rotación: {result:?}"
    );
    let (mut current, _) = result.unwrap();
    current.close(None).await.unwrap();
    drop(current);
    fixture.stop().await;
    std::fs::remove_dir_all(dir).unwrap();
}
