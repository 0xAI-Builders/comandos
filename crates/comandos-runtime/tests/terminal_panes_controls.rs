use comandos_runtime::pane_typing::TmuxResult;
use comandos_runtime::terminal_panes::{self as panes, PaneError};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::cell::RefCell;

#[derive(Default)]
struct State {
    rows: Vec<String>,
    calls: Vec<Vec<String>>,
    snapshots: usize,
    fail_snapshot: bool,
    snapshot_race: bool,
    queue_race: bool,
    changed: bool,
    clients: String,
    fail_command: Option<String>,
    callback_failure: bool,
}
struct Harness(RefCell<State>);
impl Harness {
    fn new() -> Self {
        Self(RefCell::new(State {
            rows: vec![
                "%1\t0\tzsh\t0\t/home/fixture/p\t0\t0\t59\t40".into(),
                "%2\t1\tcodex\t1\t/home/fixture/p\t60\t0\t60\t40".into(),
            ],
            ..State::default()
        }))
    }
    fn identity(&self, pane: &str) -> Value {
        json!({"socket_path":"/tmp/é😊","pid":"12","server_start":"100","session_id":"$1","pane_id":pane,"pane_pid": if self.0.borrow().changed {"999"} else {"123"},"session_name":"fixture","pane_current_command":"zsh"})
    }
    fn tmux(&self, args: &[&str]) -> panes::Result<TmuxResult> {
        let mut s = self.0.borrow_mut();
        s.calls.push(args.iter().map(|a| (*a).into()).collect());
        if s.callback_failure {
            return Err(PaneError("callback unavailable".into()));
        }
        if s.fail_command.as_deref() == Some(args[0]) {
            return Ok(TmuxResult {
                returncode: 1,
                ..TmuxResult::default()
            });
        }
        let stdout = match args[0] {
            "list-panes" => s.rows.join("\n"),
            "list-clients" => s.clients.clone(),
            "if-shell" => {
                if s.queue_race {
                    s.rows.truncate(1);
                    "COMANDOS_PANE_CHANGED".into()
                } else {
                    if args[5].starts_with("kill-pane") {
                        s.rows.retain(|r| !r.starts_with(&format!("{}\t", args[3])));
                    }
                    if args[5].starts_with("split-window") {
                        s.rows.push("%3\t1\tzsh\t2\t/home/fixture/p".into());
                    }
                    String::new()
                }
            }
            _ => String::new(),
        };
        Ok(TmuxResult {
            stdout,
            ..TmuxResult::default()
        })
    }
    fn execute(&self, data: Value) -> panes::Result<Value> {
        panes::execute(
            |a| self.tmux(a),
            |session, pane| {
                assert_eq!(session, "fixture");
                Ok(self.identity(pane))
            },
            |session, pane| {
                assert_eq!((session, pane), ("fixture", "%1"));
                let mut s = self.0.borrow_mut();
                s.snapshots += 1;
                if s.fail_snapshot {
                    return Err(PaneError("disk unavailable".into()));
                }
                if s.snapshot_race {
                    s.rows.truncate(1);
                }
                Ok(json!("saved"))
            },
            &data,
            "/home/fixture",
        )
    }
    fn target(&self) -> Value {
        self.execute(json!({"session":"fixture"})).unwrap()["panes"][0].clone()
    }
    fn request(&self, action: &str) -> Value {
        let p = self.target();
        json!({"session":"fixture","action":action,"pane":p["id"],"identity":p["identity"]})
    }
    fn mutations(&self) -> usize {
        self.0
            .borrow()
            .calls
            .iter()
            .filter(|c| matches!(c[0].as_str(), "if-shell" | "resize-pane" | "set-option"))
            .count()
    }
}
#[test]
fn identity_uses_python_sorted_default_ascii_json_and_stable_fields() {
    let h = Harness::new();
    let expected = r#"{"pane_id": "%1", "pane_pid": "123", "pid": "12", "server_start": "100", "session_id": "$1", "socket_path": "/tmp/\u00e9\ud83d\ude0a"}"#;
    assert_eq!(
        panes::version(&h.identity("%1")).unwrap(),
        format!("{:x}", Sha256::digest(expected.as_bytes()))
    );
    let mut id = h.identity("%1");
    id["pane_current_command"] = json!("different");
    assert_eq!(
        panes::version(&id).unwrap(),
        panes::version(&h.identity("%1")).unwrap()
    );
    h.0.borrow_mut().changed = true;
    assert_ne!(
        panes::version(&id).unwrap(),
        panes::version(&h.identity("%1")).unwrap()
    );
}
#[test]
fn inventory_hides_raw_identity_and_keeps_unicode_geometry_and_path_tabs() {
    let h = Harness::new();
    h.0.borrow_mut().rows[0] = format!(
        "%1\t0\t{}\t0\t/home/fixture/a\tb\t3\t4\t5\t6",
        "é😊".repeat(60)
    );
    let p = h.target();
    assert_eq!(p["title"].as_str().unwrap().chars().count(), 100);
    assert_eq!(p["path"], "~/a\tb");
    assert_eq!(p["left"], 3);
    assert!(p.get("_identity").is_none());
    h.0.borrow_mut().rows[0] = format!("%1\t0\tzsh\t0\t/{}", "é".repeat(350));
    let p = h.target();
    assert_eq!(p["path"].as_str().unwrap().chars().count(), 300);
    assert!(p.get("left").is_none());
    assert_eq!(&h.0.borrow().calls[0],&vec!["list-panes","-t","=fixture:","-F","#{pane_id}\t#{pane_active}\t#{pane_current_command}\t#{pane_index}\t#{pane_current_path}\t#{pane_left}\t#{pane_top}\t#{pane_width}\t#{pane_height}"].into_iter().map(String::from).collect::<Vec<_>>());
}
#[test]
fn close_pins_exact_pane_and_final_queue_guard_then_returns_snapshot() {
    let h = Harness::new();
    let req = h.request("close");
    let result = h.execute(req).unwrap();
    assert_eq!(result["closed"], "%1");
    assert_eq!(result["snapshot"], "saved");
    assert_eq!(result["panes"][0]["id"], "%2");
    assert_eq!(h.0.borrow().snapshots, 1);
    let s = h.0.borrow();
    let call = s.calls.iter().find(|c| c[0] == "if-shell").unwrap();
    assert_eq!(call, &vec!["if-shell","-F","-t","%1","#{&&:#{==:#{pane_pid},123},#{&&:#{==:#{pane_id},%1},#{&&:#{==:#{session_id},$1},#{&&:#{==:#{pid},12},#{>:#{window_panes},1}}}}}","kill-pane -t %1","display-message -p COMANDOS_PANE_CHANGED"].into_iter().map(String::from).collect::<Vec<_>>());
}
#[test]
fn close_rejects_last_pane_snapshot_race_queue_race_and_snapshot_failure() {
    for mode in 0..4 {
        let h = Harness::new();
        let req = h.request("close");
        {
            let mut s = h.0.borrow_mut();
            match mode {
                0 => s.rows.truncate(1),
                1 => s.snapshot_race = true,
                2 => s.queue_race = true,
                _ => s.fail_snapshot = true,
            }
        }
        assert!(h.execute(req).is_err());
        if mode != 2 {
            assert_eq!(h.mutations(), 0);
        }
        assert!(!h.0.borrow().rows.is_empty());
    }
}
#[test]
fn stale_foreign_unconfirmed_targets_never_mutate_or_snapshot() {
    for (target, identity) in [("%1", "old"), ("%999", "old"), ("%1", "")] {
        let h = Harness::new();
        assert!(
            h.execute(
                json!({"session":"fixture","action":"close","pane":target,"identity":identity})
            )
            .is_err()
        );
        assert_eq!(h.mutations(), 0);
        assert_eq!(h.0.borrow().snapshots, 0);
    }
}
#[test]
fn snapshot_identity_change_is_rejected() {
    let h = Harness::new();
    let req = h.request("close");
    let err = panes::execute(
        |a| h.tmux(a),
        |_, p| Ok(h.identity(p)),
        |_, _| {
            h.0.borrow_mut().changed = true;
            Ok(json!("saved"))
        },
        &req,
        "/home/fixture",
    )
    .unwrap_err();
    assert!(err.to_string().contains("cambió"));
    assert_eq!(h.mutations(), 0);
}
#[test]
fn split_inherits_target_cwd_and_returns_new_id() {
    for (direction, flag) in [("right", "-h"), ("down", "-v")] {
        let h = Harness::new();
        let mut req = h.request("split");
        req["direction"] = json!(direction);
        assert_eq!(h.execute(req).unwrap()["opened"], "%3");
        let s = h.0.borrow();
        let call = s.calls.iter().find(|c| c[0] == "if-shell").unwrap();
        assert_eq!(
            call[5],
            format!("split-window {flag} -t %1 -c \"#{{pane_current_path}}\"")
        );
    }
}
#[test]
fn client_focus_returns_exact_in_band_keys_and_all_thirty_two_bindings() {
    let h = Harness::new();
    let mut req = h.request("select");
    req["scope"] = json!("client");
    let result = h.execute(req).unwrap();
    assert_eq!(result["clientKeys"], "\u{1b}[4242;0~");
    assert!(h.0.borrow().calls.iter().all(|c| c[0] != "if-shell"));
    let mut expected = Vec::new();
    for i in 0..32 {
        expected.extend([
            "set-option".into(),
            "-s".into(),
            format!("user-keys[{}]", 900 + i),
            format!("\u{1b}[4242;{i}~"),
            ";".into(),
            "bind-key".into(),
            "-n".into(),
            format!("User{}", 900 + i),
            "select-pane".into(),
            "-t".into(),
            format!(":.{i}"),
            ";".into(),
        ]);
    }
    expected.pop();
    let s = h.0.borrow();
    assert_eq!(
        s.calls.iter().find(|c| c[0] == "set-option").unwrap(),
        &expected
    );
}
#[test]
fn client_focus_out_of_range_and_tmux_binding_failure_are_errors() {
    let h = Harness::new();
    h.0.borrow_mut().rows[0] = "%1\t0\tzsh\t32\t/p".into();
    let mut req = h.request("select");
    req["scope"] = json!("client");
    assert!(h.execute(req).is_err());
    assert_eq!(h.mutations(), 0);
    let h = Harness::new();
    let mut req = h.request("select");
    req["scope"] = json!("client");
    h.0.borrow_mut().fail_command = Some("set-option".into());
    assert!(h.execute(req).is_err());
}
#[test]
fn remote_focus_only_reports_one_distinct_active_pane_client() {
    for (clients, expected) in [
        ("active-pane,UTF-8\t%2\nactive-pane\t%2", json!("%2")),
        ("active-pane\t%1\nactive-pane\t%2", Value::Null),
        ("UTF-8\t%2", Value::Null),
        ("active-pane\t%２", Value::Null),
    ] {
        let h = Harness::new();
        h.0.borrow_mut().clients = clients.into();
        assert_eq!(
            h.execute(json!({"session":"fixture"})).unwrap()["remoteFocus"],
            expected
        );
    }
    let h = Harness::new();
    h.0.borrow_mut().fail_command = Some("list-clients".into());
    assert!(h.execute(json!({"session":"fixture"})).unwrap()["remoteFocus"].is_null());
}
#[test]
fn resize_needs_membership_and_exact_integer_contract_without_identity() {
    let h = Harness::new();
    assert!(
        h.execute(json!({"session":"fixture","action":"resize","pane":"%1","axis":"x","size":40}))
            .is_ok()
    );
    assert!(
        h.0.borrow().calls.contains(
            &vec!["resize-pane", "-t", "%1", "-x", "40"]
                .into_iter()
                .map(String::from)
                .collect()
        )
    );
    for (axis, size, pane) in [
        ("z", json!(30), "%1"),
        ("x", json!(true), "%1"),
        ("x", json!(1), "%1"),
        ("y", json!(1001), "%1"),
        ("x", json!("40"), "%1"),
        ("x", json!(30), "%999"),
    ] {
        let h = Harness::new();
        assert!(
            h.execute(
                json!({"session":"fixture","action":"resize","pane":pane,"axis":axis,"size":size})
            )
            .is_err()
        );
        assert_eq!(h.mutations(), 0);
    }
}
#[test]
fn validation_and_malformed_machine_output_reject_before_mutation() {
    for data in [
        json!([]),
        json!({"session":"bad:name"}),
        json!({"session":"fixture","action":"unknown"}),
        json!({"session":"fixture","action":"split","direction":"diagonal"}),
    ] {
        let h = Harness::new();
        assert!(h.execute(data).is_err());
        assert!(h.0.borrow().calls.is_empty());
    }
    for row in [
        "%１\t0\tzsh\t0\t/p",
        "%1;kill-server\t0\tzsh\t0\t/p",
        "%1\t0\tzsh\tbad\t/p",
        "%1\t0\tzsh\t０\t/p",
        "short",
    ] {
        let h = Harness::new();
        h.0.borrow_mut().rows[0] = row.into();
        assert!(
            h.execute(
                json!({"session":"fixture","action":"resize","axis":"x","size":40,"pane":"%1"})
            )
            .is_err()
        );
        assert_eq!(h.mutations(), 0);
    }
}
#[test]
fn invalid_identity_numbers_and_callback_errors_are_displayable() {
    let h = Harness::new();
    let mut id = h.identity("%1");
    id["pid"] = json!("１２");
    let digest = panes::version(&id).unwrap();
    assert!(
        panes::execute(
            |a| h.tmux(a),
            |_, _| Ok(id.clone()),
            |_, _| Ok(Value::Null),
            &json!({"session":"fixture","action":"select","pane":"%1","identity":digest}),
            "/home/fixture"
        )
        .is_err()
    );
    assert_eq!(h.mutations(), 0);
    h.0.borrow_mut().callback_failure = true;
    assert_eq!(
        h.execute(json!({"session":"fixture"}))
            .unwrap_err()
            .to_string(),
        "callback unavailable"
    );
}
#[test]
fn global_serial_guard_spans_distinct_callback_owners_and_all_mutations() {
    use std::sync::{
        Arc, Barrier,
        atomic::{AtomicUsize, Ordering},
    };
    let start = Arc::new(Barrier::new(8));
    let active = Arc::new(AtomicUsize::new(0));
    let max = Arc::new(AtomicUsize::new(0));
    let identity = json!({"pid":"12","session_id":"$1","pane_id":"%1","pane_pid":"123"});
    let version = panes::version(&identity).unwrap();
    std::thread::scope(|scope| {
        for _ in 0..8 {
            let (start, active, max, identity, version) = (
                start.clone(),
                active.clone(),
                max.clone(),
                identity.clone(),
                version.clone(),
            );
            scope.spawn(move || {
                start.wait();
                let result = panes::execute(
                    |args| {
                        if args[0] == "if-shell" {
                            let count = active.fetch_add(1, Ordering::SeqCst) + 1;
                            max.fetch_max(count, Ordering::SeqCst);
                            std::thread::sleep(std::time::Duration::from_millis(5));
                            active.fetch_sub(1, Ordering::SeqCst);
                        }
                        Ok(TmuxResult {
                            stdout: if args[0] == "list-panes" {
                                "%1\t1\tzsh\t0\t/p".into()
                            } else {
                                String::new()
                            },
                            ..TmuxResult::default()
                        })
                    },
                    |_, _| Ok(identity.clone()),
                    |_, _| Ok(Value::Null),
                    &json!({"session":"fixture","action":"select","pane":"%1","identity":version}),
                    "/home/fixture",
                );
                assert!(result.is_ok());
            });
        }
    });
    assert_eq!(max.load(Ordering::SeqCst), 1);
}
#[test]
fn global_serial_guard_allows_same_thread_snapshot_callback_reentry() {
    let h = Harness::new();
    let req = h.request("close");
    let result = panes::execute(
        |args| h.tmux(args),
        |_, p| Ok(h.identity(p)),
        |_, _| {
            assert!(h.execute(json!({"session":"fixture"})).is_ok());
            Ok(json!("saved"))
        },
        &req,
        "/home/fixture",
    );
    assert!(result.is_ok());
}

