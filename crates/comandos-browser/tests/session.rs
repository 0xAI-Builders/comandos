use comandos_browser::session::{Catalog, Registry, SessionId, ToolBackend, handle_client};
use serde_json::{Value, json};
use std::{
    future::Future,
    pin::Pin,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

struct Backend {
    released: Arc<AtomicUsize>,
    delay_ms: u64,
}

impl ToolBackend for Backend {
    fn call(
        &self,
        _session: SessionId,
        _params: Value,
    ) -> Pin<Box<dyn Future<Output = Value> + Send + 'static>> {
        let delay = self.delay_ms;
        Box::pin(async move {
            if delay > 0 {
                tokio::time::sleep(Duration::from_millis(delay)).await;
            }
            json!({"content":[]})
        })
    }

    fn release(&self, _session: SessionId) -> Pin<Box<dyn Future<Output = ()> + Send + 'static>> {
        let released = self.released.clone();
        Box::pin(async move {
            released.fetch_add(1, Ordering::SeqCst);
        })
    }
}

struct Harness {
    reader: BufReader<tokio::io::ReadHalf<tokio::io::DuplexStream>>,
    writer: tokio::io::WriteHalf<tokio::io::DuplexStream>,
    released: Arc<AtomicUsize>,
    registry: Arc<Registry>,
}

impl Harness {
    async fn start() -> Self {
        Self::start_with_delay(0).await
    }

    async fn start_with_delay(delay_ms: u64) -> Self {
        let (client, server) = tokio::io::duplex(1024 * 1024);
        let (read, write) = tokio::io::split(client);
        let released = Arc::new(AtomicUsize::new(0));
        let backend = Arc::new(Backend {
            released: released.clone(),
            delay_ms,
        });
        let registry = Arc::new(Registry::new());
        tokio::spawn(handle_client(
            server,
            Arc::new(Catalog::from_value(
                &json!({"protocolVersion":"2025-11-25","tools":[{"name":"navigate_page"}]}),
            )),
            backend,
            registry.clone(),
        ));
        Self {
            reader: BufReader::new(read),
            writer: write,
            released,
            registry,
        }
    }

    async fn send_line(&mut self, line: &str) {
        self.writer.write_all(line.as_bytes()).await.unwrap();
        self.writer.write_all(b"\n").await.unwrap();
        self.writer.flush().await.unwrap();
    }

    async fn request(&mut self, line: &str) -> String {
        self.send_line(line).await;
        let mut out = String::new();
        self.reader.read_line(&mut out).await.unwrap();
        out.trim_end().to_owned()
    }

    async fn initialize(&mut self) {
        let _ = self.request(r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"clientInfo":{"name":"claude-code"}}}"#).await;
    }

    fn released(&self) -> usize {
        self.released.load(Ordering::SeqCst)
    }
}

#[tokio::test(flavor = "current_thread")]
async fn initialize_answers_python_shape_in_order() {
    let mut c = Harness::start().await;
    let out = c.request(r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"clientInfo":{"name":"claude-code"}}}"#).await;
    assert_eq!(
        out,
        concat!(
            r#"{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2025-11-25","capabilities":{"tools":{}},"#,
            r#""serverInfo":{"name":"comandos-browser-macmini","version":"1.0.0"},"instructions":"Browser runs on the Mac mini. Each connection owns isolated pages. Two browser sessions maximum; idle sessions expire after five minutes. Local laptop paths/localhost are not remote paths. Never start a local browser fallback."}}"#
        )
    );
}

#[tokio::test(flavor = "current_thread")]
async fn invalid_json_answers_32700() {
    let mut c = Harness::start().await;
    assert_eq!(
        c.request("not json").await,
        r#"{"jsonrpc":"2.0","id":null,"error":{"code":-32700,"message":"Invalid JSON"}}"#
    );
}

#[tokio::test(flavor = "current_thread")]
async fn non_object_params_answers_32600() {
    let mut c = Harness::start().await;
    assert_eq!(
        c.request(r#"{"id":3,"method":"ping","params":[]}"#).await,
        r#"{"jsonrpc":"2.0","id":null,"error":{"code":-32600,"message":"Invalid request"}}"#
    );
}

#[tokio::test(flavor = "current_thread")]
async fn call_before_initialize_is_rejected() {
    let mut c = Harness::start().await;
    assert_eq!(
        c.request(r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#)
            .await,
        r#"{"jsonrpc":"2.0","id":2,"error":{"code":-32000,"message":"Initialize the MCP connection first"}}"#
    );
}

#[tokio::test(flavor = "current_thread")]
async fn fifth_queued_request_gets_32002() {
    let mut c = Harness::start_with_delay(500).await;
    c.initialize().await;
    for id in 10..15 {
        c.send_line(&format!(
            r#"{{"jsonrpc":"2.0","id":{id},"method":"tools/call","params":{{"name":"navigate_page"}}}}"#
        ))
        .await;
    }
    let mut seen = String::new();
    for _ in 0..5 {
        let mut line = String::new();
        c.reader.read_line(&mut line).await.unwrap();
        seen.push_str(&line);
        if line.contains(r#""id":14"#) {
            break;
        }
    }
    assert!(seen.contains(r#""code":-32002,"message":"Too many queued requests""#));
}

#[tokio::test(flavor = "current_thread")]
async fn notification_without_id_is_ignored_and_client_name_is_truncated() {
    let mut c = Harness::start().await;
    c.send_line(r#"{"jsonrpc":"2.0","method":"notifications/cancelled","params":{}}"#)
        .await;
    let name = "ñ".repeat(100);
    let out = c
        .request(&format!(
            r#"{{"jsonrpc":"2.0","id":7,"method":"initialize","params":{{"clientInfo":{{"name":"{name}"}}}}}}"#
        ))
        .await;
    assert!(out.contains(r#""id":7"#));
    let snapshots = c.registry.snapshots().await;
    assert_eq!(snapshots[0].client_name.chars().count(), 80);
    assert_eq!(c.released(), 0);
}

#[tokio::test(flavor = "current_thread")]
async fn initialized_notification_does_not_wrap_pending_counter() {
    let mut c = Harness::start().await;
    let init = c
        .request(
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"clientInfo":{"name":"claude-code"}}}"#,
        )
        .await;
    assert!(init.contains(r#""id":1"#));
    c.send_line(r#"{"jsonrpc":"2.0","method":"notifications/initialized","params":{}}"#)
        .await;
    let tools = c
        .request(r#"{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}"#)
        .await;
    assert_eq!(
        tools,
        r#"{"jsonrpc":"2.0","id":2,"result":{"tools":[{"name":"navigate_page"}]}}"#
    );
    let call = c
        .request(
            r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"navigate_page"}}"#,
        )
        .await;
    assert_eq!(call, r#"{"jsonrpc":"2.0","id":3,"result":{"content":[]}}"#);
}
