#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::disallowed_methods
)]
use comandos_app::{
    config::RunMode,
    restore::RestoreTmux,
    tab_actions::{PaneClose, QuickTerminal, select_window, valid_session, valid_window},
};
use serde_json::{Value, json};
use std::cell::RefCell;
struct Windows {
    calls: RefCell<Vec<Vec<String>>>,
    listing: String,
    shadow: bool,
}
impl RestoreTmux for Windows {
    type Ownership = ();
    fn mode(&self) -> RunMode {
        if self.shadow {
            RunMode::Shadow
        } else {
            RunMode::Sandbox
        }
    }
    fn read(&self, args: &[&str]) -> Result<String, String> {
        self.calls
            .borrow_mut()
            .push(args.iter().map(|s| s.to_string()).collect());
        Ok(self.listing.clone())
    }
    fn mutate(&self, args: &[&str], _: Option<&[u8]>) -> Result<String, String> {
        self.calls
            .borrow_mut()
            .push(args.iter().map(|s| s.to_string()).collect());
        if args.last().is_some_and(|s| s.ends_with(":claude")) {
            Err("renamed".into())
        } else {
            Ok(String::new())
        }
    }
    fn create(&self, _: &[&str]) -> Result<(String, ()), String> {
        panic!("open must not create")
    }
    fn cleanup(&self, _: ()) -> Result<(), String> {
        panic!("open must not kill")
    }
}
#[test]
fn open_uses_exact_named_window_then_running_claude_then_first_and_shadow_never_mutates() {
    for (listing, target) in [
        ("0|zsh\n3|claude", "=fixture:3"),
        ("0|zsh\n3|codex", "=fixture:^"),
    ] {
        let tmux = Windows {
            calls: RefCell::new(vec![]),
            listing: listing.into(),
            shadow: false,
        };
        select_window(&tmux, "fixture", "claude").unwrap();
        assert_eq!(
            tmux.calls.borrow().last().unwrap(),
            &vec!["select-window", "-t", target]
        );
        assert_eq!(tmux.calls.borrow().len(), 3);
    }
    let tmux = Windows {
        calls: RefCell::new(vec![]),
        listing: String::new(),
        shadow: true,
    };
    assert!(select_window(&tmux, "fixture", "claude").is_err());
    assert!(tmux.calls.borrow().is_empty());
    assert!(!valid_session("$(touch nope)"));
    assert!(!valid_session(&"x".repeat(81)));
    assert!(!valid_window("x:y"));
}
#[test]
fn quick_failed_and_malformed_responses_keep_request_until_success() {
    let quick = QuickTerminal::default();
    let seen = RefCell::new(vec![]);
    let post = |body: &Value| {
        seen.borrow_mut().push(body.clone());
        Err("timeout".into())
    };
    assert!(quick.request(|| Ok("gtk-first".into()), post).is_err());
    assert!(
        quick
            .request(
                || panic!("retry must reuse identity"),
                |body| {
                    seen.borrow_mut().push(body.clone());
                    Ok(json!({"oops":true}))
                }
            )
            .is_err()
    );
    quick
        .request(
            || panic!("retry must reuse identity"),
            |body| {
                seen.borrow_mut().push(body.clone());
                Ok(json!({"tabId":"term-q123","label":"T-2026"}))
            },
        )
        .unwrap();
    assert!(
        seen.borrow()
            .iter()
            .all(|v| *v == json!({"requestId":"gtk-first"}))
    );
    quick
        .request(
            || Ok("gtk-second".into()),
            |body| {
                assert_eq!(body["requestId"], "gtk-second");
                Ok(json!({"tabId":"term-q456"}))
            },
        )
        .unwrap();
}
#[test]
fn cancelled_confirmation_never_posts_and_confirmed_close_keeps_exact_original_identity() {
    let token = PaneClose::prepare("fixture", "%7", |body| {
        assert_eq!(*body, json!({"session":"fixture","action":"list"}));
        Ok(json!({"panes":[{"id":"%7","identity":"opaque-original","title":"codex"}]}))
    })
    .unwrap();
    for (yes, cancelled) in [(false, false), (true, true), (false, true)] {
        assert!(
            token
                .finish(yes, cancelled, |_| panic!("cancel must not post"))
                .unwrap()
                .is_none()
        );
    }
    token.finish(true,false,|body|{assert_eq!(*body,json!({"session":"fixture","action":"close","pane":"%7","identity":"opaque-original"}));Ok(json!({"closed":"%7"}))}).unwrap();
    let stale = token.finish(true, false, |body| {
        assert_eq!(body["identity"], "opaque-original");
        Err("pane identity changed; no process touched".into())
    });
    assert!(stale.is_err());
    assert!(PaneClose::prepare("fixture", "%7", |_| Ok(json!({"panes":[{"id":"%7"}]}))).is_err());
}
#[test]
fn ordinary_close_preserves_busy_sessions_like_original_ast() {
    use comandos_app::proc::{ProcSpec, run};
    let cases = json!([
        ["term-busy", "zsh\ncodex", true],
        ["term-busy", "node\nbash", true],
        ["term-busy", "sleep", true],
        ["term-idle", "zsh\nbash", true],
        ["fixture", "bash", true],
        ["term-empty", "", true],
        ["term-bad", "bash", false]
    ]);
    let home = std::env::temp_dir().join(format!("comandos-close-oracle-{}", std::process::id()));
    std::fs::create_dir(&home).unwrap();
    let script = r#"import ast,json,sys,types
nodes=ast.parse(open(sys.argv[1]).read()).body
exec(compile(ast.Module(body=[n for n in nodes if isinstance(n,ast.FunctionDef) and n.name=='close_tab'],type_ignores=[]),sys.argv[1],'exec'))
tabs={};snip_log=lambda _:None;archive_tab=lambda *a,**k:None;save_tabs=lambda:None
out=[]
for key,commands,success in json.load(sys.stdin):
 calls=[]
 def tmuxc(*args):
  calls.append(args)
  return types.SimpleNamespace(returncode=0 if success else 1,stdout=commands)
 close_tab(key,False)
 out.append(any(c[0]=='kill-session' for c in calls))
print(json.dumps(out))
"#;
    let result = run(&ProcSpec {
        program: "python3".into(),
        args: vec![
            "-c".into(),
            script.into(),
            concat!(env!("CARGO_MANIFEST_DIR"), "/../../bin/cc-app").into(),
        ],
        stdin: Some(serde_json::to_vec(&cases).unwrap()),
        env: vec![
            ("HOME".into(), home.display().to_string().into()),
            ("PATH".into(), "/usr/bin:/bin".into()),
        ],
        clear_env: true,
        env_remove: vec![],
        cwd: Some(home.clone()),
        timeout: std::time::Duration::from_secs(5),
    })
    .unwrap();
    assert_eq!(
        result.code,
        Some(0),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let oracle: Value = serde_json::from_slice(&result.stdout).unwrap();
    let actual: Vec<_> = cases
        .as_array()
        .unwrap()
        .iter()
        .map(|c| {
            comandos_app::tab_actions::idle_scratch_commands(
                c[0].as_str().unwrap(),
                c[2].as_bool().unwrap(),
                c[1].as_str().unwrap(),
            )
        })
        .collect();
    assert_eq!(json!(actual), oracle);
    std::fs::remove_dir_all(home).unwrap();
}
