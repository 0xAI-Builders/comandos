use comandos_core::event::{dedupe_key, destination, normalize};
use serde_json::{Value, json};

#[test]
fn defaults_preserve_missing_identity_and_use_injected_time_and_id() {
    let out = normalize(
        &json!({"kind":"turn_completed","source":"hook:claude"}),
        1234,
        "event-generated",
    )
    .unwrap();
    assert_eq!(out["eventId"], "event-generated");
    assert_eq!(out["receivedAtMs"], 1234);
    assert_eq!(out["occurredAtMs"], 1234);
    assert_eq!(out["turnId"], Value::Null);
    assert_eq!(out["evidence"], "unknown");
    assert_eq!(out["correlation"], "unknown");
    assert_eq!(out["title"], "");
}

#[test]
fn unicode_limits_count_characters_and_timestamps_truncate() {
    let out=normalize(&json!({"kind":"usage_alert","source":"hook:grok","title":"😺".repeat(201),
        "excerpt":"ñ".repeat(501),"projectKey":"🦀".repeat(200),"receivedAtMs":23.9,"occurredAtMs":0}),1,"generated").unwrap();
    assert_eq!(out["title"].as_str().unwrap().chars().count(), 200);
    assert_eq!(out["excerpt"].as_str().unwrap().chars().count(), 500);
    assert_eq!(out["receivedAtMs"], 23);
    assert_eq!(out["occurredAtMs"], 0);
}

#[test]
fn invalid_contract_fields_have_compatible_errors() {
    for (field, value, message) in [
        ("kind", json!("bad"), "kind desconocido"),
        ("source", json!(""), "source inválido"),
        ("turnId", json!("bad\nidentity"), "turnId inválido"),
        ("paneKey", json!(42), "paneKey inválido"),
        ("evidence", json!("wrong"), "evidence inválida"),
        ("correlation", json!("wrong"), "correlation inválida"),
        ("receivedAtMs", json!(true), "receivedAtMs inválido"),
        ("occurredAtMs", json!(-1), "occurredAtMs inválido"),
    ] {
        let mut event = json!({"kind":"turn_completed","source":"test"});
        event[field] = value;
        assert_eq!(
            normalize(&event, 0, "generated").unwrap_err(),
            message,
            "{event}"
        );
    }
}

#[test]
fn deduplication_requires_provider_identity_or_confirmed_cycle() {
    assert_eq!(
        dedupe_key(&json!({"sourceEventId":"x"})).as_deref(),
        Some("src:x")
    );
    let mut event = json!({"kind":"permission_requested","turnId":"t","requestId":"r","harness":"codex","conversationId":"c","evidence":"confirmed","correlation":"source"});
    assert_eq!(
        dedupe_key(&event).as_deref(),
        Some("cycle:codex|c|t|r|permission_requested")
    );
    event["correlation"] = json!("local");
    assert_eq!(dedupe_key(&event), None);
    event["correlation"] = json!("source");
    event["evidence"] = json!("inferred");
    assert_eq!(dedupe_key(&event), None);
}

#[test]
fn destination_never_invents_a_pane_from_project_or_conversation() {
    assert_eq!(
        destination(&json!({"paneKey":"p","sessionKey":"s"})),
        "pane"
    );
    assert_eq!(
        destination(&json!({"sessionKey":"s","paneId":"%1"})),
        "session"
    );
    assert_eq!(
        destination(&json!({"projectKey":"x","conversationId":"c"})),
        "none"
    );
}
