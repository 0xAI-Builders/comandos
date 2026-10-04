use comandos_core::workspace::{self as ws, layout};
use serde_json::{Value, json};
use std::collections::HashMap;

#[test]
fn numeric_process_identity_attaches_without_resuming() {
    for (saved, live) in [
        (json!(5), json!(5.0)),
        (json!(5.0), json!(5)),
        (json!(-5), json!(-5.0)),
        (json!(true), json!(1.0)),
        (json!(9007199254740992_u64), json!(9007199254740992.0)),
    ] {
        let d = json!({"schema":1,"groups":[{"id":"g","tree":{"type":"tab","tabId":"a"}}],"tabs":{"a":{"session":"a","paneKeys":["p"]}},"bindings":{"p":{"pid":saved,"startTime":saved,"conversation":{"id":"exact"}}}});
        let mut resumes = 0;
        let out = ws::restore_workspace(
            &d,
            |_| Ok(Some(json!({"pid":live,"startTime":live}))),
            |_| {
                resumes += 1;
                Ok(Some(json!({"paneId":"new"})))
            },
        )
        .unwrap();
        assert_eq!(resumes, 0, "saved={saved}, live={live}");
        assert_eq!(out["attached"], json!(["p"]));
    }
}

#[test]
fn changed_process_identity_never_attaches_including_large_integer_rounding() {
    for (saved, live) in [
        (json!(5), json!(5.5)),
        (json!(5), json!("5")),
        (json!(9007199254740993_u64), json!(9007199254740992.0)),
        (
            serde_json::from_str("340282366920938463463374607431768211457").unwrap(),
            serde_json::from_str("340282366920938463463374607431768211456").unwrap(),
        ),
    ] {
        for changed_key in ["pid", "startTime"] {
            let d = json!({"schema":1,"groups":[{"id":"g","tree":{"type":"tab","tabId":"a"}}],"tabs":{"a":{"session":"a","paneKeys":["p"]}},"bindings":{"p":{"pid":saved,"startTime":saved}}});
            let mut process = json!({"pid":saved,"startTime":saved});
            process[changed_key] = live.clone();
            let out = ws::restore_workspace(
                &d,
                |_| Ok(Some(process.clone())),
                |_| panic!("missing conversation must not resume"),
            )
            .unwrap();
            assert_eq!(out["attached"], json!([]), "saved={saved}, live={live}");
            assert_eq!(out["unavailable"], json!(["p"]));
        }
    }
}

#[test]
fn unrepresentable_decimal_process_identity_cannot_compare_as_zero() {
    let overflow: Value = serde_json::from_str("1e9999").unwrap();
    let d = json!({"schema":1,"groups":[{"id":"g","tree":{"type":"tab","tabId":"a"}}],"tabs":{"a":{"session":"a","paneKeys":["p"]}},"bindings":{"p":{"pid":0,"startTime":0}}});
    let out = ws::restore_workspace(
        &d,
        |_| Ok(Some(json!({"pid":overflow,"startTime":overflow}))),
        |_| panic!("missing conversation must not resume"),
    )
    .unwrap();
    assert_eq!(out["attached"], json!([]));
    assert_eq!(out["unavailable"], json!(["p"]));
}

#[test]
fn numeric_close_revision_accepts_equivalent_values_and_rejects_changed_values() {
    let p = ws::close_group_preview(&close_doc(), "g", identity).unwrap();
    for revision in [json!(1.0), json!(true)] {
        let mut s = state();
        let mut calls = vec![];
        assert!(
            ws::close_group(
                &mut s,
                "g",
                &revision,
                &p["members"],
                "equal",
                identity,
                |session| {
                    calls.push(session.to_string());
                    Ok(None)
                }
            )
            .unwrap()["ok"]
                .as_bool()
                .unwrap()
        );
        assert_eq!(calls, ["alpha", "beta"]);
    }
    for (saved, revision) in [
        (json!(1), json!(1.5)),
        (json!(1), json!("1")),
        (json!(9007199254740993_u64), json!(9007199254740992.0)),
    ] {
        let mut s = state();
        s.value.as_mut().unwrap()["revision"] = saved;
        assert!(matches!(
            ws::close_group(
                &mut s,
                "g",
                &revision,
                &p["members"],
                "changed",
                identity,
                |_| panic!("stale revision must not close")
            ),
            Err(ws::WorkspaceError::Conflict(_))
        ));
    }
}

#[test]
fn numeric_nested_session_identity_accepts_equivalent_values_before_effects() {
    let p = ws::close_group_preview(&close_doc(), "g", |session| json!({"session":session,"identity":[5,{"start":9007199254740992_u64,"bool":true}]})).unwrap();
    let mut calls = vec![];
    let out = ws::close_group(&mut state(), "g", &json!(1), &p["members"], "nested", |session| json!({"identity":[5.0,{"bool":1.0,"start":9007199254740992.0}],"session":session}), |session| { calls.push(session.to_string()); Ok(None) }).unwrap();
    assert_eq!(out["ok"], true);
    assert_eq!(calls, ["alpha", "beta"]);
}

