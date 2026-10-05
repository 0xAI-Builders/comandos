//! D8, rama A: rutas de la barra retirada sin efectos ni reenvío.
mod support;

use support::{FakeLegacy, TestHome, front, get, request_body};

const BODY: &str = r#"{"error": "El chat de CommandOS se retir\u00f3; usa la barra de comandos", "code": "retired"}"#;

#[tokio::test]
async fn command_bar_routes_are_retired_without_effects_or_legacy() {
    let home = TestHome::new("ops-bar-retired");
    home.write("proxy.json", r#"{"enabled":true}"#);
    home.write("motor-results.json", r#"{"sentinel":{"ts":1}}"#);
    let legacy = FakeLegacy::start().await;
    let front = front(&home, legacy.port, home.options()).await;
    for path in ["/proxy", "/session-config-history?session=s&pane=%250"] {
        let reply = get(front.port, path).await;
        assert_eq!((reply.status, reply.text().as_str()), (410, BODY), "{path}");
    }
    for path in [
        "/proxy",
        "/harness/switch",
        "/model/switch-cancel",
        "/session/recover",
    ] {
        let reply = request_body(
            front.port,
            "POST",
            path,
            "",
            r#"{"session":"s","pane":"%0","enable":false,"motor":"codex"}"#,
        )
        .await;
        assert_eq!((reply.status, reply.text().as_str()), (410, BODY), "{path}");
    }
    assert!(legacy.requests().is_empty());
    assert_eq!(
        std::fs::read_to_string(home.hooks().join("proxy.json")).unwrap(),
        r#"{"enabled":true}"#
    );
    assert_eq!(
        std::fs::read_to_string(home.hooks().join("motor-results.json")).unwrap(),
        r#"{"sentinel":{"ts":1}}"#
    );
    assert!(!home.root.join("systemctl.log").exists());
    front.stop().await;
}
