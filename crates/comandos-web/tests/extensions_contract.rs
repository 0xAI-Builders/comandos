#[test]
fn shelf_registered_in_both_phases() {
    let component = comandos_web::registry::COMPONENTS
        .iter()
        .find(|c| c.id == "extensions");
    assert!(
        component.is_some(),
        "extensions must replace its complete script"
    );
    assert!(component.unwrap().attach.is_some());
}
use comandos_web::components::extensions::*;
use serde_json::json;
fn state() -> serde_json::Value {
    json!({"identity":"p","conversationId":"c","revision":4,"harness":"codex","inventory":{"mcps":[{"id":"m","name":"Mail","toggleable":true,"size":{"tokens":2250},"origin":{"id":"o","label":"Repo"}}],"skills":[{"id":"a","name":"Alpha","toggleable":true},{"id":"fixed","name":"Fixed","toggleable":false},{"id":"ghost","name":"Ghost","toggleable":true}]},"desired":{"mcps":{"m":false},"skills":{"a":false,"fixed":true}},"templates":[],"busy":false,"loaded":null})
}
#[test]
fn batches_keep_unknown_and_managed_rows_and_ignore_filter_globally() {
    let mut s = Shelf::new(json!({"session":"s","pane":"%1","harness":"codex"}));
    s.state = Some(state());
    s.filter = "skills".into();
    s.query = "alp".into();
    let d = s.batch(true, None, false).unwrap();
    assert_eq!(
        d,
        json!({"mcps":{"m":true},"skills":{"a":true,"fixed":true}})
    );
    let d = s.batch(true, None, true).unwrap();
    assert_eq!(
        d,
        json!({"mcps":{"m":false},"skills":{"a":true,"fixed":true}})
    );
    assert_eq!(s.guards().get("revision"), Some(&json!(4)));
}
#[test]
fn stale_and_late_responses_cannot_replace_scope() {
    let mut s = Shelf::new(json!({"session":"s","pane":"%1"}));
    assert!(s.receive(0, state()));
    s.generation = 1;
    assert!(!s.receive(0, json!({"revision":0})));
    assert_eq!(s.state.as_ref().unwrap().get("revision"), Some(&json!(4)));
    let mut next = state();
    next["conversationId"] = json!("other");
    assert!(s.receive(1, next));
    assert!(s.stale);
    assert!(s.batch(true, None, false).is_none());
    assert_eq!(
        s.state.as_ref().unwrap().get("conversationId"),
        Some(&json!("c"))
    );
}
#[test]
fn unchanged_poll_keeps_dom_and_operations_allow_scope_transition() {
    let mut s = Shelf::new(json!({}));
    assert!(s.receive(0, state()));
    assert!(!s.receive(0, state()));
    s.state.as_mut().unwrap()["operation"] = json!({"state":"applying"});
    let mut next = state();
    next["conversationId"] = json!("new");
    assert!(s.receive(0, next));
    assert!(!s.stale);
}
#[test]
fn render_preserves_loading_accessibility_sizes_and_exclusions() {
    let mut s = Shelf::new(json!({"session":"<s>","pane":"%1"}));
    assert!(s.html().contains("loading-spinner"));
    s.error = "offline".into();
    let html = s.html();
    assert!(html.contains("Reintentar"));
    assert!(!html.contains("loading-spinner"));
    s.error.clear();
    s.receive(0, state());
    let html = s.html();
    assert!(html.contains("--bubble-size:132px"));
    assert!(html.contains("2 fuera del lote"));
    assert!(html.contains("Estado desconocido"));
    assert!(!html.contains("Interrumpir turno"));
    s.state.as_mut().unwrap()["busy"] = json!(true);
    assert!(s.html().contains("Interrumpir turno y aplicar"));
    assert!(s.html().contains("aria-pressed"));
    assert!(s.html().contains("&lt;s&gt;"));
}
#[test]
fn viewport_height_clamps_and_pane_scope_is_exact() {
    assert_eq!(shelf_height(650.0, &[900.0, 500.0, 900.0]), 380.0);
    assert_eq!(shelf_height(650.0, &[350.0, 500.0]), 230.0);
    assert_eq!(shelf_height(650.0, &[900.0]), 650.0);
    assert!(valid_target("s", "%123"));
    assert!(!valid_target("s", "%1junk"));
    assert!(!valid_target("", "%1"));
}
#[test]
fn saved_answer_reuses_scope_and_feedback_while_apply_defaults_to_validating() {
    let mut shelf = Shelf::new(json!({"session":"s","pane":"%1"}));
    shelf.receive(0, state());
    let mut saved = state();
    saved["revision"] = json!(5);
    saved["missing"] = json!({"skills":[{"id":"absent","name":"Absent"}]});
    assert!(shelf.accept_save("/template", &json!({"templateId":"t"}), saved));
    assert_eq!(shelf.guards()["revision"], 5);
    assert!(shelf.message.contains("Absent"));
    assert!(shelf.state.as_ref().unwrap().get("missing").is_none());
    assert!(!shelf.accept_save("/apply", &json!({}), json!({"operationId":"op"})));
    assert!(shelf.locked());
    assert!(shelf.html().contains("Cancelar espera"));
    assert_eq!(
        shelf.state.as_ref().unwrap()["operation"]["state"],
        "validating"
    );
}
#[test]
fn save_never_adopts_a_response_from_a_different_conversation() {
    let mut shelf = Shelf::new(json!({}));
    shelf.receive(0, state());
    let mut foreign = state();
    foreign["conversationId"] = json!("wrong");
    foreign["revision"] = json!(99);
    assert!(!shelf.accept_save("", &json!({}), foreign));
    assert_eq!(shelf.guards()["revision"], 4);
}

