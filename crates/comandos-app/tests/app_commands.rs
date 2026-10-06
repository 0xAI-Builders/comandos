#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]
use comandos_app::ui::app_commands::{COMMAND_NAMES, CommandError, Registry, parse_command};
use serde_json::json;
use std::{cell::RefCell, rc::Rc};
#[path = "support/t13_oracle.rs"]
mod oracle;
use comandos_app::{config::RunMode, restore::RestoreTmux, ui::app_commands as commands};
#[test]
fn all_commands_dispatch_once_and_missing_consumers_fail_explicitly() {
    assert_eq!(COMMAND_NAMES.len(), 30);
    let registry = Registry::default();
    let calls = Rc::new(RefCell::new(Vec::new()));
    for name in COMMAND_NAMES {
        let command = parse_command(&json!({"command":name,"args":{}})).unwrap();
        assert_eq!(
            registry.dispatch(&command),
            Err(CommandError::MissingConsumer(name.into()))
        );
        let output = calls.clone();
        registry.install(
            name,
            Rc::new(move |args| {
                output.borrow_mut().push(args.clone());
                Ok(())
            }),
        );
        registry.dispatch(&command).unwrap();
    }
    assert_eq!(calls.borrow().len(), 30);
    registry.clear();
    assert!(
        registry
            .dispatch(&parse_command(&json!({"command":"quit"})).unwrap())
            .is_err()
    );
}
#[test]
fn invalid_commands_and_args_never_reach_consumers() {
    for value in [
        json!(null),
        json!({"command":"oops"}),
        json!({"command":"split","args":[1]}),
        json!({"command":"quit","args":true}),
    ] {
        assert!(parse_command(&value).is_err());
    }
    for args in [json!(null), json!(false), json!(""), json!([]), json!(0)] {
        assert_eq!(
            parse_command(&json!({"command":"help","args":args}))
                .unwrap()
                .args,
            json!({})
        );
    }
}
#[test]
fn original_ast_catalog_is_exact_and_all_thirty_actions_are_retained() {
    assert_eq!(oracle::oracle("keys", &json!([])), json!(COMMAND_NAMES));
    let cases:Vec<_>=COMMAND_NAMES.iter().map(|name|(*name,json!({"session":"safe","pane":"%4","side":"left","state":"on","on":true,"index":3,"action":"maximize","px":"805","delta":"0.25"}))).collect();
    let actual = oracle::oracle("commands", &json!(cases));
    let expected_calls = json!([
        [["_split_cmd",["left"],{}]],[["_split_cmd",["left"],{"ssh":true}]],[["_kill_cur_pane",["safe"],{}]],[["feed",[[2,108]],{}]],
        [["_select_pane_cmd",[cases[4].1],{}]],[["_cycle_tab",[1],{}]],[["_cycle_tab",[-1],{}]],[["_mru_toggle",[],{}]],[["nb.set_current_page",[2],{}]],[["tab_reorder",["safe",3],{}]],
        [["_mosaic_open",[],{}]],[["_mosaic_zoom",["safe","safe"],{}]],[["_left_panel_set",[false],{}]],[["nb.set_visible",[true],{}]],[["wv.load_uri",["private-dashboard-url"],{}]],[["set_font_scale",[1.25],{}]],
        [["open_switcher",[],{}]],[["open_tabs_overview",[],{}]],[["show_help",[],{}]],[["open_snippets_dialog",[],{}]],[["new_local_tab",[],{}]],[["open_xterm_tab",["safe"],{}]],[["_open_wizard",[],{}]],[["open_ai_session_here",["safe","%4"],{}]],
        [["copy_vte_selection",[],{}]],[["exit_copy_mode",[],{}],["paste_clipboard",[],{}]],[["copy_claude_reply",[],{}]],[["win.maximize",[],{}]],[["paned.set_position",[805],{}]],[["Gtk.main_quit",[],{}]]
    ]);
    for (actual, expected) in actual
        .as_array()
        .unwrap()
        .iter()
        .zip(expected_calls.as_array().unwrap())
    {
        assert_eq!(&actual["calls"], expected);
    }
    // The registry carries the real wire args unchanged to its single selected handler.
    let registry = Registry::default();
    let calls = Rc::new(RefCell::new(Vec::new()));
    for (name, args) in cases {
        let sink = calls.clone();
        registry.install(
            name,
            Rc::new(move |args| {
                sink.borrow_mut().push((name, args.clone()));
                Ok(())
            }),
        );
        registry
            .dispatch(&parse_command(&json!({"command":name,"args":args})).unwrap())
            .unwrap();
    }
    assert_eq!(calls.borrow().len(), 30);
}
#[test]
fn deferred_consumers_get_original_normalized_arguments() {
    assert_eq!(
        commands::deferred_arguments("mosaic", &json!({})).unwrap(),
        json!({"state":"toggle"})
    );
    assert!(commands::deferred_arguments("mosaic", &json!({"state":"bad"})).is_err());
    assert_eq!(
        commands::deferred_arguments("mosaic_zoom", &json!({"session":"safe","pane":"%5"}))
            .unwrap(),
        json!({"session":"safe","zoom_session":"safe"})
    );
    for on in [
        json!(null),
        json!(false),
        json!(true),
        json!([]),
        json!([1]),
        json!(""),
    ] {
        let input = json!({"on":on});
        let expected = oracle::oracle("commands", &json!([["side_panel", input]]));
        assert_eq!(
            commands::deferred_arguments("side_panel", &input).unwrap()["hidden"],
            expected[0]["calls"][0][1][0]
        );
    }
    for name in [
        "help",
        "snippets",
        "open_switcher",
        "tabs_overview",
        "open_wizard",
        "copy_selection",
        "paste_clipboard",
        "copy_reply",
    ] {
        assert_eq!(
            commands::deferred_arguments(name, &json!({"unrelated":"ignored"})).unwrap(),
            json!({})
        );
        assert!(commands::deferred_consumer(name).is_some());
    }
}
#[test]
fn fonts_and_integer_conversion_match_original_command_lambdas() {
    let cases: Vec<_> = [
        json!(null),
        json!(0),
        json!(false),
        json!(true),
        json!("0.25"),
        json!(5),
        json!(-5),
        json!("٠.٢٥"),
        json!("0.2_5"),
        json!("NaN"),
        json!("-Infinity"),
    ]
    .into_iter()
    .map(|delta| ("font_scale", json!({"delta":delta})))
    .collect();
    let expected = oracle::oracle("fonts", &json!(cases));
    for ((_, args), expected) in cases.iter().zip(expected.as_array().unwrap()) {
        let original = expected["calls"][0][1][0].as_f64().unwrap().clamp(0.5, 2.5);
        assert_eq!(commands::font_scale(1.0, &args["delta"]).unwrap(), original);
    }
    let cases: Vec<_> = [
        json!("805"),
        json!("  -40  "),
        json!(800.9),
        json!(true),
        json!(null),
        json!("bad"),
        json!("8_05"),
        json!("٨٠٥"),
    ]
    .into_iter()
    .map(|px| ("paned_position", json!({"px":px})))
    .collect();
    let expected = oracle::oracle("commands", &json!(cases));
    for ((_, args), expected) in cases.iter().zip(expected.as_array().unwrap()) {
        let candidate = commands::python_int(&args["px"]);
        if let Some(integer) = expected["calls"][0][1][0].as_i64() {
            assert_eq!(candidate.unwrap(), integer as i32);
        } else {
            assert!(candidate.is_err());
        }
    }
    assert_eq!(commands::font_scale(1.0, &json!("NaN")).unwrap(), 2.5);
}
struct FakeTmux {
    mode: RunMode,
    pane: String,
    cwd: String,
    calls: RefCell<Vec<Vec<String>>>,
    cancel_after: usize,
}
impl FakeTmux {
    fn new(mode: RunMode, pane: &str) -> Self {
        Self {
            mode,
            pane: pane.into(),
            cwd: "/private home/cwd".into(),
            calls: RefCell::new(Vec::new()),
            cancel_after: usize::MAX,
        }
    }
    fn cancelled(&self) -> bool {
        self.calls.borrow().len() >= self.cancel_after
    }
}
impl RestoreTmux for FakeTmux {
    type Ownership = ();
    fn mode(&self) -> RunMode {
        self.mode
    }
    fn read(&self, args: &[&str]) -> Result<String, String> {
        self.calls
            .borrow_mut()
            .push(args.iter().map(|s| (*s).into()).collect());
        Ok(if args.last() == Some(&"#{pane_id}") {
            self.pane.clone()
        } else {
            self.cwd.clone()
        })
    }
    fn mutate(&self, args: &[&str], _: Option<&[u8]>) -> Result<String, String> {
        self.calls
            .borrow_mut()
            .push(args.iter().map(|s| (*s).into()).collect());
        Ok(String::new())
    }
    fn create(&self, _: &[&str]) -> Result<(String, ()), String> {
        Err("creation forbidden in command fake".into())
    }
    fn cleanup(&self, _: ()) -> Result<(), String> {
        Err("cleanup forbidden in command fake".into())
    }
}
#[test]
fn split_arguments_match_real_original_function_and_preserve_exact_targets() {
    let mut cases = Vec::new();
    for side in ["right", "left", "down", "up"] {
        for (session, ssh, pane) in [
            ("safe", false, "%42"),
            ("ssh-private.host", true, "%42"),
            ("sshtab-host-name-3", true, "%42"),
            ("safe", true, ""),
        ] {
            cases.push(json!([session, side, ssh, pane, "/private home/cwd"]));
        }
    }
    let expected = oracle::oracle("split", &json!(cases));
    for (case, expected) in cases.iter().zip(expected.as_array().unwrap()) {
        let fake = FakeTmux::new(RunMode::Sandbox, case[3].as_str().unwrap());
        commands::execute_split(
            &fake,
            case[0].as_str().unwrap(),
            case[1].as_str().unwrap(),
            case[2].as_bool().unwrap(),
            || false,
        )
        .unwrap();
        assert_eq!(json!(&fake.calls.borrow()[1..]), expected["calls"]);
    }
    let sessions = json!([
        "ssh-private.host",
        "sshtab-host-name-3",
        "sshtab-host-name-no",
        "safe",
        "ssh-",
        null
    ]);
    let expected = oracle::oracle("ssh", &sessions);
    for (session, expected) in sessions
        .as_array()
        .unwrap()
        .iter()
        .zip(expected.as_array().unwrap())
    {
        assert_eq!(
            json!(session.as_str().and_then(commands::ssh_host)),
            *expected
        );
    }
}
#[test]
fn cancelled_invalid_or_shadow_commands_make_no_mutation() {
    for after in [0, 1, 2] {
        let mut fake = FakeTmux::new(RunMode::Sandbox, "%42");
        fake.cancel_after = after;
        assert!(
            commands::execute_split(&fake, "safe", "right", false, || fake.cancelled()).is_err()
        );
        assert!(
            !fake
                .calls
                .borrow()
                .iter()
                .any(|args| args.first().is_some_and(|arg| arg == "split-window"))
        );
    }
    for session in ["bad;session", "safe:other", "", "web:https://example.test"] {
        let fake = FakeTmux::new(RunMode::Sandbox, "%42");
        assert!(commands::execute_split(&fake, session, "right", false, || false).is_err());
        assert!(fake.calls.borrow().is_empty());
    }
    let fake = FakeTmux::new(RunMode::Shadow, "%42");
    assert!(commands::execute_split(&fake, "safe", "right", false, || false).is_err());
    assert!(commands::execute_select_pane(&fake, "%42", false).is_err());
    assert!(fake.calls.borrow().is_empty());
    let fake = FakeTmux::new(RunMode::Sandbox, "%42");
    for pane in ["%1;kill-server", "1", "safe:", "%"] {
        assert!(commands::execute_select_pane(&fake, pane, false).is_err());
    }
    assert!(commands::execute_select_pane(&fake, "%42", true).is_err());
    assert!(fake.calls.borrow().is_empty());
    commands::execute_select_pane(&fake, "%42", false).unwrap();
    assert_eq!(*fake.calls.borrow(), vec![vec!["select-pane", "-t", "%42"]]);
}
#[test]
fn registry_does_not_retain_owner_and_allows_reentrant_registration() {
    let registry = Rc::new(Registry::default());
    let owner = Rc::new(());
    let weak = Rc::downgrade(&owner);
    registry.install(
        "help",
        Rc::new(move |_| {
            weak.upgrade()
                .map(|_| ())
                .ok_or_else(|| CommandError::Refused("owner closed".into()))
        }),
    );
    assert_eq!(Rc::strong_count(&owner), 1);
    drop(owner);
    assert!(matches!(
        registry.dispatch(&parse_command(&json!({"command":"help"})).unwrap()),
        Err(CommandError::Refused(_))
    ));
    let weak = Rc::downgrade(&registry);
    registry.install(
        "quit",
        Rc::new(move |_| {
            weak.upgrade().unwrap().install("help", Rc::new(|_| Ok(())));
            Ok(())
        }),
    );
    registry
        .dispatch(&parse_command(&json!({"command":"quit"})).unwrap())
        .unwrap();
    registry
        .dispatch(&parse_command(&json!({"command":"help"})).unwrap())
        .unwrap();
    registry.clear();
}
