use crate::wire::{Line, encode_message, read_line};
use comandos_core::json::parse_slice;
use getrandom::fill as random_fill;
use serde_json::{Map, Value, json};
use std::{
    collections::BTreeMap,
    future::Future,
    pin::Pin,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::{
    io::{AsyncRead, AsyncWrite, AsyncWriteExt, BufReader},
    sync::{Mutex, mpsc},
    task::JoinHandle,
};

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

pub const SERVER_NAME: &str = "comandos-browser-macmini";
pub const SERVER_VERSION: &str = "1.0.0";
pub const INSTRUCTIONS: &str = "Browser runs on the Mac mini. Each connection owns isolated pages. Two browser sessions maximum; idle sessions expire after five minutes. Local laptop paths/localhost are not remote paths. Never start a local browser fallback.";
pub const ERR_UNKNOWN_TOOL: &str = "Unknown browser tool";
pub const ERR_EXPIRED: &str = "Remote browser session expired or reset; its pages were closed. Start a new page before continuing. No local browser was launched.";
pub const ERR_BUSY: &str = "Mac mini browser is busy: two sessions are in use. Retry later, or close an unused browser MCP connection. No local browser was launched.";
pub const ERR_TIMEOUT: &str = "Remote browser tool timed out; its worker and browser were stopped before releasing capacity. Check the result before retrying an action.";
pub const ERR_UNAVAILABLE: &str = "Remote browser unavailable. Its worker was stopped; no local fallback was started. Reconnect or retry once the Mac service is healthy.";
pub const ERR_REMOTE_PREFIX: &str = "Remote tool failed: ";

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct SessionId(pub String);

impl SessionId {
    fn new() -> Self {
        let mut bytes = [0_u8; 16];
        if random_fill(&mut bytes).is_err() {
            let fallback = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0);
            bytes.copy_from_slice(&fallback.to_be_bytes());
        }
        let mut out = String::with_capacity(32);
        for byte in bytes {
            out.push_str(&format!("{byte:02x}"));
        }
        Self(out)
    }
}

#[derive(Clone)]
pub struct Catalog {
    pub protocol_version: String,
    pub tools: Value,
    pub tool_names: std::collections::BTreeSet<String>,
}

impl Catalog {
    pub fn from_value(value: &Value) -> Self {
        let protocol_version = value
            .get("protocolVersion")
            .and_then(Value::as_str)
            .unwrap_or("2025-11-25")
            .to_owned();
        let tools = value.get("tools").cloned().unwrap_or_else(|| json!([]));
        let tool_names = tools
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|tool| tool.get("name").and_then(Value::as_str).map(str::to_owned))
            .collect();
        Self {
            protocol_version,
            tools,
            tool_names,
        }
    }
}

pub trait ToolBackend: Send + Sync + 'static {
    fn call(&self, session: SessionId, params: Value) -> BoxFuture<'static, Value>;
    fn release(&self, session: SessionId) -> BoxFuture<'static, ()>;
}

#[derive(Clone)]
pub struct SessionSnapshot {
    pub id: SessionId,
    pub client_name: String,
    pub worker_pid: Option<u32>,
    pub busy: bool,
}

struct Entry {
    client_name: String,
    worker_pid: Option<u32>,
    busy: bool,
    notify: mpsc::Sender<Value>,
}

pub struct Registry {
    closing: AtomicBool,
    entries: Mutex<BTreeMap<SessionId, Entry>>,
}

impl Default for Registry {
    fn default() -> Self {
        Self::new()
    }
}

impl Registry {
    pub fn new() -> Self {
        Self {
            closing: AtomicBool::new(false),
            entries: Mutex::new(BTreeMap::new()),
        }
    }

    pub async fn admit(&self) -> Option<(SessionId, mpsc::Receiver<Value>)> {
        if self.closing.load(Ordering::SeqCst) {
            return None;
        }
        let mut entries = self.entries.lock().await;
        if self.closing.load(Ordering::SeqCst) {
            return None;
        }
        if entries.len() >= 128 {
            return None;
        }
        let (tx, rx) = mpsc::channel(16);
        let id = SessionId::new();
        entries.insert(
            id.clone(),
            Entry {
                client_name: "unknown".to_owned(),
                worker_pid: None,
                busy: false,
                notify: tx,
            },
        );
        Some((id, rx))
    }

    pub async fn remove(&self, id: &SessionId) {
        self.entries.lock().await.remove(id);
    }

    pub async fn set_client_name(&self, id: &SessionId, name: String) {
        if let Some(entry) = self.entries.lock().await.get_mut(id) {
            entry.client_name = name;
        }
    }

