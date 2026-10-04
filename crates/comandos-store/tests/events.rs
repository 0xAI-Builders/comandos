use comandos_store::{
    Error,
    intake::{self, Reception},
    marks,
};
use comandos_store::{append_event, claim_delivery, get_event, latest_sequence, list_events};
use rusqlite::Connection;
use serde_json::{Value, json};

fn database() -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(include_str!("fixtures/events.sql"))
        .unwrap();
    conn.execute_batch(include_str!("fixtures/workspace.sql"))
        .unwrap();
    conn.execute_batch(include_str!("fixtures/marks.sql"))
        .unwrap();
    conn.execute(
        "INSERT INTO workspace_current VALUES(1,1,?,0)",
        [
            json!({"bindings":{"logical":{"session":"s","paneId":"%1","pid":123,"startTime":42}}})
                .to_string(),
        ],
    )
    .unwrap();
    conn.execute_batch("PRAGMA foreign_keys=ON").unwrap();
    conn
}

#[test]
fn matches_original_python_storage_fixture_including_receipts_and_transactions() {
    let conn = database();
    let cases: Vec<Value> =
        serde_json::from_str(include_str!("fixtures/storage-cases.json")).unwrap();
    for (index, case) in cases.iter().enumerate() {
        let result = match case["op"].as_str().unwrap() {
            "append" => append_event(
                &conn,
                &case["event"],
                123456,
                case["newEventId"].as_str().unwrap(),
                &format!("receipt-{index}"),
            )
            .unwrap(),
            "claim" => claim_delivery(
                &conn,
                case["eventId"].as_str().unwrap(),
                case["channel"].as_str().unwrap(),
                case["deviceId"].as_str().unwrap(),
                123456,
            )
            .unwrap()
            .into(),
            "get" => get_event(&conn, case["eventId"].as_str().unwrap())
                .unwrap()
                .unwrap_or(Value::Null),
            "list" => list_events(
                &conn,
                case["after"].as_i64().unwrap(),
                case["limit"].as_i64().unwrap(),
            )
            .unwrap()
            .into(),
            "set" => match marks::set_mark(
                &conn,
                case["scope"].as_str().unwrap(),
                case["key"].as_str().unwrap(),
                &case["value"],
                &case["revision"],
                123456,
            ) {
                Ok(mark) => mark,
                Err(Error::Conflict(current)) => {
                    json!({"error":"Revisión desactualizada","current":current})
                }
                Err(Error::Validation(message)) => json!({"error":message}),
                Err(other) => panic!("{other}"),
            },
            "apply" => marks::apply_turn_event(&conn, &case["event"], 123456)
                .unwrap()
                .into(),
            "record" => intake::record(
                &conn,
                &case["payload"],
                &Reception {
                    now_ms: 123456,
                    event_id: case["newEventId"].as_str().unwrap(),
                    receipt_id: &format!("receipt-{index}"),
                    process_start: Some(if case["payload"]["panePid"] == "123" {
                        "42"
                    } else {
                        "99"
                    }),
                },
            )
            .unwrap()
            .unwrap_or(Value::Null),
            "begin" => {
                conn.execute_batch("BEGIN").unwrap();
                Value::Null
            }
            "commit" => {
                conn.execute_batch("COMMIT").unwrap();
                Value::Null
            }
            "rollback" => {
                conn.execute_batch("ROLLBACK").unwrap();
                Value::Null
            }
            other => panic!("unknown fixture operation {other}"),
        };
        assert_eq!(result, case["expected"], "case {index}");
        assert_eq!(
            !conn.is_autocommit(),
            case["inTransaction"].as_bool().unwrap(),
            "case {index}"
        );
        assert_eq!(
            latest_sequence(&conn).unwrap(),
            case["sequence"],
            "case {index}"
        );
        let mut statement=conn.prepare("SELECT event_id, source, received_at_ms, duplicate FROM event_receipts ORDER BY rowid").unwrap();
        let receipts: Vec<Value> = statement
            .query_map([], |r| {
                Ok(json!([
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, i64>(2)?,
                    r.get::<_, i64>(3)?
                ]))
            })
            .unwrap()
            .map(|r| r.unwrap())
            .collect();
        assert_eq!(Value::Array(receipts), case["receipts"], "case {index}");
        assert_eq!(
            Value::Array(marks::list_marks(&conn).unwrap()),
            case["marks"],
            "case {index}"
        );
        let mut statement = conn
            .prepare("SELECT * FROM work_mark_applied ORDER BY event_id, scope, key")
            .unwrap();
        let applied: Vec<Value> = statement
            .query_map([], |r| {
                Ok(json!([
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?
                ]))
            })
            .unwrap()
            .map(|r| r.unwrap())
            .collect();
        assert_eq!(Value::Array(applied), case["applied"], "case {index}");
    }
}
fn event(id: &str) -> Value {
    json!({"eventId":id,"source":"hook:codex","kind":"turn_completed","evidence":"confirmed","correlation":"source","turnId":"t","conversationId":"c","harness":"codex","paneKey":"pane","occurredAtMs":1000,"receivedAtMs":1001})
}

