//! Deferred native feature payloads must not be fetched by worker installation.
pub fn is_deferred(path: &str) -> bool {
    matches!(
        path.rsplit('/').next(),
        Some("comandos_web_content.js" | "comandos_web_content_bg.wasm")
    )
}
#[cfg(test)]
mod tests {
    #[test]
    fn content_is_deferred_but_audio_and_shared_transport_remain_initial() {
        for name in ["comandos_web_content.js", "comandos_web_content_bg.wasm"] {
            assert!(super::is_deferred(&format!("0123456789ab/{name}")));
        }
        for name in [
            "comandos_web.js",
            "comandos_web_bg.wasm",
            "comandos_web_sound.js",
            "comandos_web_sound_bg.wasm",
            "snippets-comandos-web-dom-port.js",
            "native_workspace.css",
            "comandos_term_web_bg.wasm",
        ] {
            assert!(!super::is_deferred(&format!("0123456789ab/{name}")));
        }
    }
}
