use comandos_core::notifications::LiveCheck;
use comandos_store::{Error, append_event, notifications as nd};
use rusqlite::Connection;
use serde_json::{Value, json};
fn schema(conn: &Connection) {
    conn.execute_batch(include_str!("fixtures/events.sql"))
        .unwrap();
    conn.execute_batch(include_str!("notification_schema.sql"))
        .unwrap();
}
fn database() -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    schema(&conn);
    conn
}
#[test]
fn durable_behavior_matches_python_memory_database_oracle() {
    let conn = database();
    let cases: Vec<Value> =
        serde_json::from_str(include_str!("notification_fixture.json")).unwrap();
    for (i, c) in cases.iter().enumerate() {
        let live = |_: &Value, pane: &Value| pane == "%live";
        let check: LiveCheck<'_> = if c["live"] == true { Some(&live) } else { None };
        let result: comandos_store::Result<Value> = match c["op"].as_str().unwrap() {
            "event" => append_event(&conn, &c["event"], 0, "generated", &format!("receipt-{i}"))
                .map(|_| Value::Null),
            "presence" => nd::record_presence(
                &conn,
                c["device"].as_str().unwrap(),
                c["visible"] == true,
                c["audio"] == true,
                c["interaction"] == true,
                c["now"].as_i64().unwrap(),
                &c["kind"],
            )
            .map(|_| Value::Null),
            "clients" => nd::clients(&conn, 1_800_000_000_000).map(Value::Array),
            "read" => nd::mark_read(
                &conn,
                c["ids"].as_array().unwrap(),
                c["now"].as_i64().unwrap(),
            )
            .map(|x| json!(x)),
            "save" => nd::save_prefs(&conn, &c["update"]),
            "load" => nd::load_prefs(&conn),
            "badge" => nd::badge_count(&conn, check).map(|x| json!(x)),
            "unread" => nd::unread_notice_ids(&conn, c["project"].as_str()).map(|x| json!(x)),
            "revision" => nd::revision(&conn).map(|x| json!(x)),
            "list" => nd::list_notices(
                &conn,
                c["after"].as_i64().unwrap(),
                c["limit"].as_i64().unwrap(),
                1_800_000_000_000,
                None,
                c["focus"] == true,
                check,
            ),
            "claim" => nd::claim_sound(
                &conn,
                c["id"].as_str().unwrap(),
                c["device"].as_str().unwrap(),
                1_800_000_000_000,
                c["focus"].as_bool(),
            ),
            "focus" => Ok(json!(nd::focus_block_active(&conn))),
            "sql" => conn
                .execute_batch(c["sql"].as_str().unwrap())
                .map(|_| Value::Null)
                .map_err(Error::from),
            "begin" => conn
                .execute_batch("BEGIN IMMEDIATE")
                .map(|_| Value::Null)
                .map_err(Error::from),
            "rollback" => conn
                .execute_batch("ROLLBACK")
                .map(|_| Value::Null)
                .map_err(Error::from),
            op => panic!("{op}"),
        };
        let result = match result {
            Ok(v) => json!({"ok":v}),
            Err(err) => json!({"error":err.to_string()}),
        };
        assert_eq!(result, c["expected"], "case {i}: {c}");
        assert_eq!(
            !conn.is_autocommit(),
            c["inTransaction"] == true,
            "case {i}"
        );
    }
}
fn insert(conn: &Connection, id: &str) {
    append_event(conn,&json!({"eventId":id,"source":"test","kind":"permission_requested","evidence":"confirmed","correlation":"local","occurredAtMs":1800000000000_u64,"receivedAtMs":1800000000000_u64}),0,"generated",&format!("r-{id}")).unwrap();
}
#[test]
fn sound_claim_is_atomic_and_busy_does_not_touch_callers_transaction() {
    let path = std::env::temp_dir().join(format!(
        "notification-claims-{}.sqlite3",
        std::process::id()
    ));
    let conn = Connection::open(&path).unwrap();
    schema(&conn);
    insert(&conn, "race");
    insert(&conn, "busy");
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(6));
    let threads: Vec<_> = (0..6)
        .map(|_| {
            let path = path.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                let c = Connection::open(path).unwrap();
                c.busy_timeout(std::time::Duration::from_secs(2)).unwrap();
                barrier.wait();
                nd::claim_sound(&c, "race", "local-speaker", 1_800_000_000_000, Some(false))
                    .unwrap()
            })
        })
        .collect();
    let results: Vec<_> = threads.into_iter().map(|t| t.join().unwrap()).collect();
    assert_eq!(results.iter().filter(|r| r["play"] == true).count(), 1);
    let reader = Connection::open(&path).unwrap();
    reader.busy_timeout(std::time::Duration::ZERO).unwrap();
    conn.execute_batch("BEGIN IMMEDIATE").unwrap();
    assert_eq!(
        nd::claim_sound(&reader, "busy", "local-speaker", 1_800_000_000_000, None).unwrap(),
        json!({"play":false,"reason":"Ocupado"})
    );
    assert!(reader.is_autocommit());
    assert_eq!(
        nd::claim_sound(&conn, "busy", "local-speaker", 1_800_000_000_000, None).unwrap(),
        json!({"play":false,"reason":"Ocupado"})
    );
    assert!(!conn.is_autocommit());
    conn.execute_batch("ROLLBACK").unwrap();
    assert_eq!(
        nd::claim_sound(&reader, "busy", "local-speaker", 1_800_000_000_000, None).unwrap()["play"],
        true
    );
    drop(reader);
    drop(conn);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn reads_cap_after_filtering_keep_first_timestamp_and_do_not_resolve_pending() {
    let conn = database();
    insert(&conn, "pending");
    let mut ids = vec![Value::Null, json!(false), json!(1), json!("")];
    ids.extend((0..1005).map(|i| json!(format!("read-{i}"))));
    let accepted = nd::mark_read(&conn, &ids, 10).unwrap();
    assert_eq!(accepted.len(), 1000);
    assert_eq!(accepted.last().unwrap(), "read-999");
    nd::mark_read(&conn, &[json!("read-0")], 20).unwrap();
    assert_eq!(nd::revision(&conn).unwrap(), "1.1000.10");
    conn.execute("UPDATE events SET session_key='s',pane_id='%1'", [])
        .unwrap();
    nd::mark_read(&conn, &[json!("pending")], 30).unwrap();
    assert_eq!(nd::badge_count(&conn, None).unwrap(), 1);
    assert_eq!(nd::badge_count(&conn, Some(&|_, _| false)).unwrap(), 0);
    assert!(nd::unread_notice_ids(&conn, None).unwrap().is_empty());
}

