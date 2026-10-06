#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]
use comandos_app::workspace_view::{DockEdge, dock_target, prune, shape, split_paths};
use serde_json::json;
use std::collections::BTreeSet;

#[test]
fn no_layout_has_no_dock_target() {
    assert!(dock_target(&serde_json::json!([]), 10.0, 10.0, &BTreeSet::new()).is_none());
}

#[test]
fn prune_collapses_absent_leaves_and_keeps_shape() {
    let tree = json!({"type":"split","axis":"x","ratio":0.4,"first":{"type":"tab","tabId":"a"},"second":{"type":"tab","tabId":"b"}});
    assert_eq!(
        prune(&tree, &BTreeSet::from(["b".to_string()])),
        json!({"type":"tab","tabId":"b"})
    );
    assert_eq!(
        shape(&tree),
        json!({"type":"split","axis":"x","ratio":0.4,"first":{"type":"tab"},"second":{"type":"tab"}})
    );
    assert_eq!(split_paths(&tree), vec![(vec![], 0.4)]);
}

#[test]
fn moved_origin_cannot_be_dock_destination() {
    let doc = json!({"groups":[{"id":"g","tree":{"type":"tab","tabId":"a"}}]});
    assert!(dock_target(&doc, 0.5, 0.5, &BTreeSet::from(["a".to_string()])).is_none());
    assert_eq!(
        dock_target(&doc, 0.5, 0.5, &BTreeSet::new()).unwrap().edge,
        DockEdge::Center
    );
}
