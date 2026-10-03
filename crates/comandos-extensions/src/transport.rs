//! Bounded raw JSON transport preserves extension fields through the facade.
use crate::{Result, auth::Auth, command, expand_vars, python_string};
use futures_util::StreamExt;
use reqwest::{
    Client, Method, Response,
    header::{HeaderMap, HeaderName, HeaderValue},
};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};
use tokio::{
    io::{AsyncBufRead, AsyncBufReadExt, AsyncWriteExt, BufReader},
    sync::{mpsc, oneshot, watch},
    task::JoinHandle,
};

pub const MAX_MESSAGE: usize = 8 * 1024 * 1024;
pub const MAX_INFLIGHT: usize = 64;
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(60);
type Pending = Arc<Mutex<HashMap<u64, oneshot::Sender<Value>>>>;

pub async fn line<R: AsyncBufRead + Unpin>(reader: &mut R) -> Result<Option<Vec<u8>>> {
    let mut out = Vec::new();
    loop {
        let bytes = reader.fill_buf().await.map_err(|_| "Stream read failed")?;
        if bytes.is_empty() {
            return if out.is_empty() {
                Ok(None)
            } else {
                Ok(Some(out))
            };
        }
        let count = bytes
            .iter()
            .position(|b| *b == b'\n')
            .map(|p| p + 1)
            .unwrap_or(bytes.len());
        if out.len() + count > MAX_MESSAGE {
            return Err("Message too large".into());
        }
        out.extend_from_slice(&bytes[..count]);
        reader.consume(count);
        if out.last() == Some(&b'\n') {
            return Ok(Some(out));
        }
    }
}

struct Registration {
    pending: Pending,
    id: u64,
    cancellations: mpsc::Sender<u64>,
}
impl Drop for Registration {
    fn drop(&mut self) {
        if self.pending.lock().unwrap().remove(&self.id).is_some() {
            let _ = self.cancellations.try_send(self.id);
        }
    }
}
struct ChildGroup(i32);
impl Drop for ChildGroup {
    fn drop(&mut self) {
        let _ = nix::sys::signal::killpg(
            nix::unistd::Pid::from_raw(self.0),
            nix::sys::signal::Signal::SIGKILL,
        );
    }
}

fn dispatch(value: Value, pending: &Pending) {
    if let Some(id) = value.get("id").and_then(Value::as_u64)
        && (value.get("result").is_some() || value.get("error").is_some())
        && let Some(sender) = pending.lock().unwrap().remove(&id)
    {
        let _ = sender.send(value);
    }
}

fn upstream_reply(value: &Value) -> Option<Value> {
    let method = value.get("method")?.as_str()?;
    let id = value.get("id")?;
    Some(if method == "ping" {
        json!({"jsonrpc":"2.0","id":id,"result":{}})
    } else {
        json!({"jsonrpc":"2.0","id":id,"error":{"code":-32601,"message":"Unsupported upstream request"}})
    })
}

#[derive(Clone)]
struct Http {
    client: Client,
    headers: HeaderMap,
    endpoint: String,
    auth: Option<Auth>,
    session: Arc<Mutex<Option<String>>>,
    protocol: Arc<Mutex<Option<String>>>,
}
impl Http {
    async fn send(&self, method: Method, url: &str, body: Option<&Value>) -> Result<Response> {
        if let Some(auth) = &self.auth
            && !auth.same_origin(url)
        {
            return Err("Credential origin mismatch".into());
        }
        let mut token = match &self.auth {
            Some(a) => a.access_token(None).await?,
            None => None,
        };
        for attempt in 0..2 {
            let mut headers = self.headers.clone();
            if let Some(token) = &token {
                headers.insert(
                    "authorization",
                    HeaderValue::from_str(&format!("Bearer {token}"))
                        .map_err(|_| "Invalid authorization")?,
                );
            }
            if let Some(session) = self.session.lock().unwrap().as_ref() {
                headers.insert(
                    "mcp-session-id",
                    HeaderValue::from_str(session).map_err(|_| "Invalid session")?,
                );
            }
            if let Some(protocol) = self.protocol.lock().unwrap().as_ref() {
                headers.insert(
                    "mcp-protocol-version",
                    HeaderValue::from_str(protocol).map_err(|_| "Invalid protocol")?,
                );
            }
            let mut req = self.client.request(method.clone(), url).headers(headers);
            if let Some(body) = body {
                req = req.json(body);
            }
            let response = req.send().await.map_err(|_| "HTTP request failed")?;
            if response.status() == 401
                && attempt == 0
                && let (Some(auth), Some(rejected)) = (&self.auth, &token)
            {
                let new = auth.access_token(Some(rejected.clone())).await?;
                if new.is_some() && new != token {
                    token = new;
                    continue;
                }
            }
            if response.status().is_redirection() {
                return Err("HTTP redirect rejected".into());
            }
            if !response.status().is_success() {
                return Err("HTTP request rejected".into());
            }
            if let Some(session) = response.headers().get("mcp-session-id") {
                *self.session.lock().unwrap() =
                    Some(session.to_str().map_err(|_| "Invalid session")?.into());
            }
            return Ok(response);
        }
        Err("Authorization rejected".into())
    }
}

