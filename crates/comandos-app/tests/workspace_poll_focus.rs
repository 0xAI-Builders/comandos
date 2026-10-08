#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]
#[path = "support/t18_native.rs"]
mod native;

#[test]
fn metadata_refresh_keeps_popover_focus_and_changed_layout_restores_terminal_focus() {
    let method = native::body(
        include_str!("../src/ui/workspace.rs"),
        "fn restore_focus_after_apply(",
    );
    let source =
        include_str!("support/workspace_poll_focus.rs.txt").replace("// ACTUAL_METHOD", &method);
    assert_eq!(
        native::execute_source(&source, &serde_json::json!({})),
        serde_json::json!({"cases":4})
    );
}
