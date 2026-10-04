//! E. Rutas sin llamador vivo (inventario §1.12): 410 con el cuerpo exacto de
//! `GET /operator` (`OPERATOR_RETIRED`, `bin/cc-dash:5751`).
//!
//! Siguen reenviadas: la UI commiteada aún las llama (barra de comandos
//! pendiente de fusionar): GET y POST `/proxy`, POST `/harness/switch`,
//! POST `/model/switch-cancel`, GET `/session-config-history` y POST
//! `/session/recover`.
use super::{Answer, Entry, Key, NativeRoute, Verb, reply};
use http::{Method, StatusCode};
use serde_json::json;
use std::{
    collections::BTreeSet,
    sync::{Mutex, OnceLock},
};

pub const OPERATOR_RETIRED_ERROR: &str = "El chat de CommandOS se retiró; usa la barra de comandos";

/// GET por ruta (consulta opcional).
pub const GET_PATHS: [&str; 12] = [
    "/pomodoro/report",
    "/project-profiles",
    "/events",
    "/dedication",
    "/ui-log/summary",
    "/session-brain",
    "/usage/guard",
    "/usage/changes",
    "/usage/provider-compare",
    "/usage/experiments",
    "/usage/analytics",
    "/usage/interactions",
];

pub const POST_PATHS: [&str; 19] = [
    "/project-profile",
    "/pause",
    "/usage/capture",
    "/chains/delete",
    "/app/command",
    "/event",
    "/optimization/default",
    "/skill-toggle",
    "/mcp-toggle",
    "/usage/experiment",
    "/usage/rating",
    "/usage/refresh",
    "/usage/quota",
    "/usage/subscription",
    "/usage/settings",
    "/news/refresh",
    "/models/refresh",
    "/open-with-account",
    "/tab-new",
];

const fn get(path: &'static str) -> Entry {
    Entry {
        verb: Verb::Get,
        key: Key::Path(path),
        route: NativeRoute::Retired,
    }
}

const fn post(path: &'static str) -> Entry {
    Entry {
        verb: Verb::Post,
        key: Key::Raw(path),
        route: NativeRoute::Retired,
    }
}

pub const ROUTES: &[Entry] = &[
    get(GET_PATHS[0]),
    get(GET_PATHS[1]),
    get(GET_PATHS[2]),
    get(GET_PATHS[3]),
    get(GET_PATHS[4]),
    get(GET_PATHS[5]),
    get(GET_PATHS[6]),
    get(GET_PATHS[7]),
    get(GET_PATHS[8]),
    get(GET_PATHS[9]),
    get(GET_PATHS[10]),
    get(GET_PATHS[11]),
    post(POST_PATHS[0]),
    post(POST_PATHS[1]),
    post(POST_PATHS[2]),
    post(POST_PATHS[3]),
    post(POST_PATHS[4]),
    post(POST_PATHS[5]),
    post(POST_PATHS[6]),
    post(POST_PATHS[7]),
    post(POST_PATHS[8]),
    post(POST_PATHS[9]),
    post(POST_PATHS[10]),
    post(POST_PATHS[11]),
    post(POST_PATHS[12]),
    post(POST_PATHS[13]),
    post(POST_PATHS[14]),
    post(POST_PATHS[15]),
    post(POST_PATHS[16]),
    post(POST_PATHS[17]),
    post(POST_PATHS[18]),
];

pub fn answer(method: &Method, path: &str) -> Answer {
    note_first_hit(method, path);
    reply(
        StatusCode::GONE,
        &json!({"error": OPERATOR_RETIRED_ERROR, "code": "retired"}),
    )
}

/// Las rutas retiradas no dejaban rastro: un llamador externo que aún las use (un plugin
/// antiguo, un script) perdería sus avisos en silencio. Una línea en stderr por ruta, la
/// primera vez que se pisa, basta para notarlo en el journal.
fn note_first_hit(method: &Method, path: &str) {
    static SEEN: OnceLock<Mutex<BTreeSet<String>>> = OnceLock::new();
    let key = format!("{method} {path}");
    let seen = SEEN.get_or_init(|| Mutex::new(BTreeSet::new()));
    let Ok(mut seen) = seen.lock() else {
        return;
    };
    if seen.insert(key.clone()) {
        eprintln!("comandos dash: ruta retirada pisada por primera vez: {key} → 410");
    }
}
