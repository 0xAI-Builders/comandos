//! Standalone extension shelf: the Rust controller, without the legacy script.
use crate::{HandlerError, Reply, dash::DashState};
use http::StatusCode;

pub fn serve(state: &DashState) -> Result<Reply, HandlerError> {
    let manifest = state.web.manifest();
    if let Err(error) = manifest.check_files(&state.web.web_dir) {
        return Reply::json(
            StatusCode::SERVICE_UNAVAILABLE,
            &serde_json::json!({"error":error,"mode":"native-extensions"}),
        );
    }
    let super::gate::Inserted::Nonce(nonce) = state.web.gate.insert_native_extensions() else {
        return Reply::json(
            StatusCode::SERVICE_UNAVAILABLE,
            &serde_json::json!({"error":"native extensions readiness capacity exhausted"}),
        );
    };
    let page = format!(
        concat!(
            "<!DOCTYPE html><html lang=\"es\"><head><meta charset=\"utf-8\">",
            "<meta name=\"viewport\" content=\"width=device-width,initial-scale=1\">",
            "<meta name=\"comandos-web\" content=\"extensions\">",
            "<meta name=\"comandos-web-mode\" content=\"native\">",
            "<link rel=\"icon\" href=\"data:,\"><title>Extensiones del panel · ComandOS</title>",
            "<link rel=\"stylesheet\" href=\"/extensions.css?v=fuentesD\">",
            "<link rel=\"stylesheet\" href=\"/buttons.css?v=2\"></head><body>",
            "<main id=\"extensions\" aria-label=\"Extensiones del panel seleccionado\" aria-busy=\"true\">",
            "<section class=\"shelf\"><header><strong>MCPs y skills</strong></header>",
            "<div class=\"loading\" role=\"status\"><span class=\"loading-spinner\" aria-hidden=\"true\"></span>",
            "<span>Cargando MCPs y skills…</span></div></section></main>",
            "<script type=\"module\" data-k=\"{}\" src=\"/web/{}\"></script></body></html>"
        ),
        comandos_web_view::escape::attr_esc(&nonce),
        comandos_web_view::escape::attr_esc(&manifest.path("comandos_web_boot.js")),
    );
    Ok(Reply::bytes(
        StatusCode::OK,
        "text/html; charset=utf-8",
        page.into_bytes(),
    ))
}
