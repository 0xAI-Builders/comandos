#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]
#[allow(dead_code)]
#[path = "support/t18_native.rs"]
mod native;

#[test]
fn auxiliary_page_uses_parent_storage_and_keeps_its_own_bridge() {
    let source = include_str!("../src/ui/webview.rs");
    let constructor = native::body(source, "fn create_for_page(");
    let setup = &constructor[constructor.find('{').unwrap() + 1
        ..constructor.find("webview.set_hexpand(true)").unwrap()];
    let probe = include_str!("support/webview_context.rs.txt").replace("// CONSTRUCTOR", setup);
    let receipt = native::execute_source(&probe, &serde_json::json!({}));
    assert_eq!(receipt["shared_context"], true);
    assert_eq!(receipt["request_visible"], true);
    assert_eq!(receipt["separate_handlers"], true);
    assert_eq!(receipt["profiles_isolated"], true);
}
