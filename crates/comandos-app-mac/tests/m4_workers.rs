#![allow(clippy::unwrap_used)]
use comandos_app_mac::{
    app::{Action, App},
    jobs::{Backend, Jobs, ResultData, Task},
    tabs_ops::RestoreState,
};
use comandos_desktop::{
    Lang, TabKind, TabMeta,
    proc::{ProcOutput, ProcSpec, run},
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
struct Fake {
    calls: Mutex<Vec<Value>>,
    alive: Mutex<Vec<String>>,
    state: Arc<Mutex<RestoreState>>,
    cancel_post: bool,
    swapped: bool,
    identity: Mutex<u32>,
    commands: String,
    swap_after_check: bool,
    identity_reads: Mutex<u32>,
}
impl Fake {
    fn new() -> Self {
        Self {
            calls: Mutex::new(vec![]),
            alive: Mutex::new(vec!["alive".into()]),
            state: Arc::new(Mutex::new(RestoreState::default())),
            cancel_post: false,
            swapped: false,
            identity: Mutex::new(1),
            commands: "zsh\nbash\n".into(),
            swap_after_check: false,
            identity_reads: Mutex::new(0),
        }
    }
}
impl Backend for Fake {
    fn get(&self, _: &str) -> Result<Value, String> {
        Ok(Value::Null)
    }
    fn cancel(&self) {}
    fn post(&self, path: &str, body: &Value) -> Result<Value, String> {
        self.calls.lock().unwrap().push(json!(["post", path, body]));
        if path == "/tab-metadata-remove" && self.swapped {
            *self.identity.lock().unwrap() = 2;
        }
        if path == "/ssh-connect" {
            self.alive.lock().unwrap().push("ssh-actual".into());
            if self.cancel_post {
                self.state.lock().unwrap().cancel("ssh-host");
            }
            return Ok(json!({"session":"ssh-actual"}));
        }
        if path == "/ensure" {
            self.alive
                .lock()
                .unwrap()
                .push(body["session"].as_str().unwrap().into());
        }
        Ok(Value::Null)
    }
    fn tmux(&self, args: &[&str], allowed: &dyn Fn() -> bool) -> Result<ProcOutput, String> {
        assert!(allowed());
        self.calls.lock().unwrap().push(json!(["tmux", args]));
        let session = args
            .windows(2)
            .find(|a| a[0] == "-t")
            .map_or("", |a| a[1].trim_start_matches('=').trim_end_matches(':'));
        let mut code = 0;
        let text = match args[0] {
            "has-session" => {
                if !self.alive.lock().unwrap().iter().any(|s| s == session) {
                    code = 1;
                }
                String::new()
            }
            "new-session" => {
                let s = args.windows(2).find(|a| a[0] == "-s").unwrap()[1];
                self.alive.lock().unwrap().push(s.into());
                String::new()
            }
            "display-message" if args.last().unwrap().starts_with("#{pid}") => {
                assert_eq!(
                    args[3],
                    format!("={session}:"),
                    "display-message requires a pane target on tmux 3.6"
                );
                let mut reads = self.identity_reads.lock().unwrap();
                *reads += 1;
                let result = format!("991|${}|3|{}", self.identity.lock().unwrap(), session);
                if self.swap_after_check && *reads == 2 {
                    *self.identity.lock().unwrap() = 2;
                }
                result
            }
            "display-message" if args.last() == Some(&"#{pane_current_path}") => "/own/cwd".into(),
            "display-message" => "node".into(),
            "list-panes" => self.commands.clone(),
            _ => String::new(),
        };
        Ok(ProcOutput {
            code: Some(code),
            stdout: text.into_bytes(),
            ..Default::default()
        })
    }
    fn archive(&self, item: &Value, allowed: &dyn Fn() -> bool) -> Result<(), String> {
        assert!(allowed());
        self.calls.lock().unwrap().push(json!(["archive", item]));
        Ok(())
    }
}
#[test]
fn replacement_after_last_identity_check_is_never_targeted_by_name() {
    let mut fake = Fake::new();
    fake.swap_after_check = true;
    let fake = Arc::new(fake);
    let mut model = App::new("noche", Lang::En);
    let ticket = model.ticket();
    model.finish_boot(&ticket, "owned");
    model.add_tab("term-own", "Own", "term-own", false).unwrap();
    model.set_metadata(
        "term-own",
        Some(TabMeta {
            kind: TabKind::Scratch,
            host: None,
            cwd: None,
        }),
    );
    execute(
        fake.clone(),
        Task::ArchiveClose {
            tab: model.tabs()[0].clone(),
        },
    );
    assert_eq!(*fake.identity.lock().unwrap(), 2);
    let calls = fake.calls.lock().unwrap();
    let kill = calls
        .iter()
        .find(|v| v[0] == "tmux" && v[1][0] == "kill-session")
        .unwrap();
    assert_eq!(
        kill[1][2], "$1",
        "the replacement named term-own is $2 and must survive"
    );
}
fn execute(fake: Arc<Fake>, task: Task) -> ResultData {
    let model = App::new("noche", Lang::En);
    let jobs = Jobs::new(fake, Arc::new(|| {})).unwrap();
    jobs.submit(model.ticket(), task).unwrap();
    let end = Instant::now() + Duration::from_secs(3);
    loop {
        if let Some(d) = jobs.drain().into_iter().next() {
            return d.result.unwrap();
        }
        assert!(Instant::now() < end);
        std::thread::sleep(Duration::from_millis(1));
    }
}
fn original(input: Value) -> Value {
    let mut input = input;
    input["source"] = Value::String(
        std::env::var("COMANDOS_MAC_APP_ORACLE").unwrap_or_else(|_| {
            concat!(env!("CARGO_MANIFEST_DIR"), "/../../bin/cc-app-mac").into()
        }),
    );
    let out = run(&ProcSpec {
        program: "python3".into(),
        args: vec!["-c".into(), include_str!("support/m4_original.py").into()],
        stdin: Some(serde_json::to_vec(&input).unwrap()),
        env: vec![],
        clear_env: false,
        env_remove: vec![],
        cwd: Some(std::env::temp_dir()),
        timeout: Duration::from_secs(3),
    })
    .unwrap();
    assert_eq!(
        out.code,
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).unwrap()
}
#[test]
fn restoration_matches_executed_original_across_alive_scratch_ssh_alias_and_xterm() {
    let saved = vec![
        ("alive:1".into(), json!("Alive")),
        ("term-own".into(), Value::Null),
        ("ssh-host".into(), json!("SSH")),
        ("xterm-no".into(), Value::Null),
        ("alive:2".into(), json!("Duplicate")),
    ];
    let fake = Arc::new(Fake::new());
    fake.state.lock().unwrap().saved = saved.clone();
    let result = execute(
        fake.clone(),
        Task::Restore {
            saved: saved.clone(),
            metadata: BTreeMap::new(),
            home: std::env::temp_dir(),
            state: fake.state.clone(),
        },
    );
    let ResultData::Restored(rows) = result else {
        panic!("wrong delivery");
    };
    let actual: Vec<_> = rows
        .iter()
        .map(|r| {
            json!([
                r.opened.session,
                r.opened.label,
                r.opened.metadata.as_ref().unwrap().kind
            ])
        })
        .collect();
    let expected = original(json!({"op":"restore","saved":saved,"home":std::env::temp_dir()}));
    assert_eq!(json!(actual), expected["opened"]);
    assert_eq!(
        fake.state.lock().unwrap().current_session("ssh-host"),
        "ssh-actual"
    );
    assert!(rows.iter().all(|r| !r.opened.select && !r.opened.raise));
}
#[test]
fn cancelled_ssh_response_records_alias_but_never_selects_publishes_metadata_or_replays() {
    let mut f = Fake::new();
    f.cancel_post = true;
    let fake = Arc::new(f);
    let saved = vec![("ssh-host".into(), json!("SSH"))];
    fake.state.lock().unwrap().saved = saved.clone();
    let result = execute(
        fake.clone(),
        Task::Restore {
            saved,
            metadata: BTreeMap::new(),
            home: std::env::temp_dir(),
            state: fake.state.clone(),
        },
    );
    let ResultData::Restored(rows) = result else {
        panic!("wrong delivery")
    };
    assert!(rows.is_empty());
    assert_eq!(
        fake.state.lock().unwrap().current_session("ssh-host"),
        "ssh-actual"
    );
    assert!(
        !fake
            .calls
            .lock()
            .unwrap()
            .iter()
            .any(|v| v[0] == "tmux" && v[1][0] == "select-window" || v[1] == "/tab-metadata")
    );
}
#[test]
fn scratch_cleanup_archives_original_contract_and_refuses_replaced_session() {
    for swapped in [false, true] {
        let mut f = Fake::new();
        f.swapped = swapped;
        let fake = Arc::new(f);
        let mut model = App::new("noche", Lang::En);
        let t = model.ticket();
        model.finish_boot(&t, "own");
        model.add_tab("term-own", "Own", "term-own", false).unwrap();
        model.set_metadata(
            "term-own",
            Some(TabMeta {
                kind: TabKind::Scratch,
                host: None,
                cwd: None,
            }),
        );
        let tab = model.tabs()[0].clone();
        assert!(matches!(
            execute(fake.clone(), Task::ArchiveClose { tab }),
            ResultData::Closed
        ));
        let calls = fake.calls.lock().unwrap();
        let archive = calls.iter().find(|v| v[0] == "archive").unwrap();
        assert_eq!(archive[1]["cwd"], "/own/cwd");
        assert_eq!(archive[1]["agent"], "codex");
        assert_eq!(archive[1]["reason"], "closed");
        let killed = calls
            .iter()
            .any(|v| v[0] == "tmux" && v[1][0] == "kill-session");
        assert_eq!(killed, !swapped);
    }
}
#[test]
fn background_open_never_requests_selection_or_raise_and_uses_persisted_label() {
    let fake = Arc::new(Fake::new());
    let model = App::new("noche", Lang::En);
    let opened = comandos_app_mac::jobs::execute_open(
        &*fake,
        &Action::OpenBackground {
            session: "own".into(),
            label: Some("Label".into()),
        },
        false,
        &model.ticket(),
    )
    .unwrap();
    assert!(!opened.raise && !opened.select);
    assert_eq!(opened.label, "Label");
    assert_eq!(opened.metadata.unwrap().kind, TabKind::Shell);
}

#[test]
fn scratch_with_agent_or_empty_panes_and_non_scratch_shell_are_never_killed() {
    for (kind, commands) in [
        (TabKind::Scratch, "node"),
        (TabKind::Scratch, "zsh\nnode"),
        (TabKind::Scratch, ""),
        (TabKind::Shell, "zsh"),
    ] {
        let mut f = Fake::new();
        f.commands = commands.into();
        let fake = Arc::new(f);
        let mut model = App::new("noche", Lang::En);
        let ticket = model.ticket();
        model.finish_boot(&ticket, "own");
        model.add_tab("own", "Own", "own", false).unwrap();
        model.set_metadata(
            "own",
            Some(TabMeta {
                kind,
                host: None,
                cwd: None,
            }),
        );
        let tab = model.tabs()[0].clone();
        execute(fake.clone(), Task::ArchiveClose { tab });
        assert!(
            !fake
                .calls
                .lock()
                .unwrap()
                .iter()
                .any(|v| v[0] == "tmux" && v[1][0] == "kill-session")
        );
    }
}
