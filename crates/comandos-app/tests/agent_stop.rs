#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]
use comandos_app::agent_stop::{CtrlCAction, ctrl_c_action};
#[test]
fn selection_uses_copy_before_stop() {
    assert_eq!(ctrl_c_action(true, false, 1., 0.9, 0.4), CtrlCAction::Copy);
    assert_eq!(
        ctrl_c_action(false, true, 1., 0.9, 0.4),
        CtrlCAction::SendInterrupt
    );
    assert_eq!(
        ctrl_c_action(false, false, 1., f64::NAN, 1.),
        CtrlCAction::SendInterrupt
    );
    assert_eq!(
        ctrl_c_action(false, false, 1., 0.9, 1.),
        CtrlCAction::Cleanup
    );
    assert_eq!(
        ctrl_c_action(false, false, 0.8, 0.9, 1.),
        CtrlCAction::SendInterrupt
    );
}
use comandos_app::{
    agent_stop::{
        Clock, PaneCloseIntent, ProcessReader, ProcessRecord, Signal, SignalSink, StopPlan,
    },
    config::RunMode,
};
use std::{cell::RefCell, collections::BTreeMap, sync::atomic::AtomicBool, time::Duration};
fn process(pid: u32, parent: u32, group: u32, fg: i32, name: &str) -> ProcessRecord {
    ProcessRecord {
        pid,
        parent,
        group,
        foreground: fg,
        start_time: 100 + pid as u64,
        argv: vec![name.into()],
        comm: name.into(),
        session: 1,
    }
}
fn fixture() -> Vec<ProcessRecord> {
    vec![
        process(10, 1, 10, 20, "bash"),
        process(20, 10, 20, 20, "codex"),
        process(30, 20, 30, 20, "mcp-tool"),
        process(31, 30, 30, 20, "worker"),
        process(40, 20, 20, 20, "same-group"),
        process(50, 10, 50, 20, "unrelated-shell"),
    ]
}
#[derive(Default)]
struct Reader {
    records: RefCell<BTreeMap<u32, ProcessRecord>>,
}
impl ProcessReader for Reader {
    fn snapshot(&self) -> Result<Vec<ProcessRecord>, String> {
        Ok(self.records.borrow().values().cloned().collect())
    }
    fn read(&self, pid: u32) -> Option<ProcessRecord> {
        self.records.borrow().get(&pid).cloned()
    }
}
#[derive(Default)]
struct Sink {
    sent: RefCell<Vec<(u32, Signal)>>,
    race: RefCell<Option<ProcessRecord>>,
}
impl SignalSink for Sink {
    type Handle = ProcessRecord;
    fn pin(&self, rec: &ProcessRecord, reader: &dyn ProcessReader) -> Result<Self::Handle, String> {
        if let Some(race) = self.race.borrow_mut().take() {
            assert_eq!(race.pid, rec.pid);
            return Err("raced before pin".into());
        }
        if reader.read(rec.pid).as_ref() != Some(rec) {
            return Err("identity changed".into());
        }
        Ok(rec.clone())
    }
    fn send(
        &self,
        handle: &Self::Handle,
        rec: &ProcessRecord,
        signal: Signal,
        reader: &dyn ProcessReader,
    ) -> Result<(), String> {
        if handle != rec || reader.read(rec.pid).as_ref() != Some(rec) {
            return Err("identity changed".into());
        }
        self.sent.borrow_mut().push((rec.pid, signal));
        Ok(())
    }
}
#[derive(Default)]
struct Time(std::cell::Cell<f64>);
impl Clock for Time {
    fn now(&self) -> f64 {
        self.0.get()
    }
    fn sleep(&self, d: Duration) {
        self.0.set(self.0.get() + d.as_secs_f64());
    }
}
#[test]
fn cleanup_requires_foreground_exit_and_exact_detached_births() {
    let rows = fixture();
    let plan = StopPlan::build(
        10,
        &rows,
        &serde_json::json!({"pane_pid":10,"key_table":"root","prefix":false,"in_mode":false}),
    );
    assert_eq!(
        plan.targets().iter().map(|p| p.pid).collect::<Vec<_>>(),
        vec![30, 31]
    );
    let reader = Reader {
        records: RefCell::new(
            rows.into_iter()
                .filter(|r| r.pid != 20)
                .map(|r| (r.pid, r))
                .collect(),
        ),
    };
    let sink = Sink::default();
    plan.execute(
        &reader,
        &sink,
        &Time::default(),
        RunMode::Sandbox,
        &AtomicBool::new(false),
    )
    .unwrap();
    assert_eq!(
        *sink.sent.borrow(),
        vec![
            (30, Signal::Interrupt),
            (31, Signal::Interrupt),
            (30, Signal::Terminate),
            (31, Signal::Terminate)
        ]
    );
}
#[test]
fn recycled_reparented_regrouped_and_pinning_race_receive_no_signals() {
    for change in 0..4 {
        let rows = fixture();
        let plan = StopPlan::build(10, &rows, &serde_json::json!({"pane_pid":10}));
        let reader = Reader {
            records: RefCell::new(
                rows.into_iter()
                    .filter(|r| r.pid != 20 && r.pid != 31)
                    .map(|r| (r.pid, r))
                    .collect(),
            ),
        };
        let sink = Sink::default();
        let mut changed = reader.read(30).unwrap();
        match change {
            0 => changed.start_time += 1,
            1 => changed.parent = 999,
            2 => changed.group = 999,
            _ => *sink.race.borrow_mut() = Some(changed.clone()),
        };
        if change < 3 {
            reader.records.borrow_mut().insert(30, changed);
        }
        plan.execute(
            &reader,
            &sink,
            &Time::default(),
            RunMode::Sandbox,
            &AtomicBool::new(false),
        )
        .unwrap();
        assert!(sink.sent.borrow().is_empty(), "change {change}");
    }
}
#[test]
fn copy_mode_prefix_shadow_and_running_agent_are_noops() {
    let rows = fixture();
    for state in [
        serde_json::json!({}),
        serde_json::json!({"pane_pid":10,"prefix":true}),
        serde_json::json!({"pane_pid":10,"in_mode":true}),
        serde_json::json!({"pane_pid":10,"key_table":"copy-mode"}),
    ] {
        assert!(StopPlan::build(10, &rows, &state).targets().is_empty());
    }
    let plan = StopPlan::build(10, &rows, &serde_json::json!({"pane_pid":10}));
    let reader = Reader {
        records: RefCell::new(rows.into_iter().map(|r| (r.pid, r)).collect()),
    };
    let sink = Sink::default();
    plan.execute(
        &reader,
        &sink,
        &Time::default(),
        RunMode::Shadow,
        &AtomicBool::new(false),
    )
    .unwrap();
    plan.execute(
        &reader,
        &sink,
        &Time::default(),
        RunMode::Sandbox,
        &AtomicBool::new(false),
    )
    .unwrap();
    assert!(sink.sent.borrow().is_empty());
}
#[test]
fn pane_close_binds_socket_confirmation_and_server_identity() {
    let socket = std::path::Path::new("/private/fake-tmux/socket");
    let intent = PaneCloseIntent::prepare(socket, "fixture", "%1", |_| {
        Ok(serde_json::json!({"panes":[{"id":"%1","identity":"birth-one","title":"fake"}]}))
    })
    .unwrap();
    let calls = RefCell::new(vec![]);
    assert!(
        intent
            .finish(socket, false, false, RunMode::Sandbox, |v| {
                calls.borrow_mut().push(v.clone());
                Ok(serde_json::json!({}))
            })
            .unwrap()
            .is_none()
    );
    assert!(
        intent
            .finish(
                std::path::Path::new("/other/socket"),
                true,
                false,
                RunMode::Sandbox,
                |v| {
                    calls.borrow_mut().push(v.clone());
                    Ok(serde_json::json!({}))
                }
            )
            .is_err()
    );
    assert!(
        intent
            .finish(socket, true, false, RunMode::Shadow, |v| {
                calls.borrow_mut().push(v.clone());
                Ok(serde_json::json!({}))
            })
            .unwrap()
            .is_none()
    );
    assert!(calls.borrow().is_empty());
    intent
        .finish(socket, true, false, RunMode::Sandbox, |v| {
            assert_eq!(v["identity"], "birth-one");
            assert_eq!(v["action"], "close");
            calls.borrow_mut().push(v.clone());
            Err("pane identity changed".into())
        })
        .unwrap_err();
    assert_eq!(calls.borrow().len(), 1);
}

