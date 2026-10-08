#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]
use comandos_app::{
    config::RunMode,
    restore::{RestoreAction, RestorePlan},
    tabs::TabRegistry,
};
use serde_json::json;
use std::collections::BTreeSet;
#[test]
fn gtk_models_and_policies_are_the_shared_types_without_conversion() {
    let registry = TabRegistry::from_json(&json!({"local":"Local","web:owned":"Owned"})).unwrap();
    let shared: &comandos_desktop::tabs::TabRegistry = &registry;
    assert_eq!(shared.to_json(), registry.to_json());
    let plan = RestorePlan::build(
        &json!({"term-owned":"Owned"}),
        &json!({}),
        &BTreeSet::new(),
        RunMode::Shadow,
    )
    .unwrap();
    let shared: &comandos_desktop::restore::RestorePlan = &plan;
    assert!(matches!(
        shared.actions.as_slice(),
        [RestoreAction::ShowAmbiguity { .. }]
    ));
    let mode: comandos_desktop::mode::RunMode = RunMode::Shadow;
    assert_eq!(mode, RunMode::Shadow);
    fn state(
        _: &comandos_desktop::state_files::StateFiles<
            comandos_app::config::AppConfig,
            comandos_app::guard::WriteGuard,
        >,
    ) {
    }
    let _: fn(&comandos_app::state_files::StateFiles) = state;
    fn ipc(_: &comandos_desktop::ipc::IpcConsumer<comandos_app::guard::WriteGuard>) {}
    let _: fn(&comandos_app::ipc::IpcConsumer) = ipc;
    let raw = r#"{"theme":"neon"}"#;
    let actual: comandos_desktop::bridge::BridgeMsg =
        comandos_app::ui::bridge::parse_bridge(raw).unwrap();
    assert_eq!(actual, comandos_desktop::bridge::parse_bridge(raw).unwrap());
}