#[test]
fn changed_nested_session_identity_rejects_before_any_close() {
    let p = ws::close_group_preview(
        &close_doc(),
        "g",
        |session| json!({"session":session,"identity":[5,{"start":9007199254740993_u64}]}),
    )
    .unwrap();
    for changed in [
        json!(9007199254740992.0),
        json!("9007199254740993"),
        Value::Null,
        json!([9007199254740993_u64]),
    ] {
        let result = ws::close_group(
            &mut state(),
            "g",
            &json!(1),
            &p["members"],
            "changed",
            |session| json!({"session":session,"identity":[5,{"start":if session == "beta" { changed.clone() } else { json!(9007199254740993_u64) }}]}),
            |_| panic!("changed identity must prevent all closes"),
        );
        assert!(matches!(result, Err(ws::WorkspaceError::Invalid(_))));
    }
}

// Expected numeric spelling is supplied explicitly from Python's persisted
// compact representation; do not use the codec under test to size the padding.
fn assert_python_client_byte_boundary(raw: &str, persisted: &str, text: &str) {
    let number: Value = serde_json::from_str(raw).unwrap();
    let mut input = json!({"activeTabId":null,"activePaneKey":null,"drafts":{"p":{"n":number,"text":text}},"readingAnchors":{}});
    let lexical_len = serde_json::to_vec(&input).unwrap().len();
    let persisted_len = lexical_len - number.to_string().len() + persisted.len();
    input["drafts"]["p"]["text"] = json!(format!(
        "{text}{}",
        "x".repeat(ws::MAX_CLIENT_BYTES - persisted_len)
    ));
    assert_eq!(
        ws::clean_client_state("device", &json!({}), &input, 0.0).unwrap(),
        input,
        "raw={raw}, persisted={persisted}"
    );
    let over = format!("{}x", input["drafts"]["p"]["text"].as_str().unwrap());
    input["drafts"]["p"]["text"] = json!(over);
    assert!(
        matches!(ws::clean_client_state("device", &json!({}), &input, 0.0), Err(ws::WorkspaceError::Invalid(message)) if message == "Estado de cliente demasiado grande"),
        "raw={raw}, persisted={persisted}"
    );
}

#[test]
fn client_retained_overflow_decimals_use_python_byte_limits() {
    for (raw, persisted) in [("1e9999", "Infinity"), ("-1e9999", "-Infinity")] {
        assert_python_client_byte_boundary(raw, persisted, "é😊");
        let number: Value = serde_json::from_str(raw).unwrap();
        let previous = json!({"drafts":{"p":{"n":number}},"readingAnchors":{"p":{"n":number}}});
        let retained = ws::clean_client_state("device", &previous, &json!({}), 0.0).unwrap();
        assert_eq!(retained["drafts"], previous["drafts"]);
        assert_eq!(retained["readingAnchors"], previous["readingAnchors"]);
    }
}

#[test]
fn client_retained_deep_values_do_not_inherit_metadata_depth_limit() {
    for depth in [125, 129, 256] {
        // Construct Values directly: parser recursion limits do not define
        // the workspace domain API's acceptance policy.
        let mut value = Value::Null;
        for level in 0..depth {
            value = if level % 2 == 0 {
                Value::Array(vec![value])
            } else {
                Value::Object([(String::from("n"), value)].into_iter().collect())
            };
        }
        let state = json!({"drafts":{"p":{"n":value}},"readingAnchors":{"p":{"n":value}}});
        let supplied = ws::clean_client_state("device", &json!({}), &state, 0.0).unwrap();
        assert_eq!(supplied["drafts"], state["drafts"]);
        assert_eq!(supplied["readingAnchors"], state["readingAnchors"]);
        let retained = ws::clean_client_state("device", &state, &json!({}), 0.0).unwrap();
        assert_eq!(retained, supplied);
    }
}

#[test]
fn client_active_identifiers_reject_deep_direct_values_with_identifier_errors() {
    for key in ["activeTabId", "activePaneKey"] {
        for from_previous in [false, true] {
            let mut nested = Value::Null;
            for _ in 0..256 {
                nested = Value::Array(vec![nested]);
            }
            let mut input = json!({});
            input[key] = nested;
            let empty = json!({});
            let (previous, state) = if from_previous {
                (&input, &empty)
            } else {
                (&empty, &input)
            };
            assert!(matches!(
                ws::clean_client_state("device", previous, state, 0.0),
                Err(ws::WorkspaceError::Invalid(message)) if message == format!("{key} inválido")
            ));
        }
    }
}