#[test]
fn provider_duplicates_keep_the_original_event_and_record_every_reception() {
    let conn = database();
    let first = append_event(&conn, &event("one"), 0, "generated", "receipt-1").unwrap();
    let second = append_event(&conn, &event("two"), 0, "generated", "receipt-2").unwrap();
    assert_eq!(first["duplicate"], false);
    assert_eq!(second["duplicate"], true);
    assert_eq!(second["eventId"], "one");
    assert_eq!(second["sequence"], first["sequence"]);
    assert_eq!(second["destination"], "pane");
    assert_eq!(
        conn.query_row("SELECT COUNT(*) FROM event_receipts", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        2
    );
    assert_eq!(latest_sequence(&conn).unwrap(), 1);
}

#[test]
fn ambiguous_receptions_remain_distinct_and_paginate_in_sequence_order() {
    let conn = database();
    for i in 0..5 {
        let mut e = event(&format!("e-{i}"));
        e["correlation"] = json!("local");
        e["occurredAtMs"] = json!(500 - i);
        append_event(&conn, &e, 0, "generated", &format!("r-{i}")).unwrap();
    }
    assert_eq!(latest_sequence(&conn).unwrap(), 5);
    let page = list_events(&conn, 1, 2).unwrap();
    assert_eq!(page.len(), 2);
    assert_eq!(page[0]["eventId"], "e-1");
    assert_eq!(page[1]["eventId"], "e-2");
    assert_eq!(list_events(&conn, -100, 0).unwrap().len(), 1);
    assert!(get_event(&conn, "missing").unwrap().is_none());
}

#[test]
fn caller_rollback_removes_event_receipt_and_delivery_together() {
    let conn = database();
    conn.execute_batch("BEGIN IMMEDIATE").unwrap();
    append_event(&conn, &event("one"), 0, "generated", "r1").unwrap();
    assert!(!conn.is_autocommit());
    assert!(claim_delivery(&conn, "one", "sound", "device", 1234).unwrap());
    assert!(!conn.is_autocommit());
    conn.execute_batch("ROLLBACK").unwrap();
    assert_eq!(latest_sequence(&conn).unwrap(), 0);
    for table in ["event_receipts", "deliveries"] {
        assert_eq!(
            conn.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            0
        );
    }
}

#[test]
fn delivery_claim_is_once_per_event_channel_and_device() {
    let conn = database();
    append_event(&conn, &event("one"), 0, "generated", "r1").unwrap();
    assert!(claim_delivery(&conn, "one", "sound", "device", 1234).unwrap());
    assert!(!claim_delivery(&conn, "one", "sound", "device", 1235).unwrap());
    assert!(claim_delivery(&conn, "one", "sound", "second", 1235).unwrap());
    assert!(claim_delivery(&conn, "one", "push", "device", 1235).unwrap());
    assert_eq!(
        claim_delivery(&conn, "missing", "sound", "device", 0)
            .unwrap_err()
            .to_string(),
        "evento inexistente"
    );
    assert_eq!(
        claim_delivery(&conn, "one", "", "device", 0)
            .unwrap_err()
            .to_string(),
        "channel inválido"
    );
}

#[test]
fn append_errors_do_not_commit_or_rollback_callers_transaction() {
    let conn = database();
    conn.execute_batch("BEGIN IMMEDIATE").unwrap();
    append_event(&conn, &event("one"), 0, "generated", "r1").unwrap();
    let mut invalid = event("two");
    invalid["source"] = json!("");
    assert!(append_event(&conn, &invalid, 0, "generated", "r2").is_err());
    assert!(!conn.is_autocommit());
    assert_eq!(latest_sequence(&conn).unwrap(), 1);
    conn.execute_batch("ROLLBACK").unwrap();
}
