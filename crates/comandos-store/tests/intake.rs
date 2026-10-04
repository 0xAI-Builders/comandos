use comandos_store::{
    Error,
    intake::{Reception, record},
    latest_sequence,
    marks::{apply_turn_event, get_mark, set_mark},
};
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
    conn
}
fn reception() -> Reception<'static> {
    Reception {
        now_ms: 1234,
        event_id: "event-1",
        receipt_id: "receipt-1",
        process_start: Some("42"),
    }
}
fn payload() -> Value {
    json!({"hookEvent":"UserPromptSubmit","agent":"codex","session":"s","pane":"%1","panePid":"123","turnId":"t","conversationId":"c"})
}

#[test]
fn favorite_and_mark_have_independent_scopes_and_optimistic_revisions() {
    let conn = database();
    let empty = get_mark(&conn, "pane", "p").unwrap();
    assert_eq!(empty["revision"], 0);
    let first = set_mark(&conn, "pane", "p", &json!("resolved"), &json!(0), 100).unwrap();
    assert_eq!(first["mark"], "resolved");
    let next = set_mark(
        &conn,
        "pane",
        "p",
        &json!({"favorite":true}),
        &json!(1),
        101,
    )
    .unwrap();
    assert_eq!(next["favorite"], true);
    assert_eq!(next["mark"], "resolved");
    assert_eq!(get_mark(&conn, "session", "p").unwrap()["mark"], "none");
    match set_mark(&conn, "pane", "p", &json!("frozen"), &json!(1), 102).unwrap_err() {
        Error::Conflict(current) => assert_eq!(current, next),
        other => panic!("{other}"),
    }
    assert!(conn.is_autocommit());
}

#[test]
fn only_confirmed_prompts_reopen_resolved_once_and_keep_blocking_marks() {
    let conn = database();
    for mark in ["resolved", "frozen", "awaiting_reply"] {
        set_mark(&conn, "pane", mark, &json!(mark), &json!(0), 100).unwrap();
    }
    for mark in ["resolved", "frozen", "awaiting_reply"] {
        let event =
            json!({"eventId":"e","kind":"prompt_accepted","evidence":"confirmed","paneKey":mark});
        let changed = apply_turn_event(&conn, &event, 101).unwrap();
        assert_eq!(changed.len(), usize::from(mark == "resolved"));
        assert!(apply_turn_event(&conn, &event, 102).unwrap().is_empty());
    }
    assert_eq!(get_mark(&conn, "pane", "frozen").unwrap()["mark"], "frozen");
    assert_eq!(
        get_mark(&conn, "pane", "awaiting_reply").unwrap()["mark"],
        "awaiting_reply"
    );
}

#[test]
fn recording_a_hook_resolves_exact_process_and_reopens_marks_atomically() {
    let conn = database();
    set_mark(
        &conn,
        "pane",
        "logical",
        &json!({"mark":"resolved","favorite":true}),
        &json!(0),
        100,
    )
    .unwrap();
    set_mark(&conn, "session", "s", &json!("resolved"), &json!(0), 100).unwrap();
    conn.execute_batch("BEGIN IMMEDIATE").unwrap();
    let event = record(&conn, &payload(), &reception()).unwrap().unwrap();
    assert_eq!(event["paneKey"], "logical");
    assert_eq!(event["processKey"], "123-42");
    assert_eq!(get_mark(&conn, "pane", "logical").unwrap()["mark"], "none");
    assert_eq!(
        get_mark(&conn, "pane", "logical").unwrap()["favorite"],
        true
    );
    assert_eq!(get_mark(&conn, "session", "s").unwrap()["mark"], "none");
    assert!(!conn.is_autocommit());
    conn.execute_batch("ROLLBACK").unwrap();
    assert_eq!(latest_sequence(&conn).unwrap(), 0);
    assert_eq!(
        get_mark(&conn, "pane", "logical").unwrap()["mark"],
        "resolved"
    );
}