#[test]
fn durable_writes_fail_nested_without_committing_or_rolling_back_the_caller() {
    let conn = database();
    conn.execute_batch("BEGIN IMMEDIATE").unwrap();
    conn.execute("INSERT INTO notice_reads VALUES('caller',1)", [])
        .unwrap();
    assert!(nd::mark_read(&conn, &[json!("new")], 2).is_err());
    assert!(nd::record_presence(&conn, "phone", true, true, true, 2, &json!("web")).is_err());
    assert!(nd::save_prefs(&conn, &json!({"muted":true})).is_err());
    assert!(!conn.is_autocommit());
    assert_eq!(nd::revision(&conn).unwrap(), "0.1.1");
    assert!(nd::clients(&conn, 2).unwrap().is_empty());
    assert_eq!(nd::load_prefs(&conn).unwrap()["muted"], false);
    conn.execute_batch("ROLLBACK").unwrap();
    assert_eq!(nd::revision(&conn).unwrap(), "0.0.0");
}

#[test]
fn claimed_event_never_sounds_again_after_the_chosen_device_changes() {
    let conn = database();
    insert(&conn, "one");
    assert_eq!(
        nd::claim_sound(&conn, "one", "local-speaker", 1_800_000_000_000, None).unwrap()["play"],
        true
    );
    nd::record_presence(
        &conn,
        "phone",
        true,
        true,
        true,
        1_800_000_000_000,
        &json!("web"),
    )
    .unwrap();
    assert_eq!(
        nd::claim_sound(&conn, "one", "phone", 1_800_000_000_000, None).unwrap(),
        json!({"play":false,"reason":"Ya sonó"})
    );
    assert_eq!(
        conn.query_row(
            "SELECT device_id FROM deliveries WHERE event_id='one'",
            [],
            |r| r.get::<_, String>(0)
        )
        .unwrap(),
        ""
    );
}

#[test]
fn integrity_failures_roll_back_the_owned_transaction_and_propagate() {
    let conn = database();
    insert(&conn, "one");
    conn.execute_batch("CREATE TRIGGER fail_claim BEFORE INSERT ON deliveries BEGIN SELECT RAISE(ABORT,'claim failed'); END;").unwrap();
    let err = nd::claim_sound(&conn, "one", "local-speaker", 1_800_000_000_000, None).unwrap_err();
    assert!(matches!(
        err,
        Error::Sql(rusqlite::Error::SqliteFailure(_, _))
    ));
    assert!(conn.is_autocommit());
    assert_eq!(
        conn.query_row("SELECT COUNT(*) FROM deliveries", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        0
    );
    conn.execute_batch("DROP TRIGGER fail_claim;").unwrap();
    assert_eq!(
        nd::claim_sound(&conn, "one", "local-speaker", 1_800_000_000_000, None).unwrap()["play"],
        true
    );
    conn.execute_batch("CREATE TRIGGER fail_read BEFORE INSERT ON notice_reads WHEN NEW.event_id='fail' BEGIN SELECT RAISE(ABORT,'read failed'); END;").unwrap();
    assert!(nd::mark_read(&conn, &[json!("first"), json!("fail")], 3).is_err());
    assert_eq!(nd::revision(&conn).unwrap(), "1.0.0");
    assert!(conn.is_autocommit());
}
