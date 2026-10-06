//! Opt-in terminal routes and authenticated WebSocket application handler.
use super::{
    attach::valid_session,
    bridge::{BridgeLimits, Source, run_bridge},
};
use crate::{
    HandlerError, Reply, WsAdmission, WsHandler, WsRoute,
    dash::{DashState, router::path_of},
};
use comandos_term::proto::{self, Dialect};
use futures_util::{SinkExt, StreamExt};
use http::{Method, StatusCode};
use std::{path::PathBuf, sync::Arc, time::Duration};
use tokio_tungstenite::tungstenite::{
    Message,
    protocol::{CloseFrame, frame::coding::CloseCode},
};
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum TermMode {
    #[default]
    Off,
    Ttyd,
    Native,
}
impl TermMode {
    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "off" => Ok(Self::Off),
            "ttyd" => Ok(Self::Ttyd),
            "native" => Ok(Self::Native),
            _ => Err(format!("modo de terminal no válido: {value}")),
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Ttyd => "ttyd",
            Self::Native => "native",
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TermRoute {
    Page,
    Token,
    Redirect,
    Missing,
}
pub fn route(method: &Method, target: &str) -> Option<TermRoute> {
    if *method != Method::GET && *method != Method::HEAD {
        return None;
    }
    match path_of(target) {
        "/term/" => Some(TermRoute::Page),
        "/term/token" => Some(TermRoute::Token),
        "/term" => Some(TermRoute::Redirect),
        path if path.starts_with("/term/") => Some(TermRoute::Missing),
        _ => None,
    }
}
pub async fn http_route(
    state: &DashState,
    route: TermRoute,
    target: &str,
) -> Result<Reply, HandlerError> {
    let mut reply = match route {
        TermRoute::Page => {
            let page = match state.config.term {
                TermMode::Ttyd => tokio::fs::read(state.config.dash_dir.join("term.html"))
                    .await
                    .map_err(|_| HandlerError::Failure)?,
                TermMode::Native => comandos_web_view::term_page::shell(&Default::default())
                    .into_string()
                    .into_bytes(),
                TermMode::Off => return crate::dash::not_found(),
            };
            Reply::bytes(StatusCode::OK, "text/html; charset=utf-8", page)
        }
        TermRoute::Token => Reply::bytes(StatusCode::OK, "application/json", r#"{"token":""}"#),
        TermRoute::Redirect => {
            let mut reply = Reply::bytes(StatusCode::MOVED_PERMANENTLY, "text/plain", "");
            let query = target
                .split_once('?')
                .map(|(_, q)| format!("?{q}"))
                .unwrap_or_default();
            let value = http::HeaderValue::from_str(&format!("/term/{query}"))
                .map_err(|_| HandlerError::Failure)?;
            reply.headers.insert(http::header::LOCATION, value);
            reply
        }
        TermRoute::Missing => return crate::dash::not_found(),
    };
    reply.headers.insert(
        "x-comandos-term",
        http::HeaderValue::from_static(state.config.term.name()),
    );
    let ports = state
        .term_control
        .ports()
        .iter()
        .map(u16::to_string)
        .collect::<Vec<_>>()
        .join(",");
    if let Ok(value) = ports.parse() {
        reply.headers.insert("x-comandos-term-compat", value);
    }
    Ok(reply)
}
async fn close(socket: &mut crate::ws::WsStream, code: CloseCode, reason: &'static str) {
    let _ = tokio::time::timeout(
        Duration::from_millis(200),
        socket.send(Message::Close(Some(CloseFrame {
            code,
            reason: reason.into(),
        }))),
    )
    .await;
}
pub fn ws_route(state: Arc<DashState>) -> WsRoute {
    let mode = state.config.term;
    let control = state.term_control.clone();
    let handler: WsHandler = Arc::new(move |request, mut socket| {
        let state = state.clone();
        Box::pin(async move {
            let Some(dialect) = Dialect::from_protocol(request.protocol) else {
                close(&mut socket, CloseCode::Protocol, "terminal protocol").await;
                return;
            };
            let query = request.target.split_once('?').map(|(_, q)| q).unwrap_or("");
            let args = proto::tty_args(query);
            if dialect == Dialect::Tty {
                // Read the token for each connection, honoring rotation without restart.
                let expected = tokio::fs::read(crate::dash::token_path(&state.config.home))
                    .await
                    .ok()
                    .and_then(|bytes| String::from_utf8(bytes).ok())
                    .unwrap_or_default();
                let presented = args.first().map(String::as_str).unwrap_or("").trim();
                if expected.trim().is_empty()
                    || !comandos_core::dashboard_access::token_matches(
                        presented.as_bytes(),
                        expected.trim().as_bytes(),
                    )
                {
                    let _ = socket
                        .send(Message::binary(proto::output(
                            b"Acceso denegado al terminal de ComandOS.\r\n",
                        )))
                        .await;
                    close(&mut socket, CloseCode::Policy, "terminal authentication").await;
                    return;
                }
            }
            let mut shutdown = request.shutdown;
            let mut enabled = state.term_control.subscribe();
            if !state.term_control.enabled() {
                return;
            }
            if *shutdown.borrow() {
                return;
            }
            let receive = async {
                loop {
                    match socket.next().await {
                        Some(Ok(message)) if message.is_binary() || message.is_text() => {
                            return proto::parse_init(dialect, &message.into_data()).ok();
                        }
                        Some(Ok(Message::Ping(_) | Message::Pong(_))) => {}
                        _ => return None,
                    }
                }
            };
            let init = tokio::select! {
                _=shutdown.changed()=>return,
                _=enabled.changed()=>return,
                result=tokio::time::timeout(Duration::from_secs(10),receive)=>result.unwrap_or(None),
            };
            let Some(init) = init else {
                close(&mut socket, CloseCode::Protocol, "terminal init").await;
                return;
            };
            let session = match dialect {
                Dialect::Tty => args.get(1).cloned(),
                Dialect::V1 => init.session.clone(),
            };
            let source = if state.config.shadow_readonly {
                let Some(root) = state.config.term_replay_dir.as_ref() else {
                    close(&mut socket, CloseCode::Policy, "shadow requires replay").await;
                    return;
                };
                Source::Replay(replay_file(root, session.as_deref()))
            } else {
                Source::Pty {
                    target: state.term_target.clone(),
                    session,
                }
            };
            let (stop, rx) = tokio::sync::watch::channel(false);

            if !state.term_control.enabled() {
                let _ = stop.send(true);
            }
            let bridge = run_bridge(socket, dialect, init, source, BridgeLimits::default(), rx);
            tokio::pin!(bridge);
            let mut stopping = false;
            loop {
                tokio::select! {
                    result=&mut bridge=>{if let Err(error)=result{eprintln!("comandos terminal: bridge {}",error.kind());}break},
                    _=shutdown.changed(),if !stopping=>{let _=stop.send(true);stopping=true;},
                    _=enabled.changed(),if !stopping=>{if !*enabled.borrow(){let _=stop.send(true);stopping=true;}},
                }
            }
        })
    });
    WsRoute::new(
        Arc::new(move |path| {
            if path != "/term/ws" || !control.enabled() {
                None
            } else {
                match mode {
                    TermMode::Off => None,
                    TermMode::Ttyd => Some(&["tty"][..]),
                    TermMode::Native => Some(proto::PROTOCOLS),
                }
            }
        }),
        handler,
    )
    .with_admission(Arc::new(|protocol| {
        if protocol == Some("tty") {
            WsAdmission::HandlerTokenOrigin
        } else {
            WsAdmission::DashboardTokenOrigin
        }
    }))
}
/// Validated recording filename, never a user-controlled path.
pub fn replay_file(root: &std::path::Path, session: Option<&str>) -> PathBuf {
    root.join(
        session
            .filter(|s| valid_session(s))
            .map(|s| format!("{s}.bin"))
            .unwrap_or_else(|| "default.bin".into()),
    )
}
