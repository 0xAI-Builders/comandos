//! Service-worker admission: only same-origin static GET requests may be cached.
pub const SHELL: &str = "comandos-shell-v13";
pub const LIVE: &[&str] = &[
    "/state",
    "/events",
    "/conf",
    "/prefs",
    "/ssh",
    "/tabs",
    "/tab-history",
    "/remote-state",
    "/remote-qr.png",
    "/webterm-token",
    "/term",
    "/operator",
    "/session-",
    "/extension-usage",
    "/model/",
    "/usage/",
    "/providers",
    "/news/",
    "/push/",
];
pub fn intercepts(origin: &str, ours: &str, method: &str, path: &str) -> bool {
    origin == ours
        && method == "GET"
        && !LIVE.iter().any(|prefix| path.starts_with(prefix))
        && (path == "/"
            || [
                ".html",
                ".css",
                ".js",
                ".png",
                ".svg",
                ".ico",
                ".woff",
                ".woff2",
                ".ttf",
                ".webmanifest",
            ]
            .iter()
            .any(|extension| path.ends_with(extension)))
}
pub fn valid_event_id(value: &str) -> bool {
    value.encode_utf16().count() <= 128 && !value.chars().any(|ch| ch <= '\u{1f}')
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn static_shell_admission_never_intercepts_live_or_unknown_api() {
        let ours = "https://dash.test";
        for path in [
            "/",
            "/manifest.webmanifest",
            "/buttons.css",
            "/x/y.js",
            "/icon.png",
            "/font.woff",
            "/font.woff2",
            "/font.ttf",
            "/help.html",
        ] {
            assert!(intercepts(ours, ours, "GET", path), "static {path}");
        }
        for path in [
            "/state",
            "/state.css",
            "/events",
            "/conf",
            "/prefs",
            "/ssh",
            "/tabs",
            "/tab-history",
            "/remote-state",
            "/remote-qr.png",
            "/webterm-token",
            "/term/",
            "/operator/chat/stream",
            "/session-close",
            "/extension-usage",
            "/model/status",
            "/usage/state",
            "/providers",
            "/news/x.js",
            "/push/public-key",
            "/new-api",
            "/unknown.json",
            "/icon.PNG",
            "/font.woff22",
        ] {
            assert!(!intercepts(ours, ours, "GET", path), "live {path}");
        }
        assert!(!intercepts("https://else.test", ours, "GET", "/"));
        for method in ["POST", "PUT", "DELETE", "HEAD", "get"] {
            assert!(!intercepts(ours, ours, method, "/"));
        }
    }
    #[test]
    fn event_identity_uses_utf16_limit_and_rejects_control_characters() {
        assert!(valid_event_id("a b&c"));
        assert!(valid_event_id(&"😀".repeat(64)));
        assert!(!valid_event_id(&"😀".repeat(65)));
        assert!(valid_event_id(&"a".repeat(128)));
        assert!(!valid_event_id(&"a".repeat(129)));
        for ch in 0..=31 {
            assert!(!valid_event_id(&char::from(ch).to_string()));
        }
        assert!(valid_event_id("\u{7f}"));
    }
}