#[test]
fn malformed_identity_and_foreign_machine_identity_reject_before_every_mutation() {
    for action in ["resize", "select", "close", "split"] {
        for field in ["pid", "session_id", "pane_id", "pane_pid"] {
            let h = Harness::new();
            let mut id = h.identity("%1");
            id[field] = json!("１２");
            let digest = panes::version(&id).unwrap();
            let req = json!({"session":"fixture","action":action,"scope":"client","pane":"%1","identity":digest,"axis":"x","size":40,"direction":"right"});
            let result = panes::execute(
                |args| h.tmux(args),
                |_, _| Ok(id.clone()),
                |_, _| Ok(json!("saved")),
                &req,
                "/home/fixture",
            );
            assert!(result.is_err(), "malformed {field} must stop {action}");
            assert_eq!(h.mutations(), 0);
        }
    }
    let h = Harness::new();
    let mut id = h.identity("%1");
    id["pane_id"] = json!("%2");
    let digest = panes::version(&id).unwrap();
    assert!(
        panes::execute(
            |a| h.tmux(a),
            |_, _| Ok(id.clone()),
            |_, _| Ok(Value::Null),
            &json!({"session":"fixture","action":"select","pane":"%1","identity":digest}),
            "/home/fixture"
        )
        .is_err()
    );
    assert_eq!(h.mutations(), 0);
}
