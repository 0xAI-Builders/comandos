use comandos_core::workspace as domain;
use comandos_store::{
    state,
    workspace::{Error, Phase, WorkspaceStore},
};
use rusqlite::{Connection, params};
use serde_json::{Value, json};
use std::{
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "comandos-workspace-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn open(&self) -> Connection {
        let conn = state::connect(&self.0.join("state.sqlite3")).unwrap();
        state::migrate(&conn, &state::MIGRATIONS[..1], 1.0).unwrap();
        conn
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn doc(tab: &str) -> Value {
    json!({"schema":1,"groups":[{"id":"g","tree":{"type":"tab","tabId":tab}}],"tabs":{tab:{"session":tab,"paneKeys":[format!("pk-{tab}")]}}})
}
fn count(conn: &Connection, table: &str) -> i64 {
    conn.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
        .unwrap()
}
fn commit(store: &WorkspaceStore<'_>, rev: i64, tab: &str, req: &str, now: f64) {
    store
        .commit(&json!(rev), &doc(tab), req, "user", now)
        .unwrap();
}

#[test]
fn commit_reopens_with_unknown_data_and_previous_snapshot() {
    let temp = Temp::new();
    let mut d = doc("a");
    d["unknown"] = json!({"ñ":"🐈","huge":serde_json::from_str::<Value>("123456789012345678901234567890").unwrap()});
    {
        let conn = temp.open();
        let store = WorkspaceStore::new(&conn);
        assert!(store.current().unwrap().is_none());
        let saved = store.commit(&json!(0), &d, "first", "user", 10.0).unwrap();
        assert_eq!(saved.revision, 1);
        assert_eq!(saved.document, d);
        commit(&store, 1, "b", "second", 11.0);
        let previous: String = conn
            .query_row("SELECT document FROM workspace_previous", [], |r| r.get(0))
            .unwrap();
        assert_eq!(serde_json::from_str::<Value>(&previous).unwrap(), d);
    }
    let conn = temp.open();
    let store = WorkspaceStore::new(&conn);
    assert_eq!(store.current().unwrap().unwrap().document, doc("b"));
}

#[test]
fn corrupt_current_recovers_valid_previous_and_keeps_raw_revision_for_commit() {
    let temp = Temp::new();
    let conn = temp.open();
    let store = WorkspaceStore::new(&conn);
    commit(&store, 0, "a", "one", 1.0);
    commit(&store, 1, "b", "two", 2.0);
    for broken in ["{truncated", r#"{"schema":2,"groups":[],"tabs":{}}"#] {
        conn.execute("UPDATE workspace_current SET document=?", [broken])
            .unwrap();
        let recovered = store.current().unwrap().unwrap();
        assert!(recovered.recovered);
        assert_eq!(recovered.revision, 1);
        assert_eq!(recovered.document, doc("a"));
    }
    match store
        .commit(&json!(1), &doc("c"), "stale", "user", 3.0)
        .unwrap_err()
    {
        Error::Conflict {
            current: Some(s), ..
        } => assert_eq!(s.revision, 1),
        other => panic!("{other:?}"),
    }
    commit(&store, 2, "c", "three", 3.0);
    assert_eq!(store.current().unwrap().unwrap().revision, 3);
    conn.execute("UPDATE workspace_current SET document='{}'", [])
        .unwrap();
    assert!(store.current().unwrap().is_none());
}

#[test]
fn stale_revision_carries_current_and_does_not_overwrite() {
    let temp = Temp::new();
    let conn = temp.open();
    let store = WorkspaceStore::new(&conn);
    commit(&store, 0, "a", "one", 1.0);
    commit(&store, 1, "b", "two", 2.0);
    let error = store
        .commit(&json!(1), &doc("z"), "three", "user", 3.0)
        .unwrap_err();
    assert_eq!(error.to_string(), "Revisión desactualizada");
    match error {
        Error::Conflict {
            current: Some(s), ..
        } => {
            assert_eq!(s.revision, 2);
            assert_eq!(s.document, doc("b"));
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(count(&conn, "workspace_requests"), 2);
    assert!(conn.is_autocommit());
    commit(&store, 2, "c", "three", 3.0);
}

#[test]
fn replay_returns_original_revision_after_later_commit_and_changed_digest_conflicts() {
    let temp = Temp::new();
    let conn = temp.open();
    let store = WorkspaceStore::new(&conn);
    let first = store
        .commit(&json!(0), &doc("a"), "same", "user", 1.0)
        .unwrap();
    commit(&store, 1, "b", "later", 2.0);
    assert_eq!(
        store
            .commit(&json!(999), &doc("a"), "same", "user", 3.0)
            .unwrap(),
        first
    );
    let error = store
        .commit(&json!(2), &doc("b"), "same", "user", 4.0)
        .unwrap_err();
    assert_eq!(
        error.to_string(),
        "requestId reutilizado con otro contenido"
    );
    assert!(matches!(
        error,
        Error::Conflict {
            current: Some(_),
            ..
        }
    ));
    assert_eq!(store.current().unwrap().unwrap().revision, 2);
}

#[test]
fn phases_gate_automatic_saves_and_explicit_user_empty_is_allowed() {
    let temp = Temp::new();
    let conn = temp.open();
    let mut store = WorkspaceStore::new(&conn);
    assert_eq!(store.phase(), Phase::Restoring);
    for phase in ["restoring", "failed"] {
        store.set_phase(phase).unwrap();
        assert!(matches!(
            store.commit(&json!(0), &doc("a"), "auto", "auto", 1.0),
            Err(Error::NotReady(_))
        ));
    }
    assert_eq!(
        store.set_phase("invalid").unwrap_err().to_string(),
        "Fase desconocida"
    );
    commit(&store, 0, "a", "user", 1.0);
    store.set_phase("ready").unwrap();
    assert!(matches!(
        store.commit(&json!(1), &domain::empty_document(), "empty", "auto", 2.0),
        Err(Error::EmptyInventory)
    ));
    store
        .commit(
            &json!(1),
            &domain::empty_document(),
            "user-empty",
            "user",
            2.0,
        )
        .unwrap();
    store
        .commit(
            &json!(2),
            &domain::empty_document(),
            "already-empty",
            "auto",
            3.0,
        )
        .unwrap();
    assert!(domain::is_empty(
        &store.current().unwrap().unwrap().document
    ));
}

#[test]
fn first_automatic_empty_can_be_saved_when_ready() {
    let temp = Temp::new();
    let conn = temp.open();
    let mut store = WorkspaceStore::new(&conn);
    store.set_phase("ready").unwrap();
    store
        .commit(&json!(0), &domain::empty_document(), "empty", "auto", 1.0)
        .unwrap();
}

#[test]
fn invalid_documents_request_reason_and_clock_leave_store_usable() {
    let temp = Temp::new();
    let conn = temp.open();
    let store = WorkspaceStore::new(&conn);
    for d in [
        json!(null),
        json!({"schema":2,"groups":[],"tabs":{}}),
        json!({"schema":1,"groups":[],"tabs":{"orphan":{}}}),
    ] {
        assert!(matches!(
            store.commit(&json!(0), &d, "bad", "user", 1.0),
            Err(Error::Invalid(_))
        ));
    }
    for req in ["", &"x".repeat(201)] {
        assert!(matches!(
            store.commit(&json!(0), &doc("a"), req, "user", 1.0),
            Err(Error::Invalid(_))
        ));
    }
    assert_eq!(
        store
            .commit(&json!(0), &doc("a"), "bad", "poll", 1.0)
            .unwrap_err()
            .to_string(),
        "Motivo inválido"
    );
    assert!(
        store
            .commit(&json!(0), &doc("a"), "bad", "user", f64::NAN)
            .is_err()
    );
    assert_eq!(count(&conn, "workspace_requests"), 0);
    assert!(conn.is_autocommit());
    commit(&store, 0, "a", "good", 1.0);
}

#[test]
fn python_numeric_revision_equality_accepts_bool_and_integral_float_only() {
    let temp = Temp::new();
    let conn = temp.open();
    let store = WorkspaceStore::new(&conn);
    store
        .commit(&json!(false), &doc("a"), "zero", "user", 1.0)
        .unwrap();
    store
        .commit(&json!(true), &doc("a"), "one", "user", 2.0)
        .unwrap();
    store
        .commit(&json!(2.0), &doc("a"), "two", "user", 3.0)
        .unwrap();
    for rev in [
        json!("3"),
        json!(3.1),
        json!(null),
        serde_json::from_str("9007199254740993").unwrap(),
    ] {
        assert!(matches!(
            store.commit(&rev, &doc("a"), "bad", "user", 4.0),
            Err(Error::Conflict { .. })
        ));
    }
}

#[test]
fn canonical_python_utf8_numeric_bytes_have_known_digest() {
    let temp = Temp::new();
    let conn = temp.open();
    let store = WorkspaceStore::new(&conn);
    let d: Value=serde_json::from_str(r#"{"tabs":{},"schema":1,"groups":[],"ñ":[1.0,1e-7,-0.0,123456789012345678901234567890,"🐈"]}"#).unwrap();
    store.commit(&json!(0), &d, "digest", "user", 1.0).unwrap();
    let (encoded, digest): (String, String) = conn
        .query_row(
            "SELECT document,digest FROM workspace_current,workspace_requests",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(
        encoded,
        r#"{"groups":[],"schema":1,"tabs":{},"ñ":[1.0,1e-07,-0.0,123456789012345678901234567890,"🐈"]}"#
    );
    assert_eq!(
        digest,
        "b374723f554aee437f3b20ad665d9f75925e332636dedd094e7f2e8bf321cf8b"
    );
}

#[test]
fn request_expiry_is_strict_and_occurs_only_after_successful_new_commit() {
    let temp = Temp::new();
    let conn = temp.open();
    let store = WorkspaceStore::new(&conn);
    commit(&store, 0, "a", "old", 1.0);
    commit(&store, 1, "b", "edge", 86401.0);
    assert_eq!(count(&conn, "workspace_requests"), 2);
    assert_eq!(
        store
            .commit(&json!(0), &doc("a"), "old", "user", 86402.0)
            .unwrap()
            .revision,
        1
    );
    commit(&store, 2, "c", "expire", 86402.0);
    assert_eq!(count(&conn, "workspace_requests"), 2);
    assert!(matches!(
        store.commit(&json!(0), &doc("a"), "old", "user", 86403.0),
        Err(Error::Conflict { .. })
    ));
    commit(&store, 3, "a", "old", 86403.0);
    assert_eq!(store.current().unwrap().unwrap().revision, 4);
}

#[test]
fn trigger_failure_rolls_back_current_previous_request_and_expiration() {
    let temp = Temp::new();
    let conn = temp.open();
    let store = WorkspaceStore::new(&conn);
    commit(&store, 0, "a", "one", 1.0);
    commit(&store, 1, "b", "two", 2.0);
    conn.execute_batch("CREATE TRIGGER fail_request BEFORE INSERT ON workspace_requests WHEN NEW.request_id='three' BEGIN SELECT RAISE(ABORT,'disk full'); END;").unwrap();
    assert!(matches!(
        store.commit(&json!(2), &doc("c"), "three", "user", 90000.0),
        Err(Error::Sql(_))
    ));
    assert!(conn.is_autocommit());
    assert_eq!(store.current().unwrap().unwrap().document, doc("b"));
    assert_eq!(count(&conn, "workspace_requests"), 2);
    let previous: String = conn
        .query_row("SELECT document FROM workspace_previous", [], |r| r.get(0))
        .unwrap();
    assert_eq!(serde_json::from_str::<Value>(&previous).unwrap(), doc("a"));
    conn.execute_batch("DROP TRIGGER fail_request").unwrap();
    commit(&store, 2, "c", "three", 90000.0);
}

#[test]
fn expiration_trigger_failure_rolls_back_inserted_request() {
    let temp = Temp::new();
    let conn = temp.open();
    let store = WorkspaceStore::new(&conn);
    commit(&store, 0, "a", "one", 1.0);
    conn.execute_batch("CREATE TRIGGER fail_delete BEFORE DELETE ON workspace_requests BEGIN SELECT RAISE(ABORT,'delete failed'); END;").unwrap();
    assert!(
        store
            .commit(&json!(1), &doc("b"), "two", "user", 90000.0)
            .is_err()
    );
    assert_eq!(store.current().unwrap().unwrap().revision, 1);
    assert_eq!(count(&conn, "workspace_requests"), 1);
    assert_eq!(count(&conn, "workspace_previous"), 0);
    assert!(conn.is_autocommit());
}

#[test]
fn separate_devices_merge_patches_and_explicit_null_without_layout_revision_change() {
    let temp = Temp::new();
    let conn = temp.open();
    let store = WorkspaceStore::new(&conn);
    commit(&store, 0, "a", "one", 1.0);
    store
        .save_client(
            "phone",
            &json!({"drafts":{"pk-a":{"text":"hola"}},"readingAnchors":{"pk-a":{"line":40}}}),
            1.0,
        )
        .unwrap();
    store
        .save_client("phone", &json!({"activeTabId":"a"}), 2.0)
        .unwrap();
    let saved=store.save_client("phone",&json!({"activeTabId":null,"draftsPatch":{"pk-b":{"text":"ñ🐈","selStart":1,"selEnd":2}},"anchorsPatch":{"pk-b":{"text":"line","ratio":0.5}}}),3.0).unwrap();
    assert!(saved["activeTabId"].is_null());
    assert_eq!(saved["drafts"]["pk-a"]["text"], "hola");
    assert_eq!(saved["readingAnchors"]["pk-a"]["line"], 40);
    assert_eq!(saved["drafts"]["pk-b"]["updatedAt"], 3000.0);
    store
        .save_client(
            "phone",
            &json!({"draftsPatch":{"pk-a":null},"anchorsPatch":{"pk-b":null}}),
            4.0,
        )
        .unwrap();
    let state = store.client("phone").unwrap().unwrap();
    assert!(state["drafts"].get("pk-a").is_none());
    assert!(state["readingAnchors"].get("pk-b").is_none());
    assert!(store.client("desk").unwrap().is_none());
    store
        .save_client("desk", &json!({"activeTabId":"b"}), 5.0)
        .unwrap();
    assert_eq!(store.client("desk").unwrap().unwrap()["activeTabId"], "b");
    assert_eq!(store.current().unwrap().unwrap().revision, 1);
}

#[test]
fn invalid_client_and_trigger_failure_rollback_then_allow_next_save() {
    let temp = Temp::new();
    let conn = temp.open();
    let store = WorkspaceStore::new(&conn);
    let previous = store
        .save_client("phone", &json!({"activeTabId":"a"}), 1.0)
        .unwrap();
    for bad in [
        json!(null),
        json!({"activeTabId":""}),
        json!({"draftsPatch":{"k":{"text":4}}}),
        json!({"anchorsPatch":[]}),
        json!({"drafts":{"k":"x".repeat(300000)}}),
    ] {
        assert!(matches!(
            store.save_client("phone", &bad, 2.0),
            Err(Error::Invalid(_))
        ));
        assert!(conn.is_autocommit());
        assert_eq!(store.client("phone").unwrap().unwrap(), previous);
    }
    assert!(store.save_client("", &json!({}), 1.0).is_err());
    conn.execute_batch("CREATE TRIGGER fail_client AFTER INSERT ON workspace_clients BEGIN SELECT RAISE(ABORT,'client failed'); END;").unwrap();
    assert!(
        store
            .save_client("phone", &json!({"activeTabId":"b"}), 2.0)
            .is_err()
    );
    assert_eq!(store.client("phone").unwrap().unwrap(), previous);
    assert!(conn.is_autocommit());
    conn.execute_batch("DROP TRIGGER fail_client").unwrap();
    store
        .save_client("phone", &json!({"activeTabId":"b"}), 3.0)
        .unwrap();
}

#[test]
fn mutators_reject_caller_transaction_without_committing_or_rolling_back() {
    let temp = Temp::new();
    let conn = temp.open();
    let store = WorkspaceStore::new(&conn);
    conn.execute_batch("BEGIN IMMEDIATE; INSERT INTO workspace_meta VALUES ('caller','pending')")
        .unwrap();
    assert!(matches!(
        store.commit(&json!(0), &doc("a"), "one", "user", 1.0),
        Err(Error::ActiveTransaction)
    ));
    assert!(matches!(
        store.save_client("phone", &json!({}), 1.0),
        Err(Error::ActiveTransaction)
    ));
    assert!(matches!(
        store.set_meta("adapter", "value"),
        Err(Error::ActiveTransaction)
    ));
    assert!(!conn.is_autocommit());
    assert_eq!(store.meta("caller").unwrap().as_deref(), Some("pending"));
    assert_eq!(count(&conn, "workspace_current"), 0);
    assert_eq!(count(&conn, "workspace_clients"), 0);
    conn.execute_batch("ROLLBACK").unwrap();
    assert!(store.meta("caller").unwrap().is_none());
    commit(&store, 0, "a", "one", 1.0);
}

#[test]
fn text_metadata_roundtrips_and_failed_write_rolls_back() {
    let temp = Temp::new();
    let conn = temp.open();
    let store = WorkspaceStore::new(&conn);
    assert!(store.meta("missing").unwrap().is_none());
    store.set_meta("key", "ñ🐈\ntext").unwrap();
    assert_eq!(store.meta("key").unwrap().as_deref(), Some("ñ🐈\ntext"));
    conn.execute_batch("CREATE TRIGGER fail_meta AFTER INSERT ON workspace_meta BEGIN SELECT RAISE(ABORT,'meta failed'); END").unwrap();
    assert!(store.set_meta("key", "changed").is_err());
    assert!(conn.is_autocommit());
    assert_eq!(store.meta("key").unwrap().as_deref(), Some("ñ🐈\ntext"));
}

#[test]
fn close_group_adapter_persists_json_replay_across_reopen_without_second_callback() {
    let temp = Temp::new();
    let first;
    {
        let conn = temp.open();
        let mut store = WorkspaceStore::new(&conn);
        commit(&store, 0, "a", "seed", 1.0);
        let identities = json!([{"tabId":"a","session":"a","sessionId":"$1"}]);
        let mut calls = 0;
        first = domain::close_group(
            &mut store,
            "g",
            &json!(1),
            &identities,
            "close",
            |_| json!("$1"),
            |_| {
                calls += 1;
                Ok(None)
            },
        )
        .unwrap();
        assert_eq!(calls, 1);
        assert_eq!(first["closed"], json!(["a"]));
        assert_eq!(
            serde_json::from_str::<Value>(&store.meta("close-group:close").unwrap().unwrap())
                .unwrap(),
            first
        );
    }
    let conn = temp.open();
    let mut store = WorkspaceStore::new(&conn);
    let replay = domain::close_group(
        &mut store,
        "missing",
        &json!(999),
        &json!([]),
        "close",
        |_| panic!("identity on replay"),
        |_| panic!("close on replay"),
    )
    .unwrap();
    assert_eq!(replay, first);
}

#[test]
fn close_group_replay_returns_falsy_json_without_identity_or_close_effects() {
    for raw in ["false", "0", "null", "{}", "[]", r#""""#] {
        let temp = Temp::new();
        let conn = temp.open();
        let mut store = WorkspaceStore::new(&conn);
        commit(&store, 0, "a", "seed", 1.0);
        store.set_meta("close-group:close", raw).unwrap();
        let mut inspections = 0;
        let mut closes = 0;
        let result = domain::close_group(
            &mut store,
            "g",
            &json!(1),
            &json!([{"tabId":"a","session":"a","sessionId":"$1"}]),
            "close",
            |_| {
                inspections += 1;
                json!("$1")
            },
            |_| {
                closes += 1;
                Ok(None)
            },
        )
        .unwrap();
        assert_eq!((inspections, closes), (0, 0), "saved {raw}");
        assert_eq!(result, serde_json::from_str::<Value>(raw).unwrap());
        assert_eq!(
            store.meta("close-group:close").unwrap().as_deref(),
            Some(raw)
        );
        assert_eq!(store.current().unwrap().unwrap().revision, 1);
        assert_eq!(count(&conn, "workspace_requests"), 1);
        assert!(conn.is_autocommit());
    }
}

#[test]
fn close_group_replay_returns_falsy_json_before_reading_current_state() {
    for raw in ["false", "0", "null", "{}", "[]", r#""""#] {
        let temp = Temp::new();
        let conn = temp.open();
        let mut store = WorkspaceStore::new(&conn);
        store.set_meta("close-group:close", raw).unwrap();
        // A real adapter state read would fail; saved replay must skip it.
        conn.execute_batch("DROP TABLE workspace_current").unwrap();
        let result = domain::close_group(
            &mut store,
            "missing",
            &json!(999),
            &json!(null),
            "close",
            |_| panic!("identity on replay of {raw}"),
            |_| panic!("close on replay of {raw}"),
        )
        .unwrap();
        assert_eq!(result, serde_json::from_str::<Value>(raw).unwrap());
        assert_eq!(
            store.meta("close-group:close").unwrap().as_deref(),
            Some(raw)
        );
        assert!(conn.is_autocommit());
    }
}

#[test]
fn close_group_replay_absent_or_empty_text_performs_and_saves_the_close() {
    for raw in [None, Some("")] {
        let temp = Temp::new();
        let conn = temp.open();
        let mut store = WorkspaceStore::new(&conn);
        commit(&store, 0, "a", "seed", 1.0);
        if let Some(raw) = raw {
            store.set_meta("close-group:close", raw).unwrap();
        }
        let mut inspections = 0;
        let mut closes = 0;
        let result = domain::close_group(
            &mut store,
            "g",
            &json!(1),
            &json!([{"tabId":"a","session":"a","sessionId":"$1"}]),
            "close",
            |_| {
                inspections += 1;
                json!("$1")
            },
            |_| {
                closes += 1;
                Ok(None)
            },
        )
        .unwrap();
        assert_eq!((inspections, closes), (1, 1), "saved {raw:?}");
        assert_eq!(
            result,
            json!({"ok":true,"closed":["a"],"remaining":[],"kept":[],"error":null})
        );
        assert_eq!(
            serde_json::from_str::<Value>(&store.meta("close-group:close").unwrap().unwrap())
                .unwrap(),
            result
        );
        assert_eq!(store.current().unwrap().unwrap().revision, 1);
        assert_eq!(count(&conn, "workspace_requests"), 1);
        assert!(conn.is_autocommit());
    }
}

#[test]
fn close_group_replay_malformed_nonempty_text_fails_before_state_or_callbacks() {
    for raw in ["{broken", " ", "false trailing"] {
        let temp = Temp::new();
        let conn = temp.open();
        let mut store = WorkspaceStore::new(&conn);
        store.set_meta("close-group:close", raw).unwrap();
        conn.execute_batch("DROP TABLE workspace_current").unwrap();
        let error = domain::close_group(
            &mut store,
            "missing",
            &json!(999),
            &json!(null),
            "close",
            |_| panic!("identity on malformed replay"),
            |_| panic!("close on malformed replay"),
        )
        .unwrap_err();
        let expected = comandos_core::json::workspace_loads(raw)
            .unwrap_err()
            .to_string();
        assert!(
            matches!(error, domain::WorkspaceError::Callback(ref message) if message == &expected)
        );
        assert_eq!(
            store.meta("close-group:close").unwrap().as_deref(),
            Some(raw)
        );
        assert!(conn.is_autocommit());
    }
}

#[test]
fn client_disk_reopen_and_corrupt_json_errors_leave_connection_usable() {
    let temp = Temp::new();
    {
        let conn = temp.open();
        WorkspaceStore::new(&conn)
            .save_client("phone", &json!({"activePaneKey":"pk-a"}), 1.0)
            .unwrap();
    }
    let conn = temp.open();
    let store = WorkspaceStore::new(&conn);
    assert_eq!(
        store.client("phone").unwrap().unwrap()["activePaneKey"],
        "pk-a"
    );
    conn.execute(
        "UPDATE workspace_clients SET state=? WHERE device_id=?",
        params!["{broken", "phone"],
    )
    .unwrap();
    assert!(matches!(
        store.save_client("phone", &json!({}), 2.0),
        Err(Error::Json(_))
    ));
    assert!(conn.is_autocommit());
    store.save_client("desk", &json!({}), 3.0).unwrap();
}

#[test]
fn large_sqlite_revision_is_compared_without_rounding_to_float() {
    let temp = Temp::new();
    let conn = temp.open();
    let store = WorkspaceStore::new(&conn);
    commit(&store, 0, "a", "seed", 1.0);
    conn.execute("UPDATE workspace_current SET revision=9007199254740993", [])
        .unwrap();
    assert!(matches!(
        store.commit(
            &json!(9007199254740992.0),
            &doc("b"),
            "rounded",
            "user",
            2.0
        ),
        Err(Error::Conflict { .. })
    ));
    let saved = store
        .commit(&json!(9007199254740993i64), &doc("b"), "exact", "user", 2.0)
        .unwrap();
    assert_eq!(saved.revision, 9007199254740994);
}

#[test]
fn immediate_writer_lock_failure_preserves_other_connection_transaction() {
    let temp = Temp::new();
    let conn = temp.open();
    let other = temp.open();
    other
        .busy_timeout(std::time::Duration::from_millis(1))
        .unwrap();
    conn.execute_batch("BEGIN IMMEDIATE; INSERT INTO workspace_meta VALUES ('caller','locked')")
        .unwrap();
    let store = WorkspaceStore::new(&other);
    assert!(matches!(
        store.commit(&json!(0), &doc("a"), "seed", "user", 1.0),
        Err(Error::Sql(_))
    ));
    assert!(other.is_autocommit());
    assert!(!conn.is_autocommit());
    conn.execute_batch("ROLLBACK").unwrap();
    commit(&store, 0, "a", "seed", 1.0);
}

#[test]
fn retained_overflow_float_metadata_roundtrips_through_python_canonical_infinity() {
    let temp = Temp::new();
    let conn = temp.open();
    let store = WorkspaceStore::new(&conn);
    let mut d = doc("a");
    d["unknown"] = serde_json::from_str(r#"{"positive":1e9999,"negative":-1e9999}"#).unwrap();
    // Construct the reserved-key object directly; the workspace codec must
    // treat application metadata as an object, never as serde's number tag.
    d["unknown"]["reserved"] = json!({"$serde_json::private::Number":"user text"});
    let saved = store
        .commit(&json!(0), &d, "overflow", "user", 1.0)
        .unwrap();
    let encoded: String = conn
        .query_row("SELECT document FROM workspace_current", [], |r| r.get(0))
        .unwrap();
    assert!(encoded.contains(":Infinity"));
    assert!(encoded.contains(":-Infinity"));
    assert_eq!(store.current().unwrap().unwrap(), saved);
    assert_eq!(
        saved.document["unknown"]["reserved"],
        d["unknown"]["reserved"]
    );
    assert_eq!(
        store
            .commit(&json!(999), &d, "overflow", "user", 2.0)
            .unwrap(),
        saved
    );
    let client = store
        .save_client(
            "phone",
            &json!({"drafts":{"legacy":d["unknown"].clone()}}),
            1.0,
        )
        .unwrap();
    assert_eq!(
        store.client("phone").unwrap().unwrap()["drafts"]["legacy"]["reserved"],
        client["drafts"]["legacy"]["reserved"]
    );
}

#[test]
fn retained_document_client_and_replay_metadata_beyond_128_depth_survive_reopen() {
    use domain::CloseGroupState;
    let temp = Temp::new();
    let mut deep = json!({"leaf":"ñ"});
    for _ in 0..150 {
        deep = json!({"next":deep});
    }
    let mut d = doc("a");
    d["retained"] = deep.clone();
    {
        let conn = temp.open();
        let mut store = WorkspaceStore::new(&conn);
        store.commit(&json!(0), &d, "deep", "user", 1.0).unwrap();
        store
            .save_client("phone", &json!({"drafts":{"legacy":deep.clone()}}), 1.0)
            .unwrap();
        CloseGroupState::set_meta(&mut store, "replay", &deep).unwrap();
    }
    let conn = temp.open();
    let mut store = WorkspaceStore::new(&conn);
    assert_eq!(store.current().unwrap().unwrap().document, d);
    assert_eq!(
        store.client("phone").unwrap().unwrap()["drafts"]["legacy"],
        deep
    );
    assert_eq!(
        CloseGroupState::meta(&mut store, "replay")
            .unwrap()
            .unwrap(),
        deep
    );
}