#[test]
fn managed_mcp_only_changes_do_not_offer_an_agent_restart() {
    let mut shelf = Shelf::new(json!({"session":"s","pane":"%1"}));
    let mut s = state();
    s["mcpGate"] = json!({"mode":"managed_calls","selection":{"m":false}});
    s["skillsRestartRequired"] = json!(false);
    s["busy"] = json!(true);
    s["loaded"] = json!({"mcps":{"m":true},"skills":{"a":false,"fixed":true}});
    shelf.receive(0, s);
    assert_eq!(shelf.diff(), Some((0, 0)));
    let html = shelf.html();
    assert!(html.contains("MCP guardados · sin reinicio"));
    assert!(html.contains("data-action=\"apply\" disabled"));
    assert!(!html.contains("Interrumpir turno y aplicar"));
    assert!(html.contains("próximas llamadas de MCP conectados al nuevo servicio global"));
    assert!(html.contains("Las conexiones anteriores necesitan reconectarse"));
    shelf.state.as_mut().unwrap()["skillsRestartRequired"] = json!(true);
    shelf.state.as_mut().unwrap()["desired"]["skills"]["a"] = json!(true);
    assert!(shelf.html().contains("Aplicar skills y reanudar"));
}

#[test]
fn gate_only_apply_response_does_not_create_a_fake_restart_operation() {
    let mut shelf = Shelf::new(json!({"session":"s","pane":"%1"}));
    shelf.receive(0, state());
    let mut applied = state();
    applied["mcpGate"] = json!({"mode":"managed_calls","selection":{"m":false}});
    applied["skillsRestartRequired"] = json!(false);
    applied["revision"] = json!(5);
    assert!(shelf.accept_save("/apply", &json!({}), applied));
    assert!(!shelf.locked());
    assert!(shelf.state.as_ref().unwrap()["operation"].is_null());
    assert_eq!(shelf.guards()["revision"], 5);
}
#[test]
fn recovery_and_operation_terminals_preserve_controls() {
    let mut shelf = Shelf::new(json!({}));
    let mut s = state();
    s["operation"] = json!({"state":"recovery_required","error":"Oops"});
    shelf.receive(0, s);
    let html = shelf.html();
    assert!(html.contains("Recuperar sesión anterior"));
    assert!(html.contains("Oops"));
    assert!(shelf.locked());
    shelf.state.as_mut().unwrap()["operation"] = json!({"state":"rolled_back"});
    assert!(!shelf.locked());
    shelf.sending = true;
    assert!(shelf.locked());
    shelf.sending = false;
    shelf.stale = true;
    assert!(shelf.toggle("mcps", "m", true).is_none());
}
#[test]
fn json_integer_floats_keep_javascript_number_is_integer_size_contract() {
    let mut shelf = Shelf::new(json!({}));
    let mut s = state();
    s["inventory"]["mcps"][0]["size"]["tokens"] = json!(2250.0);
    shelf.receive(0, s);
    assert!(shelf.html().contains("--bubble-size:132px"));
}
fn canonical_html(s: &str) -> String {
    let mut s = s.split_whitespace().collect::<Vec<_>>().join(" ");
    while s.contains("> <") {
        s = s.replace("> <", "><");
    }
    s
}
#[test]
fn original_recorded_shelf_html_preserves_all_visible_states() {
    let cases: serde_json::Value =
        serde_json::from_str(include_str!("extensions_render_golden.json")).unwrap();
    for case in cases.as_array().unwrap() {
        let mut s = Shelf::new(case["target"].clone());
        let opts = &case["opts"];
        if !case["state"].is_null() {
            s.state = Some(case["state"].clone());
        }
        s.filter = opts["filter"].as_str().unwrap_or("all").into();
        s.query = opts["query"].as_str().unwrap_or("").into();
        s.error = opts["error"].as_str().unwrap_or("").into();
        s.message = opts["message"].as_str().unwrap_or("").into();
        s.sending = opts["sending"].as_bool().unwrap_or(false);
        s.stale = opts["stale"].as_bool().unwrap_or(false);
        s.template_name = opts["template_name"].as_str().unwrap_or("").into();
        for key in opts["closed_groups"].as_array().into_iter().flatten() {
            s.closed_groups.insert(key.as_str().unwrap().into());
        }
        assert_eq!(
            canonical_html(&s.html()),
            canonical_html(case["html"].as_str().unwrap()),
            "{}",
            case["name"]
        );
    }
}
