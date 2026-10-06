#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]
#[path = "support/mac_oracle.rs"]
mod oracle;
use comandos_desktop::{
    bridge::{BridgeMessage, parse_mac_bridge},
    dash_client::RetrySchedule,
    lang::{initial_theme, ui_lang},
    mac_tabs::*,
    term_url::TermUrl,
};
use serde_json::{Value, json};
use std::time::Duration;
#[test]
fn metadata_loading_and_restore_identity_match_real_mac_functions() {
    for value in [
        json!(null),
        json!([]),
        json!({}),
        json!({"ssh-client":{"kind":"project","cwd":"/owned"},"term-r1":{"kind":"scratch","cwd":"relative"},"s":{"kind":"unknown"},"wrong":null,"":{"kind":"ssh","host":"host"}}),
    ] {
        let actual = load_tab_metadata(&serde_json::to_vec(&value).unwrap()).unwrap();
        assert_eq!(
            serde_json::to_value(&actual).unwrap(),
            oracle::original(json!({"op":"metadata","data":value}))
        );
        for key in [
            "ssh-client",
            "sshtab-host-3:claude",
            "sshtab--1",
            "ssh-",
            "xterm-1",
            "term-r1",
            "project",
        ] {
            let raw = serde_json::to_value(&actual).unwrap();
            for project in ["", "/owned/project"] {
                assert_eq!(
                    serde_json::to_value(restore_tab_spec(key, &actual, project)).unwrap(),
                    oracle::original(
                        json!({"op":"restore","key":key,"data":raw,"project_dir":project})
                    )
                );
            }
        }
    }
    for raw in [b"{bad".as_slice(), b"null"] {
        assert!(load_tab_metadata(raw).unwrap().is_empty());
    }
}
#[test]
fn ssh_suffix_digit_semantics_match_original_without_prefix_guessing() {
    for session in [
        "ssh-host",
        "ssh-",
        "sshtab-host-3",
        "sshtab--1",
        "sshtab-host-name-03",
        "sshtab-host-١",
        "sshtab-host-²",
        "sshtab-host-Ⅻ",
        "sshtab-host-",
        "term-r1",
        "sshtab-host-x",
    ] {
        assert_eq!(
            serde_json::to_value(ssh_host_from_session(session)).unwrap(),
            oracle::original(json!({"op":"ssh","session":session})),
            "{session}"
        );
    }
}
#[test]
fn saved_raw_labels_merge_and_cancel_preserve_legacy_order_and_unknown_values() {
    for raw in [
        json!([]),
        json!(["term-r1", "", "ssh-host", 3, null]),
        json!({"term-r1":"Old","odd":false,"other":{"unknown":1},"":null}),
        json!(null),
    ] {
        let saved = load_saved_tabs(&serde_json::to_vec(&raw).unwrap());
        assert_eq!(
            serde_json::to_value(&saved).unwrap(),
            oracle::original(json!({"op":"saved","data":raw}))
        );
        let current = vec![
            ("term-r1".into(), "New".into()),
            ("new".into(), "Fresh".into()),
        ];
        let actual: serde_json::Map<String, Value> = merge_tab_labels(&saved, &current)
            .into_iter()
            .map(|(k, v)| (k, Value::String(v)))
            .collect();
        assert_eq!(
            Value::Object(actual),
            oracle::original(json!({"op":"merge","saved":saved,"current":current}))
        );
        for session in ["term-r1", "missing"] {
            assert_eq!(
                serde_json::to_value(cancel_restore_snapshot(&saved, session)).unwrap(),
                oracle::original(json!({"op":"cancel","saved":saved,"session":session}))
            );
        }
    }
    let saved = vec![
        ("s:claude".into(), json!(null)),
        ("s:codex".into(), json!(false)),
        ("other".into(), json!("O")),
    ];
    assert_eq!(
        serde_json::to_value(cancel_restore_snapshot(&saved, "s")).unwrap(),
        oracle::original(json!({"op":"cancel","saved":saved,"session":"s"}))
    );
}
#[test]
fn history_uses_actual_agent_mapping_dedup_unknown_items_and_eighty_limit() {
    for cmd in [
        "node", "claude", "grok", "opencode", "gemini", "agy", "aider", "codex", "bash", " node \n",
    ] {
        let history = Value::Array(
            (0..90)
                .map(|i| {
                    if i % 5 == 0 {
                        json!({"session":"s","unknown":i})
                    } else if i % 3 == 0 {
                        json!(i)
                    } else {
                        json!({"session":format!("old{i}")} )
                    }
                })
                .collect(),
        );
        let item = history_item("s", "", " /owned \n", cmd, "closed", 42);
        let actual = archive_into(Some(history.clone()), item);
        assert_eq!(
            actual,
            oracle::original(
                json!({"op":"history","key":"s","label":"","cwd":" /owned \n","cmd":cmd,"reason":"closed","ts":42,"data":history})
            )
        );
    }
}
fn observed(message: Option<BridgeMessage>) -> Value {
    match message {
        None => json!([]),
        Some(BridgeMessage::Theme(theme)) => json!([["theme", theme]]),
        Some(BridgeMessage::Rename { session, label }) => json!([["rename", session, label]]),
        Some(BridgeMessage::Open {
            session,
            win,
            label,
        }) => json!([["open", session, win, label]]),
    }
}
#[test]
fn mac_bridge_body_forms_priority_whitespace_and_raw_payload_match_real_method() {
    for data in [
        json!({"theme":"neon"}),
        json!({"theme":7,"session":"s"}),
        json!({"type":"rename","session":"s","label":"  x  "}),
        json!({"type":"rename","session":"s","label":"\u{1c}x\u{1f}"}),
        json!({"session":"s"}),
        json!({"session":"s","win":null,"label":3}),
        json!({"session":7,"win":false}),
        json!("{\"session\":\"s\"}"),
        json!("bad"),
        json!([]),
        json!({"type":"rename","session":"s","label":3}),
    ] {
        assert_eq!(
            observed(parse_mac_bridge(&data)),
            oracle::original(json!({"op":"bridge","data":data}))
        );
    }
}
#[test]
fn term_url_keeps_python_query_order_auth_encoding_and_http_origin() {
    assert_eq!(
        TermUrl::build("http://127.0.0.1:7337", Some("term-r1"), "neon", "tok en"),
        "http://127.0.0.1:7337/term.html?theme=neon&auth=tok+en&arg=term-r1"
    );
    for session in [
        None,
        Some(""),
        Some("sshtab-host-3:claude"),
        Some("ñ😀 +/%"),
    ] {
        for auth in ["", "tok en", "ñ &+/~"] {
            let url = TermUrl::build("http://127.0.0.1:7337", session, "neon", auth);
            let expected = oracle::original(
                json!({"op":"term-url","session":session,"theme":"neon","auth":auth}),
            );
            assert_eq!(url.split_once('?').unwrap().1, expected);
            assert!(url.starts_with("http://127.0.0.1:7337/term.html?theme=neon&auth="));
            assert!(!url.contains("ws="));
        }
    }
}
#[test]
fn language_theme_catalog_and_retry_interval_match_real_mac_functions() {
    for conf in [None, Some("es"), Some("en"), Some("xx")] {
        for env in ["", "es_MX", "ES-mx", "en_US", " es"] {
            assert_eq!(
                ui_lang(conf, env).as_str(),
                oracle::original(json!({"op":"lang","conf_lang":conf,"env_lang":env}))
            );
        }
    }
    for prefs in [
        json!(null),
        json!([]),
        json!({}),
        json!({"theme":"neon"}),
        json!({"theme":"unknown"}),
        json!({"theme":null}),
    ] {
        assert_eq!(
            initial_theme(&prefs),
            oracle::original(json!({"op":"theme","prefs":prefs}))
        );
    }
    let constants = oracle::original(json!({"op":"constants"}));
    assert_eq!(
        serde_json::to_value(comandos_desktop::VALID_THEMES).unwrap(),
        constants["VALID_THEMES"]
    );
    assert_eq!(comandos_desktop::DOT_IDLE, constants["DOT_IDLE"]);
    assert_eq!(comandos_desktop::DEFAULT_THEME, constants["DEFAULT_THEME"]);
    let colors: serde_json::Map<String, Value> = comandos_desktop::DOT_COLORS
        .iter()
        .map(|(k, v)| ((*k).into(), json!(v)))
        .collect();
    assert_eq!(Value::Object(colors), constants["DOT_COLORS"]);
    for dashboard in [false, true] {
        let actual = RetrySchedule::after_failure(Duration::ZERO, dashboard);
        let oracle = oracle::original(json!({"op":"retry","dashboard":dashboard}));
        assert_eq!(
            actual.map(|v| v.as_secs_f64()),
            oracle
                .as_array()
                .unwrap()
                .first()
                .map(|v| v[0].as_f64().unwrap())
        );
    }
}
#[test]
fn project_lookup_scans_only_owned_two_levels_and_keeps_first_match() {
    use std::{os::unix::fs::DirBuilderExt, path::PathBuf};
    struct Dir(PathBuf);
    impl Drop for Dir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    let path = std::env::temp_dir().join(format!("m2-project-{}", std::process::id()));
    let root = Dir(path);
    for name in [
        "Owner/thing.name",
        "direct.name",
        "Owner/node_modules/hidden",
        "Owner/deep/third",
    ] {
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(root.0.join(name))
            .unwrap();
    }
    for session in [
        "direct-name",
        "thing-name",
        "node_modules",
        "hidden",
        "third",
        "missing",
    ] {
        assert_eq!(
            serde_json::to_value(find_project_dir(&root.0, session)).unwrap(),
            oracle::original(json!({"op":"project","session":session,"codebase":root.0}))
        );
    }
}

#[test]
fn malformed_metadata_kind_preserves_original_fail_closed_exception() {
    for (kind, name) in [(json!([]), "list"), (json!({}), "dict")] {
        let raw = json!({"s":{"kind":kind}});
        assert_eq!(
            load_tab_metadata(&serde_json::to_vec(&raw).unwrap()),
            Err(MetadataError::UnhashableKind(name))
        );
        assert_eq!(
            oracle::original(json!({"op":"metadata","data":raw})),
            json!({"error":format!("unhashable type: '{name}'")})
        );
    }
}
