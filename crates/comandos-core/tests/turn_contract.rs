use comandos_core::turn::{reduce_turn, turns_from_events};
use serde_json::{Value, json};

#[test]
fn mixed_numeric_timestamps_keep_integer_precision_in_both_directions() {
    let current = json!({"state":"working","turnId":"local:1","startedAtMs":9007199254740993u64});
    assert!(
        reduce_turn(
            current.as_object(),
            &event(
                "turn_completed",
                Value::Null,
                "p",
                json!(9007199254740992.0)
            )
        )
        .is_none()
    );
    let current = json!({"state":"working","turnId":"local:1","startedAtMs":9007199254740994.0});
    assert!(
        reduce_turn(
            current.as_object(),
            &event(
                "turn_completed",
                Value::Null,
                "p",
                json!(9007199254740993u64)
            )
        )
        .is_none()
    );
    let current = json!({"state":"working","turnId":"local:1","startedAtMs":1.5});
    assert!(
        reduce_turn(
            current.as_object(),
            &event("turn_completed", Value::Null, "p", json!(1))
        )
        .is_none()
    );
    assert!(
        reduce_turn(
            current.as_object(),
            &event("turn_completed", Value::Null, "p", json!(2))
        )
        .is_some()
    );
}

fn event(kind: &str, turn: Value, process: &str, at: Value) -> Value {
    json!({"kind":kind,"turnId":turn,"processKey":process,"occurredAtMs":at,
        "eventId":"event-1","evidence":"confirmed","correlation":"source"})
}

#[test]
fn working_permission_completion_preserves_identity() {
    let first = event("prompt_accepted", json!("t"), "p", json!(10));
    let working = reduce_turn(None, &first).expect("a prompt starts work");
    assert_eq!(working["state"], "working");
    let permission = reduce_turn(
        Some(&working),
        &event("permission_requested", json!("t"), "p", json!(20)),
    )
    .unwrap();
    assert_eq!(permission["state"], "awaiting_permission");
    let done = reduce_turn(
        Some(&permission),
        &event("turn_completed", json!("t"), "p", json!(30)),
    )
    .unwrap();
    assert_eq!(done["state"], "completed");
    assert_eq!(done["finishedAtMs"], 30);
    assert_eq!(done["processKey"], "p");
}

#[test]
fn late_foreign_duplicate_and_unconfirmed_events_do_not_mutate() {
    let current = json!({"state":"working","turnId":"new","startedAtMs":12,"processKey":"p","extension":{"a":1}});
    let current = current.as_object().unwrap();
    for event in [
        event("turn_completed", json!("old"), "p", json!(20)),
        event("prompt_accepted", json!("old"), "p", json!(10)),
        event("prompt_accepted", json!("new"), "p", json!(20)),
        event("pane_closed", Value::Null, "foreign", json!(20)),
        json!({"kind":"turn_completed","turnId":"new","evidence":"historical"}),
        json!({"kind":"news_edition","evidence":"confirmed"}),
    ] {
        assert!(reduce_turn(Some(current), &event).is_none(), "{event}");
    }
    let next = reduce_turn(
        Some(current),
        &event("input_requested", json!("new"), "p", json!(20)),
    )
    .unwrap();
    assert_eq!(next["extension"], json!({"a":1}));
}

#[test]
fn replacement_process_clears_previous_process_metadata() {
    let old = json!({"state":"completed","turnId":"old","startedAtMs":12,"processKey":"old-pid","finishedAtMs":13,"requestId":"old-request","custom":"old"});
    let next = reduce_turn(
        old.as_object(),
        &event("turn_started", json!("next"), "new-pid", json!(14)),
    )
    .unwrap();
    assert_eq!(next["turnId"], "next");
    assert_eq!(next["requestId"], Value::Null);
    assert!(!next.contains_key("finishedAtMs"));
    assert!(!next.contains_key("custom"));
}

#[test]
fn local_turns_use_occurrence_time_and_ignore_boolean_timestamps() {
    let current = reduce_turn(
        None,
        &event("prompt_accepted", Value::Null, "p", json!(100)),
    )
    .unwrap();
    assert_eq!(current["turnId"], "local:event-1");
    assert!(
        reduce_turn(
            Some(&current),
            &event("turn_completed", Value::Null, "p", json!(99))
        )
        .is_none()
    );
    let done = reduce_turn(
        Some(&current),
        &event("turn_completed", Value::Null, "p", json!(true)),
    )
    .unwrap();
    assert_eq!(done["state"], "completed");
    assert_eq!(done["finishedAtMs"], Value::Null);
    assert!(
        reduce_turn(
            Some(&done),
            &event("input_requested", Value::Null, "p", json!(200))
        )
        .is_none()
    );
}

#[test]
fn folds_exact_panes_and_keeps_metadata_on_noop() {
    let events = vec![
        json!({"kind":"prompt_accepted","evidence":"confirmed","paneKey":"a","turnId":"ta","projectKey":"proj"}),
        json!({"kind":"prompt_accepted","evidence":"confirmed","sessionKey":"s","paneId":"%2","turnId":"tb"}),
        json!({"kind":"turn_failed","evidence":"historical","paneKey":"a","projectKey":"wrong"}),
        json!({"kind":"turn_completed","evidence":"confirmed","projectKey":"proj"}),
    ];
    let turns = turns_from_events(&events);
    assert_eq!(turns.len(), 2);
    assert_eq!(turns["pane:a"]["projectKey"], "proj");
    assert_eq!(turns["tmux:s:%2"]["state"], "working");
}
