use comandos_desktop::{
    mode::RunMode,
    restore::{RestoreAction, RestorePlan},
};
use serde_json::json;
use std::collections::BTreeSet;

#[test]
fn local_session_omitted_by_the_legacy_registry_is_attached_without_recreation() {
    let plan = RestorePlan::build(
        &json!({"term-a":"Project"}),
        &json!({}),
        &BTreeSet::from(["local".into(), "term-a".into()]),
        RunMode::Live,
    )
    .unwrap();
    assert_eq!(
        plan.actions,
        vec![
            RestoreAction::AttachExisting {
                key: "local".into(),
                label: "Local".into()
            },
            RestoreAction::AttachExisting {
                key: "term-a".into(),
                label: "Project".into()
            },
        ]
    );
}
