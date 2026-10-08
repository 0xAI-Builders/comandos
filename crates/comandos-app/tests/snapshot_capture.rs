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

struct CountingReads<'a> {
    ctl: &'a comandos_app::tmux::TmuxCtl,
    calls: std::cell::RefCell<Vec<Vec<String>>>,
}
impl restore::RestoreTmux for CountingReads<'_> {
    type Ownership = comandos_app::tmux::OwnedSession;
    fn mode(&self) -> RunMode {
        self.ctl.mode()
    }
    fn read(&self, args: &[&str]) -> Result<String, String> {
        self.calls
            .borrow_mut()
            .push(args.iter().map(|v| v.to_string()).collect());
        restore::RestoreTmux::read(self.ctl, args)
    }
    fn mutate(&self, args: &[&str], stdin: Option<&[u8]>) -> Result<String, String> {
        restore::RestoreTmux::mutate(self.ctl, args, stdin)
    }
    fn create(&self, args: &[&str]) -> Result<(String, Self::Ownership), String> {
        restore::RestoreTmux::create(self.ctl, args)
    }
    fn cleanup(&self, own: Self::Ownership) -> Result<(), String> {
        restore::RestoreTmux::cleanup(self.ctl, own)
    }
}

#[test]
fn snapshot_reads_window_options_together_without_redundant_subprocesses() {
    let f = TestTmux::for_mode(RunMode::Sandbox).unwrap();
    f.tmux.new_session("term-options", 100, 30);
    let id = f
        .ctl
        .read(&["list-windows", "-t", "=term-options", "-F", "#{window_id}"])
        .unwrap()
        .stdout
        .trim()
        .to_owned();
    let counted = CountingReads {
        ctl: &f.ctl,
        calls: Default::default(),
    };
    for (border, rename, size) in [
        ("off", "off", "latest"),
        ("top", "on", "largest"),
        ("bottom", "off", "smallest"),
        ("off", "on", "manual"),
    ] {
        for (option, value) in [
            ("pane-border-status", border),
            ("automatic-rename", rename),
            ("window-size", size),
        ] {
            assert!(
                f.ctl
                    .mutate(&["set-option", "-w", "-t", &id, option, value])
                    .unwrap()
                    .ok()
            );
        }
        counted.calls.borrow_mut().clear();
        let captured =
            snapshot::capture_with(&counted, "term-options", |_| Ok(Default::default())).unwrap();
        let window = &captured["windows"][0];
        assert_eq!(window["border_status"], border);
        assert_eq!(window["automatic_rename"], rename);
        assert_eq!(window["window_size"], size);
        assert_eq!(window["panes"].as_array().unwrap().len(), 1);
        let calls = counted.calls.borrow();
        assert_eq!(
            calls.len(),
            3,
            "window options must not each fork another tmux process: {calls:?}"
        );
        assert_eq!(
            calls[2][0], "display-message",
            "the final layout race check must remain"
        );
        assert_eq!(calls[2].last().unwrap(), "#{window_layout}");
    }
}

#[test]
fn snapshot_options_keep_inherited_global_values() {
    let f = TestTmux::for_mode(RunMode::Sandbox).unwrap();
    f.tmux.new_session("term-inherited", 100, 30);
    for (option, value) in [
        ("pane-border-status", "bottom"),
        ("automatic-rename", "on"),
        ("window-size", "largest"),
    ] {
        assert!(
            f.ctl
                .mutate(&["set-option", "-g", option, value])
                .unwrap()
                .ok()
        );
    }
    let captured =
        snapshot::capture_with(&f.ctl, "term-inherited", |_| Ok(Default::default())).unwrap();
    assert_eq!(captured["windows"][0]["border_status"], "bottom");
    assert_eq!(captured["windows"][0]["automatic_rename"], "on");
    assert_eq!(captured["windows"][0]["window_size"], "largest");
}

#[test]
fn snapshot_still_rejects_layout_change_during_metadata_lookup() {
    let f = TestTmux::for_mode(RunMode::Sandbox).unwrap();
    f.tmux.new_session("term-layout-race", 100, 30);
    let captured = snapshot::capture_with(&f.ctl, "term-layout-race", |pane| {
        assert!(
            f.ctl
                .mutate(&[
                    "split-window",
                    "-d",
                    "-h",
                    "-t",
                    pane["id"].as_str().unwrap(),
                    "/bin/sh"
                ])
                .unwrap()
                .ok()
        );
        Ok(Default::default())
    });
    assert!(
        matches!(captured, Err(snapshot::SnapshotError::Invalid(ref reason)) if reason == "Window resized during capture")
    );
}
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
