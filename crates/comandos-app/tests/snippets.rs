// Filesystem writes below create only injected private fixtures under TMPDIR.
#![allow(clippy::disallowed_methods)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]
use comandos_app::ui::snippets::{
    Snippet, delete_document, edit_document, filter_documents, filter_snippets,
};
use comandos_app::{
    config::RunMode,
    tmux::{TmuxError, TmuxOut},
    ui::{clipboard::TmuxIo, snippets::paste_snippet_when},
};
use serde_json::json;
use std::{
    cell::{Cell, RefCell},
    path::{Path, PathBuf},
};
#[path = "support/t16_oracle.rs"]
mod oracle;
#[test]
fn filter_matches_text_without_changing_body() {
    let items = vec![Snippet {
        name: "ñandú".into(),
        tags: vec!["rust".into()],
        body: "printf x; printf y".into(),
    }];
    assert_eq!(filter_snippets(&items, "ÑANDÚ"), vec![0]);
    assert_eq!(items[0].body, "printf x; printf y");
}
#[test]
fn document_filter_preserves_weight_stable_timestamp_order_and_payload() {
    let docs = vec![
        json!({"id":"a","name":"z","tags":[],"body":"rust","updated_at":99}),
        json!({"id":"b","name":"rust","tags":[],"body":"b","updated_at":1}),
        json!({"id":"c","name":"x","tags":["rust"],"body":"c","updated_at":100}),
        json!({"id":"d","name":"rust","tags":[],"body":"d","updated_at":1}),
    ];
    assert_eq!(filter_documents(&docs, " rust "), vec![1, 3, 2, 0]);
    assert_eq!(filter_documents(&docs, ""), vec![2, 0, 1, 3]);
    for query in [" rust ", "", "b", "none"] {
        assert_eq!(
            json!(filter_documents(&docs, query)),
            oracle::original(json!({"op":"filter","items":docs,"query":query}))
        );
    }
    assert_eq!(docs[0]["body"], "rust");
}
#[test]
fn editor_roundtrip_keeps_unknown_fields_tags_and_exact_body() {
    let old = vec![
        json!({"id":"a","name":"old","body":"old","tags":[],"updated_at":1,"unknown":{"keep":true}}),
    ];
    let next = edit_document(
        &old,
        Some("a"),
        " new ",
        " rust, rust, ñ, ",
        " \n😀; $(touch nope)\n",
        8,
        "unused",
    )
    .unwrap();
    assert_eq!(next[0]["id"], "a");
    assert_eq!(next[0]["unknown"], old[0]["unknown"]);
    assert_eq!(next[0]["tags"], json!(["rust", "rust", "ñ"]));
    assert_eq!(next[0]["body"], " \n😀; $(touch nope)\n");
    assert!(edit_document(&next, None, " ", "", "x", 9, "new").is_err());
    assert!(edit_document(&next, None, "name", "", "\n ", 9, "new").is_err());
    let more = edit_document(&next, None, "new", "", "body", 9, "b").unwrap();
    assert_eq!(more[0]["id"], "b");
    assert_eq!(delete_document(&more, "a").len(), 1);
}
struct FakeTmux {
    mode: RunMode,
    commands: RefCell<Vec<(Vec<String>, Vec<u8>)>>,
    identity: Cell<u32>,
    reads: Cell<usize>,
    replace_at: usize,
    fail: &'static str,
    socket: PathBuf,
}
impl FakeTmux {
    fn new() -> Self {
        Self {
            mode: RunMode::Sandbox,
            commands: RefCell::new(vec![]),
            identity: Cell::new(1),
            reads: Cell::new(0),
            replace_at: usize::MAX,
            fail: "",
            socket: std::env::temp_dir().join("fake-private-S"),
        }
    }
}
impl TmuxIo for FakeTmux {
    fn mode(&self) -> RunMode {
        self.mode
    }
    fn socket(&self) -> &Path {
        &self.socket
    }
    fn read(&self, args: &[&str]) -> Result<TmuxOut, TmuxError> {
        self.commands
            .borrow_mut()
            .push((args.iter().map(|s| s.to_string()).collect(), vec![]));
        self.reads.set(self.reads.get() + 1);
        if self.reads.get() >= self.replace_at {
            self.identity.set(2);
        }
        Ok(TmuxOut {
            code: 0,
            stdout: if args[0] == "display-message" {
                format!("111|${}|42|%7", self.identity.get())
            } else {
                String::new()
            },
            stderr: String::new(),
        })
    }
    fn mutate(&self, args: &[&str], stdin: Option<&[u8]>) -> Result<TmuxOut, TmuxError> {
        self.commands.borrow_mut().push((
            args.iter().map(|s| s.to_string()).collect(),
            stdin.unwrap_or_default().to_vec(),
        ));
        Ok(TmuxOut {
            code: if args[0] == self.fail { 1 } else { 0 },
            stdout: String::new(),
            stderr: if args[0] == self.fail {
                "fixture error".into()
            } else {
                String::new()
            },
        })
    }
}
#[test]
fn paste_is_utf8_stdin_exact_target_and_owns_only_its_named_buffer() {
    let t = FakeTmux::new();
    let body = "ñ😀; $(touch not-run)\nsecond\n";
    paste_snippet_when("fixture", body, &t, || true).unwrap();
    let commands = t.commands.borrow();
    let load = commands
        .iter()
        .find(|(a, _)| a[0] == "load-buffer")
        .unwrap();
    assert_eq!(load.1, body.as_bytes());
    assert_eq!(load.0[3], "-");
    assert!(load.0[2].starts_with("comandos-snip-"));
    assert_eq!(load.0[2].len(), 30);
    let paste = commands
        .iter()
        .find(|(a, _)| a[0] == "paste-buffer")
        .unwrap();
    assert_eq!(
        paste.0,
        vec![
            "paste-buffer",
            "-p",
            "-d",
            "-b",
            &load.0[2],
            "-t",
            "=fixture:"
        ]
    );
    assert!(!commands.iter().any(|(a, _)| {
        a.iter()
            .any(|v| v.contains("touch") || v == "new-session" || v == "send-keys")
    }));
}
#[test]
fn paste_failure_cleans_created_buffer_but_load_failure_cannot_claim_one() {
    for fail in ["load-buffer", "paste-buffer"] {
        let mut t = FakeTmux::new();
        t.fail = fail;
        assert!(paste_snippet_when("fixture", "text", &t, || true).is_err());
        let c = t.commands.borrow();
        let deleted = c
            .iter()
            .filter(|(a, _)| a[0] == "delete-buffer")
            .collect::<Vec<_>>();
        assert_eq!(deleted.len(), usize::from(fail == "paste-buffer"));
        if let Some(del) = deleted.first() {
            let load = c.iter().find(|(a, _)| a[0] == "load-buffer").unwrap();
            assert_eq!(del.0[2], load.0[2]);
        }
    }
}
#[test]
fn shadow_cancelled_or_replaced_destination_never_pastes() {
    let mut t = FakeTmux::new();
    t.mode = RunMode::Shadow;
    assert!(paste_snippet_when("fixture", "text", &t, || true).is_err());
    assert!(t.commands.borrow().is_empty());
    let t = FakeTmux::new();
    assert!(paste_snippet_when("fixture", "text", &t, || false).is_err());
    assert!(t.commands.borrow().is_empty());
    let mut t = FakeTmux::new();
    t.replace_at = 3;
    assert!(paste_snippet_when("fixture", "text", &t, || true).is_err());
    assert!(
        !t.commands
            .borrow()
            .iter()
            .any(|(a, _)| a[0] == "load-buffer" || a[0] == "paste-buffer")
    );
    let mut t = FakeTmux::new();
    t.replace_at = 4;
    assert!(paste_snippet_when("fixture", "text", &t, || true).is_err());
    assert!(
        !t.commands
            .borrow()
            .iter()
            .any(|(a, _)| a[0] == "paste-buffer")
    );
}
#[test]
fn live_snippet_crud_uses_existing_authorized_hooks_and_checks_domain_first() {
    use comandos_app::{config::parse_args, guard::WriteGuard, state_files::StateFiles};
    use comandos_store::unified::{self, Mode};
    use std::os::unix::fs::PermissionsExt;
    for (i, mode) in [Mode::Legacy, Mode::Mirror, Mode::Unified, Mode::Sealed]
        .into_iter()
        .enumerate()
    {
        let root = std::env::temp_dir().join(format!("t16-live-crud-{}-{i}", std::process::id()));
        for d in ["home", "hooks", "run", "tmp"] {
            std::fs::create_dir_all(root.join(d)).unwrap();
            std::fs::set_permissions(root.join(d), std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        let cfg = parse_args(
            &[
                "--mode".into(),
                "live".into(),
                "--hooks-dir".into(),
                root.join("hooks").display().to_string(),
            ],
            false,
            &|k| match k {
                "HOME" => Some(root.join("home").display().to_string()),
                "XDG_RUNTIME_DIR" => Some(root.join("run").display().to_string()),
                "TMPDIR" => Some(root.join("tmp").display().to_string()),
                _ => None,
            },
        )
        .unwrap();
        let guard = WriteGuard::from_config(&cfg, ":owned-private");
        // The Live directory itself must remain outside the write whitelist.
        assert!(guard.create_dir_all(cfg.hooks_dir(), 0o700).is_err());
        let files = StateFiles::new(cfg.clone(), guard);
        let db = unified::open_unified(&unified::unified_path(cfg.home())).unwrap();
        unified::set_mode(&db, "ui-docs", mode, "fixture", 1).unwrap();
        let first = vec![
            json!({"id":"one","name":"first","body":"ñ😀\n","tags":[],"unknown":{"keep":true}}),
        ];
        let result = files.write_snippets_when(&[], &first, || true);
        if mode != Mode::Legacy {
            assert!(result.is_err());
            assert!(!cfg.hooks_dir().join("snippets.json").exists());
            assert!(!cfg.hooks_dir().join("snippets.json.lock").exists());
            continue;
        }
        assert!(
            result.unwrap(),
            "Live Legacy save should use the existing hooks directory"
        );
        assert_eq!(files.read_snippets().unwrap(), first);
        let next = edit_document(
            &first,
            Some("one"),
            "edited",
            "rust",
            "exact\n",
            2,
            "unused",
        )
        .unwrap();
        assert!(files.write_snippets_when(&first, &next, || true).unwrap());
        assert!(!files.write_snippets_when(&first, &[], || true).unwrap());
        assert_eq!(files.read_snippets().unwrap(), next);
        assert_eq!(next[0]["unknown"], first[0]["unknown"]);
        let deleted = delete_document(&next, "one");
        assert!(files.write_snippets_when(&next, &deleted, || true).unwrap());
        assert!(files.read_snippets().unwrap().is_empty());
        assert!(!root.join("home/.comandos/store.lock").exists());
    }
}

#[test]
fn legacy_cas_preserves_python_json_and_refuses_nonlegacy_authority() {
    use comandos_app::{config::parse_args, guard::WriteGuard, state_files::StateFiles};
    use comandos_store::unified::{self, Mode};
    use std::os::unix::fs::PermissionsExt;
    for (i, mode) in [Mode::Legacy, Mode::Mirror, Mode::Unified, Mode::Sealed]
        .into_iter()
        .enumerate()
    {
        let root = std::env::temp_dir().join(format!("t16-store-{}-{i}", std::process::id()));
        for d in ["home", "run", "tmp"] {
            std::fs::create_dir_all(root.join(d)).unwrap();
            std::fs::set_permissions(root.join(d), std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        let env = |k: &str| match k {
            "HOME" => Some(root.join("home").to_string_lossy().into()),
            "XDG_RUNTIME_DIR" => Some(root.join("run").to_string_lossy().into()),
            "TMPDIR" => Some(root.join("tmp").to_string_lossy().into()),
            _ => None,
        };
        let cfg = parse_args(&["--mode".into(), "sandbox".into()], false, &env).unwrap();
        let guard = WriteGuard::from_config(&cfg, ":private");
        guard.create_dir_all(cfg.hooks_dir(), 0o700).unwrap();
        let files = StateFiles::new(cfg.clone(), guard.clone());
        let before = json!([{"id":"a","name":"ñ","body":"😀\n","tags":[],"unknown":true}]);
        files.write("snippets.json", &before).unwrap();
        let dbpath = unified::unified_path(cfg.home());
        let db = unified::open_unified(&dbpath).unwrap();
        unified::doc_put(
            &db,
            "H/snippets.json",
            "ui-docs",
            comandos_core::json::response_dumps(&before)
                .unwrap()
                .as_bytes(),
            unified::Origin::Import,
            1,
        )
        .unwrap();
        unified::set_mode(&db, "ui-docs", mode, "fixture", 1).unwrap();
        assert_eq!(
            files.read_snippets().unwrap(),
            before.as_array().unwrap().clone()
        );
        let next = json!([{"id":"b","name":"new","body":"body","tags":[]}]);
        let result =
            files.write_snippets_when(before.as_array().unwrap(), next.as_array().unwrap(), || {
                true
            });
        if mode == Mode::Legacy {
            assert!(result.unwrap());
            assert!(
                !files
                    .write_snippets_when(before.as_array().unwrap(), &[], || true)
                    .unwrap()
            );
            assert_eq!(files.read("snippets.json").unwrap(), next);
            assert!(
                !files
                    .write_snippets_when(next.as_array().unwrap(), &[], || false)
                    .unwrap()
            );
        } else {
            assert!(result.is_err());
            assert_eq!(files.read("snippets.json").unwrap(), before);
            assert!(
                !cfg.hooks_dir().join("snippets.json.lock").exists(),
                "authority must be checked before creating the legacy lock"
            );
        }
    }
}
#[test]
fn dialog_operation_ticket_dies_on_replacement_and_close() {
    use comandos_app::ui::snippets::Scope;
    let scope = Scope::default();
    let old = scope.ticket();
    assert!(old.current());
    scope.advance();
    assert!(!old.current());
    let new = scope.ticket();
    assert!(new.current());
    scope.close();
    assert!(!new.current());
    assert!(!scope.ticket().current());
}
#[test]
fn newly_unified_authority_cannot_publish_legacy_bytes() {
    use comandos_app::{config::parse_args, guard::WriteGuard, state_files::StateFiles};
    use comandos_store::unified::{self, Mode};
    use std::os::unix::fs::PermissionsExt;
    let root = std::env::temp_dir().join(format!("t16-authority-race-{}", std::process::id()));
    for d in ["home", "run", "tmp"] {
        std::fs::create_dir_all(root.join(d)).unwrap();
        std::fs::set_permissions(root.join(d), std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    let env = |k: &str| match k {
        "HOME" => Some(root.join("home").to_string_lossy().into()),
        "XDG_RUNTIME_DIR" => Some(root.join("run").to_string_lossy().into()),
        "TMPDIR" => Some(root.join("tmp").to_string_lossy().into()),
        _ => None,
    };
    let cfg = parse_args(&["--mode".into(), "sandbox".into()], false, &env).unwrap();
    let guard = WriteGuard::from_config(&cfg, ":private");
    guard.create_dir_all(cfg.hooks_dir(), 0o700).unwrap();
    let files = StateFiles::new(cfg.clone(), guard);
    let before = json!([{"id":"a","name":"a","body":"a","tags":[]}]);
    files.write("snippets.json", &before).unwrap();
    let result = files.write_snippets_when(before.as_array().unwrap(), &[], || {
        let db = unified::open_unified(&unified::unified_path(cfg.home())).unwrap();
        unified::set_mode(&db, "ui-docs", Mode::Unified, "fixture", 1).unwrap();
        true
    });
    assert!(result.is_err());
    assert_eq!(files.read("snippets.json").unwrap(), before);
}

#[test]
fn original_ast_editor_and_paste_preserve_data_and_named_buffer_contract() {
    let items = vec![
        json!({"id":"a","name":"old","body":"old","tags":[],"unknown":{"ñ":"😀"},"updated_at":1}),
    ];
    for id in [Some("a"), None] {
        let actual = edit_document(
            &items,
            id,
            " name ",
            " rust, rust,ñ ",
            " ñ😀; $(never)\n",
            8,
            "0123456789abcdef",
        )
        .unwrap();
        assert_eq!(
            json!(actual),
            oracle::original(
                json!({"op":"crud","items":items,"id":id,"name":" name ","tags":" rust, rust,ñ ","body":" ñ😀; $(never)\n","now":8,"new_id":"0123456789abcdef"})
            )
        );
    }
    assert_eq!(
        json!(delete_document(&items, "a")),
        oracle::original(json!({"op":"crud","items":items,"id":"a","delete":true}))
    );
    let expected = oracle::original(
        json!({"op":"paste","session":"fixture","body":"ñ😀; $(never)\n","new_id":"0123456789abcdef"}),
    );
    let t = FakeTmux::new();
    paste_snippet_when("fixture", "ñ😀; $(never)\n", &t, || true).unwrap();
    let calls = t.commands.borrow();
    let load = calls
        .iter()
        .find(|(args, _)| args[0] == "load-buffer")
        .unwrap();
    let actual = calls
        .iter()
        .filter(|(a, _)| a[0] != "display-message")
        .map(|(args, input)| {
            let mut argv = vec!["tmux".to_string()];
            argv.extend(args.iter().map(|a| {
                if a == &load.0[2] {
                    "comandos-snip-0123456789abcdef".to_string()
                } else {
                    a.clone()
                }
            }));
            json!([
                argv,
                if input.is_empty() {
                    None
                } else {
                    Some(String::from_utf8(input.clone()).unwrap())
                }
            ])
        })
        .collect::<Vec<_>>();
    assert_eq!(json!(actual), expected["calls"]);
    assert!(expected["error"].is_null());
}
#[test]
fn optional_log_never_creates_files_or_contains_payload() {
    use comandos_app::{
        config::parse_args,
        guard::WriteGuard,
        ui::snippets::{LogEvent, log_operation},
    };
    use std::os::unix::fs::PermissionsExt;
    let root = std::env::temp_dir().join(format!("t16-log-{}", std::process::id()));
    for d in ["home", "run", "tmp"] {
        std::fs::create_dir_all(root.join(d)).unwrap();
        std::fs::set_permissions(root.join(d), std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    let cfg = parse_args(&["--mode".into(), "sandbox".into()], false, &|k| match k {
        "HOME" => Some(root.join("home").display().to_string()),
        "XDG_RUNTIME_DIR" => Some(root.join("run").display().to_string()),
        "TMPDIR" => Some(root.join("tmp").display().to_string()),
        _ => None,
    })
    .unwrap();
    let guard = WriteGuard::from_config(&cfg, ":private");
    let path = cfg.sandbox_temp().unwrap().join("cc-app-snip.log");
    assert!(!log_operation(&guard, &path, LogEvent::Send, 4).unwrap());
    assert!(!path.exists());
    std::fs::write(&path, "").unwrap();
    assert!(log_operation(&guard, &path, LogEvent::Save, 2).unwrap());
    let text = std::fs::read_to_string(path).unwrap();
    assert!(text.contains("save count=2"));
    assert!(!text.contains("HOME") && !text.contains("body"));
}
#[test]
fn native_tmux_boundary_child() {
    let Ok(root) = std::env::var("T16_FAKE_NATIVE_ROOT") else {
        return;
    };
    let cfg = comandos_app::config::parse_args(
        &[
            "--mode".into(),
            "live".into(),
            "--tmux-socket".into(),
            "t16-owned-fake".into(),
        ],
        false,
        &|k| std::env::var(k).ok(),
    )
    .unwrap();
    let tmux = comandos_app::tmux::TmuxCtl::from_config(&cfg, &|k| std::env::var(k).ok()).unwrap();
    assert!(tmux.socket_path().starts_with(std::path::Path::new(&root)));
    paste_snippet_when("fixture", "ñ😀; $(never run)\nsecond\n", &tmux, || true).unwrap();
}
#[test]
fn actual_tmuxctl_uses_private_s_and_utf8_stdin_without_real_server() {
    use comandos_app::proc::{ProcSpec, run};
    use std::os::unix::fs::PermissionsExt;
    let root = std::env::temp_dir().join(format!("t16-native-boundary-{}", std::process::id()));
    for d in [
        "home", "config", "cache", "data", "state", "run", "tmp", "bin",
    ] {
        std::fs::create_dir_all(root.join(d)).unwrap();
        std::fs::set_permissions(root.join(d), std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    let script = root.join("bin/tmux");
    std::fs::write(&script,r#"#!/usr/bin/python3
import json,os,sys,pathlib
args=sys.argv[1:];root=os.environ['T16_FAKE_NATIVE_ROOT']
assert args[0]=='-S' and args[1].startswith(root+'/tmp/') and args[1].endswith('/t16-owned-fake'), args
assert not os.environ.get('TMUX') and not os.environ.get('TMUX_PANE')
body=sys.stdin.buffer.read()
with open(root+'/log.jsonl','a') as out:out.write(json.dumps({'argv':args,'body':body.decode('utf-8')})+'\n')
if args[2]=='display-message':print('111|$1|42|%7')
"#).unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).unwrap();
    let mut env = vec![
        ("T16_FAKE_NATIVE_ROOT".into(), root.as_os_str().to_owned()),
        ("PATH".into(), root.join("bin").into_os_string()),
        ("TMUX_TMPDIR".into(), root.join("tmp").into_os_string()),
        ("HOME".into(), root.join("home").into_os_string()),
        ("TMPDIR".into(), root.join("tmp").into_os_string()),
        ("TMP".into(), root.join("tmp").into_os_string()),
        ("TEMP".into(), root.join("tmp").into_os_string()),
    ];
    for (name, dir) in [
        ("XDG_RUNTIME_DIR", "run"),
        ("XDG_CONFIG_HOME", "config"),
        ("XDG_CACHE_HOME", "cache"),
        ("XDG_DATA_HOME", "data"),
        ("XDG_STATE_HOME", "state"),
        ("XDG_CONFIG_DIRS", "config"),
        ("XDG_DATA_DIRS", "data"),
    ] {
        env.push((name.into(), root.join(dir).into_os_string()));
    }
    let out = run(&ProcSpec {
        program: std::env::current_exe().unwrap().display().to_string(),
        args: ["--exact", "native_tmux_boundary_child", "--nocapture"]
            .map(Into::into)
            .to_vec(),
        stdin: None,
        env,
        clear_env: false,
        env_remove: vec![],
        cwd: Some(root.clone()),
        timeout: std::time::Duration::from_secs(10),
    })
    .unwrap();
    assert_eq!(
        out.code,
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let rows = std::fs::read_to_string(root.join("log.jsonl"))
        .unwrap()
        .lines()
        .map(|s| serde_json::from_str::<serde_json::Value>(s).unwrap())
        .collect::<Vec<_>>();
    let load = rows.iter().find(|v| v["argv"][2] == "load-buffer").unwrap();
    assert_eq!(load["body"], "ñ😀; $(never run)\nsecond\n");
    assert!(rows.iter().all(|v| v["argv"][0] == "-S"));
    assert!(rows.iter().all(|v| v["argv"][2] != "new-session"));
    assert_eq!(
        rows.iter()
            .find(|v| v["argv"][2] == "paste-buffer")
            .unwrap()["argv"]
            .as_array()
            .unwrap()
            .last()
            .unwrap(),
        "=fixture:"
    );
}
#[test]
fn authority_published_at_final_cancel_check_cannot_write_legacy() {
    use comandos_app::{config::parse_args, guard::WriteGuard, state_files::StateFiles};
    use comandos_store::unified::{self, Mode};
    use std::os::unix::fs::PermissionsExt;
    let root = std::env::temp_dir().join(format!("t16-authority-final-{}", std::process::id()));
    for d in ["home", "run", "tmp"] {
        std::fs::create_dir_all(root.join(d)).unwrap();
        std::fs::set_permissions(root.join(d), std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    let cfg = parse_args(&["--mode".into(), "sandbox".into()], false, &|k| match k {
        "HOME" => Some(root.join("home").display().to_string()),
        "XDG_RUNTIME_DIR" => Some(root.join("run").display().to_string()),
        "TMPDIR" => Some(root.join("tmp").display().to_string()),
        _ => None,
    })
    .unwrap();
    let guard = WriteGuard::from_config(&cfg, ":private");
    guard.create_dir_all(cfg.hooks_dir(), 0o700).unwrap();
    let files = StateFiles::new(cfg.clone(), guard);
    let before = json!([{"id":"old","name":"old","body":"old"}]);
    files.write("snippets.json", &before).unwrap();
    let checks = Cell::new(0);
    let result = files.write_snippets_when(before.as_array().unwrap(), &[], || {
        checks.set(checks.get() + 1);
        if checks.get() == 2 {
            let db = unified::open_unified(&unified::unified_path(cfg.home())).unwrap();
            unified::doc_put(
                &db,
                "H/snippets.json",
                "ui-docs",
                b"[]",
                unified::Origin::Import,
                1,
            )
            .unwrap();
            unified::set_mode(&db, "ui-docs", Mode::Unified, "fixture", 1).unwrap();
        }
        true
    });
    assert!(result.is_err());
    assert_eq!(
        files.read("snippets.json").unwrap(),
        before,
        "mode must be proven after final cancellation callback before publishing bytes"
    );
}
fn private_files(
    suffix: &str,
) -> (
    comandos_app::state_files::StateFiles,
    comandos_app::config::AppConfig,
    comandos_app::guard::WriteGuard,
) {
    use std::os::unix::fs::PermissionsExt;
    let root = std::env::temp_dir().join(format!("t16-{suffix}-{}", std::process::id()));
    for d in ["home", "run", "tmp"] {
        std::fs::create_dir_all(root.join(d)).unwrap();
        std::fs::set_permissions(root.join(d), std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    let cfg =
        comandos_app::config::parse_args(
            &["--mode".into(), "sandbox".into()],
            false,
            &|k| match k {
                "HOME" => Some(root.join("home").display().to_string()),
                "XDG_RUNTIME_DIR" => Some(root.join("run").display().to_string()),
                "TMPDIR" => Some(root.join("tmp").display().to_string()),
                _ => None,
            },
        )
        .unwrap();
    let guard = comandos_app::guard::WriteGuard::from_config(&cfg, ":private");
    guard.create_dir_all(cfg.hooks_dir(), 0o700).unwrap();
    (
        comandos_app::state_files::StateFiles::new(cfg.clone(), guard.clone()),
        cfg,
        guard,
    )
}
#[test]
fn cancelled_publication_keeps_legacy_bytes_and_removes_only_owned_temp() {
    let (files, cfg, _) = private_files("publish-cancel");
    let before = json!([{"id":"a","name":"old","body":"old"}]);
    files.write("snippets.json", &before).unwrap();
    let calls = Cell::new(0);
    let result = files.write_snippets_when(before.as_array().unwrap(), &[], || {
        calls.set(calls.get() + 1);
        calls.get() < 3
    });
    assert!(
        !matches!(result, Ok(true)),
        "closing the dialog before rename must cancel publication"
    );
    assert_eq!(files.read("snippets.json").unwrap(), before);
    assert!(!std::fs::read_dir(cfg.hooks_dir()).unwrap().any(|entry| {
        entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".snippets.json.")
    }));
}
#[test]
fn moved_legacy_placeholder_is_never_overwritten() {
    let (files, cfg, guard) = private_files("moved");
    let path = cfg.hooks_dir().join("snippets.json");
    guard
        .write_atomic(&path, b"MOVED", ".snippets.json.")
        .unwrap();
    assert!(files.read_snippets().is_err());
    assert!(files.write_snippets_when(&[], &[], || true).is_err());
    assert_eq!(std::fs::read(path).unwrap(), b"MOVED");
}
