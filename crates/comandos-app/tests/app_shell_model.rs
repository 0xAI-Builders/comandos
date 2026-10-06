#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]
use comandos_app::ui::webview::dashboard_uri;

#[test]
fn disconnected_dashboard_has_no_live_url() {
    assert!(dashboard_uri(None, "fixture-v1").is_none());
    let uri = dashboard_uri(Some("http://127.0.0.1:7311"), "fixture-v1").expect("fixture");
    assert!(uri.contains("app=1") && uri.contains("anwin=1") && uri.contains("fixture-v1"));
}
