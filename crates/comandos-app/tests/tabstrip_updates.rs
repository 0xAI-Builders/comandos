#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]
#[allow(dead_code)]
#[path = "support/t18_native.rs"]
mod native;
use serde_json::json;

#[test]
fn actual_set_items_keeps_cards_stable_when_labels_change() {
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
    assert_eq!(result["rows_width_rebuild"], 2);
    assert_eq!(result["rows_allocation_rebuild"], 4);
    assert_eq!(result["rows_replaced_attach"], 2);
    assert_eq!(result["height"], 42);
}

#[test]
fn rows_mode_reveals_the_tab_container_and_preserves_all_tabs() {
    let source = include_str!("../src/ui/tabstrip.rs");
    let methods = native::body(source, "pub fn set_items(")
        + &native::body(source, "pub fn reflow(")
        + &native::body(source, "pub fn set_rows(");
    let probe = include_str!("support/tabstrip_updates.rs.txt")
        .replace("// ACTUAL_METHODS", &methods)
        .replace(
            "// ROWS_VISIBILITY_PROBE",
            r#"
    let v = make(false);
    v.set_items((0..21).map(|i| (format!("tab-{i}"), Widget::new(100+i, 60))).collect());
    v.set_rows(true);
    assert!(v.0.rows_view.0.visible.get(), "rows viewport must be visible");
    assert!(v.0.flow.0.visible.get(), "rows container must be visible");
    assert!(!v.0.scroller.0.visible.get());
    assert_eq!(v.0.flow.children().iter().map(|line| line.children().len()).sum::<usize>(), 21);
    v.set_rows(false);
    assert!(!v.0.rows_view.0.visible.get());
    assert!(v.0.scroller.0.visible.get());
    assert_eq!(v.0.items.borrow().len(), 21);
    v.set_rows(true);
    assert!(v.0.flow.0.visible.get());
"#,
        );
    native::execute_source(&probe, &json!({}));
}

#[test]
fn arrow_navigation_starts_at_local_and_keeps_the_other_sessions_in_order() {
    use comandos_app::ui::tabstrip::cycle_page;
    // Notebook stores Local last; the visible card order is Local, A, B, C.
    assert_eq!(cycle_page(4, Some(3), Some(3), 1), Some(0));
    assert_eq!(cycle_page(4, Some(0), Some(3), -1), Some(3));
    assert_eq!(cycle_page(4, Some(1), Some(3), 1), Some(2));
    assert_eq!(cycle_page(4, Some(2), Some(3), 1), Some(3));
    assert_eq!(cycle_page(4, Some(3), Some(3), -1), Some(2));
    assert_eq!(cycle_page(4, None, Some(3), 1), Some(3));
    assert_eq!(cycle_page(1, Some(0), Some(0), -1), Some(0));
    assert_eq!(cycle_page(0, None, None, 1), None);
    assert_eq!(cycle_page(3, Some(0), None, -1), Some(2));
}
