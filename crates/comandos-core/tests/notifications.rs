use comandos_core::notifications as nd;
use serde_json::{Value, json};
use std::collections::BTreeSet;

#[test]
fn python_oracle_routes_preferences_grouping_pending_and_notices() {
    let f: Value = serde_json::from_str(include_str!("notification_fixture.json")).unwrap();
    for case in f["routes"].as_array().unwrap() {
        assert_eq!(
            nd::route_event(
                &case["event"],
                case["clients"].as_array().unwrap(),
                &case["prefs"],
                case["now"].as_i64().unwrap(),
                case["focus"].as_bool().unwrap()
            ),
            case["expected"],
            "{case}"
        );
    }
    for case in f["merge"].as_array().unwrap() {
        let result = match nd::merge_prefs(&Value::Null, &case["update"]) {
            Ok(v) => json!({"ok":v}),
            Err(e) => json!({"error":e}),
        };
        assert_eq!(result, case["expected"], "{case}");
    }
    let events = f["events"].as_array().unwrap();
    assert_eq!(json!(nd::group_keys(events, &Value::Null)), f["groups"]);
    assert_eq!(json!(nd::pending_requests(events)), f["pending"]);
    for (event, expected) in events.iter().zip(f["notices"].as_array().unwrap()) {
        assert_eq!(
            nd::notice(
                event,
                &[],
                &Value::Null,
                1_800_000_000_000,
                &BTreeSet::from(["p2".into()]),
                "group",
                true
            ),
            *expected
        );
    }
    assert_eq!(
        nd::live_pending(
            events,
            Some(&|session, pane| session == "s" && pane == "%2")
        ),
        vec!["p2"]
    );
    assert!(!nd::push_policy(
        &json!({"eventId":"read","kind":"turn_failed"}),
        &[],
        &Value::Null,
        1_800_000_000_000,
        false,
        &BTreeSet::from(["read".into()])
    ));
    assert!(nd::push_policy(
        &json!({"eventId":"new","kind":"turn_failed"}),
        &[],
        &Value::Null,
        1_800_000_000_000,
        false,
        &BTreeSet::new()
    ));
}
