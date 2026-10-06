//! RFC6455 upgrade after dashboard admission; handlers own their application codec.
use bytes::Bytes;
use http::{HeaderMap, HeaderValue, Response, StatusCode, header};
use http_body_util::Full;
use hyper_util::rt::TokioIo;
use std::{future::Future, io, net::SocketAddr, pin::Pin, sync::Arc};
use tokio::sync::watch;
use tokio_tungstenite::{
    WebSocketStream,
    tungstenite::protocol::{Role, WebSocketConfig},
};
pub type WsStream = WebSocketStream<TokioIo<hyper::upgrade::Upgraded>>;
pub struct WsRequest {
    pub target: String,
    pub peer: SocketAddr,
    pub headers: Vec<(String, String)>,
    pub protocol: Option<&'static str>,
    pub internal_producer: bool,
    pub shutdown: watch::Receiver<bool>,
}
pub type WsHandler =
    Arc<dyn Fn(WsRequest, WsStream) -> Pin<Box<dyn Future<Output = ()> + Send>> + Send + Sync>;
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum WsAdmission {
    #[default]
    Dashboard,
    /// Transport always checks Host/Origin; handler must authenticate before effects.
    HandlerToken,
}
pub type WsAccepts = Arc<dyn Fn(&str) -> Option<&'static [&'static str]> + Send + Sync>;
pub type WsAdmissionSelector = Arc<dyn Fn(Option<&'static str>) -> WsAdmission + Send + Sync>;
#[derive(Clone)]
pub struct WsRoute {
    pub accepts: WsAccepts,
    pub handler: WsHandler,
    admission: Option<WsAdmissionSelector>,
}
impl WsRoute {
    pub fn new(accepts: WsAccepts, handler: WsHandler) -> Self {
        Self {
            accepts,
            handler,
            admission: None,
        }
    }
    pub fn with_admission(mut self, admission: WsAdmissionSelector) -> Self {
        self.admission = Some(admission);
        self
    }
    pub(crate) fn admission(&self, protocol: Option<&'static str>) -> WsAdmission {
        self.admission
            .as_ref()
            .map_or(WsAdmission::Dashboard, |policy| policy(protocol))
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WsReject {
    BadHandshake,
    Protocol,
}
fn has_token(headers: &HeaderMap, name: header::HeaderName, token: &str) -> bool {
    headers
        .get_all(name)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(','))
        .any(|v| v.trim().eq_ignore_ascii_case(token))
}
pub fn is_upgrade(headers: &HeaderMap) -> bool {
    has_token(headers, header::CONNECTION, "upgrade")
        && has_token(headers, header::UPGRADE, "websocket")
}
pub fn accept_key(key: &str) -> String {
    tokio_tungstenite::tungstenite::handshake::derive_accept_key(key.as_bytes())
}
pub fn pick_protocol(
    headers: &HeaderMap,
    allowed: &'static [&'static str],
) -> Result<Option<&'static str>, WsReject> {
    let offered: Vec<&str> = headers
        .get_all(header::SEC_WEBSOCKET_PROTOCOL)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(','))
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .collect();
    if allowed.is_empty() {
        return if offered.is_empty() {
            Ok(None)
        } else {
            Err(WsReject::Protocol)
        };
    }
    offered
        .iter()
        .find_map(|offered| {
            allowed
                .iter()
                .find(|allowed| **allowed == *offered)
                .copied()
        })
        .map(Some)
        .ok_or(WsReject::Protocol)
}
pub fn handshake_key(headers: &HeaderMap) -> Result<String, WsReject> {
    use base64::Engine;
    let unique = |name: header::HeaderName| {
        let mut values = headers.get_all(name).iter();
        let value = values.next()?.to_str().ok()?;
        if values.next().is_some() {
            None
        } else {
            Some(value)
        }
    };
    if unique(header::SEC_WEBSOCKET_VERSION) != Some("13") {
        return Err(WsReject::BadHandshake);
    }
    let key = unique(header::SEC_WEBSOCKET_KEY).ok_or(WsReject::BadHandshake)?;
    if base64::engine::general_purpose::STANDARD
        .decode(key)
        .is_ok_and(|bytes| bytes.len() == 16)
    {
        Ok(key.into())
    } else {
        Err(WsReject::BadHandshake)
    }
}
pub fn switching_response(key: &str, protocol: Option<&str>) -> Response<Full<Bytes>> {
    let mut response = Response::new(Full::new(Bytes::new()));
    *response.status_mut() = StatusCode::SWITCHING_PROTOCOLS;
    let headers = response.headers_mut();
    headers.insert(header::CONNECTION, HeaderValue::from_static("Upgrade"));
    headers.insert(header::UPGRADE, HeaderValue::from_static("websocket"));
    if let Ok(value) = HeaderValue::from_str(&accept_key(key)) {
        headers.insert(header::SEC_WEBSOCKET_ACCEPT, value);
    }
    if let Some(value) = protocol.and_then(|v| HeaderValue::from_str(v).ok()) {
        headers.insert(header::SEC_WEBSOCKET_PROTOCOL, value);
    }
    response
}
/// 4 MiB permits A2 to discard a 2 MiB paste without disconnecting the client.
/// The terminal's own accepted input and cumulative queued budget remain 1 MiB.
pub fn config() -> WebSocketConfig {
    WebSocketConfig::default()
        .max_message_size(Some(4 << 20))
        .max_frame_size(Some(4 << 20))
        .max_write_buffer_size(4 << 20)
}
pub async fn into_stream(upgrade: hyper::upgrade::OnUpgrade) -> io::Result<WsStream> {
    let stream = upgrade.await.map_err(io::Error::other)?;
    Ok(WebSocketStream::from_raw_socket(TokioIo::new(stream), Role::Server, Some(config())).await)
}