    pub async fn set_worker(&self, id: &SessionId, pid: Option<u32>, busy: bool) {
        if let Some(entry) = self.entries.lock().await.get_mut(id) {
            entry.worker_pid = pid;
            entry.busy = busy;
        }
    }

    pub async fn notify(&self, id: &SessionId, value: Value) {
        let tx = self
            .entries
            .lock()
            .await
            .get(id)
            .map(|entry| entry.notify.clone());
        if let Some(tx) = tx {
            let _ = tx.try_send(value);
        }
    }

    pub async fn snapshots(&self) -> Vec<SessionSnapshot> {
        self.entries
            .lock()
            .await
            .iter()
            .map(|(id, entry)| SessionSnapshot {
                id: id.clone(),
                client_name: entry.client_name.clone(),
                worker_pid: entry.worker_pid,
                busy: entry.busy,
            })
            .collect()
    }

    pub async fn count(&self) -> usize {
        self.entries.lock().await.len()
    }

    pub fn close(&self) {
        self.closing.store(true, Ordering::SeqCst);
    }

    pub fn is_closing(&self) -> bool {
        self.closing.load(Ordering::SeqCst)
    }
}

struct Writer<W> {
    inner: Arc<Mutex<W>>,
    closed: Arc<AtomicBool>,
}

impl<W> Clone for Writer<W> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            closed: self.closed.clone(),
        }
    }
}

impl<W: AsyncWrite + Unpin + Send + 'static> Writer<W> {
    fn new(writer: W) -> Self {
        Self {
            inner: Arc::new(Mutex::new(writer)),
            closed: Arc::new(AtomicBool::new(false)),
        }
    }

    async fn send(&self, message: &Value) {
        if self.closed() {
            return;
        }
        let bytes = match encode_message(message) {
            Ok(bytes) => bytes,
            Err(_) => {
                self.closed.store(true, Ordering::SeqCst);
                return;
            }
        };
        let mut writer = self.inner.lock().await;
        let result = tokio::time::timeout(Duration::from_secs(10), async {
            writer.write_all(&bytes).await?;
            writer.flush().await
        })
        .await;
        if !matches!(result, Ok(Ok(()))) {
            self.closed.store(true, Ordering::SeqCst);
        }
    }

    fn closed(&self) -> bool {
        self.closed.load(Ordering::SeqCst)
    }
}

enum Incoming {
    Request(Value),
    Invalid(Value),
}

struct AbortOnDrop<T>(JoinHandle<T>);

impl<T> AbortOnDrop<T> {
    fn new(handle: JoinHandle<T>) -> Self {
        Self(handle)
    }

    fn abort(&self) {
        self.0.abort();
    }
}

impl<T> Drop for AbortOnDrop<T> {
    fn drop(&mut self) {
        self.0.abort();
    }
}

pub async fn handle_client<S>(
    stream: S,
    catalog: Arc<Catalog>,
    backend: Arc<dyn ToolBackend>,
    registry: Arc<Registry>,
) where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let Some((id, mut notify_rx)) = registry.admit().await else {
        return;
    };
    let (reader, writer) = tokio::io::split(stream);
    let writer = Writer::new(writer);
    let (tx, mut rx) = mpsc::channel::<Value>(4);
    let pending = Arc::new(AtomicUsize::new(0));
    let serve_id = id.clone();
    let serve_writer = writer.clone();
    let serve_registry = registry.clone();
    let serve_backend = backend.clone();
    let serve_pending = pending.clone();
    let worker = AbortOnDrop::new(tokio::spawn(async move {
        let mut initialized = false;
        loop {
            tokio::select! {
                Some(message) = rx.recv() => {
                    let has_id = message.get("id").is_some();
                    dispatch_one(
                        serve_id.clone(),
                        &mut initialized,
                        message,
                        catalog.clone(),
                        serve_backend.clone(),
                        serve_registry.clone(),
                        serve_writer.clone(),
                    ).await;
                    if has_id {
                        serve_pending.fetch_sub(1, Ordering::SeqCst);
                    }
                    if serve_writer.closed() {
                        break;
                    }
                }
                Some(note) = notify_rx.recv() => {
                    serve_writer.send(&note).await;
                }
                else => break,
            }
        }
    }));

    let mut reader = BufReader::new(reader);
    let mut buf = Vec::new();
    loop {
        match read_line(&mut reader, &mut buf).await {
            Ok(Line::Message(raw)) => match classify(&raw) {
                Incoming::Invalid(err) => writer.send(&err).await,
                Incoming::Request(message) => {
                    if pending.load(Ordering::SeqCst) >= 4 {
                        if let Some(ident) = message.get("id").cloned() {
                            writer
                                .send(&error_response(ident, -32002, "Too many queued requests"))
                                .await;
                        }
                        continue;
                    }
                    if message.get("id").is_some() {
                        pending.fetch_add(1, Ordering::SeqCst);
                    }
                    match tx.try_send(message) {
                        Ok(()) => {}
                        Err(mpsc::error::TrySendError::Full(message)) => {
                            if message.get("id").is_some() {
                                pending.fetch_sub(1, Ordering::SeqCst);
                            }
                            if let Some(ident) = message.get("id").cloned() {
                                writer
                                    .send(&error_response(
                                        ident,
                                        -32002,
                                        "Too many queued requests",
                                    ))
                                    .await;
                            }
                        }
                        Err(mpsc::error::TrySendError::Closed(_)) => break,
                    }
                }
            },
            Ok(Line::TooLarge | Line::Eof) | Err(_) => break,
        }
        if writer.closed() {
            break;
        }
    }
    drop(tx);
    worker.abort();
    backend.release(id.clone()).await;
    registry.remove(&id).await;
}

