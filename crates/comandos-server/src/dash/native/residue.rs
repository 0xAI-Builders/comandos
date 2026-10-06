//! Residuo parcial de 2f-3/T7. Las ramas dinámicas pendientes declinan;
//! la reconciliación final del orden requiere fusionar todos los cortes.
mod static_files;
use super::{Answer, Entry, Fault, Key, Native, NativeRoute, Verb, reply, retired, target};
use crate::{HandlerError, Request};
use http::{Method, StatusCode};
use serde_json::json;
pub use static_files::serve as static_response;
use std::sync::Arc;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResidueRoute {
    GetFallback,
    HeadFallback,
    PostUnknown,
    DeleteUnknown,
}
const fn entry(verb: Verb, route: ResidueRoute) -> Entry {
    Entry {
        verb,
        key: Key::Prefix("/"),
        route: NativeRoute::Residue(route),
    }
}
pub const ROUTES: &[Entry] = &[
    entry(Verb::Get, ResidueRoute::GetFallback),
    entry(Verb::Head, ResidueRoute::HeadFallback),
    entry(Verb::Post, ResidueRoute::PostUnknown),
    entry(Verb::Delete, ResidueRoute::DeleteUnknown),
];
// Comparaciones literales de Handler.do_POST: protección provisional para
// que una rama aún no fusionada nunca se convierta aquí en un 404.
const PENDING_POST: &[&str] = &[
    "/pane-extensions",
    "/pane-extensions/apply",
    "/pane-extensions/template",
    "/pane-extensions/cancel",
    "/pane-extensions/recover",
    "/workspace/sort",
    "/workspace",
    "/notify-popup",
    "/presence",
    "/notices/read",
    "/notices/sound",
    "/notices/prefs",
    "/workspace/close-group",
    "/work-marks",
    "/events/v2",
    "/workspace/client",
    "/session/recover",
    "/terminal-history",
    "/terminal-panes",
    "/open-path",
    "/project-profile",
    "/optimization/default",
    "/proxy",
    "/ui-log",
    "/pomodoro",
    "/conf-set",
    "/session-profiles",
    "/session-profile-apply",
    "/skill-toggle",
    "/mcp-toggle",
    "/pause",
    "/event",
    "/test",
    "/remote-on",
    "/remote-off",
    "/remote-webterm-on",
    "/remote-webterm-off",
    "/ssh-add",
    "/ssh-del",
    "/ssh-update",
    "/prefs-set",
    "/usage/experiment",
    "/usage/rating",
    "/usage/capture",
    "/usage/refresh",
    "/push/subscription",
    "/push/test",
    "/news/saved",
    "/news/notes",
    "/news/chat",
    "/news/chat/note",
    "/news/translate",
    "/news/refresh",
    "/open-url",
    "/models/refresh",
    "/fs/mkdir",
    "/usage/quota",
    "/usage/subscription",
    "/usage/settings",
    "/ssh-connect",
    "/ssh-new-tab",
    "/ssh-key-setup",
    "/chains",
    "/chains/delete",
    "/snippets",
    "/snippets/update",
    "/snippets/delete",
    "/session-new",
    "/open-with-account",
    "/terminal/quick",
    "/tab-new",
    "/account/add",
    "/app/command",
    "/tmux-mouse",
    "/tmux-scroll",
    "/tab-register",
    "/tab-metadata",
    "/tab-metadata-remove",
    "/tab-close",
    "/recover-tab",
    "/session/configure",
    "/account/switch",
    "/harness/switch",
    "/model/switch-cancel",
    "/model/switch",
    "/pane/type",
    "/send",
    "/paste",
    "/key",
    "/export",
    "/kill",
    "/focus",
    "/ensure",
    "/new",
    "/shell",
    "/up",
];
pub async fn answer(native: &Arc<Native>, route: ResidueRoute, request: &Request) -> Answer {
    if matches!(request.method, Method::GET | Method::POST)
        && request.target.starts_with("/operator")
    {
        return retired::answer(&request.method, &request.target);
    }
    match route {
        ResidueRoute::GetFallback | ResidueRoute::HeadFallback => {
            if route == ResidueRoute::GetFallback
                && crate::dash::router::is_dynamic_get(&request.target)
            {
                return Err(Fault::Decline);
            }
            let root = native.options().dash_dir.as_deref().ok_or(Fault::Decline)?;
            static_response(root, request).await
        }
        ResidueRoute::PostUnknown => {
            if PENDING_POST.contains(&request.target.as_str()) {
                return Err(Fault::Decline);
            }
            let data = request
                .data
                .as_ref()
                .ok_or(Fault::Error(HandlerError::Failure))?;
            match target::post_target(native, &request.target, data).await {
                Ok(_) => reply(StatusCode::NOT_FOUND, &json!({"error":"Ruta desconocida"})),
                Err(answer) => answer,
            }
        }
        ResidueRoute::DeleteUnknown => {
            super::delete_body(request)?;
            reply(StatusCode::NOT_FOUND, &json!({"error":"No encontrado"}))
        }
    }
}
