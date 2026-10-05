//! Real access-policy -> worker -> SQLite integration, in sandbox port0 only.
use bytes::Bytes;
use comandos_server::{
    Config, Limits, Reply,
    blocking::BlockingWorker,
    events_routes::{EventRoutes, Facts},
};
use http::{Request, StatusCode};
use http_body_util::{BodyExt, Full};
use hyper::{body::Incoming, client::conn::http1};
use hyper_util::rt::TokioIo;
use rusqlite::Connection;
use serde_json::{Value, json};
use std::{
    io,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};
use tokio::{
    net::{TcpListener, TcpStream},
    sync::watch,
    task::JoinHandle,
    time::timeout,
};

const WAIT: Duration = Duration::from_secs(3);
struct FixedFacts {
    next: u64,
}
impl Facts for FixedFacts {
    fn now_ms(&mut self) -> comandos_store::Result<u64> {
        Ok(123_456)
    }
    fn fresh_id(&mut self, prefix: &str) -> comandos_store::Result<String> {
        self.next += 1;
        Ok(format!("{prefix}-socket-{}", self.next))
    }
    fn process_start(&mut self, pid: &str) -> Option<String> {
        (pid == "123").then(|| "42".into())
    }
}
struct Fixture {
    address: std::net::SocketAddr,
    directory: PathBuf,
    observer: Option<Connection>,
    stop: watch::Sender<bool>,
    task: Option<JoinHandle<io::Result<()>>>,
    worker: Option<BlockingWorker>,
}
impl Fixture {
    async fn start(binding: bool) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let directory = std::env::temp_dir().join(format!(
            "events-http-socket-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&directory).unwrap();
        let conn = comandos_store::state::connect(&directory.join("state.sqlite3")).unwrap();
        comandos_store::state::migrate(&conn, comandos_store::state::MIGRATIONS, 0.0).unwrap();
        if binding {
            let document = json!({"schema":1,"groups":[{"id":"g","tree":{"type":"tab","tabId":"sess"}}],"tabs":{"sess":{"session":"sess","paneKeys":["pk1"]}},"bindings":{"pk1":{"session":"sess","paneId":"%3","pid":123,"startTime":42}}});
            conn.execute(
                "INSERT INTO workspace_current VALUES(1,1,?,0)",
                [document.to_string()],
            )
            .unwrap();
        }
        let legacy = directory.join("events.jsonl");
        std::fs::write(
            &legacy,
            "{\"project\":\"old\",\"status\":\"done\",\"ts\":10}\n",
        )
        .unwrap();
        let observer = comandos_store::state::connect(&directory.join("state.sqlite3")).unwrap();
        let mut routes = EventRoutes::new(legacy, FixedFacts { next: 0 });
        let worker =
            BlockingWorker::start(2, move |request| match routes.handle(&conn, &request)? {
                Some(reply) => Ok(reply),
                None => Reply::json(StatusCode::NOT_FOUND, &json!({"error":"No encontrado"})),
            })
            .unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (stop, shutdown) = watch::channel(false);
        let limits = Limits {
            connections: 8,
            header_bytes: 65536,
            buffered_wire_bytes: 20_000_000,
            header_timeout: WAIT,
            body_timeout: Duration::from_millis(150),
            handler_timeout: WAIT,
            write_timeout: Duration::from_millis(150),
            shutdown_grace: Duration::from_millis(100),
        };
        let config = Config {
            token: b"socket-fixture-token".to_vec(),
            token_file: None,
            asset_exists: Arc::new(|_| false),
            handler: worker.handler(),
            limits,
        };
        let task = tokio::spawn(comandos_server::serve(listener, config, shutdown));
        Self {
            address,
            directory,
            observer: Some(observer),
            stop,
            task: Some(task),
            worker: Some(worker),
        }
    }
    fn count(&self, table: &str) -> i64 {
        assert!(["events", "event_receipts", "work_marks", "workspace_meta"].contains(&table));
        self.observer
            .as_ref()
            .unwrap()
            .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
            .unwrap()
    }
    async fn call(
        &self,
        method: &str,
        target: &str,
        headers: &[(&str, &str)],
        body: impl Into<Bytes>,
    ) -> (u16, Value) {
        let stream = timeout(WAIT, TcpStream::connect(self.address))
            .await
            .unwrap()
            .unwrap();
        let (mut sender, connection) = http1::handshake(TokioIo::new(stream)).await.unwrap();
        let client = tokio::spawn(async move {
            let _ = connection.await;
        });
        let body = body.into();
        let mut request = Request::builder()
            .method(method)
            .uri(target)
            .header("host", "localhost")
            .header("connection", "close");
        if method != "GET" {
            request = request.header("content-length", body.len());
        }
        for (key, value) in headers {
            request = request.header(*key, *value);
        }
        let response: http::Response<Incoming> = timeout(
            WAIT,
            sender.send_request(request.body(Full::new(body)).unwrap()),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(response.headers()["cache-control"], "no-store");
        let status = response.status().as_u16();
        let raw = timeout(WAIT, response.into_body().collect())
            .await
            .unwrap()
            .unwrap()
            .to_bytes();
        let value = comandos_core::json::workspace_loads_bytes(&raw).unwrap();
        timeout(WAIT, client).await.unwrap().unwrap();
        (status, value)
    }
    async fn post(&self, target: &str, headers: &[(&str, &str)], value: &Value) -> (u16, Value) {
        self.call("POST", target, headers, serde_json::to_vec(value).unwrap())
            .await
    }
    async fn finish(mut self) {
        self.stop.send(true).unwrap();
        timeout(WAIT, self.task.take().unwrap())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        timeout(WAIT, self.worker.take().unwrap().shutdown())
            .await
            .unwrap()
            .unwrap();
        self.observer.take();
        std::fs::remove_dir_all(&self.directory).unwrap();
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = self.stop.send(true);
        if let Some(task) = self.task.take() {
            task.abort();
        }
        self.worker.take();
        self.observer.take();
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}
fn hook(kind: &str) -> Value {
    json!({"hookEvent":kind,"agent":"codex","session":"sess","pane":"%3","panePid":"123","turnId":"turn-1","occurredAtMs":1000})
}
const TOKEN: [(&str, &str); 1] = [("x-comandos-token", "socket-fixture-token")];

#[tokio::test]
async fn access_and_internal_producer_denials_never_import_or_write_real_sqlite() {
    let fixture = Fixture::start(false).await;
    for (headers, status) in [
        (vec![], 403),
        (vec![("x-forwarded-for", "203.0.113.4")], 401),
        (vec![("x-forwarded-for", "203.0.113.4"), TOKEN[0]], 403),
        (vec![("origin", "http://localhost"), TOKEN[0]], 403),
        (vec![("authorization", "Bearer socket-fixture-token")], 403),
    ] {
        assert_eq!(
            fixture.post("/events/v2", &headers, &hook("Stop")).await.0,
            status
        );
        assert_eq!(fixture.count("events"), 0);
        assert_eq!(fixture.count("event_receipts"), 0);
        assert_eq!(fixture.count("workspace_meta"), 0);
    }
    fixture.finish().await;
}

#[tokio::test]
async fn authenticated_http_intake_resolves_process_binding_and_updates_human_marks_once() {
    let fixture = Fixture::start(true).await;
    assert_eq!(
        fixture
            .post(
                "/work-marks",
                &[],
                &json!({"scope":"pane","key":"pk1","value":"resolved","expectedRevision":0})
            )
            .await
            .0,
        200
    );
    let (status, prompt) = fixture
        .post("/events/v2", &TOKEN, &hook("UserPromptSubmit"))
        .await;
    assert_eq!(status, 200);
    assert_eq!(prompt["event"]["paneKey"], "pk1");
    assert_eq!(prompt["event"]["processKey"], "123-42");
    fixture.post("/events/v2", &TOKEN, &hook("Stop")).await;
    let (_, duplicate) = fixture.post("/events/v2", &TOKEN, &hook("Stop")).await;
    assert_eq!(duplicate["event"]["duplicate"], true);
    let (_, body) = fixture.call("GET", "/work-marks", &[], Bytes::new()).await;
    assert_eq!(body["marks"][0]["mark"], "none");
    assert_eq!(body["marks"][0]["revision"], 2);
    assert_eq!(
        body["panes"],
        json!([{"paneKey":"pk1","session":"sess","paneId":"%3"}])
    );
    assert_eq!(body["activity"]["pane:pk1"]["state"], "completed");
    assert_eq!(fixture.count("events"), 3);
    assert_eq!(fixture.count("event_receipts"), 4);
    let (_, page) = fixture
        .call(
            "GET",
            "/events/v2?after=&after=1&limit=1&turns=1",
            &[],
            Bytes::new(),
        )
        .await;
    assert_eq!(page["events"][0]["kind"], "prompt_accepted");
    assert_eq!(page["nextAfter"], 2);
    assert_eq!(page["latest"], 3);
    fixture.finish().await;
}

#[tokio::test]
async fn query_get_imports_once_while_query_post_remains_unknown_and_effect_free() {
    let fixture = Fixture::start(false).await;
    assert_eq!(
        fixture
            .post("/events/v2?turns=1", &TOKEN, &hook("Stop"))
            .await
            .0,
        404
    );
    assert_eq!(
        fixture
            .post(
                "/work-marks?extra=1",
                &[],
                &json!({"scope":"pane","key":"pk","value":"frozen","expectedRevision":0})
            )
            .await
            .0,
        404
    );
    assert_eq!(fixture.count("events"), 0);
    assert_eq!(fixture.count("work_marks"), 0);
    assert_eq!(fixture.count("workspace_meta"), 0);
    let (_, first) = fixture
        .call("GET", "/events/v2?after=0", &[], Bytes::new())
        .await;
    assert_eq!(first["events"].as_array().unwrap().len(), 1);
    assert_eq!(first["events"][0]["destination"], "none");
    let (_, second) = fixture
        .call("GET", "/events/v2?after=1", &[], Bytes::new())
        .await;
    assert_eq!(second["events"], json!([]));
    assert_eq!(second["latest"], 1);
    assert_eq!(fixture.count("events"), 1);
    fixture.finish().await;
}

#[tokio::test]
async fn transport_json_rejections_keep_adapter_database_untouched() {
    let fixture = Fixture::start(false).await;
    for raw in ["{", "[]", "null"] {
        assert_eq!(fixture.call("POST", "/events/v2", &TOKEN, raw).await.0, 400);
        assert_eq!(fixture.count("events"), 0);
        assert_eq!(fixture.count("workspace_meta"), 0);
    }
    fixture.finish().await;
}