/// Incremental SSE parser: CRLF, multiline data, comments and empty events.
#[derive(Default)]
struct Sse {
    buffer: Vec<u8>,
    event: String,
    data: String,
    skip_lf: bool,
    seen_line: bool,
}
impl Sse {
    fn feed(&mut self, bytes: &[u8]) -> Result<Vec<(String, String)>> {
        if self.buffer.len() + self.data.len() + bytes.len() > MAX_MESSAGE {
            return Err("SSE event too large".into());
        }
        self.buffer.extend_from_slice(bytes);
        let mut events = Vec::new();
        loop {
            if self.skip_lf && !self.buffer.is_empty() {
                if self.buffer[0] == b'\n' {
                    self.buffer.remove(0);
                }
                self.skip_lf = false;
            }
            let Some(end) = self.buffer.iter().position(|b| matches!(b, b'\r' | b'\n')) else {
                break;
            };
            self.skip_lf = self.buffer[end] == b'\r';
            let raw: Vec<u8> = self.buffer.drain(..=end).collect();
            let line =
                std::str::from_utf8(&raw[..raw.len() - 1]).map_err(|_| "Invalid SSE encoding")?;
            let line = if self.seen_line {
                line
            } else {
                line.strip_prefix('\u{feff}').unwrap_or(line)
            };
            self.seen_line = true;
            if line.is_empty() {
                if !self.data.is_empty() {
                    self.data.pop();
                    events.push((
                        std::mem::take(&mut self.event),
                        std::mem::take(&mut self.data),
                    ));
                }
                self.event.clear();
            } else if !line.starts_with(':') {
                let (key, value) = line.split_once(':').unwrap_or((line, ""));
                let value = value.strip_prefix(' ').unwrap_or(value);
                match key {
                    "event" => self.event = value.into(),
                    "data" => {
                        self.data.push_str(value);
                        self.data.push('\n');
                    }
                    _ => {}
                }
            }
        }
        Ok(events)
    }
}

