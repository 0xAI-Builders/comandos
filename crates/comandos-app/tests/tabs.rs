#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]
use comandos_app::tabs::{TabKind, TabRecord, TabRegistry};
use serde_json::json;

#[test]
fn malformed_tabs_never_become_a_snapshot() {
    assert!(TabRegistry::from_json(&serde_json::json!("broken")).is_err());
}

#[test]
fn registry_preserves_labels_and_orders_local_favorites_first() {
    let mut registry =
        TabRegistry::from_json(&json!({"b": "Bee", "local": "Local", "a": "Aye"})).unwrap();
    registry.apply_favorites(&json!(["b"]), 7);
    assert_eq!(registry.ordered_keys(), vec!["local", "b", "a"]);
    registry.rename("a", "Renamed");
    registry.rename("a", "");
    assert_eq!(
        registry.to_json(),
        json!({"b": "Bee", "local": "Local", "a": "Renamed"})
    );
}

#[test]
fn stale_favorite_generation_is_ignored() {
    let mut registry = TabRegistry::from_json(&json!({"a": "A", "b": "B"})).unwrap();
    registry.apply_favorites(&json!(["a"]), 10);
    registry.apply_favorites(&json!(["b"]), 9);
    assert_eq!(registry.ordered_keys(), vec!["a", "b"]);
}

#[test]
fn insert_and_archive_update_inventory_without_history_leakage() {
    let mut registry = TabRegistry::from_json(&json!({"local": "Local"})).unwrap();
    registry.insert(TabRecord {
        key: "web:docs".into(),
        label: "Docs".into(),
        favorite: false,
        kind: TabKind::Web,
    });
    assert_eq!(
        registry.to_json(),
        json!({"local": "Local", "web:docs": "Docs"})
    );
    registry.archive("web:docs", "closed");
    assert_eq!(registry.to_json(), json!({"local": "Local"}));
}
