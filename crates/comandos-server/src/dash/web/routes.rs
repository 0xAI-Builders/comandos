use super::{WebRoute, gate::Inserted};
use crate::{
    HandlerError, Reply, Request,
    dash::{self, DashState, statics},
};
use bytes::Bytes;
use http::{HeaderValue, StatusCode, header};
use std::{fs, time::Duration};

pub async fn handle(
    state: &DashState,
    route: WebRoute,
    request: &Request,
) -> Result<Reply, HandlerError> {
    match route {
        WebRoute::Index => index(state, request).await,
        WebRoute::Gate => gate(state, request).await,
        WebRoute::Ready => ready(state, request),
        WebRoute::Status => status_with_term(
            &super::WebState::new(&state.config),
            state.term_control.status(),
        ),
        WebRoute::Asset(rel) => asset(state, &rel).await,
    }
}

async fn index(state: &DashState, request: &Request) -> Result<Reply, HandlerError> {
    let web = super::WebState::new(&state.config);
    if query_value(&request.target, "web").as_deref() == Some("off") {
        return statics::serve(&state.config.dash_dir, request).await;
    }
    let page =
        fs::read(state.config.dash_dir.join("index.html")).map_err(|_| HandlerError::Failure)?;
    let shadow = query_value(&request.target, "web").as_deref() == Some("shadow")
        || request
            .headers
            .iter()
            .any(|(k, v)| k == "cookie" && v.split(';').any(|c| c.trim() == "cc_web=shadow"));
    let selection = web.selection();
    let preview = super::compose::compose(
        &page,
        &web.registry,
        &selection,
        shadow,
        &web.manifest,
        "pending",
    );
    if preview.active.is_empty() {
        return statics::serve(&state.config.dash_dir, request).await;
    }
    let nonce = match super::gate::global().insert() {
        Inserted::Nonce(k) => k,
        Inserted::Full => {
            return statics::serve(&state.config.dash_dir, request).await;
        }
    };
    let composed = super::compose::compose(
        &page,
        &web.registry,
        &selection,
        shadow,
        &web.manifest,
        &nonce,
    );
    let mut reply = Reply::bytes(StatusCode::OK, "text/html", composed.html);
    if query_value(&request.target, "web").as_deref() == Some("shadow") {
        reply.headers.insert(
            header::SET_COOKIE,
            HeaderValue::from_static("cc_web=shadow; Path=/; SameSite=Strict; HttpOnly"),
        );
    }
    Ok(reply)
}

async fn gate(_state: &DashState, request: &Request) -> Result<Reply, HandlerError> {
    let Some(k) = query_value(&request.target, "k") else {
        return Ok(Reply::bytes(StatusCode::OK, "text/javascript", fallback()));
    };
    let Some(mut rx) = super::gate::global().subscribe(&k) else {
        return Ok(Reply::bytes(StatusCode::OK, "text/javascript", fallback()));
    };
    let ready = tokio::time::timeout(Duration::from_secs(8), async move {
        loop {
            if *rx.borrow() {
                return true;
            }
            if rx.changed().await.is_err() {
                return false;
            }
        }
    })
    .await
    .unwrap_or(false);
    super::gate::global().finish(&k);
    if ready {
        Ok(Reply::bytes(
            StatusCode::OK,
            "text/javascript",
            Bytes::new(),
        ))
    } else {
        Ok(Reply::bytes(StatusCode::OK, "text/javascript", fallback()))
    }
}

fn ready(_state: &DashState, request: &Request) -> Result<Reply, HandlerError> {
    let k = request
        .data
        .as_ref()
        .and_then(|v| v.get("k"))
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    if !super::gate::global().mark_ready(k) {
        return Reply::json(
            StatusCode::BAD_REQUEST,
            &serde_json::json!({"error":"nonce desconocido"}),
        );
    }
    Reply::json(StatusCode::OK, &serde_json::json!({"ok":true}))
}

pub fn status(web: &super::WebState) -> Result<Reply, HandlerError> {
    Reply::json(StatusCode::OK, &status_value(web, None))
}

pub fn status_with_term(
    web: &super::WebState,
    term: serde_json::Value,
) -> Result<Reply, HandlerError> {
    Reply::json(StatusCode::OK, &status_value(web, Some(term)))
}

fn status_value(web: &super::WebState, term: Option<serde_json::Value>) -> serde_json::Value {
    let selection = web.selection();
    let c = super::compose::compose(
        &[],
        &web.registry,
        &selection,
        false,
        &web.manifest,
        "status",
    );
    let (in_flight, gate_full) = super::gate::global().snapshot();
    let mut value = serde_json::json!({
        "components": super::status::states_json(&c.states),
        "active": c.active,
        "artifacts": {"web_dir": web.web_dir.display().to_string()},
        "nonces": {"in_flight": in_flight, "gate_full": gate_full},
    });
    if let Some(term) = term
        && let Some(map) = value.as_object_mut()
    {
        map.insert("term".into(), term);
    }
    value
}

async fn asset(state: &DashState, rel: &str) -> Result<Reply, HandlerError> {
    if !super::assets::valid_relative(rel) {
        return dash::not_found();
    }
    let web = super::WebState::new(&state.config);
    let path = web.web_dir.join(rel);
    let bytes = tokio::fs::read(&path)
        .await
        .map_err(|_| HandlerError::Failure)?;
    let mut reply = Reply::bytes(StatusCode::OK, statics::mime_for(rel), bytes);
    reply.headers.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("public, max-age=31536000, immutable"),
    );
    Ok(reply)
}

fn fallback() -> &'static str {
    "if(!sessionStorage.cc_web_fallback){sessionStorage.cc_web_fallback=1;location.replace(location.pathname+'?web=off')}"
}

fn query_value(target: &str, key: &str) -> Option<String> {
    let query = target
        .split_once('?')?
        .1
        .split_once('#')
        .map_or(target.split_once('?')?.1, |(q, _)| q);
    for pair in query.split('&') {
        let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
        if k == key {
            return Some(v.to_string());
        }
    }
    None
}