pub struct Transport {
    http: Option<Http>,
    sender: Option<mpsc::Sender<Value>>,
    post_url: Option<String>,
    pending: Pending,
    next: AtomicU64,
    cancellations: mpsc::Sender<u64>,
    pub closed: watch::Receiver<bool>,
    tasks: Vec<JoinHandle<()>>,
    child: Option<tokio::process::Child>,
    group: Option<ChildGroup>,
}
impl Drop for Transport {
    fn drop(&mut self) {
        for task in &self.tasks {
            task.abort();
        }
        self.pending.lock().unwrap().clear();
    }
}
impl Transport {
    pub async fn connect(home: &Path, name: &str, spec: &Value) -> Result<Self> {
        let pending: Pending = Arc::new(Mutex::new(HashMap::new()));
        let (dead, closed) = watch::channel(false);
        let (cancellations, mut cancelled) = mpsc::channel::<u64>(MAX_INFLIGHT);
        let mut this = Self {
            cancellations,
            http: None,
            sender: None,
            post_url: None,
            pending,
            next: AtomicU64::new(1),
            closed,
            tasks: vec![],
            child: None,
            group: None,
        };
        if spec["command"].as_str().is_some() {
            use std::os::unix::process::CommandExt;
            let mut cmd = command(spec, false)?;
            cmd.stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::piped())
                .process_group(0);
            let mut child = tokio::process::Command::from(cmd)
                .kill_on_drop(true)
                .spawn()
                .map_err(|_| "Extension command failed")?;
            let mut output = BufReader::new(child.stdout.take().ok_or("Missing child stdout")?);
            let mut input = child.stdin.take().ok_or("Missing child stdin")?;
            let (sender, mut receiver) = mpsc::channel::<Value>(MAX_INFLIGHT);
            this.sender = Some(sender.clone());
            let pending = this.pending.clone();
            let died = dead.clone();
            this.tasks.push(tokio::spawn(async move {
                while let Ok(Some(bytes)) = line(&mut output).await {
                    let Ok(value) = comandos_core::json::parse_slice(&bytes) else {
                        break;
                    };
                    if value.get("method").is_some() && value.get("id").is_some() {
                        if let Some(reply) = upstream_reply(&value)
                            && sender.send(reply).await.is_err()
                        {
                            break;
                        }
                    } else {
                        dispatch(value, &pending);
                    }
                }
                pending.lock().unwrap().clear();
                let _ = died.send(true);
            }));
            this.tasks.push(tokio::spawn(async move {
                while let Some(value) = receiver.recv().await {
                    let mut bytes = value.to_string().into_bytes();
                    bytes.push(b'\n');
                    if input.write_all(&bytes).await.is_err() {
                        break;
                    }
                }
                let _ = dead.send(true);
            }));
            this.group = child.id().map(|id| ChildGroup(id as i32));
            this.child = Some(child);
        } else {
            let endpoint = spec["url"].as_str().ok_or("Missing endpoint")?.to_owned();
            let url = url::Url::parse(&endpoint).map_err(|_| "Invalid endpoint")?;
            if !matches!(url.scheme(), "http" | "https")
                || !url.username().is_empty()
                || url.password().is_some()
            {
                return Err("Invalid endpoint".into());
            }
            let mut headers = HeaderMap::new();
            headers.insert(
                "accept",
                HeaderValue::from_static("application/json, text/event-stream"),
            );
            if let Some(values) = spec.get("headers") {
                for (key, value) in values.as_object().ok_or("Invalid headers")? {
                    insert_header(&mut headers, key, &expand_vars(&python_string(value)))?;
                }
            }
            if let Some(var) = spec["bearer_token_env_var"].as_str()
                && let Ok(value) = std::env::var(var)
                && !value.is_empty()
            {
                insert_header(&mut headers, "Authorization", &format!("Bearer {value}"))?;
            }
            if let Some(values) = spec.get("env_http_headers") {
                for (key, var) in values.as_object().ok_or("Invalid headers")? {
                    if let Some(value) = var.as_str().and_then(|v| std::env::var(v).ok()) {
                        insert_header(&mut headers, key, &value)?;
                    }
                }
            }
            let auth = if headers.contains_key("authorization") {
                None
            } else {
                Auth::new(home.into(), name.into(), endpoint.clone())?
            };
            let builder = Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .connect_timeout(Duration::from_secs(20));
            let builder = if spec["transport"] == "sse" {
                builder.read_timeout(Duration::from_secs(300))
            } else {
                builder.timeout(Duration::from_secs(90))
            };
            let http = Http {
                client: builder.build().map_err(|_| "HTTP client failed")?,
                headers,
                endpoint: endpoint.clone(),
                auth,
                session: Arc::new(Mutex::new(None)),
                protocol: Arc::new(Mutex::new(None)),
            };
            if spec["transport"] == "sse" {
                let response = http.send(Method::GET, &endpoint, None).await?;
                let pending = this.pending.clone();
                let (endpoint_tx, endpoint_rx) = oneshot::channel();
                let event_http = http.clone();
                let base = url.clone();
                this.tasks.push(tokio::spawn(async move {
                    let mut endpoint_tx = Some(endpoint_tx);
                    let mut message_url = None;
                    let mut stream = response.bytes_stream();
                    let mut parser = Sse::default();
                    while let Some(Ok(bytes)) = stream.next().await {
                        let Ok(events) = parser.feed(&bytes) else {
                            break;
                        };
                        for (event, data) in events {
                            if event == "endpoint" {
                                if let Some(sender) = endpoint_tx.take() {
                                    if let Ok(parsed) = base.join(&data)
                                        && parsed.origin() == base.origin()
                                    {
                                        message_url = Some(parsed.to_string());
                                    }
                                    let _ = sender.send(data);
                                }
                            } else if let Ok(value) = comandos_core::json::parse_value(&data) {
                                if let (Some(reply), Some(url)) =
                                    (upstream_reply(&value), &message_url)
                                {
                                    let _ = tokio::time::timeout(
                                        REQUEST_TIMEOUT,
                                        event_http.send(Method::POST, url, Some(&reply)),
                                    )
                                    .await;
                                } else {
                                    dispatch(value, &pending);
                                }
                            }
                        }
                    }
                    pending.lock().unwrap().clear();
                    let _ = dead.send(true);
                }));
                let destination = tokio::time::timeout(Duration::from_secs(20), endpoint_rx)
                    .await
                    .map_err(|_| "SSE connection timed out")?
                    .map_err(|_| "SSE endpoint unavailable")?;
                let post = url.join(&destination).map_err(|_| "Invalid SSE endpoint")?;
                // Restrict static headers too: an endpoint event is never authority to leak them.
                if post.origin() != url.origin() {
                    return Err("SSE origin mismatch".into());
                }
                this.post_url = Some(post.into());
            } else {
                // Keep sender alive for HTTP, whose failures are reported per request.
                this.tasks.push(tokio::spawn(async move {
                    let _dead = dead;
                    std::future::pending::<()>().await;
                }));
            }
            this.http = Some(http);
        }
        let http = this.http.clone();
        let sender = this.sender.clone();
        let post = this.post_url.clone();
        this.tasks.push(tokio::spawn(async move {
            while let Some(id)=cancelled.recv().await {
                let value=json!({"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":id}});
                let _=tokio::time::timeout(Duration::from_secs(2),async {
                    if let Some(http)=&http {let _=http.send(Method::POST,post.as_deref().unwrap_or(&http.endpoint),Some(&value)).await;}
                    else if let Some(sender)=&sender {let _=sender.send(value).await;}
                }).await;
            }
        }));
        Ok(this)
    }

    pub async fn request(&self, mut value: Value) -> Result<Value> {
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        value["id"] = json!(id);
        let (tx, rx) = oneshot::channel();
        {
            let mut pending = self.pending.lock().unwrap();
            if pending.len() >= MAX_INFLIGHT {
                return Err("Too many upstream requests".into());
            }
            pending.insert(id, tx);
        }
        let _registration = Registration {
            pending: self.pending.clone(),
            id,
            cancellations: self.cancellations.clone(),
        };
        let result = tokio::time::timeout(REQUEST_TIMEOUT, async {
            if let Some(http) = &self.http {
                let url = self.post_url.as_deref().unwrap_or(&http.endpoint);
                let response = http.send(Method::POST, url, Some(&value)).await?;
                if self.post_url.is_none() {
                    let sse = response
                        .headers()
                        .get("content-type")
                        .and_then(|v| v.to_str().ok())
                        .is_some_and(|v| v.starts_with("text/event-stream"));
                    let mut stream = response.bytes_stream();
                    let mut data = Vec::new();
                    let mut parser = Sse::default();
                    while let Some(chunk) = stream.next().await {
                        let chunk = chunk.map_err(|_| "HTTP body failed")?;
                        if sse {
                            for (_, data) in parser.feed(&chunk)? {
                                let result: Value = comandos_core::json::parse_value(&data)
                                    .map_err(|_| "Invalid upstream response")?;
                                if let Some(reply) = upstream_reply(&result) {
                                    http.send(Method::POST, url, Some(&reply)).await?;
                                    continue;
                                }
                                if result["id"] == id
                                    && (result.get("result").is_some()
                                        || result.get("error").is_some())
                                {
                                    return Ok(result);
                                }
                            }
                        } else {
                            if data.len() + chunk.len() > MAX_MESSAGE {
                                return Err("Response too large".into());
                            }
                            data.extend_from_slice(&chunk);
                        }
                    }
                    if !sse {
                        let result: Value = comandos_core::json::parse_slice(&data)
                            .map_err(|_| "Invalid upstream response")?;
                        if result["id"] == id
                            && (result.get("result").is_some() || result.get("error").is_some())
                        {
                            return Ok(result);
                        }
                    }
                    return Err("Missing upstream response".into());
                }
            } else {
                self.sender
                    .as_ref()
                    .ok_or("Transport closed")?
                    .send(value)
                    .await
                    .map_err(|_| "Transport closed")?;
            }
            rx.await.map_err(|_| "Upstream closed".into())
        })
        .await
        .map_err(|_| "Upstream request timed out")?;
        if result.is_ok() {
            self.pending.lock().unwrap().remove(&id);
        }
        result
    }

    pub async fn notify(&self, value: Value) -> Result<()> {
        tokio::time::timeout(REQUEST_TIMEOUT, async {
            if let Some(http) = &self.http {
                http.send(
                    Method::POST,
                    self.post_url.as_deref().unwrap_or(&http.endpoint),
                    Some(&value),
                )
                .await?;
            } else {
                self.sender
                    .as_ref()
                    .ok_or("Transport closed")?
                    .send(value)
                    .await
                    .map_err(|_| "Transport closed")?;
            }
            Ok(())
        })
        .await
        .map_err(|_| "Notification timed out")?
    }
    pub fn set_protocol(&self, version: &str) {
        if let Some(http) = &self.http {
            *http.protocol.lock().unwrap() = Some(version.into());
        }
    }
    pub async fn shutdown(&mut self) {
        for task in &self.tasks {
            task.abort();
        }
        self.group.take();
        if let Some(child) = &mut self.child {
            let _ = child.kill().await;
            let _ = child.wait().await;
        }
        if let Some(http) = &self.http
            && self.post_url.is_none()
            && http.session.lock().unwrap().is_some()
        {
            let _ = tokio::time::timeout(
                Duration::from_secs(2),
                http.send(Method::DELETE, &http.endpoint, None),
            )
            .await;
        }
    }
}
fn insert_header(headers: &mut HeaderMap, key: &str, value: &str) -> Result<()> {
    headers.insert(
        HeaderName::from_bytes(key.as_bytes()).map_err(|_| "Invalid header")?,
        HeaderValue::from_str(value).map_err(|_| "Invalid header")?,
    );
    Ok(())
}
