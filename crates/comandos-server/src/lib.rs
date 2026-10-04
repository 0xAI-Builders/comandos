//! Native dashboard transport. Construction never discovers live application state.
mod write_timeout;
use bytes::Bytes;
use comandos_core::dashboard_access as access;
use futures_util::FutureExt;
use http::{HeaderMap, Method, StatusCode};
use http_body::{Body, Frame, SizeHint};
use http_body_util::BodyExt;
use hyper::{body::Incoming, server::conn::http1, service::service_fn};
use hyper_util::rt::{TokioIo, TokioTimer};
use serde_json::Value;
use std::{
    convert::Infallible,
    future::Future,
    io,
    net::SocketAddr,
    panic::AssertUnwindSafe,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
    time::Duration,
};
use tokio::{
    net::TcpListener,
    sync::{Semaphore, mpsc, watch},
    task::JoinSet,
    time::timeout,
};

pub struct Request {
    pub method: Method,
    pub target: String,
    pub peer: SocketAddr,
    /// Header names are lowercase; duplicates of the same name retain order.
    /// Non-ASCII header bytes use HTTP's Latin-1 mapping, matching Python.
    pub headers: Vec<(String, String)>,
    /// GET has no parsed body; admitted POST/DELETE always has an object.
    pub data: Option<Value>,
    /// Direct local hook/service authentication, stricter than ordinary access.
    pub internal_producer: bool,
}

#[derive(Debug, Clone, Copy)]
pub enum HandlerError {
    Timeout,
    Failure,
}

pub enum ReplyBody {
    Bytes(Bytes),
    /// Producers own their task cancellation and must bound channel capacity
    /// and frame size. Dropping the response drops the receiver.
    Stream(mpsc::Receiver<io::Result<Bytes>>),
}
pub struct Reply {
    pub status: StatusCode,
    pub headers: HeaderMap,
    pub body: ReplyBody,
}
impl Reply {
    pub fn bytes(status: StatusCode, content_type: &'static str, body: impl Into<Bytes>) -> Self {
        let mut headers = HeaderMap::new();
        headers.insert(
            http::header::CONTENT_TYPE,
            http::HeaderValue::from_static(content_type),
        );
        Self {
            status,
            headers,
            body: ReplyBody::Bytes(body.into()),
        }
    }
    pub fn json(status: StatusCode, value: &Value) -> Result<Self, HandlerError> {
        let raw = comandos_core::json::response_dumps(value).map_err(|_| HandlerError::Failure)?;
        Ok(Self::bytes(status, "application/json", raw))
    }
}
pub type HandlerFuture = Pin<Box<dyn Future<Output = Result<Reply, HandlerError>> + Send>>;
pub type Handler = Arc<dyn Fn(Request) -> HandlerFuture + Send + Sync>;
/// Receives a validated absolute URL path without query or fragment. The
/// adapter supplies the installed asset root and follows its intended symlinks.
pub type AssetExists = Arc<dyn Fn(&str) -> bool + Send + Sync>;

#[derive(Clone)]
pub struct Limits {
    pub connections: usize,
    /// Maximum HTTP/1 connection read buffer, including request headers.
    pub header_bytes: usize,
    /// Aggregate admitted wire bytes, held through handler completion.
    /// Parsed JSON and responses have additional allocations owned by adapters.
    pub buffered_wire_bytes: u32,
    pub header_timeout: Duration,
    pub body_timeout: Duration,
    pub handler_timeout: Duration,
    /// Maximum wait for progress on a pending socket write/flush. Waiting for
    /// the next stream event does not consume this deadline.
    pub write_timeout: Duration,
    pub shutdown_grace: Duration,
}
pub struct Config {
    pub token: Vec<u8>,
    pub asset_exists: AssetExists,
    pub handler: Handler,
    pub limits: Limits,
}

struct State {
    config: Config,
    body_budget: Arc<Semaphore>,
}