fn classify(raw: &[u8]) -> Incoming {
    let Ok(message) = parse_slice(raw) else {
        return Incoming::Invalid(
            json!({"jsonrpc":"2.0","id":null,"error":{"code":-32700,"message":"Invalid JSON"}}),
        );
    };
    let Some(object) = message.as_object() else {
        return Incoming::Invalid(invalid_request());
    };
    if !object.get("method").is_some_and(Value::is_string) {
        return Incoming::Invalid(invalid_request());
    }
    if object
        .get("params")
        .is_some_and(|params| !params.is_object())
    {
        return Incoming::Invalid(invalid_request());
    }
    if !object.contains_key("id") {
        return Incoming::Request(message);
    }
    Incoming::Request(message)
}

fn invalid_request() -> Value {
    json!({"jsonrpc":"2.0","id":null,"error":{"code":-32600,"message":"Invalid request"}})
}

async fn dispatch_one<W: AsyncWrite + Unpin + Send + 'static>(
    session: SessionId,
    initialized: &mut bool,
    message: Value,
    catalog: Arc<Catalog>,
    backend: Arc<dyn ToolBackend>,
    registry: Arc<Registry>,
    writer: Writer<W>,
) {
    let Some(ident) = message.get("id").cloned() else {
        return;
    };
    let method = message.get("method").and_then(Value::as_str).unwrap_or("");
    let (result, error) = if method == "initialize" {
        let params = message.get("params").and_then(Value::as_object);
        let client = params
            .and_then(|p| p.get("clientInfo"))
            .and_then(Value::as_object)
            .and_then(|c| c.get("name"))
            .map(|v| {
                v.as_str()
                    .map(str::to_owned)
                    .unwrap_or_else(|| v.to_string())
                    .chars()
                    .take(80)
                    .collect::<String>()
            })
            .unwrap_or_else(|| "unknown".to_owned());
        registry.set_client_name(&session, client).await;
        *initialized = true;
        let mut result = Map::new();
        result.insert(
            "protocolVersion".to_owned(),
            Value::String(catalog.protocol_version.clone()),
        );
        result.insert("capabilities".to_owned(), json!({"tools":{}}));
        result.insert(
            "serverInfo".to_owned(),
            json!({"name":SERVER_NAME,"version":SERVER_VERSION}),
        );
        result.insert(
            "instructions".to_owned(),
            Value::String(INSTRUCTIONS.to_owned()),
        );
        (Some(Value::Object(result)), None)
    } else if !*initialized {
        (
            None,
            Some(json!({"code":-32000,"message":"Initialize the MCP connection first"})),
        )
    } else if method == "tools/list" {
        (Some(json!({"tools":catalog.tools})), None)
    } else if method == "ping" {
        (Some(json!({})), None)
    } else if method == "tools/call" {
        let params = message.get("params").cloned().unwrap_or_else(|| json!({}));
        (Some(backend.call(session, params).await), None)
    } else {
        (
            None,
            Some(json!({"code":-32601,"message":"Method not found"})),
        )
    };
    let response = match error {
        Some(error) => json!({"jsonrpc":"2.0","id":ident,"error":error}),
        None => json!({"jsonrpc":"2.0","id":ident,"result":result.unwrap_or(Value::Null)}),
    };
    writer.send(&response).await;
}

fn error_response(id: Value, code: i64, message: &str) -> Value {
    json!({"jsonrpc":"2.0","id":id,"error":{"code":code,"message":message}})
}

pub fn tool_error(message: &str) -> Value {
    json!({"content":[{"type":"text","text":message}],"isError":true})
}
