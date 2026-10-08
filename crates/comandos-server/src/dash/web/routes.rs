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
    if matches!(
        &route,
        WebRoute::Index | WebRoute::Ready | WebRoute::Status
    ) {
        state.web.refresh_selection().await;
    }
    match route {
        WebRoute::Index => index(state, request).await,
        WebRoute::NativeWorker => super::native_worker::serve(&state.web),
        WebRoute::Gate => gate(state, request).await,
        WebRoute::Ready => ready(state, request),
        WebRoute::Markdown => super::markdown::handle(request),
        WebRoute::Status if query_value(&request.target, "web").as_deref() == Some("native") => {
            native_status(&state.web, state.term_control.status())
        }
        WebRoute::Status => status_with_term(&state.web, state.term_control.status()),
        WebRoute::Asset(rel) => asset(state, &rel).await,
        WebRoute::NativeAsset(url) => {
            let manifest = state.web.manifest();
            let Some(rel) = super::native_page::alias(
                &manifest,
                &state.web.web_dir,
                &state.config.dash_dir,
                &url,
            )
            .or_else(|| {
                super::Manifest::load_terminal(&state.web.web_dir)
                    .ok()
                    .and_then(|m| super::native_term_page::alias(&m, &state.web.web_dir, &url))
            }) else {
                return dash::not_found();
            };
            asset(state, &rel).await
        }
    }
}

