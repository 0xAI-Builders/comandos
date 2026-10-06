//! Desktop loopback HTTP boundary. Cancellation shuts down only registered
//! sockets belonging to an isolated polling client, never a user's service.
use crate::config::{RunMode, loopback_only, sandbox_dash};
use serde_json::Value;
use std::{
    collections::HashMap,
    io::{ErrorKind, Read, Write},
    net::{Shutdown, SocketAddr, TcpStream},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};
const MAX_RESPONSE: usize = 8 * 1024 * 1024;
const MAX_HEADERS: usize = 65536;
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DashError {
    Disconnected,
    ShadowRefused,
    Cancelled,
    Invalid(String),
    Transport(String),
    Json(String),
    HttpStatus(u16, Value),
}
#[derive(Default)]
struct Connections {
    cancelled: AtomicBool,
    next: AtomicU64,
    sockets: Mutex<HashMap<u64, TcpStream>>,
}
#[derive(Clone)]
pub struct DashClient {
    base: Option<String>,
    address: Option<SocketAddr>,
    mode: RunMode,
    connections: Arc<Connections>,
}
struct Registration {
    id: u64,
    connections: Arc<Connections>,
}
impl Drop for Registration {
    fn drop(&mut self) {
        if let Ok(mut sockets) = self.connections.sockets.lock() {
            sockets.remove(&self.id);
        }
    }
}
impl DashClient {
    pub fn new(base: Option<&str>, mode: RunMode) -> Result<Self, DashError> {
        let base = base
            .map(|raw| {
                if mode == RunMode::Sandbox {
                    sandbox_dash(raw)
                } else {
                    loopback_only(raw).map(|(url, _)| url)
                }
            })
            .transpose()
            .map_err(DashError::Invalid)?;
        let address = base
            .as_ref()
            .map(|url| {
                url.strip_prefix("http://")
                    .ok_or_else(|| DashError::Invalid("not HTTP loopback".into()))?
                    .parse()
                    .map_err(|e: std::net::AddrParseError| DashError::Invalid(e.to_string()))
            })
            .transpose()?;
        Ok(Self {
            base,
            address,
            mode,
            connections: Arc::new(Connections::default()),
        })
    }
    pub(crate) fn isolated(&self) -> Self {
        Self {
            base: self.base.clone(),
            address: self.address,
            mode: self.mode,
            connections: Arc::new(Connections::default()),
        }
    }
    pub(crate) fn cancel_pending(&self) {
        self.connections.cancelled.store(true, Ordering::Release);
        if let Ok(sockets) = self.connections.sockets.lock() {
            for socket in sockets.values() {
                let _ = socket.shutdown(Shutdown::Both);
            }
        }
    }
    pub fn get(&self, path: &str, timeout: Duration) -> Result<Value, DashError> {
        let (status, body) = self.exchange("GET", path, None, timeout)?;
        if !(200..300).contains(&status) {
            return Err(DashError::HttpStatus(
                status,
                parse_body(&body)
                    .unwrap_or_else(|_| Value::String(String::from_utf8_lossy(&body).into_owned())),
            ));
        }
        parse_body(&body)
    }
    pub fn post(
        &self,
        path: &str,
        payload: &Value,
        timeout: Duration,
    ) -> Result<(u16, Value), DashError> {
        if self.mode == RunMode::Shadow {
            return Err(DashError::ShadowRefused);
        }
        if self.base.is_none() {
            return Err(DashError::Disconnected);
        }
        let body = comandos_core::json::response_dumps(payload).map_err(DashError::Json)?;
        let (status, body) = self.exchange("POST", path, Some(body.as_bytes()), timeout)?;
        Ok((status, parse_body(&body)?))
    }
    fn exchange(
        &self,
        method: &str,
        path: &str,
        body: Option<&[u8]>,
        timeout: Duration,
    ) -> Result<(u16, Vec<u8>), DashError> {
        let address = self.address.ok_or(DashError::Disconnected)?;
        if !path.starts_with('/') || path.bytes().any(|b| b <= 32 || b == 127) {
            return Err(DashError::Invalid("invalid request path".into()));
        }
        if self.connections.cancelled.load(Ordering::Acquire) {
            return Err(DashError::Cancelled);
        }
        let deadline = Instant::now()
            .checked_add(timeout)
            .ok_or_else(|| DashError::Invalid("timeout overflow".into()))?;
        let mut stream =
            TcpStream::connect_timeout(&address, timeout.max(Duration::from_millis(1)))
                .map_err(|e| DashError::Transport(e.to_string()))?;
        let id = self.connections.next.fetch_add(1, Ordering::Relaxed);
        let copy = stream
            .try_clone()
            .map_err(|e| DashError::Transport(e.to_string()))?;
        self.connections
            .sockets
            .lock()
            .map_err(|_| DashError::Transport("connection registry poisoned".into()))?
            .insert(id, copy);
        let _registration = Registration {
            id,
            connections: self.connections.clone(),
        };
        stream
            .set_read_timeout(Some(Duration::from_millis(50)))
            .map_err(|e| DashError::Transport(e.to_string()))?;
        stream
            .set_write_timeout(Some(Duration::from_millis(50)))
            .map_err(|e| DashError::Transport(e.to_string()))?;
        let mut request = format!(
            "{method} {path} HTTP/1.0\r\nHost: {address}\r\nAccept-Encoding: identity\r\nConnection: close\r\n"
        );
        if let Some(body) = body {
            request.push_str(&format!(
                "Content-Type: application/json\r\nContent-Length: {}\r\n",
                body.len()
            ));
        }
        request.push_str("\r\n");
        let mut wire = request.into_bytes();
        if let Some(body) = body {
            wire.extend_from_slice(body);
        }
        let mut sent = 0;
        while sent < wire.len() {
            self.check(deadline)?;
            match stream.write(wire.get(sent..).unwrap_or_default()) {
                Ok(0) => return Err(DashError::Transport("closed while writing".into())),
                Ok(n) => sent += n,
                Err(e) if retry(&e) => continue,
                Err(e) => return Err(DashError::Transport(e.to_string())),
            }
        }
        let mut response = Vec::new();
        let header_end = loop {
            if let Some(end) = response.windows(4).position(|w| w == b"\r\n\r\n") {
                break end + 4;
            }
            if response.len() > MAX_HEADERS {
                return Err(DashError::Transport("headers too large".into()));
            }
            if !self.read_more(&mut stream, &mut response, deadline)? {
                return Err(DashError::Transport("closed before response".into()));
            }
        };
        if header_end > MAX_HEADERS {
            return Err(DashError::Transport("headers too large".into()));
        }
        let header = std::str::from_utf8(response.get(..header_end).unwrap_or_default())
            .map_err(|e| DashError::Transport(e.to_string()))?;
        let mut lines = header.split("\r\n");
        let mut status_line = lines.next().unwrap_or_default().split_whitespace();
        if !matches!(status_line.next(), Some("HTTP/1.0" | "HTTP/1.1")) {
            return Err(DashError::Transport("invalid HTTP version".into()));
        }
        let status = status_line
            .next()
            .and_then(|s| s.parse::<u16>().ok())
            .filter(|s| *s >= 100 && *s <= 599)
            .ok_or_else(|| DashError::Transport("invalid HTTP status".into()))?;
        let mut length = None;
        let mut chunked = false;
        for line in lines.filter(|line| !line.is_empty()) {
            let (name, value) = line
                .split_once(':')
                .ok_or_else(|| DashError::Transport("invalid header".into()))?;
            if name.eq_ignore_ascii_case("content-length") {
                let n = value
                    .trim()
                    .parse::<usize>()
                    .map_err(|e| DashError::Transport(e.to_string()))?;
                if length.replace(n).is_some() {
                    return Err(DashError::Transport("duplicate content length".into()));
                }
            } else if name.eq_ignore_ascii_case("transfer-encoding") {
                chunked = value.trim().eq_ignore_ascii_case("chunked");
                if !chunked {
                    return Err(DashError::Transport("unsupported transfer encoding".into()));
                }
            }
        }
        if length.is_some_and(|n| n > MAX_RESPONSE) {
            return Err(DashError::Transport("response too large".into()));
        }
        let mut body = response.get(header_end..).unwrap_or_default().to_vec();
        if chunked {
            loop {
                if let Some(decoded) = decode_chunks(&body)? {
                    return Ok((status, decoded));
                }
                if !self.read_more(&mut stream, &mut body, deadline)? {
                    return Err(DashError::Transport("truncated chunked response".into()));
                }
            }
        }
        if let Some(length) = length {
            while body.len() < length {
                if !self.read_more(&mut stream, &mut body, deadline)? {
                    return Err(DashError::Transport("truncated response".into()));
                }
            }
            body.truncate(length);
        } else {
            while self.read_more(&mut stream, &mut body, deadline)? {}
        }
        Ok((status, body))
    }
    fn check(&self, deadline: Instant) -> Result<(), DashError> {
        if self.connections.cancelled.load(Ordering::Acquire) {
            Err(DashError::Cancelled)
        } else if Instant::now() >= deadline {
            Err(DashError::Transport("HTTP timeout".into()))
        } else {
            Ok(())
        }
    }
    fn read_more(
        &self,
        stream: &mut TcpStream,
        out: &mut Vec<u8>,
        deadline: Instant,
    ) -> Result<bool, DashError> {
        let mut buf = [0u8; 8192];
        loop {
            self.check(deadline)?;
            match stream.read(&mut buf) {
                Ok(0) => {
                    self.check(deadline)?;
                    return Ok(false);
                }
                Ok(n) => {
                    if out.len().saturating_add(n) > MAX_RESPONSE + MAX_HEADERS {
                        return Err(DashError::Transport("response too large".into()));
                    }
                    out.extend_from_slice(buf.get(..n).unwrap_or_default());
                    return Ok(true);
                }
                Err(e) if retry(&e) => continue,
                Err(e) => return Err(DashError::Transport(e.to_string())),
            }
        }
    }
}
fn retry(error: &std::io::Error) -> bool {
    matches!(
        error.kind(),
        ErrorKind::WouldBlock | ErrorKind::TimedOut | ErrorKind::Interrupted
    )
}
fn parse_body(bytes: &[u8]) -> Result<Value, DashError> {
    std::str::from_utf8(bytes).map_err(|e| DashError::Json(e.to_string()))?;
    comandos_core::json::workspace_loads_bytes(bytes)
        .ok_or_else(|| DashError::Json("invalid JSON response".into()))
}
fn decode_chunks(wire: &[u8]) -> Result<Option<Vec<u8>>, DashError> {
    let mut offset = 0;
    let mut output = Vec::new();
    loop {
        let rest = wire.get(offset..).unwrap_or_default();
        let Some(end) = rest.windows(2).position(|w| w == b"\r\n") else {
            return Ok(None);
        };
        let number = std::str::from_utf8(rest.get(..end).unwrap_or_default())
            .map_err(|e| DashError::Transport(e.to_string()))?
            .split(';')
            .next()
            .unwrap_or_default()
            .trim();
        let size =
            usize::from_str_radix(number, 16).map_err(|e| DashError::Transport(e.to_string()))?;
        offset = offset.saturating_add(end + 2);
        if size == 0 {
            return Ok(Some(output));
        }
        if output.len().saturating_add(size) > MAX_RESPONSE {
            return Err(DashError::Transport("response too large".into()));
        }
        let Some(end) = offset.checked_add(size) else {
            return Err(DashError::Transport("chunk size overflow".into()));
        };
        let Some(data) = wire.get(offset..end) else {
            return Ok(None);
        };
        if wire.get(end..end.saturating_add(2)) != Some(b"\r\n") {
            if wire.len() < end.saturating_add(2) {
                return Ok(None);
            }
            return Err(DashError::Transport("invalid chunk delimiter".into()));
        }
        output.extend_from_slice(data);
        offset = end.saturating_add(2);
    }
}