#[test]
fn agent_directly_owned_by_pane_can_exit_before_proven_detached_cleanup() {
    let mut rows = fixture();
    rows.retain(|r| r.pid != 10);
    let leader = rows.iter_mut().find(|r| r.pid == 20).unwrap();
    leader.parent = 1;
    let plan = StopPlan::build(20, &rows, &serde_json::json!({"pane_pid":20}));
    assert_eq!(plan.targets().len(), 2);
    let reader = Reader {
        records: RefCell::new(
            rows.into_iter()
                .filter(|r| r.pid != 20)
                .map(|r| (r.pid, r))
                .collect(),
        ),
    };
    let sink = Sink::default();
    plan.execute(
        &reader,
        &sink,
        &Time::default(),
        RunMode::Sandbox,
        &AtomicBool::new(false),
    )
    .unwrap();
    assert_eq!(sink.sent.borrow().len(), 4);
}

#[test]
fn foreground_tree_targets_and_signal_trace_match_actual_library_ast() {
    use comandos_app::proc::{ProcSpec, run};
    let mut scenarios = Vec::new();
    for name in [
        "grok",
        "grok-dev",
        "codex",
        "/usr/bin/codex",
        "claude",
        "gemini",
        "agy",
        "opencode",
        "bash",
    ] {
        let mut rows = fixture();
        let leader = rows.iter_mut().find(|r| r.pid == 20).unwrap();
        leader.comm = name.into();
        leader.argv = vec![name.into()];
        scenarios.push(rows);
    }
    for wrapper in ["node", "bun", "deno", "python3"] {
        let mut rows = fixture();
        let leader = rows.iter_mut().find(|r| r.pid == 20).unwrap();
        leader.comm = wrapper.into();
        leader.argv = vec![wrapper.into(), "/private/bin/codex".into()];
        scenarios.push(rows);
    }
    let mut background = fixture();
    background
        .iter_mut()
        .find(|r| r.pid == 10)
        .unwrap()
        .foreground = 50;
    scenarios.push(background);
    let mut foreign = fixture();
    foreign.iter_mut().find(|r| r.pid == 20).unwrap().parent = 999;
    scenarios.push(foreign);
    let mut cycle = fixture();
    cycle.push(process(60, 31, 60, 20, "tool"));
    cycle.iter_mut().find(|r| r.pid == 30).unwrap().parent = 60;
    scenarios.push(cycle);
    let input=serde_json::json!(scenarios.iter().map(|rows|serde_json::json!({"root":10,"records":rows.iter().map(|r|serde_json::json!({"pid":r.pid,"parent":r.parent,"group":r.group,"session":r.session,"foreground":r.foreground,"start_time":r.start_time,"comm":r.comm,"argv":r.argv})).collect::<Vec<_>>()})).collect::<Vec<_>>());
    let path = std::path::Path::new(
        &std::env::var("COMANDOS_CC_APP_ORACLE")
            .unwrap_or_else(|_| concat!(env!("CARGO_MANIFEST_DIR"), "/../../bin/cc-app").into()),
    )
    .parent()
    .unwrap()
    .parent()
    .unwrap()
    .join("lib/agent_stop.py");
    let out = run(&ProcSpec {
        program: "python3".into(),
        args: vec![
            "-c".into(),
            include_str!("support/t15_original.py").into(),
            path.into_os_string(),
            "agents".into(),
        ],
        stdin: Some(serde_json::to_vec(&input).unwrap()),
        env: vec![],
        clear_env: false,
        env_remove: vec![],
        cwd: None,
        timeout: Duration::from_secs(5),
    })
    .unwrap();
    assert_eq!(
        out.code,
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let expected: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    for (i, rows) in scenarios.into_iter().enumerate() {
        let plan = StopPlan::build(10, &rows, &serde_json::json!({"pane_pid":10}));
        assert_eq!(
            serde_json::json!(plan.targets().iter().map(|p| p.pid).collect::<Vec<_>>()),
            expected[i]["targets"],
            "scenario{i}"
        );
        let reader = Reader {
            records: RefCell::new(
                rows.into_iter()
                    .filter(|r| r.pid != 20)
                    .map(|r| (r.pid, r))
                    .collect(),
            ),
        };
        let sink = Sink::default();
        let time = Time::default();
        plan.execute(
            &reader,
            &sink,
            &time,
            RunMode::Sandbox,
            &AtomicBool::new(false),
        )
        .unwrap();
        let actual = serde_json::json!(
            sink.sent
                .borrow()
                .iter()
                .map(|(pid, sig)| serde_json::json!([pid, format!("{sig:?}")]))
                .collect::<Vec<_>>()
        );
        assert_eq!(actual, expected[i]["signals"], "scenario{i}");
        if !plan.targets().is_empty() {
            assert!(
                (time.now() - expected[i]["elapsed"].as_f64().unwrap()).abs() < 1e-8,
                "grace differs from original: {}",
                time.now()
            );
        }
    }
}

#[test]
fn grace_rechecks_target_parent_group_and_pane_scope_before_terminate() {
    for change in 0..4 {
        let rows = fixture();
        let plan = StopPlan::build(10, &rows, &serde_json::json!({"pane_pid":10}));
        let reader = Reader {
            records: RefCell::new(
                rows.into_iter()
                    .filter(|r| r.pid != 20 && r.pid != 31)
                    .map(|r| (r.pid, r))
                    .collect(),
            ),
        };
        struct Mutate<'a> {
            reader: &'a Reader,
            time: std::cell::Cell<f64>,
            change: u8,
        }
        impl Clock for Mutate<'_> {
            fn now(&self) -> f64 {
                self.time.get()
            }
            fn sleep(&self, d: Duration) {
                self.time.set(self.time.get() + d.as_secs_f64());
                if let Some(r) = self
                    .reader
                    .records
                    .borrow_mut()
                    .get_mut(if self.change == 3 { &10 } else { &30 })
                {
                    match self.change {
                        0 => r.start_time += 1,
                        1 => r.parent = 999,
                        2 => r.group = 999,
                        _ => r.start_time += 1,
                    }
                }
            }
        }
        let sink = Sink::default();
        let clock = Mutate {
            reader: &reader,
            time: std::cell::Cell::new(0.),
            change,
        };
        plan.execute(
            &reader,
            &sink,
            &clock,
            RunMode::Sandbox,
            &AtomicBool::new(false),
        )
        .unwrap();
        assert_eq!(
            *sink.sent.borrow(),
            [(30, Signal::Interrupt)],
            "change{change}"
        );
    }
}
#[test]
#[allow(clippy::disallowed_methods)] // Solo escritura y limpieza de la fixture en TMPDIR privado.
fn proc_reader_uses_only_injected_private_fixture_and_stat_tail() {
    use comandos_app::agent_stop::{ProcReader, parse_stat};
    let root = std::env::temp_dir().join(format!("t15-proc-fixture-{}", std::process::id()));
    std::fs::create_dir_all(root.join("30")).unwrap();
    let raw = "30 (fixture with ) parens) S 20 30 1 0 20 0 0 0 0 0 0 0 0 0 0 0 0 0 123";
    std::fs::write(root.join("30/stat"), raw).unwrap();
    std::fs::write(root.join("30/status"), "Name:\tworker\n").unwrap();
    std::fs::write(root.join("30/cmdline"), b"worker\0--fake\0").unwrap();
    let reader = ProcReader::at(root.clone());
    let actual = reader.read(30).unwrap();
    assert_eq!(actual.parent, 20);
    assert_eq!(actual.group, 30);
    assert_eq!(actual.foreground, 20);
    assert_eq!(actual.start_time, 123);
    assert_eq!(actual.argv, ["worker", "--fake"]);
    assert!(reader.read(31).is_none());
    assert!(parse_stat(30, b"bad stat", vec![], String::new()).is_none());
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn ctrl_c_windows_and_client_table_validation_match_original_primitives() {
    use comandos_app::{
        agent_stop::{client_blocks_stop, parse_client_state},
        proc::{ProcSpec, run},
    };
    let mut actions = Vec::new();
    for sel in [false, true] {
        for copy in [false, true] {
            for last in [None, Some(0.), Some(0.5), Some(1.), Some(1.0001), Some(2.)] {
                actions.push((sel, copy, 1., last, 1.));
            }
        }
    }
    let clients = [
        "",
        "other|root|0|0|10",
        "/private/tty|root|0|0|10",
        "/private/tty||1|0|10",
        "/private/tty|copy-mode|0|1|10",
        "/private/tty|root|0|0|+10",
        "/private/tty|root|0|0|x",
        "/private/tty|root|0|0|",
        "/private/tty|root|0|0|0",
    ];
    let states = [
        serde_json::Value::Null,
        serde_json::json!({}),
        serde_json::json!({"pane_pid":10}),
        serde_json::json!({"pane_pid":10,"key_table":null}),
        serde_json::json!({"pane_pid":10,"key_table":"copy-mode"}),
        serde_json::json!({"pane_pid":10,"key_table":""}),
        serde_json::json!({"pane_pid":10,"prefix":true}),
        serde_json::json!({"pane_pid":10,"in_mode":true}),
    ];
    let original = std::env::var("COMANDOS_CC_APP_ORACLE")
        .unwrap_or_else(|_| concat!(env!("CARGO_MANIFEST_DIR"), "/../../bin/cc-app").into());
    let path = std::path::Path::new(&original)
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("lib/agent_stop.py");
    let out = run(&ProcSpec {
        program: "python3".into(),
        args: vec![
            "-c".into(),
            include_str!("support/t15_original.py").into(),
            path.into_os_string(),
            "primitives".into(),
        ],
        stdin: Some(
            serde_json::to_vec(
                &serde_json::json!({"actions":actions,"clients":clients,"states":states}),
            )
            .unwrap(),
        ),
        env: vec![],
        clear_env: false,
        env_remove: vec![],
        cwd: None,
        timeout: Duration::from_secs(5),
    })
    .unwrap();
    assert_eq!(
        out.code,
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let expected: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    for (i, (sel, copy, now, last, window)) in actions.into_iter().enumerate() {
        let action = match ctrl_c_action(sel, copy, now, last.unwrap_or(f64::NAN), window) {
            CtrlCAction::Copy => "copy",
            CtrlCAction::SendInterrupt => "pass",
            CtrlCAction::Cleanup => "arm",
        };
        assert_eq!(action, expected["actions"][i]);
    }
    for (i, text) in clients.into_iter().enumerate() {
        assert_eq!(
            parse_client_state(text, "/private/tty"),
            expected["clients"][i],
            "client{i}"
        );
    }
    for (i, state) in states.iter().enumerate() {
        assert_eq!(
            serde_json::json!(client_blocks_stop(state)),
            expected["blocked"][i],
            "state{i}"
        );
    }
}