#[test]
fn client_active_identifiers_select_supplied_values_before_previous_values() {
    for key in ["activeTabId", "activePaneKey"] {
        let mut previous = json!({"activeTabId":"tab","activePaneKey":"pane"});
        let mut nested = Value::Null;
        for _ in 0..256 {
            nested = Value::Array(vec![nested]);
        }
        previous[key] = nested;
        for replacement in [json!("selected"), Value::Null] {
            let mut state = json!({});
            state[key] = replacement.clone();
            let out = ws::clean_client_state("device", &previous, &state, 0.0).unwrap();
            assert_eq!(out[key], replacement);
            let omitted = if key == "activeTabId" {
                "activePaneKey"
            } else {
                "activeTabId"
            };
            assert_eq!(out[omitted], previous[omitted]);
        }
    }
}

#[test]
fn workspace_codec_roundtrips_nonfinite_atoms_without_changing_strings_or_keys() {
    use comandos_core::json::{workspace_dumps, workspace_loads};
    let raw = r#"{"\u0024serde_json::private::Number":"123","$serde_json::private::RawValue":"[1]","positive":Infinity,"negative":-Infinity,"nan":NaN,"overflow":1e9999,"duplicate":0,"duplicate":1,"float":1e3,"large":340282366920938463463374607431768211457,"text":"é😊 Infinity NaN -Infinity"}"#;
    let value = workspace_loads(raw).unwrap();
    assert_eq!(value["$serde_json::private::Number"], "123");
    assert_eq!(value["$serde_json::private::RawValue"], "[1]");
    assert_eq!(value["duplicate"], 1);
    let expected = r#"{"$serde_json::private::Number":"123","$serde_json::private::RawValue":"[1]","duplicate":1,"float":1000.0,"large":340282366920938463463374607431768211457,"nan":NaN,"negative":-Infinity,"overflow":Infinity,"positive":Infinity,"text":"é😊 Infinity NaN -Infinity"}"#;
    assert_eq!(workspace_dumps(&value).unwrap(), expected);
    assert_eq!(
        workspace_dumps(&workspace_loads(expected).unwrap()).unwrap(),
        expected
    );
}

#[test]
fn workspace_codec_roundtrips_deep_containers_beyond_metadata_limits() {
    use comandos_core::json::{workspace_dumps, workspace_loads};
    let raw = format!("{}null{}", "[{\"n\":".repeat(130), "}]".repeat(130));
    let value = workspace_loads(&raw).unwrap();
    assert_eq!(workspace_dumps(&value).unwrap(), raw);
}

#[test]
fn workspace_codec_rejects_malformed_and_trailing_input() {
    use comandos_core::json::workspace_loads;
    for raw in [
        "",
        "[",
        "{",
        "[1,]",
        "{\"n\":1,}",
        "{n:1}",
        "[1 2]",
        "{\"n\" 1}",
        "{\"n\":}",
        "null true",
        "Infinitya",
        "NaN0",
        "-Infinity0",
        "01",
        "+1",
        "1.",
        "1e",
        "\"bad\\q\"",
        "\"unterminated",
        "[Infinity]x",
    ] {
        assert!(workspace_loads(raw).is_err(), "accepted {raw:?}");
    }
}

#[test]
fn workspace_codec_rejects_read_depth_above_workspace_ceiling() {
    use comandos_core::json::workspace_loads;
    for (atom, depth) in [("null", 1001), ("[]", 1000), ("{}", 1000)] {
        let raw = format!("{}{atom}{}", "[".repeat(depth), "]".repeat(depth));
        assert!(
            workspace_loads(&raw).is_err(),
            "accepted 1001 container levels"
        );
    }
}

#[test]
fn workspace_codec_rejects_write_depth_above_workspace_ceiling() {
    use comandos_core::json::workspace_dumps;
    for (mut value, depth) in [
        (Value::Null, 1001),
        (Value::Array(vec![]), 1000),
        (json!({}), 1000),
    ] {
        for _ in 0..depth {
            value = Value::Array(vec![value]);
        }
        assert_eq!(
            workspace_dumps(&value).unwrap_err(),
            "Workspace JSON nesting limit reached"
        );
    }
}

#[test]
fn workspace_codec_accepts_container_depth_at_workspace_ceiling() {
    use comandos_core::json::{workspace_dumps, workspace_loads};
    assert_eq!(comandos_core::json::MAX_WORKSPACE_JSON_DEPTH, 1000);
    for depth in [999, 1000] {
        let raw = format!("{}null{}", "[".repeat(depth), "]".repeat(depth));
        let value = workspace_loads(&raw).unwrap();
        assert_eq!(workspace_dumps(&value).unwrap(), raw);
        assert!(workspace_loads(&format!("{raw} false")).is_err());
        assert!(workspace_loads(&raw[..raw.len() - 1]).is_err());
    }
    // Empty containers count too; scalar depth alone must not define the cap.
    let raw = format!("{}[]{}", "[".repeat(1000), "]".repeat(1000));
    assert!(workspace_loads(&raw).is_err());
    for atom in ["[]", "{}"] {
        let raw = format!("{}{atom}{}", "[".repeat(999), "]".repeat(999));
        assert_eq!(
            workspace_dumps(&workspace_loads(&raw).unwrap()).unwrap(),
            raw
        );
    }
}

