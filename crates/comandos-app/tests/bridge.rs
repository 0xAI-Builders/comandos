#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]
use comandos_app::ui::bridge::{BridgeMsg, parse_bridge};
#[path = "support/t13_oracle.rs"]
mod oracle;
use serde_json::{Value, json};
#[test]
fn malformed_bridge_never_reaches_ui_dispatch() {
    for raw in ["{", "null", "[]", "1", "{}", r#"{"session":"bad;target"}"#] {
        assert!(parse_bridge(raw).is_err());
    }
}
#[test]
fn branch_priority_and_selected_invalid_branch_do_not_fall_through() {
    assert_eq!(
        parse_bridge(r#"{"type":"reader","action":"close","theme":"bruno","session":"safe"}"#)
            .unwrap(),
        BridgeMsg::Reader {
            action: "close".into(),
            id: "".into(),
            on: false
        }
    );
    assert!(parse_bridge(r#"{"openUrlModal":"file:///tmp/private","session":"safe"}"#).is_err());
    assert_eq!(
        parse_bridge(r#"{"theme":"bruno","openUrl":"https://example.test","session":"safe"}"#)
            .unwrap(),
        BridgeMsg::Theme("bruno".into())
    );
}
#[test]
fn reader_and_unicode_url_bounds() {
    assert_eq!(
        parse_bridge(r#"{"type":"reader","action":"open","id":"bad"}"#).unwrap(),
        BridgeMsg::Reader {
            action: "open".into(),
            id: "".into(),
            on: false
        }
    );
    let raw = serde_json::json!({"openUrl":"https://example.test/".to_string()+&"é".repeat(2100),"label":"漢".repeat(100)}).to_string();
    let BridgeMsg::OpenUrl { url, label, modal } = parse_bridge(&raw).unwrap() else {
        panic!("url branch")
    };
    assert_eq!(url.chars().count(), 2000);
    assert_eq!(label.chars().count(), 60);
    assert!(!modal);
}
fn observed(message: BridgeMsg) -> Value {
    let (name, args, kw) = match message {
        BridgeMsg::Extensions {
            session,
            pane,
            harness,
        } => (
            "_open_extension_shelf",
            json!([session, pane, harness]),
            json!({}),
        ),
        BridgeMsg::Reader { action, id, on } => match action.as_str() {
            "open" => ("reader_open", json!([id]), json!({})),
            "close" => ("reader_close", json!([]), json!({})),
            _ => ("reader_terminal", json!([on]), json!({})),
        },
        BridgeMsg::ButtonStyle(style) => ("apply_button_style", json!([style]), json!({})),
        BridgeMsg::HeaderAction(action) => return json!([[action, [], {}]]),
        BridgeMsg::SidebarTerm(value) => (
            "_side_term_show",
            json!([value.get("session").and_then(Value::as_str).unwrap_or("")]),
            json!({}),
        ),
        BridgeMsg::LeftPanel(action) => ("_left_panel_set", json!([action != "show"]), json!({})),
        BridgeMsg::ChainModal(value) => ("_chain_modal_msg", json!([value]), json!({})),
        BridgeMsg::Theme(theme) => ("apply_theme", json!([theme]), json!({})),
        BridgeMsg::OpenUrl { url, label, modal } => (
            if modal {
                "open_web_modal"
            } else {
                "open_web_tab"
            },
            json!([url, label]),
            json!({}),
        ),
        BridgeMsg::Rename { session, label } => {
            ("set_tab_label", json!([session, label]), json!({}))
        }
        BridgeMsg::OpenSession {
            session,
            window,
            label,
        } => ("open_tab", json!([session, window]), json!({"label":label})),
    };
    json!([[name, args, kw]])
}
#[test]
fn original_ast_oracle_covers_every_branch_and_priority() {
    let mut cases = vec![
        json!({"type":"extensions","session":"safe","pane":"%4","harness":"claude","theme":"bruno"}),
        json!({"type":"reader","action":"open","id":"2026-10-05@10:42"}),
        json!({"type":"reader","action":"open","id":"٢٠٢٦-١٠-٠٥@١٠:٤٢"}),
        json!({"type":"reader","action":"open","id":"bad"}),
        json!({"type":"reader","action":"close","id":{"unused":"ignored"},"session":"safe"}),
        json!({"type":"reader","action":"terminal","on":[1],"headerAction":"quickTerminal"}),
        json!({"sidebarTerm":{"session":"safe","hidden":true,"tabs":[]},"leftPanel":"show","theme":"bruno"}),
        json!({"theme":"bruno","openUrlModal":"https://example.test"}),
        json!({"theme":"","openUrl":"https://example.test","label":true}),
        json!({"openUrl":"https://example.test","label":{"é":"'quoted'","bool":true,"float":1e-6,"list":["漢","\u{a0}",null,3.0]}}),
        json!({"openUrlModal":"https://example.test/".to_string()+&"é".repeat(2100),"label":"漢".repeat(100),"openUrl":"https://ignored.test"}),
        json!({"type":"rename","session":"safe","label":"  Nuevo  ","openUrl":""}),
        json!({"session":"safe","win":"invalid:window","label":"Label"}),
        json!({"session":"safe","label":42}),
        json!({"buttonStyle":"invalid","headerAction":"invalid","session":"safe"}),
    ];
    for style in ["sutil", "arcade", "tecla", "pixel", "consola"] {
        cases.push(json!({"buttonStyle":style,"headerAction":"sortMenu","theme":"bruno"}));
    }
    for action in comandos_app::ui::bridge::HEADER_ACTIONS {
        cases.push(json!({"headerAction":action,"sidebarTerm":{},"theme":"bruno"}));
    }
    for action in ["toggle", "hide", "show"] {
        cases.push(json!({"leftPanel":action,"theme":"bruno"}));
    }
    for action in ["close", "saved", "run"] {
        cases.push(json!({"chainModal":action,"slug":"safe","theme":"bruno"}));
    }
    let expected = oracle::oracle("bridge", &json!(cases));
    for (case, expected) in cases.iter().zip(expected.as_array().unwrap()) {
        assert_eq!(
            &observed(parse_bridge(&case.to_string()).unwrap()),
            expected,
            "{case}"
        );
    }
}
#[test]
fn known_javascript_targets_quote_untrusted_arguments() {
    use comandos_app::ui::bridge::{JsFunction, js_call};
    let text = "');window.evil(); //\n\"漢";
    let script = js_call(JsFunction::StartChain, &[json!(text)]).unwrap();
    assert!(script.ends_with(&format!(
        "window.commandSidebar.startChain({});",
        json!(text)
    )));
    assert!(script.contains("throw new Error('Missing commandSidebar.startChain')"));
}
#[test]
fn queued_messages_wait_for_restore_and_close_discards_pending_work() {
    use comandos_app::ui::bridge::BridgeQueue;
    let mut queue = BridgeQueue::default();
    let message = BridgeMsg::Theme("bruno".into());
    queue.push(message.clone()).unwrap();
    assert!(queue.drain_ready(false).is_empty());
    assert_eq!(queue.drain_ready(true), vec![message.clone()]);
    for _ in 0..128 {
        queue.push(message.clone()).unwrap();
    }
    assert!(queue.push(message.clone()).is_err());
    queue.close();
    assert!(queue.drain_ready(true).is_empty());
    assert!(queue.push(message).is_err());
}
#[test]
fn malformed_header_is_rejected_before_session_and_nonstring_window_defaults() {
    for header in [json!([]), json!({"bad":"type"})] {
        let case = json!({"headerAction":header,"session":"safe"});
        assert_eq!(oracle::oracle("bridge", &json!([case])), json!([[]]));
        assert!(parse_bridge(&case.to_string()).is_err());
    }
    for window in [json!(false), json!(null), json!(0), json!([]), json!({})] {
        let case = json!({"session":"safe","win":window});
        let expected = oracle::oracle("bridge", &json!([case]));
        assert_eq!(
            observed(parse_bridge(&case.to_string()).unwrap()),
            expected[0]
        );
    }
    for window in [json!(true), json!(42), json!(["claude"])] {
        let case = json!({"session":"safe","win":window});
        assert_eq!(oracle::oracle("bridge", &json!([case])), json!([[]]));
        assert!(parse_bridge(&case.to_string()).is_err());
    }
}
