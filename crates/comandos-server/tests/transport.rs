use bytes::Bytes;
use comandos_server::{self as server, Config, Handler, HandlerError, Limits, Reply, ReplyBody};
use http::{Request, StatusCode};
use http_body_util::{BodyExt, Full};
use hyper::{
    body::Incoming,
    client::conn::http1::{self, SendRequest},
};
use hyper_util::rt::TokioIo;
use serde_json::{Value, json};
use std::{
    future::Future,
    io,
    net::SocketAddr,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::{Notify, mpsc, watch},
    task::JoinHandle,
    time::timeout,
};

const WAIT: Duration = Duration::from_secs(3);
fn limits() -> Limits {
    Limits {
        connections: 8,
        header_bytes: 65536,
        buffered_wire_bytes: 20_000_000,
        header_timeout: Duration::from_secs(1),
        body_timeout: Duration::from_millis(150),
        handler_timeout: Duration::from_millis(150),
        write_timeout: Duration::from_millis(150),
        shutdown_grace: Duration::from_millis(100),
    }
}
fn handler<F, Fut>(f: F) -> Handler
where
    F: Fn(server::Request) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<Reply, HandlerError>> + Send + 'static,
{
    Arc::new(move |request| Box::pin(f(request)))
}
fn echo() -> Handler {
    handler(|r| async move {
        Reply::json(
            StatusCode::OK,
            &json!({"target":r.target,"data":r.data,"headers":r.headers,"internalProducer":r.internal_producer}),
        )
    })
}
struct Fixture {
    address: SocketAddr,
    stop: watch::Sender<bool>,
    task: Option<JoinHandle<io::Result<()>>>,
}
impl Fixture {
    async fn start(handler: Handler, limits: Limits) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (stop, shutdown) = watch::channel(false);
        let config = Config {
            token: b"fixture-token".to_vec(),
            asset_exists: Arc::new(|p| p == "/workspace.css"),
            handler,
            limits,
        };
        let task = tokio::spawn(server::serve(listener, config, shutdown));
        Self {
            address,
            stop,
            task: Some(task),
        }
    }
    async fn connect(&self) -> Client {
        let stream = timeout(WAIT, TcpStream::connect(self.address))
            .await
            .unwrap()
            .expect("native listener must remain available");
        let (sender, connection) = http1::handshake(TokioIo::new(stream)).await.unwrap();
        let task = tokio::spawn(async move {
            let _ = connection.await;
        });
        Client { sender, task }
    }
    async fn raw(&self, bytes: &[u8]) -> Vec<u8> {
        let mut stream = TcpStream::connect(self.address)
            .await
            .expect("native listener must remain available");
        stream.write_all(bytes).await.unwrap();
        let mut output = Vec::new();
        timeout(WAIT, stream.read_to_end(&mut output))
            .await
            .expect("rejection must close without reading the absent body")
            .unwrap();
        output
    }
    async fn finish(mut self) {
        self.stop.send(true).unwrap();
        timeout(WAIT, self.task.take().unwrap())
            .await
            .expect("shutdown must reap connections")
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
struct Client {
    sender: SendRequest<Full<Bytes>>,
    task: JoinHandle<()>,
}
impl Client {
    async fn request(
        &mut self,
        method: &str,
        target: &str,
        extra: &[(&str, &str)],
        body: impl Into<Bytes>,
    ) -> http::Response<Incoming> {
        let body = body.into();
        let mut req = Request::builder()
            .method(method)
            .uri(target)
            .header("host", "localhost");
        if method != "GET" {
            req = req.header("content-length", body.len());
        }
        for (k, v) in extra {
            req = req.header(*k, *v);
        }
        timeout(
            WAIT,
            self.sender.send_request(req.body(Full::new(body)).unwrap()),
        )
        .await
        .unwrap()
        .expect("native HTTP response")
    }
}
impl Drop for Client {
    fn drop(&mut self) {
        self.task.abort();
    }
}
async fn json_response(response: http::Response<Incoming>, status: u16) -> Value {
    assert_eq!(response.status().as_u16(), status);
    assert_eq!(response.headers()["cache-control"], "no-store");
    let body = timeout(WAIT, response.into_body().collect())
        .await
        .unwrap()
        .unwrap()
        .to_bytes();
    serde_json::from_slice(&body).unwrap()
}

#[tokio::test]
async fn review_json_response_retains_python_default_bytes_and_insertion_order() {
    let fixture = Fixture::start(
        handler(|_| async {
            let value =
                comandos_core::json::workspace_loads(r#"{"z":"ñ😊","a":{"z":1e-5,"a":-0.0}}"#)
                    .unwrap();
            Reply::json(StatusCode::OK, &value)
        }),
        limits(),
    )
    .await;
    let wire = fixture
        .raw(b"GET /state HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .await;
    let boundary = wire.windows(4).position(|b| b == b"\r\n\r\n").unwrap() + 4;
    assert_eq!(
        &wire[boundary..],
        br#"{"z": "\u00f1\ud83d\ude0a", "a": {"z": 1e-05, "a": -0.0}}"#
    );
    let wire = fixture
        .raw(b"PATCH /state HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .await;
    let boundary = wire.windows(4).position(|b| b == b"\r\n\r\n").unwrap() + 4;
    assert_eq!(
        &wire[boundary..],
        br#"{"error": "M\u00e9todo no implementado"}"#
    );
    fixture.finish().await;
}

#[tokio::test]
async fn review_stalled_reader_releases_stream_and_connection_capacity() {
    let (sender, receiver) = mpsc::channel(1);
    let receiver = Arc::new(std::sync::Mutex::new(Some(receiver)));
    let mut caps = limits();
    caps.connections = 1;
    let fixture = Fixture::start(
        handler(move |_| {
            let receiver = receiver.lock().unwrap().take();
            async move {
                match receiver {
                    Some(receiver) => Ok(Reply {
                        status: StatusCode::OK,
                        headers: http::HeaderMap::new(),
                        body: ReplyBody::Stream(receiver),
                    }),
                    None => Reply::json(StatusCode::OK, &json!({"recovered":true})),
                }
            }
        }),
        caps,
    )
    .await;
    let mut stalled = TcpStream::connect(fixture.address).await.unwrap();
    stalled
        .write_all(b"GET /state HTTP/1.1\r\nHost: localhost\r\n\r\n")
        .await
        .unwrap();
    // 64 MiB maximum, one reused frame and one queued clone. The peer never
    // reads, so kernel buffers cannot consume the entire bounded producer.
    let frame = Bytes::from(vec![b'x'; 65536]);
    let producer = async {
        for _ in 0..1024 {
            if sender.send(Ok(frame.clone())).await.is_err() {
                return;
            }
        }
        panic!("fixture failed to saturate the nonreading peer");
    };
    timeout(WAIT, producer)
        .await
        .expect("blocked socket write must drop the stream receiver without global shutdown");
    let mut client = fixture.connect().await;
    assert_eq!(
        json_response(
            client.request("GET", "/state", &[], Bytes::new()).await,
            200
        )
        .await["recovered"],
        true
    );
    drop(client);
    drop(stalled);
    fixture.finish().await;
}

#[tokio::test]
async fn review_quiet_event_stream_can_outlive_write_and_handler_timeouts() {
    let (sender, receiver) = mpsc::channel(1);
    let receiver = Arc::new(std::sync::Mutex::new(Some(receiver)));
    let fixture = Fixture::start(
        handler(move |_| {
            let receiver = receiver.lock().unwrap().take().unwrap();
            async move {
                Ok(Reply {
                    status: StatusCode::OK,
                    headers: http::HeaderMap::new(),
                    body: ReplyBody::Stream(receiver),
                })
            }
        }),
        limits(),
    )
    .await;
    let mut stream = TcpStream::connect(fixture.address).await.unwrap();
    stream
        .write_all(b"GET /events HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .await
        .unwrap();
    sender
        .send(Ok(Bytes::from_static(b"data: first\n\n")))
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(350)).await;
    sender
        .send(Ok(Bytes::from_static(b"data: after quiet\n\n")))
        .await
        .expect("quiet stream must remain open");
    drop(sender);
    let mut output = Vec::new();
    timeout(WAIT, stream.read_to_end(&mut output))
        .await
        .unwrap()
        .unwrap();
    let output = String::from_utf8(output).unwrap();
    assert!(output.starts_with("HTTP/1.1 200"));
    assert!(output.contains("data: first\n\n") && output.contains("data: after quiet\n\n"));
    fixture.finish().await;
}

#[tokio::test]
async fn keep_alive_responses_are_all_uncacheable_and_preserve_targets() {
    let fixture = Fixture::start(echo(), limits()).await;
    let mut client = fixture.connect().await;
    for target in ["/state?token=fixture-token", "/accounts", "/workspace.css"] {
        let value =
            json_response(client.request("GET", target, &[], Bytes::new()).await, 200).await;
        assert_eq!(value["target"], target);
        fixture.stop.send(false).unwrap();
    }
    drop(client);
    fixture.finish().await;
}

#[tokio::test]
async fn security_denials_precede_mutation_and_public_assets_still_load() {
    let calls = Arc::new(AtomicUsize::new(0));
    let observed = calls.clone();
    let fixture = Fixture::start(
        handler(move |_| {
            observed.fetch_add(1, Ordering::SeqCst);
            async { Reply::json(StatusCode::OK, &json!({"ok":true})) }
        }),
        limits(),
    )
    .await;
    let denied=fixture.raw(b"POST /workspace HTTP/1.1\r\nHost: localhost\r\nX-Forwarded-For: 203.0.113.2\r\nContent-Length: 1000000\r\n\r\n").await;
    assert!(denied.starts_with(b"HTTP/1.1 401"));
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    let mut client = fixture.connect().await;
    json_response(
        client
            .request(
                "GET",
                "/workspace.css?v=fixture",
                &[("x-forwarded-for", "203.0.113.2")],
                Bytes::new(),
            )
            .await,
        200,
    )
    .await;
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    drop(client);
    fixture.finish().await;
}

#[tokio::test]
async fn duplicate_security_headers_and_cross_origin_requests_never_dispatch() {
    let fixture = Fixture::start(echo(), limits()).await;
    for extra in [
        "Host: localhost\r\n",
        "Origin: http://localhost\r\nOrigin: http://localhost\r\n",
        "Origin: http://evil.localhost\r\n",
    ] {
        let wire = format!(
            "POST /workspace HTTP/1.1\r\nHost: localhost\r\n{extra}Content-Length: 2\r\n\r\n"
        );
        assert!(
            fixture
                .raw(wire.as_bytes())
                .await
                .starts_with(b"HTTP/1.1 403")
        );
    }
    fixture.finish().await;
}

#[tokio::test]
async fn body_limits_reject_before_read_and_wrong_json_type_keeps_connection() {
    let fixture = Fixture::start(echo(), limits()).await;
    for (method, length) in [("POST", 20_000_001), ("DELETE", 64_001)] {
        let wire = format!(
            "{method} /workspace HTTP/1.1\r\nHost: localhost\r\nContent-Length: {length}\r\n\r\n"
        );
        assert!(
            fixture
                .raw(wire.as_bytes())
                .await
                .starts_with(b"HTTP/1.1 413")
        );
    }
    let mut client = fixture.connect().await;
    let value = json_response(client.request("POST", "/workspace", &[], "[]").await, 400).await;
    assert_eq!(value["error"], "El cuerpo debe ser un objeto JSON");
    json_response(
        client.request("GET", "/state", &[], Bytes::new()).await,
        200,
    )
    .await;
    drop(client);
    fixture.finish().await;
}

#[tokio::test]
async fn empty_and_encoded_json_bodies_reach_handler_without_loss() {
    let fixture = Fixture::start(echo(), limits()).await;
    let mut client = fixture.connect().await;
    let mut utf16 = vec![0xff, 0xfe];
    for unit in "{\"name\":\"🦀\"}".encode_utf16() {
        utf16.extend(unit.to_le_bytes());
    }
    let cases = [
        (Vec::new(), json!({})),
        (
            b"{\"n\":9007199254740993,\"repeat\":1,\"repeat\":2}".to_vec(),
            json!({"n":9007199254740993_u64,"repeat":2}),
        ),
        (utf16, json!({"name":"🦀"})),
    ];
    for (bytes, want) in cases {
        let value =
            json_response(client.request("POST", "/workspace", &[], bytes).await, 200).await;
        assert_eq!(value["data"], want);
    }
    drop(client);
    fixture.finish().await;
}

#[tokio::test]
async fn malformed_json_closes_and_slow_body_times_out_without_dispatch() {
    let calls = Arc::new(AtomicUsize::new(0));
    let observed = calls.clone();
    let fixture = Fixture::start(
        handler(move |_| {
            observed.fetch_add(1, Ordering::SeqCst);
            async { Reply::json(StatusCode::OK, &json!({})) }
        }),
        limits(),
    )
    .await;
    for wire in [
        b"POST /workspace HTTP/1.1\r\nHost: localhost\r\nContent-Length: 1\r\n\r\n{".as_slice(),
        b"POST /workspace HTTP/1.1\r\nHost: localhost\r\nContent-Length: 20\r\n\r\n{",
    ] {
        let reply = fixture.raw(wire).await;
        assert!(reply.starts_with(b"HTTP/1.1 400"));
    }
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    fixture.finish().await;
}

#[tokio::test]
async fn ambiguous_framing_and_transfer_encoding_never_dispatch() {
    let calls = Arc::new(AtomicUsize::new(0));
    let observed = calls.clone();
    let fixture = Fixture::start(
        handler(move |_| {
            observed.fetch_add(1, Ordering::SeqCst);
            async { Reply::json(StatusCode::OK, &json!({})) }
        }),
        limits(),
    )
    .await;
    for headers in [
        "Content-Length: 2\r\nContent-Length: 3\r\n",
        "Transfer-Encoding: chunked\r\nContent-Length: 2\r\n",
        "Transfer-Encoding: chunked\r\n",
    ] {
        let wire = format!("POST /workspace HTTP/1.1\r\nHost: localhost\r\n{headers}\r\n");
        assert!(
            fixture
                .raw(wire.as_bytes())
                .await
                .starts_with(b"HTTP/1.1 400")
        );
    }
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    fixture.finish().await;
}

#[tokio::test]
async fn handler_failure_panic_and_timeout_answer_once_and_server_survives() {
    let fixture = Fixture::start(
        handler(|request| async move {
            match request.target.as_str() {
                "/state/error" => Err(HandlerError::Failure),
                "/state/timeout" => Err(HandlerError::Timeout),
                "/state/panic" => panic!("fixture panic"),
                "/state/pending" => std::future::pending().await,
                _ => Reply::json(StatusCode::OK, &json!({"alive":true})),
            }
        }),
        limits(),
    )
    .await;
    for (path, status) in [
        ("/state/error", 500),
        ("/state/timeout", 504),
        ("/state/panic", 500),
        ("/state/pending", 504),
    ] {
        let mut client = fixture.connect().await;
        let value =
            json_response(client.request("GET", path, &[], Bytes::new()).await, status).await;
        assert!(value["error"].is_string());
    }
    let mut client = fixture.connect().await;
    json_response(
        client.request("GET", "/state", &[], Bytes::new()).await,
        200,
    )
    .await;
    drop(client);
    fixture.finish().await;
}

#[tokio::test]
async fn streaming_delivers_first_event_before_producer_finishes() {
    let release = Arc::new(Notify::new());
    let control = release.clone();
    let fixture = Fixture::start(
        handler(move |_| {
            let release = control.clone();
            async move {
                let (send, recv) = mpsc::channel(1);
                tokio::spawn(async move {
                    send.send(Ok(Bytes::from_static(b"data: first\n\n")))
                        .await
                        .unwrap();
                    release.notified().await;
                    let _ = send.send(Ok(Bytes::from_static(b"data: last\n\n"))).await;
                });
                let mut reply = Reply::bytes(StatusCode::OK, "text/event-stream", Bytes::new());
                reply.body = ReplyBody::Stream(recv);
                Ok(reply)
            }
        }),
        limits(),
    )
    .await;
    let mut client = fixture.connect().await;
    let response = client.request("GET", "/events", &[], Bytes::new()).await;
    assert_eq!(response.headers()["cache-control"], "no-store");
    let mut body = response.into_body();
    let first = timeout(WAIT, body.frame())
        .await
        .unwrap()
        .unwrap()
        .unwrap()
        .into_data()
        .unwrap();
    assert_eq!(first, b"data: first\n\n".as_slice());
    release.notify_one();
    assert_eq!(
        timeout(WAIT, body.collect())
            .await
            .unwrap()
            .unwrap()
            .to_bytes(),
        b"data: last\n\n".as_slice()
    );
    drop(client);
    fixture.finish().await;
}

#[tokio::test]
async fn streaming_error_after_headers_terminates_without_second_response() {
    let fail = Arc::new(Notify::new());
    let control = fail.clone();
    let fixture = Fixture::start(
        handler(move |_| {
            let fail = control.clone();
            async move {
                let (send, recv) = mpsc::channel(1);
                tokio::spawn(async move {
                    send.send(Ok(Bytes::from_static(b"first"))).await.unwrap();
                    fail.notified().await;
                    let _ = send
                        .send(Err(io::Error::other("fixture stream failure")))
                        .await;
                });
                let mut reply = Reply::bytes(StatusCode::OK, "text/event-stream", Bytes::new());
                reply.body = ReplyBody::Stream(recv);
                Ok(reply)
            }
        }),
        limits(),
    )
    .await;
    let mut client = fixture.connect().await;
    let response = client.request("GET", "/events", &[], Bytes::new()).await;
    assert_eq!(response.status(), 200);
    let mut body = response.into_body();
    assert_eq!(
        timeout(WAIT, body.frame())
            .await
            .unwrap()
            .unwrap()
            .unwrap()
            .into_data()
            .unwrap(),
        b"first".as_slice()
    );
    fail.notify_one();
    assert!(timeout(WAIT, body.collect()).await.unwrap().is_err());
    drop(client);
    fixture.finish().await;
}

#[tokio::test]
async fn aggregate_body_budget_is_released_when_handler_finishes() {
    let entered = Arc::new(Notify::new());
    let release = Arc::new(Notify::new());
    let entered2 = entered.clone();
    let release2 = release.clone();
    let mut l = limits();
    l.buffered_wire_bytes = 2;
    l.handler_timeout = WAIT;
    let fixture = Fixture::start(
        handler(move |r| {
            let entered = entered2.clone();
            let release = release2.clone();
            async move {
                if r.target == "/workspace/hold" {
                    entered.notify_one();
                    release.notified().await;
                }
                Reply::json(StatusCode::OK, &json!({}))
            }
        }),
        l,
    )
    .await;
    let mut first = fixture.connect().await;
    let task = tokio::spawn(async move {
        json_response(
            first.request("POST", "/workspace/hold", &[], "{}").await,
            200,
        )
        .await;
    });
    timeout(WAIT, entered.notified()).await.unwrap();
    let mut second = fixture.connect().await;
    json_response(second.request("POST", "/workspace", &[], "{}").await, 503).await;
    release.notify_one();
    timeout(WAIT, task).await.unwrap().unwrap();
    let mut third = fixture.connect().await;
    json_response(third.request("POST", "/workspace", &[], "{}").await, 200).await;
    drop(second);
    drop(third);
    fixture.finish().await;
}

#[tokio::test]
async fn connection_limit_and_shutdown_bound_idle_connections() {
    let mut l = limits();
    l.connections = 1;
    let fixture = Fixture::start(echo(), l).await;
    let mut first = fixture.connect().await;
    json_response(first.request("GET", "/state", &[], Bytes::new()).await, 200).await;
    let mut second = TcpStream::connect(fixture.address).await.unwrap();
    let mut byte = [0];
    assert_eq!(
        timeout(WAIT, second.read(&mut byte))
            .await
            .unwrap()
            .unwrap(),
        0
    );
    fixture.finish().await;
    drop(first);
}

#[tokio::test]
async fn incomplete_headers_expire_and_get_bodies_cannot_become_another_request() {
    let mut l = limits();
    l.header_timeout = Duration::from_millis(100);
    let fixture = Fixture::start(echo(), l).await;
    let expired = fixture.raw(b"GET /state HTTP/1.1\r\nHost: ").await;
    // Both the pinned Hyper parser and Python's BaseHTTPRequestHandler close
    // incomplete timed-out headers before producing an application response.
    assert!(expired.is_empty(), "header timeout wire bytes: {expired:?}");
    assert!(
        fixture
            .raw(b"GET /state HTTP/1.1\r\nHost: localhost\r\nContent-Length: 200\r\n\r\n")
            .await
            .starts_with(b"HTTP/1.1 400")
    );
    let mut client = fixture.connect().await;
    json_response(
        client.request("GET", "/state", &[], Bytes::new()).await,
        200,
    )
    .await;
    drop(client);
    fixture.finish().await;
}

#[tokio::test]
async fn header_values_use_latin1_and_same_name_order_and_equal_lengths_are_unambiguous() {
    let fixture = Fixture::start(echo(), limits()).await;
    let output=fixture.raw(b"POST /workspace HTTP/1.1\r\nHost: localhost\r\nContent-Length: 2\r\nContent-Length: 2\r\nX-Fixture: \xe9\r\nX-Fixture: next\r\nConnection: close\r\n\r\n{}").await;
    assert!(output.starts_with(b"HTTP/1.1 200"));
    let boundary = output.windows(4).position(|w| w == b"\r\n\r\n").unwrap() + 4;
    let value: Value = serde_json::from_slice(&output[boundary..]).unwrap();
    let seen = value["headers"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|v| v[0] == "x-fixture")
        .map(|v| v[1].clone())
        .collect::<Vec<_>>();
    assert_eq!(seen, vec![json!("é"), json!("next")]);
    assert_eq!(value["data"], json!({}));
    fixture.finish().await;
}

#[tokio::test]
async fn invalid_limits_and_empty_tokens_fail_before_accepting() {
    for bad in 0..5 {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let (_send, recv) = watch::channel(false);
        let mut l = limits();
        let mut token = b"fixture-token".to_vec();
        match bad {
            0 => l.connections = 0,
            1 => l.body_timeout = Duration::ZERO,
            2 => token.clear(),
            3 => token = vec![0xff],
            _ => l.write_timeout = Duration::ZERO,
        }
        let error = server::serve(
            listener,
            Config {
                token,
                asset_exists: Arc::new(|_| false),
                handler: echo(),
                limits: l,
            },
            recv,
        )
        .await
        .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
    }
}

#[tokio::test]
async fn internal_producer_remains_stricter_than_ordinary_authenticated_access() {
    let fixture = Fixture::start(echo(), limits()).await;
    let mut client = fixture.connect().await;
    let cases: Vec<(Vec<(&str, &str)>, bool)> = vec![
        (vec![("x-comandos-token", "fixture-token")], true),
        (vec![("authorization", "Bearer fixture-token")], false),
        (
            vec![
                ("x-comandos-token", "fixture-token"),
                ("origin", "http://localhost"),
            ],
            false,
        ),
        (
            vec![
                ("x-comandos-token", "fixture-token"),
                ("x-forwarded-for", "203.0.113.1"),
            ],
            false,
        ),
    ];
    for (headers, want) in cases {
        let value = json_response(
            client.request("POST", "/events/v2", &headers, "{}").await,
            200,
        )
        .await;
        assert_eq!(value["internalProducer"], want);
    }
    drop(client);
    fixture.finish().await;
}