#[test]
fn client_depth_rejects_selected_entries_before_removal_patches() {
    for (field, patch) in [
        ("drafts", "draftsPatch"),
        ("readingAnchors", "anchorsPatch"),
    ] {
        for from_previous in [true, false] {
            let mut value = Value::Null;
            for _ in 0..999 {
                value = Value::Array(vec![value]);
            }
            // Final root plus the selected field and 999 arrays = 1001.
            let selected = Value::Object([(String::from("p"), value)].into_iter().collect());
            let mut state = json!({patch:{"p":null}});
            let mut previous = json!({});
            if from_previous {
                previous[field] = selected;
            } else {
                state[field] = selected;
            }
            // A later removal must not permit cloning over-ceiling input first.
            assert!(
                matches!(ws::clean_client_state("device", &previous, &state, 0.0),
                Err(ws::WorkspaceError::Invalid(message)) if message == "Workspace JSON nesting limit reached")
            );
        }
    }
}

#[test]
fn client_depth_validation_ignores_unknown_fields_and_overridden_previous_values() {
    let mut value = Value::Null;
    for _ in 0..1001 {
        value = Value::Array(vec![value]);
    }
    let previous = Value::Object([(String::from("drafts"), value)].into_iter().collect());
    let mut state = json!({"drafts":{}});
    let mut ignored = Value::Null;
    for _ in 0..1001 {
        ignored = Value::Array(vec![ignored]);
    }
    state["unknown"] = ignored;
    assert_eq!(
        ws::clean_client_state("device", &previous, &state, 0.0).unwrap(),
        json!({"activeTabId":null,"activePaneKey":null,"drafts":{},"readingAnchors":{}})
    );
}

#[test]
fn client_byte_boundary_normalizes_decimal_numbers() {
    assert_python_client_byte_boundary("1.00", "1.0", "");
    assert_python_client_byte_boundary("100000.00", "100000.0", "");
}

#[test]
fn client_byte_boundary_normalizes_exponents_in_both_directions() {
    assert_python_client_byte_boundary("1e3", "1000.0", "");
    assert_python_client_byte_boundary("1e-5", "1e-05", "");
    assert_python_client_byte_boundary("1.000e+16", "1e+16", "");
}

