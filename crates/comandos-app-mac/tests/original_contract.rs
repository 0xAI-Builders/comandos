#![allow(clippy::unwrap_used)]
use comandos_app_mac::{
    app::{Action, App, WindowSpec},
    jobs::{Backend, execute_open},
};
use comandos_desktop::{
    Lang, parse_bridge,
    proc::{ProcOutput, ProcSpec, run},
};
use serde_json::{Value, json};
use std::{collections::VecDeque, sync::Mutex};
fn original(mut input: Value) -> Value {
    input["source"] = std::env::var("COMANDOS_MAC_APP_ORACLE")
        .unwrap_or_else(|_| concat!(env!("CARGO_MANIFEST_DIR"), "/../../bin/cc-app-mac").into())
        .into();
    let out = run(&ProcSpec {
        program: "python3".into(),
        args: vec!["-c".into(), include_str!("support/original.py").into()],
        stdin: Some(serde_json::to_vec(&input).unwrap()),
        env: vec![],
        clear_env: false,
        env_remove: vec![],
        cwd: Some(std::env::temp_dir()),
        timeout: std::time::Duration::from_secs(3),
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
fn window_spec_is_calibrated_against_executed_original_build_window() {
    let data = original(json!({"op":"window"}));
    let w = WindowSpec::default();
    let rows = data.as_array().unwrap();
    let window = rows.iter().find(|r| r[0] == "window").unwrap();
    assert_eq!(window[1][2].as_f64(), Some(w.size.0));
    assert_eq!(window[1][3].as_f64(), Some(w.size.1));
    let minimum = rows.iter().find(|r| r[0] == "minimum").unwrap();
    assert_eq!(minimum[1][0].as_f64(), Some(w.minimum.0));
    assert_eq!(minimum[1][1].as_f64(), Some(w.minimum.1));
    assert!(rows.contains(&json!(["title", w.title])));
    assert!(rows.contains(&json!([
        "divider",
        (w.size.0 * w.left_fraction).floor() as i64
    ])));
    assert!(rows.contains(&json!(["appearance", "DarkAqua"])));
    assert!(rows.contains(&json!(["scroll", true])));
}
#[test]
fn nine_theme_scripts_match_actual_original_method_byte_for_byte() {
    for name in comandos_desktop::VALID_THEMES
        .iter()
        .chain(["invalid"].iter())
    {
        let expected = original(json!({"op":"theme","name":name}));
        let mut app = App::new("noche", Lang::Es);
        app.receive_bridge(parse_bridge(&json!({"theme":name})).unwrap())
            .unwrap();
        assert_eq!(app.theme(), expected["theme"]);
        if *name != "invalid" {
            assert_eq!(app.theme_script(), expected["scripts"][0]);
        }
    }
}
#[test]
fn authentication_defer_and_drain_match_original_before_ready() {
    let expected =
        original(json!({"op":"queue","ready":false,"token":"","sessions":["one","two"]}));
    let mut app = App::new("noche", Lang::En);
    let ticket = app.ticket();
    for session in ["one", "two"] {
        app.receive_bridge(parse_bridge(&json!({"session":session})).unwrap())
            .unwrap();
    }
    assert_eq!(
        app.pending_count(),
        expected["pending"].as_u64().unwrap() as usize
    );
    let actions = app.finish_boot(&ticket, "owned");
    assert_eq!(
        actions
            .iter()
            .map(|action| match action {
                Action::Open { session, .. } => session.clone(),
                _ => String::new(),
            })
            .collect::<Vec<_>>(),
        vec!["one", "two"]
    );
}
struct Fake {
    calls: Mutex<Vec<Value>>,
    replies: Mutex<VecDeque<(i32, String)>>,
}
impl Backend for Fake {
    fn get(&self, _: &str) -> Result<Value, String> {
        Ok(Value::Null)
    }
    fn post(&self, _: &str, _: &Value) -> Result<Value, String> {
        Ok(Value::Null)
    }
    fn cancel(&self) {}
    fn tmux(&self, args: &[&str], allowed: &dyn Fn() -> bool) -> Result<ProcOutput, String> {
        assert!(allowed());
        self.calls.lock().unwrap().push(json!(args));
        let (code, out) = self.replies.lock().unwrap().pop_front().unwrap();
        Ok(ProcOutput {
            code: Some(code),
            stdout: out.into_bytes(),
            ..Default::default()
        })
    }
}
#[test]
fn select_window_pipeline_matches_original_all_fallback_branches() {
    for (win, replies) in [
        ("work", vec![(0, ""), (1, "")]),
        ("work", vec![(1, "")]),
        ("claude", vec![(1, ""), (0, "0|zsh\n2|claude\n"), (0, "")]),
        ("claude", vec![(1, ""), (0, "0|zsh\n"), (0, "")]),
    ] {
        let data = json!({"op":"select","win":win,"session":"owned","replies":replies});
        let expected = original(data);
        let fake = Fake {
            calls: Mutex::new(vec![]),
            replies: Mutex::new(replies.into_iter().map(|(c, s)| (c, s.into())).collect()),
        };
        let app = App::new("noche", Lang::Es);
        execute_open(
            &fake,
            &Action::Open {
                session: "owned".into(),
                win: win.into(),
                label: None,
            },
            true,
            &app.ticket(),
        )
        .unwrap();
        assert_eq!(*fake.calls.lock().unwrap(), *expected.as_array().unwrap());
    }
}
