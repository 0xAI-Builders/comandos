#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::disallowed_methods
)]
mod support;
use comandos_app::{
    config::RunMode,
    restore::{self, RestorePlan},
    snapshot,
};
use comandos_runtime::pane_snapshot::PaneInspector;
use serde_json::json;
use std::{collections::BTreeSet, path::Path};
use support::tmux::TestTmux;
#[test]
fn capture_and_restore_private_tmux_geometry_without_agents() {
    let f = TestTmux::for_mode(RunMode::Sandbox).unwrap();
    let out = f
        .ctl
        .new_placeholder_session(&[
            "new-session",
            "-d",
            "-s",
            "term-source",
            "-x",
            "120",
            "-y",
            "32",
            "/bin/sh",
        ])
        .unwrap();
    assert!(out.0.ok());
    let split = f
        .ctl
        .mutate(&["split-window", "-d", "-h", "-t", "=term-source:", "/bin/sh"])
        .unwrap();
    assert!(split.ok());
    let proc_root = f.config.sandbox_root().unwrap().join("fake-proc");
    std::fs::create_dir(&proc_root).unwrap();
    let inspector =
        PaneInspector::new(&f.config.sandbox_root().unwrap().join("home"), &proc_root).unwrap();
    let captured = snapshot::capture_session(&f.ctl, "term-source", &inspector).unwrap();
    assert_eq!(captured["windows"][0]["panes"].as_array().unwrap().len(), 2);
    let plan = RestorePlan::build(
        &json!({"term-copy":"Copy"}),
        &json!({"term-copy":captured}),
        &BTreeSet::new(),
        RunMode::Sandbox,
    )
    .unwrap();
    let restored = restore::execute(&plan, &f.ctl, &restore::FixtureScopes, Path::new("/tmp"));
    assert!(restored.tabs[0].attached, "{restored:?}");
    let copy = snapshot::capture_session(&f.ctl, "term-copy", &inspector).unwrap();
    assert_eq!(copy["windows"][0]["width"], captured["windows"][0]["width"]);
    assert_eq!(
        copy["windows"][0]["height"],
        captured["windows"][0]["height"]
    );
    assert_eq!(copy["windows"][0]["panes"].as_array().unwrap().len(), 2);
    let expected = restore::remap_layout(
        captured["windows"][0]["layout"].as_str().unwrap(),
        &restored.tabs[0].pane_mapping,
    )
    .unwrap();
    assert_eq!(copy["windows"][0]["layout"], expected);
}
#[test]
fn recycled_process_start_ticks_are_read_from_injected_proc() {
    let f = TestTmux::for_mode(RunMode::Sandbox).unwrap();
    let root = f.config.sandbox_root().unwrap().join("fake-proc");
    std::fs::create_dir_all(root.join("42")).unwrap();
    let mut fields = vec!["0"; 20];
    fields[19] = "987654";
    std::fs::write(
        root.join("42/stat"),
        format!("42 (shell (nested)) {}", fields.join(" ")),
    )
    .unwrap();
    assert_eq!(snapshot::process_start_time(&root, 42), 987654);
    assert_eq!(
        snapshot::process_start_time(&root, 43),
        serde_json::Value::Null
    );
}
