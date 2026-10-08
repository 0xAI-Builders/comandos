#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]
#[allow(dead_code)]
#[path = "support/t18_native.rs"]
mod native;

#[test]
fn overview_keeps_every_open_tab_in_a_split_workspace() {
    let method = native::body(
        include_str!("../src/ui/app_t15.rs"),
        "fn switcher_candidates(",
    );
    let probe =
        include_str!("support/switcher_workspace.rs.txt").replace("// ACTUAL_METHOD", &method);
    let receipt = native::execute_source(&probe, &serde_json::json!({}));
    assert_eq!(
        receipt["open"],
        serde_json::json!(["term-a", "term-b", "web:docs"])
    );
    assert_eq!(receipt["closed"], serde_json::json!(["term-closed"]));
}