#[test]
fn failed_consumer_rolls_back_event_receipt_and_any_partial_mark_updates() {
    let conn = database();
    set_mark(&conn, "pane", "logical", &json!("resolved"), &json!(0), 100).unwrap();
    conn.execute_batch("CREATE TRIGGER fail_marks BEFORE INSERT ON work_mark_applied WHEN NEW.scope = 'session' BEGIN SELECT RAISE(ABORT, 'consumer failed'); END").unwrap();
    assert!(record(&conn, &payload(), &reception()).is_err());
    assert!(conn.is_autocommit());
    assert_eq!(latest_sequence(&conn).unwrap(), 0);
    assert_eq!(
        conn.query_row("SELECT COUNT(*) FROM event_receipts", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        0
    );
    assert_eq!(
        get_mark(&conn, "pane", "logical").unwrap()["mark"],
        "resolved"
    );
    assert_eq!(get_mark(&conn, "pane", "logical").unwrap()["revision"], 1);
    assert_eq!(
        conn.query_row("SELECT COUNT(*) FROM work_mark_applied", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        0
    );
}

#[test]
fn reused_pane_does_not_reopen_a_different_process_mark() {
    let conn = database();
    set_mark(&conn, "pane", "logical", &json!("resolved"), &json!(0), 100).unwrap();
    let mut reception = reception();
    reception.process_start = Some("43");
    let event = record(&conn, &payload(), &reception).unwrap().unwrap();
    assert_eq!(event["paneKey"], Value::Null);
    assert_eq!(
        get_mark(&conn, "pane", "logical").unwrap()["mark"],
        "resolved"
    );
}

#[test]
fn concurrent_hooks_deduplicate_and_reopen_the_mark_once() {
    use std::sync::{Arc, Barrier};
    use std::time::{Duration, SystemTime, UNIX_EPOCH};
    struct Temp(std::path::PathBuf);
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    let folder = std::env::temp_dir().join(format!(
        "comandos-concurrency-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir(&folder).unwrap();
    let folder = Temp(folder);
    let path = folder.0.join("state.sqlite3");
    let conn = Connection::open(&path).unwrap();
    conn.execute_batch(include_str!("fixtures/events.sql"))
        .unwrap();
    conn.execute_batch(include_str!("fixtures/workspace.sql"))
        .unwrap();
    conn.execute_batch(include_str!("fixtures/marks.sql"))
        .unwrap();
    conn.execute_batch("PRAGMA journal_mode=WAL").unwrap();
    set_mark(&conn, "pane", "logical", &json!("resolved"), &json!(0), 100).unwrap();
    let barrier = Arc::new(Barrier::new(8));
    let workers:Vec<_>=(0..8).map(|index|{
        let path=path.clone();let barrier=Arc::clone(&barrier);
        std::thread::spawn(move||{
            let conn=Connection::open(path).unwrap();
            conn.busy_timeout(Duration::from_secs(5)).unwrap();
            barrier.wait();
            record(&conn,&json!({"kind":"prompt_accepted","source":"test","sourceEventId":"shared-source","paneKey":"logical","evidence":"confirmed"}),&Reception {
                now_ms:1234,event_id:&format!("event-{index}"),receipt_id:&format!("receipt-{index}"),process_start:None,
            }).unwrap().unwrap()
        })
    }).collect();
    let events: Vec<_> = workers.into_iter().map(|w| w.join().unwrap()).collect();
    assert_eq!(events.iter().filter(|e| e["duplicate"] == false).count(), 1);
    assert!(events.iter().all(|e| e["eventId"] == events[0]["eventId"]));
    assert_eq!(latest_sequence(&conn).unwrap(), 1);
    assert_eq!(
        conn.query_row("SELECT COUNT(*) FROM event_receipts", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        8
    );
    let mark = get_mark(&conn, "pane", "logical").unwrap();
    assert_eq!(mark["mark"], "none");
    assert_eq!(mark["revision"], 2);
    conn.close().unwrap();
}
