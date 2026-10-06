#[test]
fn dock_registered() {
    assert!(
        comandos_web::registry::COMPONENTS
            .iter()
            .any(|c| c.id == "workspace-dock" && c.attach.is_some())
    );
}
use comandos_web::components::workspace_dock::*;
use serde_json::json;
fn rect() -> Rect {
    Rect {
        left: 0.0,
        top: 0.0,
        width: 200.0,
        height: 100.0,
    }
}
#[test]
fn original_geometry_and_scroll_contract() {
    let r = rect();
    assert_eq!(edge_for(r, 10.0, 50.0, 0.31), Some("left"));
    assert_eq!(edge_for(r, 195.0, 50.0, 0.31), Some("right"));
    assert_eq!(edge_for(r, 100.0, 48.0, 0.31), Some("top"));
    assert_eq!(edge_for(r, 300.0, 50.0, 0.31), None);
    assert_eq!(outer_edge(r, 3.0, 50.0, 16.0), Some("left"));
    assert_eq!(outer_edge(r, 100.0, 50.0, 16.0), None);
    assert_eq!(
        preview_rect(r, "right"),
        Rect {
            left: 100.0,
            width: 100.0,
            ..r
        }
    );
    assert_eq!(edge_scroll(400.0, 100.0, 700.0, 56.0, 18.0), 0.0);
    assert!(edge_scroll(105.0, 100.0, 700.0, 56.0, 18.0) < -10.0);
    assert!(
        edge_scroll(150.0, 100.0, 700.0, 56.0, 18.0) > edge_scroll(105.0, 100.0, 700.0, 56.0, 18.0)
    );
    assert_eq!(
        TRAYS.iter().map(|x| x.0).collect::<Vec<_>>(),
        vec!["frozen", "awaiting_reply", "resolved", "none"]
    );
}
#[test]
fn nested_stacked_rows_follow_measured_height_not_equal_halves() {
    let tree = json!({"type":"split","axis":"x","ratio":0.5,"first":{"type":"tab","tabId":"a"},"second":{"type":"split","axis":"y","ratio":0.5,"first":{"type":"tab","tabId":"b"},"second":{"type":"tab","tabId":"c"}}});
    let wide = measure(&tree, 1400.0, None);
    assert_eq!(wide.axis, "x");
    let narrow = measure(&tree, 390.0, None);
    assert_eq!(narrow.axis, "y");
    assert_eq!(narrow.height, 1320.0);
    assert_eq!(narrow.a.unwrap().height, 440.0);
    assert_eq!(narrow.b.unwrap().height, 880.0);
}
#[test]
fn adoption_cannot_yank_pending_or_gesturing_layout_and_preserves_bindings() {
    let mut d = Dock::default();
    let state = json!({"schema":1,"revision":4,"groups":[],"tabs":{},"bindings":{"x":1}});
    assert!(d.adopt(state.clone()));
    assert!(!d.adopt(state.clone()));
    assert_eq!(d.revision, 4);
    assert_eq!(d.doc.as_ref().unwrap()["bindings"], json!({"x":1}));
    d.pending = true;
    let mut next = state.clone();
    next["revision"] = json!(5);
    assert!(!d.adopt(next.clone()));
    d.pending = false;
    d.gesturing = true;
    assert!(!d.adopt(next));
    assert_eq!(d.revision, 4);
}
#[test]
fn ratio_free_signature_keeps_same_dom_and_scoped_tabs_keep_missing_sessions() {
    let n = json!({"type":"split","axis":"x","ratio":0.3,"first":{"type":"tab","tabId":"a"},"second":{"type":"tab","tabId":"b"}});
    let mut other = n.clone();
    other["ratio"] = json!(0.8);
    assert_eq!(strip_ratios(&n), strip_ratios(&other));
    let mut d = Dock::default();
    d.adopt(json!({"revision":1,"groups":[{"id":"g","tree":n}],"tabs":{"a":{"session":"s"},"b":{"session":"missing"}}}));
    assert_eq!(d.visible_tabs("a"), vec!["s", "missing"]);
    assert_eq!(d.visible_tabs("ungrouped"), vec!["ungrouped"]);
    assert!(d.visible_tabs("").is_empty());
}
