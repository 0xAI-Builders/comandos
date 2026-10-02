use comandos_core::hook::{normalize_hook, resolve_pane_key};
use serde_json::{Value, json};

#[test]
fn hook_preserves_exact_process_and_provider_identity() {
    let event=normalize_hook(&json!({"hookEvent":"PermissionRequest","agent":"codex","session":"s","pane":"%2","panePid":"123","conversationId":"c","turnId":"t","requestId":"r","occurredAtMs":1234}),Some("99")).unwrap();
    assert_eq!(event["processKey"], "123-99");
    assert_eq!(event["kind"], "permission_requested");
    assert_eq!(event["correlation"], "source");
    assert_eq!(event["turnId"], "t");
    assert_eq!(event["paneKey"], Value::Null);
}

#[test]
fn idle_and_auth_notifications_are_not_new_requests() {
    for name in ["idle_prompt", "auth_success"] {
        assert!(
            normalize_hook(
                &json!({"hookEvent":"Notification","notificationType":name}),
                None
            )
            .is_none()
        );
    }
    assert_eq!(normalize_hook(&json!({"hookEvent":"Notification","notificationType":"permission_prompt","requestId":"r"}),None).unwrap()["kind"],"permission_requested");
}

#[test]
fn invalid_identifiers_are_not_used_to_target_sessions() {
    let event=normalize_hook(&json!({"hookEvent":"UserPromptSubmit","agent":"Invalid Agent","session":"a;tmux","pane":"%1x","panePid":"123bad","turnId":"bad\nturn","promptId":"fallback","occurredAtMs":true}),Some("99")).unwrap();
    assert_eq!(event["harness"], "unknown");
    assert_eq!(event["sessionKey"], Value::Null);
    assert_eq!(event["paneId"], Value::Null);
    assert_eq!(event["processKey"], Value::Null);
    assert_eq!(event["turnId"], "fallback");
    assert_eq!(event["occurredAtMs"], Value::Null);
}

#[test]
fn binding_resolution_checks_pid_start_time_and_ambiguity() {
    let mut doc =
        json!({"bindings":{"logical":{"session":"s","paneId":"%1","pid":123,"startTime":99}}});
    let mut event = json!({"sessionKey":"s","paneId":"%1","processKey":"123-99"});
    assert_eq!(resolve_pane_key(&doc, &event).as_deref(), Some("logical"));
    event["processKey"] = json!("123-100");
    assert_eq!(resolve_pane_key(&doc, &event), None);
    event["processKey"] = json!("456-99");
    assert_eq!(resolve_pane_key(&doc, &event), None);
    event["processKey"] = json!("123-99");
    doc["bindings"]["second"] = doc["bindings"]["logical"].clone();
    assert_eq!(resolve_pane_key(&doc, &event), None);
}