struct OutputBody {
    source: ReplyBody,
    done: bool,
}
impl Body for OutputBody {
    type Data = Bytes;
    type Error = io::Error;
    fn poll_frame(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<io::Result<Frame<Bytes>>>> {
        if self.done {
            return Poll::Ready(None);
        }
        match &mut self.source {
            ReplyBody::Bytes(bytes) => {
                let data = std::mem::take(bytes);
                self.done = true;
                Poll::Ready((!data.is_empty()).then_some(Ok(Frame::data(data))))
            }
            ReplyBody::Stream(receiver) => match receiver.poll_recv(cx) {
                Poll::Ready(Some(Ok(bytes))) => Poll::Ready(Some(Ok(Frame::data(bytes)))),
                Poll::Ready(Some(Err(error))) => {
                    self.done = true;
                    Poll::Ready(Some(Err(error)))
                }
                Poll::Ready(None) => {
                    self.done = true;
                    Poll::Ready(None)
                }
                Poll::Pending => Poll::Pending,
            },
        }
    }
    fn is_end_stream(&self) -> bool {
        self.done || matches!(&self.source, ReplyBody::Bytes(b) if b.is_empty())
    }
    fn size_hint(&self) -> SizeHint {
        match &self.source {
            ReplyBody::Bytes(bytes) => SizeHint::with_exact(bytes.len() as u64),
            ReplyBody::Stream(_) if self.done => SizeHint::with_exact(0),
            ReplyBody::Stream(_) => SizeHint::default(),
        }
    }
}

fn response(reply: Reply, close: bool) -> http::Response<OutputBody> {
    let mut response = http::Response::new(OutputBody {
        source: reply.body,
        done: false,
    });
    *response.status_mut() = reply.status;
    *response.headers_mut() = reply.headers;
    // The maintained transport derives framing from the actual body. An adapter
    // cannot accidentally send conflicting framing or make another response cacheable.
    let headers = response.headers_mut();
    headers.remove(http::header::CONTENT_LENGTH);
    headers.remove(http::header::TRANSFER_ENCODING);
    headers.insert(
        http::header::CACHE_CONTROL,
        http::HeaderValue::from_static("no-store"),
    );
    if close {
        headers.insert(
            http::header::CONNECTION,
            http::HeaderValue::from_static("close"),
        );
    }
    response
}
fn reject(status: u16, message: &'static str, close: bool) -> http::Response<OutputBody> {
    let status = StatusCode::from_u16(status).expect("internal HTTP status");
    // Static internal errors cannot exceed the core encoder's depth limit.
    let reply =
        Reply::json(status, &serde_json::json!({"error":message})).expect("static JSON error");
    response(reply, close)
}

async fn body_bytes(mut body: Incoming, length: usize) -> Result<Vec<u8>, ()> {
    let mut bytes = Vec::with_capacity(length);
    while let Some(frame) = body.frame().await {
        let frame = frame.map_err(|_| ())?;
        if let Ok(data) = frame.into_data() {
            if data.len() > length.saturating_sub(bytes.len()) {
                return Err(());
            }
            bytes.extend_from_slice(&data);
        }
    }
    if bytes.len() != length {
        return Err(());
    }
    Ok(bytes)
}

async fn dispatch(
    request: hyper::Request<Incoming>,
    peer: SocketAddr,
    state: Arc<State>,
) -> http::Response<OutputBody> {
    let (parts, body) = request.into_parts();
    // Hyper has already rejected conflicting lengths. It removes lengths in the
    // presence of Transfer-Encoding, so reject that whole unsupported request mode.
    if parts.headers.contains_key(http::header::TRANSFER_ENCODING) {
        return reject(400, "JSON invalido", true);
    }
    let method = match parts.method.as_str() {
        "GET" => access::Method::Get,
        "POST" => access::Method::Post,
        "DELETE" => access::Method::Delete,
        _ => return reject(501, "Método no implementado", true),
    };
    let target = parts.uri.to_string();
    let peer_ip = peer.ip().to_string();
    let headers: Vec<(String, String)> = parts
        .headers
        .iter()
        .map(|(key, value)| {
            (
                key.as_str().to_owned(),
                value
                    .as_bytes()
                    .iter()
                    .map(|byte| char::from(*byte))
                    .collect(),
            )
        })
        .collect();
    let borrowed: Vec<(&str, &str)> = headers
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();
    let policy_request = access::Request {
        method,
        path: &target,
        peer_ip: Some(&peer_ip),
        headers: &borrowed,
    };
    let file_exists = if method == access::Method::Get
        && access::API_GET
            .iter()
            .any(|prefix| target.starts_with(prefix))
        && access::public_asset(&target, true).unwrap_or(false)
    {
        // Invalid hosts are rejected before consulting an injected filesystem fact.
        let hosts: Vec<_> = borrowed
            .iter()
            .filter(|(k, _)| k.eq_ignore_ascii_case("host"))
            .collect();
        if hosts.len() == 1 && access::authority_matches(access::AuthorityKind::Allowed, hosts[0].1)
        {
            match std::panic::catch_unwind(AssertUnwindSafe(|| {
                (state.config.asset_exists)(parts.uri.path())
            })) {
                Ok(exists) => exists,
                Err(_) => return reject(500, "Error interno del tablero", true),
            }
        } else {
            false
        }
    } else {
        false
    };
    let admission =
        match access::request_admission(&policy_request, &state.config.token, file_exists) {
            Ok(admission) => admission,
            Err(error) => {
                return reject(
                    error.status,
                    error.message,
                    error.close || !body.is_end_stream(),
                );
            }
        };
    let internal_producer = access::internal_producer(&policy_request, &state.config.token);
    if method == access::Method::Get && !body.is_end_stream() {
        // No GET endpoint consumes a body. Closing prevents it being mistaken for
        // a subsequent keep-alive request; this is explicit wire hardening.
        return reject(400, "JSON invalido", true);
    }
    let mut budget = None;
    let data = if let Some(length) = admission.body_length {
        let permits = u32::try_from(length).expect("core policy body cap");
        let Ok(permit) = state.body_budget.clone().try_acquire_many_owned(permits) else {
            return reject(503, "Tablero ocupado", true);
        };
        budget = Some(permit);
        let raw = match timeout(state.config.limits.body_timeout, body_bytes(body, length)).await {
            Ok(Ok(raw)) => raw,
            _ => return reject(400, "JSON invalido", true),
        };
        let parsed = if raw.is_empty() {
            Some(serde_json::json!({}))
        } else {
            comandos_core::json::workspace_loads_bytes(&raw)
        };
        if let Some(error) = access::parsed_body_admission(parsed.as_ref()) {
            return reject(error.status, error.message, error.close);
        }
        parsed
    } else {
        None
    };
    let request = Request {
        method: parts.method,
        target,
        peer,
        headers,
        data,
        internal_producer,
    };
    let future =
        match std::panic::catch_unwind(AssertUnwindSafe(|| (state.config.handler)(request))) {
            Ok(future) => future,
            Err(_) => return reject(500, "Error interno del tablero", true),
        };
    let result = timeout(
        state.config.limits.handler_timeout,
        AssertUnwindSafe(future).catch_unwind(),
    )
    .await;
    // Keep admission capacity through parsing and handler completion/cancellation.
    drop(budget);
    match result {
        Ok(Ok(Ok(reply))) => response(reply, false),
        Ok(Ok(Err(HandlerError::Timeout))) | Err(_) => {
            reject(504, "Tiempo de espera agotado", true)
        }
        _ => reject(500, "Error interno del tablero", true),
    }
}

/// Run only on the supplied listener. The caller owns bind addresses, token
/// loading, endpoint routing, and the shutdown signal. No implicit defaults.
pub async fn serve(
    listener: TcpListener,
    config: Config,
    mut shutdown: watch::Receiver<bool>,
) -> io::Result<()> {
    let limits = &config.limits;
    if config.token.is_empty()
        || std::str::from_utf8(&config.token).is_err()
        || limits.connections == 0
        || limits.connections > Semaphore::MAX_PERMITS
        || limits.buffered_wire_bytes == 0
        || !(8192..=8 * 1024 * 1024).contains(&limits.header_bytes)
        || [
            limits.header_timeout,
            limits.body_timeout,
            limits.handler_timeout,
            limits.write_timeout,
            limits.shutdown_grace,
        ]
        .iter()
        .any(Duration::is_zero)
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid transport configuration",
        ));
    }
    let connections = Arc::new(Semaphore::new(limits.connections));
    let state = Arc::new(State {
        body_budget: Arc::new(Semaphore::new(limits.buffered_wire_bytes as usize)),
        config,
    });
    let (stop_connections, connection_shutdown) = watch::channel(false);
    let mut tasks = JoinSet::new();
    let result = loop {
        if *shutdown.borrow() {
            break Ok(());
        }
        tokio::select! {
            changed = shutdown.changed() => {
                if changed.is_err() || *shutdown.borrow() { break Ok(()); }
            }
            _ = tasks.join_next(), if !tasks.is_empty() => {}
            accepted = listener.accept() => {
                let (stream,peer) = match accepted { Ok(pair)=>pair, Err(error)=>break Err(error) };
                let Ok(permit) = connections.clone().try_acquire_owned() else { drop(stream); continue; };
                let state = state.clone();
                let mut shutdown = connection_shutdown.clone();
                tasks.spawn(async move {
                    let _permit = permit;
                    let service_state = state.clone();
                    let service = service_fn(move |request| {
                        let state = service_state.clone();
                        async move { Ok::<_,Infallible>(dispatch(request,peer,state).await) }
                    });
                    let mut builder = http1::Builder::new();
                    builder.keep_alive(true).timer(TokioTimer::new())
                        .header_read_timeout(state.config.limits.header_timeout)
                        .max_headers(100).max_buf_size(state.config.limits.header_bytes)
                        .pipeline_flush(false);
                    let stream = write_timeout::WriteTimeout::new(stream, state.config.limits.write_timeout);
                    let connection = builder.serve_connection(TokioIo::new(stream),service);
                    tokio::pin!(connection);
                    tokio::select! {
                        _ = &mut connection => {}
                        _ = shutdown.changed() => {
                            connection.as_mut().graceful_shutdown();
                            let _ = connection.await;
                        }
                    }
                });
            }
        }
    };
    drop(listener);
    let _ = stop_connections.send(true);
    if timeout(state.config.limits.shutdown_grace, async {
        while tasks.join_next().await.is_some() {}
    })
    .await
    .is_err()
    {
        tasks.abort_all();
        while tasks.join_next().await.is_some() {}
    }
    result
}