#[test]
fn client_byte_boundary_counts_utf8_and_normalized_numbers() {
    assert_python_client_byte_boundary("1.00", "1.0", "é😊\n\t");
    assert_python_client_byte_boundary("1e-5", "1e-05", "é😊\n\t");
}
fn fixture() -> Value {
    serde_json::from_str(include_str!(
        "../../../tests/fixtures/workspace_layout.json"
    ))
    .unwrap()
}
fn doc() -> Value {
    fixture()[0]["doc"].clone()
}
fn order(d: &Value) -> Vec<String> {
    d["groups"]
        .as_array()
        .unwrap()
        .iter()
        .map(|g| g["id"].as_str().unwrap().into())
        .collect()
}
#[test]
fn layout_shared_fixture_preserves_arrangement_and_membership() {
    for case in fixture().as_array().unwrap() {
        let d = &case["doc"];
        let args = &case["args"];
        let result = match case["op"].as_str().unwrap() {
            "move" => layout::move_tab(
                d,
                args[0].as_str().unwrap(),
                args[1].as_str().unwrap(),
                args[2].as_str().unwrap(),
            ),
            "detach" => layout::detach_tab(d, args[0].as_str().unwrap(), &args[1]),
            "resize" => layout::resize_split(d, args[0].as_str().unwrap(), &args[1], &args[2]),
            _ => panic!("unknown fixture operation"),
        };
        if case["error"] == true {
            assert!(result.is_err(), "{}", case["name"]);
        } else {
            assert_eq!(result.unwrap(), case["expect"], "{}", case["name"]);
        }
    }
}
#[test]
fn validation_rejects_duplicates_depth_invalid_bindings_and_types() {
    assert!(ws::validate_document(&doc()).is_ok());
    assert_eq!(
        ws::empty_document(),
        json!({"schema":1,"groups":[],"tabs":{}})
    );
    assert!(ws::is_empty(&ws::empty_document()));
    let mut d = doc();
    d["groups"][1]["id"] = json!("ga");
    assert!(ws::validate_document(&d).is_err());
    d = doc();
    d["groups"][2]["tree"]["ratio"] = json!(true);
    assert!(ws::validate_document(&d).is_err());
    d = doc();
    d["tabs"]["b"]["paneKeys"] = json!(["pk-a"]);
    assert!(ws::validate_document(&d).is_err());
    d = doc();
    d["bindings"] = json!({"orphan":{}});
    assert!(ws::validate_document(&d).is_err());
    d = doc();
    d["tabs"]["a"]["session"] = json!("é".repeat(200));
    assert!(ws::validate_document(&d).is_ok());
    d["tabs"]["a"]["session"] = json!("é".repeat(201));
    assert!(ws::validate_document(&d).is_err());
    let mut tree = json!({"type":"tab","tabId":"zero"});
    for i in 0..65 {
        tree = json!({"type":"split","axis":"x","ratio":0.5,"first":tree,"second":{"type":"tab","tabId":i.to_string()}});
    }
    assert!(ws::tab_ids(&tree).is_err());
}
#[test]
fn reconcile_preserves_metadata_ratios_and_last_capture() {
    let mut d = doc();
    d["custom"] = json!({"keep":true});
    d["groups"][2]["custom"] = json!("group-meta");
    let live = vec![
        ("a".into(), None),
        ("b".into(), None),
        ("c".into(), None),
        ("d".into(), None),
    ];
    assert_eq!(ws::reconcile(&d, &live, &HashMap::new()).unwrap(), d);
    let capture=ws::pane_bindings(&json!({"windows":[{"panes":[{"key":"p1","id":"%1","pid":5,"start":50,"agent":"codex","resume_id":"c1"},{"key":"p2","acp":{"sessionId":"acp1"}},{"key":""}]}]})).unwrap();
    let out = ws::reconcile(&d, &live, &HashMap::from([("a".into(), capture)])).unwrap();
    assert_eq!(
        out["bindings"]["p1"],
        json!({"paneId":"%1","pid":5,"startTime":50,"session":"a","conversation":{"agent":"codex","id":"c1"}})
    );
    assert_eq!(
        out["bindings"]["p2"]["conversation"],
        json!({"agent":"acp","id":"acp1"})
    );
    assert_eq!(
        ws::reconcile(&out, &live, &HashMap::from([("a".into(), vec![])])).unwrap(),
        out
    );
    let out = ws::reconcile(
        &out,
        &[
            ("c".into(), Some("C".into())),
            ("e".into(), Some("E".into())),
        ],
        &HashMap::new(),
    )
    .unwrap();
    assert_eq!(order(&out), vec!["gw", "group-e"]);
    assert_eq!(out["groups"][0]["custom"], "group-meta");
    assert_eq!(out["bindings"], json!({}));
    assert_eq!(out["custom"], d["custom"]);
}
#[test]
fn restore_requires_exact_process_or_exact_conversation() {
    let d = json!({"schema":1,"groups":[{"id":"g","tree":{"type":"tab","tabId":"a"}}],"tabs":{"a":{"session":"a","paneKeys":["live","res","missing","fail","absent"]}},"bindings":{"live":{"pid":100,"startTime":5},"res":{"pid":200,"startTime":8,"conversation":{"id":"exact"}},"missing":{"pid":300,"startTime":9},"fail":{"conversation":{"id":"broken"}}}});
    let mut called = vec![];
    let out = ws::restore_workspace(
        &d,
        |b| {
            Ok(if b["pid"] == 100 {
                Some(json!({"pid":100,"startTime":5}))
            } else {
                Some(json!({"pid":100,"startTime":6}))
            })
        },
        |b| {
            called.push(b["conversation"]["id"].clone());
            if b["conversation"]["id"] == "exact" {
                Ok(Some(json!({"paneId":"%new"})))
            } else {
                Err(ws::WorkspaceError::Callback("failure".into()))
            }
        },
    )
    .unwrap();
    assert_eq!(called, vec![json!("exact"), json!("broken")]);
    assert_eq!(
        out,
        json!({"attached":["live"],"resumed":{"res":{"paneId":"%new"}},"unavailable":["missing","fail","absent"]})
    );
}
#[test]
fn device_patch_preserves_focus_drafts_and_cleans_entries_with_explicit_clock() {
    let previous = json!({"activeTabId":"old","drafts":{"keep":{"text":"old"},"gone":{"text":"remove"}},"readingAnchors":{"keep":{"line":40}}});
    let patch = json!({"activeTabId":null,"draftsPatch":{"gone":null,"new":{"text":"é😊","selStart":2,"selEnd":3,"updatedAt":true}},"anchorsPatch":{"new":{"text":"anchor","ratio":0.7,"updatedAt":12}}});
    let out = ws::clean_client_state("phone", &previous, &patch, 42.0).unwrap();
    assert!(out["activeTabId"].is_null());
    assert_eq!(out["drafts"]["keep"], previous["drafts"]["keep"]);
    assert!(out["drafts"].get("gone").is_none());
    assert_eq!(
        out["drafts"]["new"],
        json!({"text":"é😊","updatedAt":42.0,"selStart":2})
    );
    assert_eq!(out["readingAnchors"]["keep"], json!({"line":40}));
    assert_eq!(out["readingAnchors"]["new"]["updatedAt"], 12);
    assert!(ws::clean_client_state("", &previous, &patch, 42.0).is_err());
    assert!(
        ws::clean_client_state(
            "phone",
            &previous,
            &json!({"draftsPatch":{"bad":{"text":"x".repeat(20001)}}}),
            42.0
        )
        .is_err()
    );
    assert!(
        ws::clean_client_state(
            "phone",
            &previous,
            &json!({"anchorsPatch":{"bad":{"text":"x".repeat(301)}}}),
            42.0
        )
        .is_err()
    );
    assert!(ws::clean_client_state("phone", &previous, &json!({"drafts":false}), 42.0).is_ok());
    let patch: serde_json::Map<String, Value> = (0..65)
        .map(|i| (format!("k{i:02}"), json!({"text":"ok","updatedAt":i})))
        .collect();
    let bounded =
        ws::clean_client_state("phone", &json!({}), &json!({"draftsPatch":patch}), 42.0).unwrap();
    assert_eq!(bounded["drafts"].as_object().unwrap().len(), 64);
    assert!(bounded["drafts"].get("k00").is_none());
}
fn close_doc() -> Value {
    json!({"schema":1,"groups":[{"id":"g","tree":{"type":"split","axis":"x","ratio":0.5,"first":{"type":"tab","tabId":"a"},"second":{"type":"split","axis":"y","ratio":0.5,"first":{"type":"tab","tabId":"b"},"second":{"type":"tab","tabId":"local"}}}}],"tabs":{"a":{"session":"alpha","label":"Alpha"},"b":{"session":"beta"},"local":{"session":"local"}}})
}
fn identity(s: &str) -> Value {
    json!(format!("id:{s}"))
}
struct State {
    value: Option<Value>,
    meta: HashMap<String, Value>,
}
impl ws::CloseGroupState for State {
    fn current(&mut self) -> ws::Result<Option<Value>> {
        Ok(self.value.clone())
    }
    fn meta(&mut self, k: &str) -> ws::Result<Option<Value>> {
        Ok(self.meta.get(k).cloned())
    }
    fn set_meta(&mut self, k: &str, v: &Value) -> ws::Result<()> {
        self.meta.insert(k.into(), v.clone());
        Ok(())
    }
}
fn state() -> State {
    State {
        value: Some(json!({"revision":1,"document":close_doc()})),
        meta: HashMap::new(),
    }
}
#[test]
fn group_close_revalidates_every_identity_before_effects_and_reports_partial_failure() {
    let p = ws::close_group_preview(&close_doc(), "g", identity).unwrap();
    assert_eq!(p["members"][2]["kept"], true);
    let mut s = state();
    let mut calls = vec![];
    let out = ws::close_group(
        &mut s,
        "g",
        &json!(1),
        &p["members"],
        "request",
        identity,
        |x| {
            calls.push(x.to_string());
            if x == "beta" {
                Err("tmux falló".into())
            } else {
                Ok(None)
            }
        },
    )
    .unwrap();
    assert_eq!(calls, vec!["alpha", "beta"]);
    assert_eq!(
        out,
        json!({"ok":false,"closed":["alpha"],"remaining":["beta"],"kept":["local"],"error":"tmux falló"})
    );
    let again = ws::close_group(
        &mut s,
        "g",
        &json!(99),
        &json!([]),
        "request",
        |_| panic!("idempotency must skip inspect"),
        |_| panic!("must skip close"),
    )
    .unwrap();
    assert_eq!(again, out);
    let mut s = state();
    let e = ws::close_group(
        &mut s,
        "g",
        &json!(1),
        &p["members"],
        "new",
        |x| {
            if x == "beta" {
                json!("changed")
            } else {
                identity(x)
            }
        },
        |_| panic!("must not close"),
    )
    .unwrap_err();
    assert!(matches!(e, ws::WorkspaceError::Invalid(_)));
    let e = ws::close_group(
        &mut s,
        "g",
        &json!(2),
        &p["members"],
        "stale",
        identity,
        |_| panic!("must not close"),
    )
    .unwrap_err();
    assert!(matches!(e, ws::WorkspaceError::Conflict(_)));
    let e = ws::close_group(
        &mut s,
        "g",
        &json!(1),
        &json!([p["members"][0].clone()]),
        "set",
        identity,
        |_| panic!("must not close"),
    )
    .unwrap_err();
    assert!(matches!(e, ws::WorkspaceError::Invalid(_)));
    let mut s = state();
    assert_eq!(
        ws::close_group(
            &mut s,
            "g",
            &json!(1),
            &p["members"],
            "success",
            identity,
            |_| Ok(None)
        )
        .unwrap(),
        json!({"ok":true,"closed":["alpha","beta"],"remaining":[],"kept":["local"],"error":null})
    );
}
#[test]
fn sorting_group_movement_and_order_restore_are_stable() {
    let d = json!({"schema":1,"tabs":{},"groups":[{"id":"g-local","tree":{"type":"tab","tabId":"local"}},{"id":"g-b","tree":{"type":"tab","tabId":"b"}},{"id":"g-a","tree":{"type":"tab","tabId":"a"}},{"id":"g-c","tree":{"type":"tab","tabId":"c"}}]});
    let info = json!({"a":{"label":"Alpha","fav":false,"activeAt":100,"need":false},"b":{"label":"Beta","fav":true,"activeAt":50,"need":false},"c":{"label":"Gamma","fav":false,"activeAt":300,"need":true}});
    for (by, want) in [
        ("fav", vec!["g-local", "g-b", "g-a", "g-c"]),
        ("recent", vec!["g-local", "g-c", "g-a", "g-b"]),
        ("need", vec!["g-local", "g-c", "g-b", "g-a"]),
        ("alpha", vec!["g-local", "g-a", "g-b", "g-c"]),
    ] {
        assert_eq!(order(&layout::sort_groups(&d, by, &info).unwrap()), want);
    }
    assert_eq!(
        layout::restore_order(
            &layout::sort_groups(&d, "alpha", &info).unwrap(),
            &order(&d)
        )
        .unwrap(),
        d
    );
    assert_eq!(
        order(&layout::move_tab_group(&d, "c", &json!(1)).unwrap().unwrap()),
        vec!["g-local", "g-c", "g-b", "g-a"]
    );
    assert_eq!(
        order(
            &layout::move_tab_group(&d, "a", &json!(-1))
                .unwrap()
                .unwrap()
        )[0],
        "g-a"
    );
    assert!(
        layout::move_tab_group(&d, "absent", &json!(0))
            .unwrap()
            .is_none()
    );
    assert!(layout::sort_groups(&d, "bad", &info).is_err());
}
#[test]
fn layout_preserves_unknown_nested_metadata_and_rejects_invalid_arguments() {
    let mut d = doc();
    d["custom"] = json!({"opaque":[1,2]});
    d["groups"][2]["tree"]["custom"] = json!("split-data");
    d["tabs"]["a"]["custom"] = json!({"title":"keep"});
    d["groups"][0]["custom"] = json!("group-data");
    let out = layout::move_tab(&d, "b", "a", "left").unwrap();
    assert_eq!(out["custom"], d["custom"]);
    assert_eq!(out["tabs"], d["tabs"]);
    assert_eq!(out["groups"][0]["custom"], "group-data");
    assert_eq!(out["groups"][1]["tree"]["custom"], "split-data");
    assert!(layout::detach_tab(&d, "a", &json!(true)).is_err());
    assert!(layout::detach_tab(&d, "a", &json!(-1)).is_err());
    assert!(layout::detach_tab(&d, "b", &json!(2)).is_err());
    // The legacy source anchor disappears when removing its own group.
    assert_eq!(
        order(&layout::detach_tab(&d, "a", &json!(0)).unwrap()),
        vec!["gb", "gw", "ga"]
    );
    assert!(layout::resize_split(&d, "gw", &json!(["third"]), &json!(0.5)).is_err());
    assert!(layout::resize_split(&d, "gw", &json!([]), &json!(false)).is_err());
    let mut conflict = doc();
    conflict["groups"][0]["id"] = json!("group-d");
    assert_eq!(
        layout::detach_tab(&conflict, "d", &json!(0)).unwrap()["groups"][0]["id"],
        "group-d-2"
    );
}
#[test]
fn validation_reports_json_errors_without_panics() {
    for bad in [
        Value::Null,
        json!([]),
        json!({"schema":2,"groups":[],"tabs":{}}),
        json!({"schema":1,"groups":null,"tabs":{}}),
    ] {
        assert!(ws::validate_document(&bad).is_err());
    }
    let mut d = doc();
    d["tabs"]["a"]["label"] = json!(1);
    assert!(ws::validate_document(&d).is_err());
    d = doc();
    d["tabs"]["a"]["paneKeys"] = json!(null);
    assert!(ws::validate_document(&d).is_err());
    d = doc();
    d["groups"][1]["tree"]["tabId"] = json!("a");
    assert!(ws::validate_document(&d).is_err());
    d = doc();
    d["tabs"]["extra"] = json!({"session":"extra"});
    assert!(ws::validate_document(&d).is_err());
    assert_eq!(ws::tab_ids(&json!({"type":"split","axis":"y","ratio":0.1,"first":{"type":"tab","tabId":"a"},"second":{"type":"tab","tabId":"b"}})).unwrap(),vec!["a","b"]);
}
#[test]
fn client_state_bounds_bytes_anchors_and_shape_and_does_not_touch_inputs() {
    let before = json!({"activeTabId":"a","drafts":{"pane":{"text":"saved"}}});
    let patch = json!({"anchorsPatch":{"new":{"text":"anchor","ratio":true}}});
    let result = ws::clean_client_state("device", &before, &patch, 10.0).unwrap();
    assert_eq!(result["activeTabId"], "a");
    assert!(result["readingAnchors"]["new"].get("ratio").is_none());
    assert_eq!(before["drafts"]["pane"]["text"], "saved");
    assert!(before.get("readingAnchors").is_none());
    for bad in [
        json!([]),
        json!({"draftsPatch":[]}),
        json!({"draftsPatch":{"pane":false}}),
        json!({"anchorsPatch":{"":{}}}),
        json!({"activePaneKey":false}),
    ] {
        assert!(ws::clean_client_state("device", &before, &bad, 10.0).is_err());
    }
    assert!(
        ws::clean_client_state(
            "device",
            &before,
            &json!({"drafts":{"large":"x".repeat(262144)}}),
            10.0
        )
        .is_err()
    );
    let anchors: serde_json::Map<String, Value> = (0..129)
        .map(|i| (format!("k{i:03}"), json!({"text":"anchor","updatedAt":i})))
        .collect();
    let out = ws::clean_client_state("device", &json!({}), &json!({"anchorsPatch":anchors}), 10.0)
        .unwrap();
    assert_eq!(out["readingAnchors"].as_object().unwrap().len(), 128);
    assert!(out["readingAnchors"].get("k000").is_none());
}
#[test]
fn group_close_rejects_duplicate_confirmations_and_missing_live_identity() {
    let p = ws::close_group_preview(&close_doc(), "g", identity).unwrap();
    assert!(ws::close_group_preview(&close_doc(), "missing", identity).is_err());
    let mut s = state();
    let duplicate = json!([
        p["members"][0].clone(),
        p["members"][0].clone(),
        p["members"][2].clone()
    ]);
    assert!(
        ws::close_group(
            &mut s,
            "g",
            &json!(1),
            &duplicate,
            "duplicate",
            identity,
            |_| panic!("must not close")
        )
        .is_err()
    );
    assert!(
        ws::close_group(
            &mut s,
            "g",
            &json!(1),
            &p["members"],
            "missing",
            |_| Value::Null,
            |_| panic!("must not close")
        )
        .is_err()
    );
    let out = ws::close_group(
        &mut s,
        "g",
        &json!(1),
        &p["members"],
        "error",
        identity,
        |_| Err(String::new()),
    )
    .unwrap();
    assert_eq!(out["error"], "Error al cerrar");
    assert_eq!(out["remaining"], json!(["alpha", "beta"]));
    assert_eq!(out["kept"], json!(["local"]));
}
#[test]
fn whole_split_group_move_and_sort_aggregate_all_members() {
    let d = json!({"schema":1,"groups":[{"id":"g-multi","tree":{"type":"split","axis":"x","ratio":0.4,"first":{"type":"tab","tabId":"a"},"second":{"type":"tab","tabId":"b"}}},{"id":"g-c","tree":{"type":"tab","tabId":"c"}},{"id":"g-local","tree":{"type":"tab","tabId":"local"}}],"tabs":{}});
    assert_eq!(
        order(
            &layout::move_tab_group(&d, "b", &json!(99))
                .unwrap()
                .unwrap()
        ),
        vec!["g-c", "g-local", "g-multi"]
    );
    let info = json!({"a":{"label":"Zulu","activeAt":2},"b":{"label":"Alpha","activeAt":100,"fav":true,"need":true},"c":{"label":"Beta","activeAt":50}});
    for by in ["fav", "recent", "need", "alpha"] {
        assert_eq!(
            order(&layout::sort_groups(&d, by, &info).unwrap()),
            vec!["g-local", "g-multi", "g-c"]
        );
    }
    assert_eq!(
        order(&layout::restore_order(&d, &["g-c".into()]).unwrap()),
        vec!["g-c", "g-multi", "g-local"]
    );
}
#[test]
fn workspace_timestamps_compare_without_rounding_integer_precision() {
    let d = json!({"groups":[{"id":"older","tree":{"type":"tab","tabId":"a"}},{"id":"newer","tree":{"type":"tab","tabId":"b"}}]});
    let info = json!({"a":{"activeAt":9007199254740992_u64},"b":{"activeAt":9007199254740993_u64}});
    assert_eq!(
        order(&layout::sort_groups(&d, "recent", &info).unwrap()),
        vec!["newer", "older"]
    );
    let mixed =
        json!({"a":{"activeAt":9007199254740992.0_f64},"b":{"activeAt":9007199254740993_u64}});
    assert_eq!(
        order(&layout::sort_groups(&d, "recent", &mixed).unwrap()),
        vec!["newer", "older"]
    );
    let mut entries = serde_json::Map::new();
    entries.insert(
        "newer".into(),
        json!({"text":"newer","updatedAt":9007199254740993_u64}),
    );
    entries.insert(
        "older".into(),
        json!({"text":"older","updatedAt":9007199254740992_u64}),
    );
    for i in 0..63 {
        entries.insert(
            format!("other-{i}"),
            json!({"text":"other","updatedAt":9007199254740994_u64}),
        );
    }
    let out =
        ws::clean_client_state("device", &json!({}), &json!({"draftsPatch":entries}), 0.0).unwrap();
    assert!(out["drafts"].get("older").is_none());
    assert!(out["drafts"].get("newer").is_some());
}
