#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]
#[allow(dead_code)]
#[path = "support/t18_native.rs"]
mod native;
use serde_json::json;

#[test]
fn actual_set_items_keeps_stable_widgets_but_reflows_changed_widths() {
    let source = include_str!("../src/ui/tabstrip.rs");
    let methods =
        native::body(source, "pub fn set_items(") + &native::body(source, "pub fn reflow(");
    let probe =
        include_str!("support/tabstrip_updates.rs.txt").replace("// ACTUAL_METHODS", &methods);
    let result = native::execute_source(&probe, &json!({}));
    assert_eq!(
        result["stable_attach"], 1,
        "unchanged poll must retain parents"
    );
    assert_eq!(
        result["replacement_attach"], 2,
        "same key/new widget must attach"
    );
    assert_eq!(result["reorder_attach"], 3);
    assert_eq!(result["changed_key_attach"], 4);
    assert_eq!(result["empty_attach"], 5);
    assert_eq!(result["rows_stable_attach"], 1);
    assert_eq!(result["rows_stable_rebuild"], 2);
    assert_eq!(result["rows_width_rebuild"], 3);
    assert_eq!(result["rows_allocation_rebuild"], 5);
    assert_eq!(result["rows_replaced_attach"], 2);
    assert_eq!(result["height"], 42);
}
