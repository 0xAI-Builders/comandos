#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]
use comandos_app::config::RunMode;
use comandos_app::restore::{RestoreAction, RestoreCoordinator, RestorePlan};
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

#[test]
fn missing_session_creates_placeholder_before_layout_and_resume() {
    let plan = RestorePlan::build(
        &json!({"term-a":"A"}),
        &json!({"term-a":{
            "windows":[{"panes":[{"id":"%1"}]}],
            "resume_id":"12345678-1234-1234-1234-123456789abc"
        }}),
        &BTreeSet::new(),
        RunMode::Sandbox,
    )
    .unwrap();
    assert!(matches!(
        plan.actions.as_slice(),
        [
            RestoreAction::CreatePlaceholder { key, .. },
            RestoreAction::RestoreLayout { key: layout_key, .. },
            RestoreAction::ResumeExact { key: resume_key, .. },
        ] if key == "term-a" && layout_key == "term-a" && resume_key == "term-a"
    ));
}
