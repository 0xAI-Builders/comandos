#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]
mod support;
use comandos_app::{config::RunMode, snapshot, tmux::TmuxOut};
use std::cell::Cell;
use support::tmux::TestTmux;

fn batch(tmux: &TestTmux, commands: &[Vec<String>]) -> TmuxOut {
    let mut args = Vec::new();
    for command in commands {
        if !args.is_empty() {
            args.push(";");
        }
        args.extend(command.iter().map(String::as_str));
    }
    let out = tmux.raw(&args);
    TmuxOut {
        code: out.status.code().unwrap_or(1),
        stdout: String::from_utf8(out.stdout).unwrap(),
        stderr: String::from_utf8(out.stderr).unwrap(),
    }
}

#[test]
fn twenty_sessions_match_individual_capture_using_three_tmux_processes() {
    let fixture = TestTmux::for_mode(RunMode::Sandbox).unwrap();
    let names: Vec<_> = (0..20).map(|i| format!("term-batch-{i}")).collect();
    for name in &names {
        fixture.tmux.new_session(name, 100, 30);
    }
    let mut calls = 0;
    let all = snapshot::capture_sessions_with(
        &names,
        |commands| {
            calls += 1;
            Ok(batch(&fixture.tmux, commands))
        },
        |_| Ok(Default::default()),
        &|| true,
    )
    .unwrap();
    assert_eq!(calls, 3);
    assert_eq!(all.len(), 20);
    for name in &names {
        let individual =
            snapshot::capture_with(&fixture.ctl, name, |_| Ok(Default::default())).unwrap();
        assert_eq!(all[name], individual);
    }
    let root = fixture.config.sandbox_root().unwrap();
    let fake_proc = root.join("batch-proc");
    fixture.guard.create_dir_all(&fake_proc, 0o700).unwrap();
    let inspector =
        comandos_runtime::pane_snapshot::PaneInspector::new(&root.join("home"), &fake_proc)
            .unwrap();
    let real_transport =
        snapshot::capture_sessions_when(&fixture.ctl, &names, &inspector, &|| true).unwrap();
    assert_eq!(real_transport.len(), 20);
    for name in &names {
        assert_eq!(real_transport[name]["windows"].as_array().unwrap().len(), 1);
    }
}

#[test]
fn failed_middle_command_never_returns_partial_batch_even_if_last_command_succeeds() {
    let fixture = TestTmux::for_mode(RunMode::Sandbox).unwrap();
    fixture.tmux.new_session("term-good", 100, 30);
    let result = snapshot::capture_sessions_with(
        &[
            "term-good".into(),
            "term-missing".into(),
            "term-good".into(),
        ],
        |commands| {
            let mut out = batch(&fixture.tmux, commands);
            // Also cover a transport reporting only the last command's success.
            out.code = 0;
            out.stderr.clear();
            Ok(out)
        },
        |_| Ok(Default::default()),
        &|| true,
    );
    assert!(result.is_err());
    assert!(snapshot::capture_with(&fixture.ctl, "term-good", |_| Ok(Default::default())).is_ok());
}

#[test]
fn final_layout_check_rejects_change_during_metadata_inspection() {
    let fixture = TestTmux::for_mode(RunMode::Sandbox).unwrap();
    fixture.tmux.new_session("term-changing", 100, 30);
    let mut calls = 0;
    let result = snapshot::capture_sessions_with(
        &["term-changing".into()],
        |commands| {
            calls += 1;
            Ok(batch(&fixture.tmux, commands))
        },
        |pane| {
            assert!(
                fixture
                    .tmux
                    .raw(&[
                        "split-window",
                        "-d",
                        "-h",
                        "-t",
                        pane["id"].as_str().unwrap(),
                        "/bin/sh"
                    ])
                    .status
                    .success()
            );
            Ok(Default::default())
        },
        &|| true,
    );
    assert_eq!(calls, 3);
    assert!(
        matches!(result, Err(snapshot::SnapshotError::Invalid(reason)) if reason == "Window resized during capture")
    );
}

#[test]
fn cancellation_stops_before_final_read_and_never_publishes_partial_state() {
    let fixture = TestTmux::for_mode(RunMode::Sandbox).unwrap();
    fixture.tmux.new_session("term-cancel", 100, 30);
    let allowed = Cell::new(true);
    let mut calls = 0;
    let result = snapshot::capture_sessions_with(
        &["term-cancel".into()],
        |commands| {
            calls += 1;
            Ok(batch(&fixture.tmux, commands))
        },
        |_| {
            allowed.set(false);
            Ok(Default::default())
        },
        &|| allowed.get(),
    );
    assert!(result.is_err());
    assert_eq!(calls, 2);
}

#[test]
fn empty_and_oversized_batches_do_not_start_tmux() {
    for names in [vec![], vec!["owned".to_string(); 257]] {
        let result = snapshot::capture_sessions_with(
            &names,
            |_| panic!("no command should run"),
            |_| panic!("no pane should be inspected"),
            &|| true,
        );
        assert_eq!(result.is_ok(), names.is_empty());
    }
}
