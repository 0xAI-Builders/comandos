use comandos_cli::codex::batch::{Process, Runtime, launch_command, restart, select_retry};
use serde_json::{Value, json};
use std::{
    fs,
    os::unix::fs::DirBuilderExt,
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
    time::Duration,
};
struct Fake {
    time: Duration,
    path: PathBuf,
    trace: Vec<Vec<String>>,
    alive: bool,
    context: Option<Value>,
    identity_ok: bool,
    stubborn: bool,
}
impl Runtime for Fake {
    fn tmux(&mut self, args: &[&str]) -> Result<String, String> {
        self.trace
            .push(args.iter().map(|s| s.to_string()).collect());
        match args[0] {
            "display-message" => Ok(match args[4] {
                "#{session_name}" => {
                    if self.identity_ok {
                        "s"
                    } else {
                        "reused"
                    }
                }
                "#{pane_pid}" => "100",
                "#{pane_current_command}" => {
                    if self.alive {
                        "codex"
                    } else {
                        "bash"
                    }
                }
                "#{pane_tty}" => "/dev/owned-fake",
                _ => panic!("unknown display"),
            }
            .into()),
            "send-keys" => {
                if args[3] == "C-c" && !self.stubborn {
                    self.alive = false;
                }
                if args[3] == "Enter"
                    && let Some(context) = &self.context
                {
                    use std::io::Write;
                    let mut f = fs::OpenOptions::new()
                        .append(true)
                        .open(&self.path)
                        .unwrap();
                    writeln!(f, "{}", json!({"type":"turn_context","payload":context})).unwrap();
                }
                Ok(String::new())
            }
            "capture-pane" => Ok("fixture screen".into()),
            _ => panic!("unexpected tmux"),
        }
    }
    fn process(&mut self, pid: i64) -> Result<Process, String> {
        if pid == 100 {
            Ok(Process {
                pid,
                start: "1".into(),
                state: "S".into(),
                args: vec!["/bin/bash".into()],
            })
        } else if self.alive {
            Ok(Process {
                pid,
                start: "2".into(),
                state: "S".into(),
                args: vec!["/fake/codex".into()],
            })
        } else {
            Err("missing".into())
        }
    }
    fn sleep(&mut self, d: Duration) {
        self.time += d;
    }
    fn elapsed(&self) -> Duration {
        self.time
    }
    fn stty(&mut self, tty: &str) -> Result<(), String> {
        self.trace
            .push(vec!["stty".into(), "sane".into(), "-F".into(), tty.into()]);
        Ok(())
    }
    fn terminate_exact(&mut self, _: &Value) -> Result<(), String> {
        self.trace.push(vec!["pidfd-term".into()]);
        self.alive = false;
        Ok(())
    }
    fn cancelled(&self) -> bool {
        false
    }
}
fn fixture() -> (PathBuf, Value, Fake) {
    static N: AtomicUsize = AtomicUsize::new(0);
    let root = std::env::temp_dir().join(format!(
        "codex-batch-c5-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    ));
    fs::DirBuilder::new().mode(0o700).create(&root).unwrap();
    let path = root.join("rollout.jsonl");
    fs::write(&path,format!("{}\n",json!({"type":"turn_context","payload":{"approval_policy":"never","sandbox_policy":{"type":"danger-full-access"}}}))).unwrap();
    let mut plan = json!({"session":"s","pane":"%1","panePid":100,"paneStart":"1","pid":101,"start":"2","sid":"11111111-1111-1111-1111-111111111111","home":root,"cwd":root,"binary":"/fake/codex","flags":["--model","fixture","-c","model_reasoning_effort=\"ultra\""],"transcript":path});
    plan["command"] = json!(launch_command(&plan, "continua").unwrap());
    plan["recoveryCommand"] = json!(launch_command(&plan, "").unwrap());
    let rt = Fake {
        time: Duration::ZERO,
        path,
        trace: vec![],
        alive: true,
        context: None,
        identity_ok: true,
        stubborn: false,
    };
    (root, plan, rt)
}
#[test]
fn fresh_context_only_confirms_same_conversation_and_continua() {
    let (root, mut p, mut rt) = fixture();
    rt.context = Some(
        json!({"approval_policy":"never","sandbox_policy":{"type":"danger-full-access"},"permission_profile":{"type":"disabled"},"file_system_sandbox_policy":{"type":"unrestricted"}}),
    );
    let mut releases = 0;
    let result = restart(
        &mut rt,
        &mut p,
        |_, _| {
            releases += 1;
            Ok(())
        },
        &mut |_| Ok(()),
    )
    .unwrap();
    assert_eq!(
        result,
        json!({"status":"confirmed","conversationId":p["sid"],"prompt":"continua"})
    );
    assert_eq!(releases, 1);
    let literal = rt
        .trace
        .iter()
        .find(|a| a.get(3).map(String::as_str) == Some("-l"))
        .unwrap();
    assert_eq!(literal[4], "--");
    assert!(literal[5].ends_with(" continua"));
    fs::remove_dir_all(root).unwrap();
}
#[test]
fn old_full_access_context_never_confirms_new_resume() {
    let (root, mut p, mut rt) = fixture();
    let result = restart(&mut rt, &mut p, |_, _| Ok(()), &mut |_| Ok(())).unwrap();
    assert_eq!(result["status"], "unverified");
    assert_eq!(result["screen"], "fixture screen");
    fs::remove_dir_all(root).unwrap();
}
#[test]
fn restricted_context_is_reported_and_pid_identity_change_prevents_keystrokes() {
    let (root, mut p, mut rt) = fixture();
    rt.context =
        Some(json!({"approval_policy":"on-request","sandbox_policy":{"type":"workspace-write"}}));
    assert_eq!(
        restart(&mut rt, &mut p, |_, _| Ok(()), &mut |_| Ok(())).unwrap()["status"],
        "restricted"
    );
    let (_, mut other, mut reused) = fixture();
    reused.identity_ok = false;
    assert!(
        restart(
            &mut reused,
            &mut other,
            |_, _| panic!("release forbidden"),
            &mut |_| Ok(())
        )
        .is_err()
    );
    assert!(reused.trace.iter().all(|t| t[0] != "send-keys"));
    fs::remove_dir_all(root).unwrap();
    fs::remove_dir_all(reused.path.parent().unwrap()).unwrap();
}
#[test]
fn retry_rejects_changed_birth_and_can_restore_same_saved_shell() {
    let (root, p, mut rt) = fixture();
    let mut fresh = p.clone();
    fresh["paneStart"] = json!("other");
    assert!(select_retry(std::slice::from_ref(&p), &[fresh], &mut rt).is_err());
    rt.alive = false;
    let selected = select_retry(std::slice::from_ref(&p), &[], &mut rt).unwrap();
    assert!(selected[0]["pid"].is_null());
    assert_eq!(selected[0]["sid"], p["sid"]);
    assert_eq!(selected[0]["command"], p["command"]);
    fs::remove_dir_all(root).unwrap();
}
#[test]
fn stubborn_agent_uses_only_exact_termination_after_tui_exit_attempt() {
    let (root, mut p, mut rt) = fixture();
    rt.stubborn = true;
    let result = restart(&mut rt, &mut p, |_, _| Ok(()), &mut |_| Ok(())).unwrap();
    assert_eq!(result["status"], "unverified");
    let t = rt.trace.iter().map(|a| a.join(" ")).collect::<Vec<_>>();
    assert_eq!(t.iter().filter(|s| s.contains(" C-c")).count(), 3);
    assert!(t.iter().any(|s| s.contains("/exit")));
    assert_eq!(t.iter().filter(|s| s.as_str() == "pidfd-term").count(), 1);
    fs::remove_dir_all(root).unwrap();
}