async fn index(state: &DashState, request: &Request) -> Result<Reply, HandlerError> {
    let web = &state.web;
    if query_value(&request.target, "web").as_deref() == Some("native") {
        let manifest = web.manifest();
        let plan = match super::native_page::admit(
            &web.registry,
            &web.selection(),
            &manifest,
            &web.web_dir,
        ) {
            Ok(plan) => plan,
            Err(error) => {
                return Reply::json(
                    StatusCode::SERVICE_UNAVAILABLE,
                    &serde_json::json!({"error":error,"mode":"native"}),
                );
            }
        };
        let nonce = match web.gate.insert_native() {
            Inserted::Nonce(nonce) => nonce,
            Inserted::Full => {
                return Reply::json(
                    StatusCode::SERVICE_UNAVAILABLE,
                    &serde_json::json!({"error":"native readiness capacity exhausted","mode":"native"}),
                );
            }
        };
        return Ok(Reply::bytes(
            StatusCode::OK,
            "text/html",
            super::native_page::render(
                &plan,
                &manifest,
                &nonce,
                query_value(&request.target, "app").as_deref() == Some("1"),
            ),
        ));
    }
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
        &web.manifest(),
        "pending",
    );
    if preview.active.is_empty() {
        return statics::serve(&state.config.dash_dir, request).await;
    }
    let nonce = match web.gate.insert() {
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
        &web.manifest(),
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

async fn gate(state: &DashState, request: &Request) -> Result<Reply, HandlerError> {
    let Some(k) = query_value(&request.target, "k") else {
        return Ok(Reply::bytes(StatusCode::OK, "text/javascript", fallback()));
    };
    let Some(mut rx) = state.web.gate.subscribe(&k) else {
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
    state.web.gate.finish(&k);
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

fn ready(state: &DashState, request: &Request) -> Result<Reply, HandlerError> {
    let k = request
        .data
        .as_ref()
        .and_then(|v| v.get("k"))
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    if k.starts_with("native-") {
        let data = request.data.as_ref();
        let failed = data
            .and_then(|v| v.get("failed"))
            .and_then(|v| v.as_array());
        let mounted = data
            .and_then(|v| v.get("mounted"))
            .and_then(|v| v.as_array())
            .and_then(|v| {
                let names = v
                    .iter()
                    .map(|item| item.as_str().map(str::to_string))
                    .collect::<Option<Vec<_>>>()?;
                let unique = names
                    .iter()
                    .cloned()
                    .collect::<std::collections::BTreeSet<_>>();
                (unique.len() == names.len()).then_some(unique)
            });
        let expected = if k.starts_with("native-term-") {
            super::Manifest::load_terminal(&state.web.web_dir)
                .ok()
                .and_then(|m| super::native_term_page::bundle(&m, &state.web.web_dir).ok())
                .map(|p| {
                    p.components
                        .into_iter()
                        .collect::<std::collections::BTreeSet<_>>()
                })
        } else {
            super::native_page::admit(
                &state.web.registry,
                &state.web.selection(),
                &state.web.manifest(),
                &state.web.web_dir,
            )
            .ok()
            .map(|p| p.ids.into_iter().collect::<std::collections::BTreeSet<_>>())
        };
        if failed.is_none_or(|v| !v.is_empty()) || mounted.is_none() || mounted != expected {
            return Reply::json(
                StatusCode::BAD_REQUEST,
                &serde_json::json!({"error":"native page did not complete all mounts"}),
            );
        }
    }
    if !state.web.gate.mark_ready(k) {
        return Reply::json(
            StatusCode::BAD_REQUEST,
            &serde_json::json!({"error":"nonce desconocido"}),
        );
    }
    if k.starts_with("native-") {
        state.web.gate.finish(k);
    }
    Reply::json(StatusCode::OK, &serde_json::json!({"ok":true}))
}

fn native_status(web: &super::WebState, term: serde_json::Value) -> Result<Reply, HandlerError> {
    match super::native_page::admit(
        &web.registry,
        &web.selection(),
        &web.manifest(),
        &web.web_dir,
    ) {
        Ok(plan) => Reply::json(
            StatusCode::OK,
            &serde_json::json!({"mode":"native","active":plan.ids,"artifacts":{"web_dir":web.web_dir.display().to_string()},"term":term}),
        ),
        Err(error) => Reply::json(
            StatusCode::SERVICE_UNAVAILABLE,
            &serde_json::json!({"mode":"native","error":error}),
        ),
    }
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
    let page = fs::read(web.dash_dir.join("index.html")).unwrap_or_default();
    let c = super::compose::compose(
        &page,
        &web.registry,
        &selection,
        false,
        &web.manifest(),
        "status",
    );
    let (in_flight, gate_full) = web.gate.snapshot();
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
    let web = &state.web;
    let path = web.web_dir.join(rel);
    let bytes = tokio::fs::read(&path)
        .await
        .map_err(|_| HandlerError::Failure)?;
    let mut reply = Reply::bytes(StatusCode::OK, statics::mime_for(rel), bytes);
    if web.manifest().is_versioned(rel)
        || super::Manifest::load_terminal(&web.web_dir).is_ok_and(|m| m.is_versioned(rel))
    {
        reply.cache = crate::ReplyCache::Immutable;
    }
    Ok(reply)
}

fn fallback() -> &'static str {
    "if(!sessionStorage.cc_web_fallback){sessionStorage.cc_web_fallback=1;try{fetch('/ui-log',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({event:'web-gate-timeout'}),keepalive:true}).catch(()=>{})}catch(e){}const u=new URL(location.href);u.searchParams.set('web','off');location.replace(u.href)}"
}

pub(crate) fn query_value(target: &str, key: &str) -> Option<String> {
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

#[cfg(test)]
mod tests {
    #[test]
    fn fallback_preserves_application_query_hash_and_redirects_once() {
        let fixture = "globalThis.sessionStorage={};globalThis.location={href:'http://127.0.0.1:7311/?app=1&anwin=1&token=fixture#tab',pathname:'/',replace(u){this.redirect=u;this.count=(this.count||0)+1}};globalThis.fetch=()=>Promise.resolve({});";
        let script = format!(
            "{fixture}{};{};console.log(JSON.stringify(location));",
            super::fallback(),
            super::fallback()
        );
        let output = std::process::Command::new("node")
            .args(["-e", &script])
            .output()
            .expect("fixture Node de JS sin navegador");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let result: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(
            result["redirect"],
            "http://127.0.0.1:7311/?app=1&anwin=1&token=fixture&web=off#tab"
        );
        assert_eq!(result["count"], 1);
    }
}
