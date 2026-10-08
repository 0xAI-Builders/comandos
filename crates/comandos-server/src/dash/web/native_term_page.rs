//! Opt-in compiled terminal page and its dedicated artifact admission.
use super::Manifest;
use comandos_core::web_assets::NativePage;
use std::{fs, path::Path};
pub const DESCRIPTOR: &str = "comandos_native_term_page.json";
pub const IDS: [&str; 2] = ["term-main", "term-tail"];
pub fn bundle(manifest: &Manifest, dir: &Path) -> Result<NativePage, String> {
    let rel = manifest.path(DESCRIPTOR);
    if rel.is_empty() {
        return Err("native terminal bundle missing".into());
    }
    let page: NativePage =
        serde_json::from_slice(&fs::read(dir.join(rel)).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
    let hash = super::registry::sha256_hex(
        comandos_web_view::term_page::shell(&Default::default())
            .into_string()
            .as_bytes(),
    );
    if page.version != 1 || page.template_sha256 != hash || page.components != IDS {
        return Err("native terminal bundle does not match compiled template/controllers".into());
    }
    for key in [
        "comandos_term_web_boot.js",
        "comandos_term_web.js",
        "comandos_term_web_bg.wasm",
    ] {
        let path = manifest.path(key);
        if path.is_empty() || !dir.join(path).is_file() {
            return Err(format!("native terminal artifact missing: {key}"));
        }
    }
    for root in [
        "/buttons.css",
        "/icon-192.png",
        "/assets/fonts/JetBrainsMono/JetBrainsMonoNerdFontMono-Regular.ttf",
        "/assets/fonts/JetBrainsMono/JetBrainsMonoNerdFontMono-Italic.ttf",
        "/assets/fonts/JetBrainsMono/JetBrainsMonoNerdFontMono-Bold.ttf",
        "/assets/fonts/JetBrainsMono/JetBrainsMonoNerdFontMono-BoldItalic.ttf",
    ] {
        if !page.assets.contains_key(root) {
            return Err(format!("native terminal asset missing: {root}"));
        }
    }
    for (root, logical) in &page.assets {
        if !root.starts_with('/')
            || root.contains(['?', '#', '\\', '\0'])
            || root.split('/').any(|s| s == ".." || s == ".")
        {
            return Err("invalid native terminal asset".into());
        }
        let path = manifest.path(logical);
        if path.is_empty() || !dir.join(path).is_file() {
            return Err(format!("native terminal asset missing: {logical}"));
        }
    }
    Ok(page)
}
pub fn render(manifest: &Manifest, page: &NativePage, nonce: &str) -> Vec<u8> {
    let mut text = comandos_web_view::term_page::shell(&Default::default()).into_string();
    for (root, logical) in &page.assets {
        let target = format!("/web/{}", manifest.path(logical));
        text = text
            .replace(&format!("../{}", root.trim_start_matches('/')), &target)
            .replace(
                &format!("href=\"{root}?v=2\""),
                &format!("href=\"{target}\""),
            );
    }
    text=text.replacen("</head>","<meta name=\"comandos-web-mode\" content=\"native-term\"><meta name=\"comandos-web\" content=\"term-main term-tail\"></head>",1);
    let boot = format!(
        "<script type=\"module\" data-k=\"{}\" src=\"/web/{}\"></script>",
        comandos_web_view::escape::attr_esc(nonce),
        comandos_web_view::escape::attr_esc(&manifest.path("comandos_term_web_boot.js"))
    );
    text.replacen("</body>", &format!("{boot}</body>"), 1)
        .into_bytes()
}
pub fn alias(manifest: &Manifest, dir: &Path, url: &str) -> Option<String> {
    let page = bundle(manifest, dir).ok()?;
    let key = page.assets.get(url)?;
    Some(manifest.path(key))
}
pub fn serve(state: &crate::dash::DashState) -> Result<crate::Reply, crate::HandlerError> {
    let manifest = Manifest::load_terminal(&state.web.web_dir).unwrap_or_default();
    let page = match bundle(&manifest, &state.web.web_dir) {
        Ok(page) => page,
        Err(error) => {
            return crate::Reply::json(
                http::StatusCode::SERVICE_UNAVAILABLE,
                &serde_json::json!({"error":error,"mode":"native-term"}),
            );
        }
    };
    let super::gate::Inserted::Nonce(nonce) = state.web.gate.insert_native_term() else {
        return crate::Reply::json(
            http::StatusCode::SERVICE_UNAVAILABLE,
            &serde_json::json!({"error":"native terminal readiness capacity exhausted"}),
        );
    };
    Ok(crate::Reply::bytes(
        http::StatusCode::OK,
        "text/html; charset=utf-8",
        render(&manifest, &page, &nonce),
    ))
}
