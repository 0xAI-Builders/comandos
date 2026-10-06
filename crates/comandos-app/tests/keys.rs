#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]
use comandos_app::ui::keys::{KeyAction, KeyInput, action};
#[test]
fn shortcuts_consume_only_app_actions_and_question_reaches_chat() {
    let mut key = KeyInput::new(*gdk::keys::constants::F1);
    assert_eq!(action(key), KeyAction::Help);
    key.key = *gdk::keys::constants::question;
    assert_eq!(action(key), KeyAction::Pass);
    key.key = *gdk::keys::constants::k;
    key.control = true;
    assert_eq!(action(key), KeyAction::Switcher);
    key.shift = true;
    assert_eq!(action(key), KeyAction::Snippets);
    key.key = *gdk::keys::constants::Tab;
    assert_eq!(action(key), KeyAction::Cycle(-1));
    key.shift = false;
    assert_eq!(action(key), KeyAction::MruOrNext);
}
#[path = "support/t15_adapter.rs"]
mod adapter;
#[test]
fn source_extracted_callbacks_release_row_borrows_and_require_selection() {
    assert_eq!(adapter::execute()["panel"], true);
}

#[test]
fn original_on_key_ast_matches_native_decision_matrix() {
    use comandos_app::proc::{ProcSpec, run};
    use gdk::keys::constants as k;
    let keys = [
        ("Escape", k::Escape),
        ("F1", k::F1),
        ("F12", k::F12),
        ("F5", k::F5),
        ("v", k::v),
        ("V", k::V),
        ("c", k::c),
        ("C", k::C),
        ("a", k::a),
        ("A", k::A),
        ("Tab", k::Tab),
        ("ISO_Left_Tab", k::ISO_Left_Tab),
        ("g", k::g),
        ("G", k::G),
        ("Page_Up", k::Page_Up),
        ("Page_Down", k::Page_Down),
        ("plus", k::plus),
        ("equal", k::equal),
        ("KP_Add", k::KP_Add),
        ("minus", k::minus),
        ("underscore", k::underscore),
        ("KP_Subtract", k::KP_Subtract),
        ("0", k::_0),
        ("parenright", k::parenright),
        ("k", k::k),
        ("K", k::K),
        ("t", k::t),
        ("T", k::T),
        ("w", k::w),
        ("W", k::W),
        ("q", k::q),
        ("Q", k::Q),
        ("question", k::question),
        ("Control_L", k::Control_L),
        ("Control_R", k::Control_R),
        ("Shift_L", k::Shift_L),
        ("Shift_R", k::Shift_R),
        ("Alt_L", k::Alt_L),
        ("Alt_R", k::Alt_R),
        ("Super_L", k::Super_L),
        ("Super_R", k::Super_R),
    ];
    let mut cases = Vec::new();
    let mut actual = Vec::new();
    for (_, key) in keys {
        for bits in 0..64 {
            let mut e = KeyInput::new(*key);
            e.control = bits & 1 != 0;
            e.shift = bits & 2 != 0;
            e.terminal = bits & 4 != 0;
            e.has_term = bits & 8 != 0;
            e.selection = bits & 16 != 0;
            e.drag = bits & 32 != 0;
            cases.push(serde_json::json!({"key":e.key,"control":e.control,"shift":e.shift,"terminal":e.terminal,"has_term":e.has_term,"selection":e.selection,"drag":e.drag}));
            actual.push(format!("{:?}", action(e)));
        }
    }
    let input = serde_json::json!({"keys":keys.into_iter().map(|(n,k)|(n,*k)).collect::<std::collections::BTreeMap<_,_>>(),"cases":cases});
    let out = run(&ProcSpec {
        program: "python3".into(),
        args: vec![
            "-c".into(),
            include_str!("support/t15_original.py").into(),
            std::env::var("COMANDOS_CC_APP_ORACLE")
                .unwrap_or_else(|_| concat!(env!("CARGO_MANIFEST_DIR"), "/../../bin/cc-app").into())
                .into(),
            "keys".into(),
        ],
        stdin: Some(serde_json::to_vec(&input).unwrap()),
        env: vec![],
        clear_env: false,
        env_remove: vec![],
        cwd: None,
        timeout: std::time::Duration::from_secs(15),
    })
    .unwrap();
    assert_eq!(
        out.code,
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let expected: Vec<String> = serde_json::from_slice(&out.stdout).unwrap();
    for (i, (a, b)) in actual.iter().zip(&expected).enumerate() {
        assert_eq!(a, b, "case {i}: {}", cases[i]);
    }
    assert_eq!(expected.len(), 2624);
}

#[test]
fn original_ctrl_c_owns_timestamp_per_terminal_object() {
    use comandos_app::proc::{ProcSpec, run};
    let scenarios = serde_json::json!([
        [
            ["new", "old"],
            ["c", "old", 1.0, false, false],
            ["close", "old"],
            ["new", "new"],
            ["c", "new", 1.2, false, false]
        ],
        [
            ["new", "same"],
            ["c", "same", 1.0, false, false],
            ["c", "same", 1.2, false, false]
        ],
        [
            ["new", "selection"],
            ["c", "selection", 1.0, true, false],
            ["c", "selection", 1.2, true, false]
        ],
        [
            ["new", "copy"],
            ["c", "copy", 1.0, false, true],
            ["c", "copy", 1.2, false, true]
        ],
        [
            ["new", "context"],
            ["c", "context", 1.0, false, false],
            ["c", "context", 1.1, false, true],
            ["c", "context", 1.2, false, false]
        ],
        [
            ["new", "old"],
            ["c", "old", 1.0, false, false],
            ["close", "old"],
            ["new", "new"],
            ["c", "new", 1.2, false, false],
            ["c", "new", 1.3, false, false]
        ]
    ]);
    let out = run(&ProcSpec {
        program: "python3".into(),
        args: vec![
            "-c".into(),
            include_str!("support/t15_original.py").into(),
            std::env::var("COMANDOS_CC_APP_ORACLE")
                .unwrap_or_else(|_| concat!(env!("CARGO_MANIFEST_DIR"), "/../../bin/cc-app").into())
                .into(),
            "gestures".into(),
        ],
        stdin: Some(serde_json::to_vec(&scenarios).unwrap()),
        env: vec![],
        clear_env: false,
        env_remove: vec![],
        cwd: None,
        timeout: std::time::Duration::from_secs(5),
    })
    .unwrap();
    assert_eq!(
        out.code,
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let expected: Vec<usize> = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(expected, [0, 1, 0, 0, 0, 1]);
}
