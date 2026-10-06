//! Dedicated compatibility listeners share admission and the terminal handler.
use super::routes::{self, TermMode};
use crate::{
    Config, Handler,
    dash::{self, DashState},
};
use std::{io, sync::Arc};
use tokio::{net::TcpListener, sync::watch};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CompatProfile {
    Path,
    Plain,
}
fn rewrite(profile: CompatProfile, target: &str) -> Option<String> {
    let (path, query) = target
        .split_once('?')
        .map(|(p, q)| (p, format!("?{q}")))
        .unwrap_or((target, String::new()));
    let path = match (profile, path) {
        (CompatProfile::Path, "/term" | "/term/" | "/term/token" | "/term/ws") => path,
        (CompatProfile::Plain, "/") => "/term/",
        (CompatProfile::Plain, "/token") => "/term/token",
        (CompatProfile::Plain, "/ws") => "/term/ws",
        _ => return None,
    };
    Some(format!("{path}{query}"))
}
pub fn config(profile: CompatProfile, state: Arc<DashState>) -> Config {
    let handler_state = state.clone();
    let handler: Handler = Arc::new(move |request| {
        let state = handler_state.clone();
        Box::pin(async move {
            let Some(target) = rewrite(profile, &request.target) else {
                return dash::not_found();
            };
            if state.config.term == TermMode::Off {
                return dash::not_found();
            }
            let Some(route) = routes::route(&request.method, &target) else {
                return dash::not_found();
            };
            routes::http_route(&state, route, &target).await
        })
    });
    let ws = routes::ws_route(state.clone());
    let accept = ws.accepts.clone();
    let application = ws.handler.clone();
    let mut websocket = ws;
    websocket.accepts = Arc::new(move |path| rewrite(profile, path).and_then(|p| accept(&p)));
    websocket.handler = Arc::new(move |mut request, socket| {
        if let Some(target) = rewrite(profile, &request.target) {
            request.target = target;
        }
        application(request, socket)
    });
    Config {
        token: state.config.token.clone(),
        token_file: state.config.token_file.clone(),
        asset_exists: Arc::new(|_| false),
        handler,
        websocket: Some(websocket),
        limits: dash::limits(),
    }
}
pub async fn serve_compat(
    port: u16,
    profile: CompatProfile,
    state: Arc<DashState>,
    shutdown: watch::Receiver<bool>,
) -> io::Result<()> {
    let listener = dash::bind(port).await?;
    serve_listener(listener, profile, state, shutdown).await
}
pub async fn serve_listener(
    listener: TcpListener,
    profile: CompatProfile,
    state: Arc<DashState>,
    shutdown: watch::Receiver<bool>,
) -> io::Result<()> {
    crate::serve(listener, config(profile, state), shutdown).await
}
