#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]
use comandos_app::config::RunMode;
use comandos_app::restore::{RestoreCoordinator, RestorePlan};
use serde_json::json;
use std::collections::BTreeSet;

#[test]
fn startup_blocks_snapshots_until_restore_finishes() {
    let mut coordinator = RestoreCoordinator::default();
    coordinator.begin();
    assert!(!coordinator.ready());
}

#[test]
fn restore_existing_never_sends_resume() {
    let plan = RestorePlan::build(
        &json!({"term-a":"A"}),
        &json!({"term-a":{"resume_id":"12345678-1234-1234-1234-123456789abc"}}),
        &BTreeSet::from(["term-a".to_string()]),
        RunMode::Sandbox,
    )
    .unwrap();
    assert!(matches!(
        plan.actions[0],
        comandos_app::restore::RestoreAction::AttachExisting { .. }
    ));
}
