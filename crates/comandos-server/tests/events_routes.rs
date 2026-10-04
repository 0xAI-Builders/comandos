use comandos_server::{
    HandlerError, ReplyBody, Request,
    events_routes::{EventRoutes, Facts},
};
use comandos_store::{
    intake::{Reception, record},
    marks, state,
};
use http::Method;
use rusqlite::Connection;
use serde_json::{Value, json};
use std::{
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

const NOW: u64 = 123_456;
struct FixtureFacts {
    id: u64,
}
impl Facts for FixtureFacts {
    fn now_ms(&mut self) -> comandos_store::Result<u64> {
        Ok(NOW)
    }
    fn fresh_id(&mut self, prefix: &str) -> comandos_store::Result<String> {
        self.id += 1;
        Ok(format!("{prefix}-fixture-{}", self.id))
    }
    fn process_start(&mut self, pid: &str) -> Option<String> {
        (pid == "123").then(|| "42".into())
    }
}
struct Fixture {
    conn: Connection,
    path: PathBuf,
    routes: EventRoutes<FixtureFacts>,
}
impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "events-routes-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE TABLE schema_migrations(version INTEGER PRIMARY KEY,name TEXT NOT NULL,applied_at REAL NOT NULL)").unwrap();
        state::migrate(&conn, state::MIGRATIONS, 0.0).unwrap();
        let routes = EventRoutes::new(path.clone(), FixtureFacts { id: 0 });
        Self { conn, path, routes }
    }
    fn call(
        &mut self,
        method: &str,
        target: &str,
        data: Option<Value>,
        internal: bool,
    ) -> (u16, Value) {
        let request = request(method, target, data, internal);
        let reply = self
            .routes
            .handle(&self.conn, &request)
            .unwrap()
            .expect("owned route must dispatch");
        let ReplyBody::Bytes(bytes) = reply.body else {
            panic!("finite route response required")
        };
        (
            reply.status.as_u16(),
            comandos_core::json::workspace_loads_bytes(&bytes).unwrap(),
        )
    }
    fn seed(&self, event_id: &str, event: Value) -> Value {
        record(
            &self.conn,
            &event,
            &Reception {
                now_ms: NOW,
                event_id,
                receipt_id: &format!("receipt-{event_id}"),
                process_start: Some("42"),
            },
        )
        .unwrap()
        .unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
        let _ = std::fs::remove_dir(&self.path);
    }
}
fn request(method: &str, target: &str, data: Option<Value>, internal: bool) -> Request {
    Request {
        method: Method::from_bytes(method.as_bytes()).unwrap(),
        target: target.into(),
        peer: "127.0.0.1:12345".parse().unwrap(),
        headers: vec![],
        data,
        internal_producer: internal,
    }
}
fn hook(kind: &str, turn: &str) -> Value {
    json!({"hookEvent":kind,"agent":"codex","session":"sess","pane":"%3","turnId":turn,"conversationId":"thread-1","occurredAtMs":1000})
}

#[test]
fn routing_preserves_normalized_get_and_raw_exact_post_boundaries() {
    let mut f = Fixture::new();
    for (method, target) in [
        ("POST", "/events/v2?turns=1"),
        ("POST", "/work-marks?x=1"),
        ("DELETE", "/events/v2"),
        ("GET", "/events"),
        ("GET", "/events/v2/"),
        ("GET", "/%65vents/v2"),
    ] {
        assert!(
            f.routes
                .handle(&f.conn, &request(method, target, Some(json!({})), true))
                .unwrap()
                .is_none(),
            "{method} {target}"
        );
    }
    for target in [
        "/events/v2?after=0#fragment",
        "http://localhost/events/v2?after=0",
        "\t/events/v2\r\n?after=0",
    ] {
        assert_eq!(f.call("GET", target, None, false).0, 200, "{target}");
    }
    assert!(matches!(
        f.routes.handle(
            &f.conn,
            &request("GET", "http://[bad/events/v2", None, false)
        ),
        Err(HandlerError::Failure)
    ));
}

#[test]
fn query_helpers_and_pagination_keep_blanks_duplicates_and_decoding() {
    let decoded = comandos_core::dashboard_access::query_pairs(
        "after=&after=%31&turns=%31&x=%FF&x=a+b&flag",
        false,
    );
    assert_eq!(
        decoded,
        vec![
            ("after".into(), "1".into()),
            ("turns".into(), "1".into()),
            ("x".into(), "�".into()),
            ("x".into(), "a b".into())
        ]
    );
    assert_eq!(
        comandos_core::dashboard_access::query_pairs("after=&flag", true),
        vec![("after".into(), "".into()), ("flag".into(), "".into())]
    );
    let cases: Value =
        serde_json::from_str(include_str!("fixtures/events_routes_query.json")).unwrap();
    let mut f = Fixture::new();
    for row in cases.as_array().unwrap() {
        let target = format!("/events/v2?{}", row["query"].as_str().unwrap());
        let (status, body) = f.call("GET", &target, None, false);
        assert_eq!(status, row["status"].as_u64().unwrap() as u16, "{target}");
        if status == 400 {
            assert_eq!(body["error"], row["error"], "{target}");
        } else {
            assert_eq!(body["nextAfter"], row["after"], "{target}");
            assert_eq!(
                body.get("turns").is_some(),
                row["turns"] == true,
                "{target}"
            );
        }
    }
}

#[test]
fn legacy_import_retries_after_failure_and_is_once_across_adapter_restart() {
    let mut overflow = Fixture::new();
    std::fs::write(
        &overflow.path,
        r#"{"project":"old","status":"done","ts":1e20}
"#,
    )
    .unwrap();
    assert!(matches!(
        overflow
            .routes
            .handle(&overflow.conn, &request("GET", "/events/v2", None, false)),
        Err(HandlerError::Failure)
    ));
    assert_eq!(comandos_store::latest_sequence(&overflow.conn).unwrap(), 0);
    let markers: i64 = overflow
        .conn
        .query_row(
            "SELECT COUNT(*) FROM workspace_meta WHERE key = ?",
            [comandos_runtime::legacy::LEGACY_MARKER],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(markers, 0);
    let mut f = Fixture::new();
    std::fs::create_dir(&f.path).unwrap();
    assert!(matches!(
        f.routes
            .handle(&f.conn, &request("GET", "/events/v2", None, false)),
        Err(HandlerError::Failure)
    ));
    std::fs::remove_dir(&f.path).unwrap();
    std::fs::write(
        &f.path,
        "{\"project\":\"old\",\"status\":\"done\",\"detail\":\"viejo\",\"ts\":10}\n",
    )
    .unwrap();
    let (_, body) = f.call("GET", "/events/v2", None, false);
    assert_eq!(body["events"].as_array().unwrap().len(), 1);
    assert_eq!(body["events"][0]["destination"], "none");
    assert_eq!(body["events"][0]["evidence"], "historical");
    assert_eq!(body["nextAfter"], 1);
    assert_eq!(body["latest"], 1);
    std::fs::write(
        &f.path,
        "{\"project\":\"new\",\"status\":\"done\",\"ts\":11}\n",
    )
    .unwrap();
    f.routes = EventRoutes::new(f.path.clone(), FixtureFacts { id: 50 });
    let (_, body) = f.call("GET", "/events/v2?after=1", None, false);
    assert_eq!(body["events"], json!([]));
    assert_eq!(body["nextAfter"], 1);
    assert_eq!(body["latest"], 1);
}

#[test]
fn event_post_denies_noninternal_producers_before_import_or_mutation() {
    let mut f = Fixture::new();
    std::fs::write(
        &f.path,
        "{\"project\":\"old\",\"status\":\"done\",\"ts\":10}\n",
    )
    .unwrap();
    assert_eq!(
        f.call("POST", "/events/v2", Some(hook("Stop", "t1")), false),
        (
            403,
            json!({"error":"Solo productores internos de este equipo"})
        )
    );
    assert_eq!(comandos_store::latest_sequence(&f.conn).unwrap(), 0);
    let count: i64 = f
        .conn
        .query_row(
            "SELECT COUNT(*) FROM workspace_meta WHERE key = ?",
            [comandos_runtime::legacy::LEGACY_MARKER],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 0);
}

#[test]
fn event_posts_page_normalized_events_and_ignore_unrelated_hooks() {
    let mut f = Fixture::new();
    assert_eq!(
        f.call("POST", "/events/v2", Some(json!({"hookEvent":{}})), true),
        (400, json!({"error":"unhashable type: 'dict'"}))
    );
    assert_eq!(
        f.call(
            "POST",
            "/events/v2",
            Some(json!({"hookEvent":"Notification","notificationType":[]})),
            true
        ),
        (400, json!({"error":"unhashable type: 'list'"}))
    );
    assert_eq!(
        f.call("POST", "/events/v2", Some(hook("Unknown", "t1")), true),
        (202, json!({"ignored":true}))
    );
    let (status, prompt) = f.call(
        "POST",
        "/events/v2",
        Some(hook("UserPromptSubmit", "t1")),
        true,
    );
    assert_eq!(status, 200);
    assert_eq!(prompt["event"]["kind"], "prompt_accepted");
    let (status, _) = f.call("POST", "/events/v2", Some(hook("Stop", "t1")), true);
    assert_eq!(status, 200);
    let (_, duplicate) = f.call("POST", "/events/v2", Some(hook("Stop", "t1")), true);
    assert_eq!(duplicate["event"]["duplicate"], true);
    let (_, page) = f.call("GET", "/events/v2?after=0&limit=1&turns=1", None, false);
    assert_eq!(page["events"].as_array().unwrap().len(), 1);
    assert_eq!(page["nextAfter"], 1);
    assert_eq!(page["latest"], 2);
    assert_eq!(page["turns"]["tmux:sess:%3"]["state"], "completed");
    assert_eq!(page["turns"]["tmux:sess:%3"]["turnId"], "t1");
    let (status, bad) = f.call(
        "POST",
        "/events/v2",
        Some(json!({"source":"x","kind":"unknown"})),
        true,
    );
    assert_eq!(status, 400);
    assert_eq!(bad["error"], "kind desconocido");
}

#[test]
fn confirmed_prompt_reopens_resolved_marks_only_once_with_real_binding_identity() {
    let mut f = Fixture::new();
    let document = json!({"schema":1,"groups":[{"id":"g","tree":{"type":"tab","tabId":"sess"}}],"tabs":{"sess":{"session":"sess","paneKeys":["pk1"]}},"bindings":{"pk1":{"session":"sess","paneId":"%3","pid":123,"startTime":42}}});
    f.conn
        .execute(
            "INSERT INTO workspace_current VALUES(1,1,?,0)",
            [document.to_string()],
        )
        .unwrap();
    for (scope, key) in [("pane", "pk1"), ("session", "sess")] {
        assert_eq!(
            f.call(
                "POST",
                "/work-marks",
                Some(json!({"scope":scope,"key":key,"value":"resolved","expectedRevision":0})),
                false
            )
            .0,
            200
        );
    }
    let mut prompt = hook("UserPromptSubmit", "t1");
    prompt["panePid"] = json!("123");
    let (_, stored) = f.call("POST", "/events/v2", Some(prompt.clone()), true);
    assert_eq!(stored["event"]["paneKey"], "pk1");
    assert_eq!(stored["event"]["processKey"], "123-42");
    assert_eq!(
        marks::get_mark(&f.conn, "pane", "pk1").unwrap()["mark"],
        "none"
    );
    assert_eq!(
        marks::get_mark(&f.conn, "session", "sess").unwrap()["mark"],
        "none"
    );
    f.call(
        "POST",
        "/work-marks",
        Some(json!({"scope":"pane","key":"pk1","value":"resolved","expectedRevision":2})),
        false,
    );
    let (_, duplicate) = f.call("POST", "/events/v2", Some(prompt), true);
    assert_eq!(duplicate["event"]["duplicate"], true);
    assert_eq!(
        marks::get_mark(&f.conn, "pane", "pk1").unwrap()["mark"],
        "resolved"
    );
    let (_, payload) = f.call("GET", "/work-marks", None, false);
    assert_eq!(
        payload["panes"],
        json!([{"paneKey":"pk1","session":"sess","paneId":"%3"}])
    );
    assert_eq!(
        payload["activity"]["pane:pk1"],
        json!({"state":"working","paneKey":"pk1","session":"sess","paneId":"%3"})
    );
}

#[test]
fn work_mark_validation_and_conflicts_preserve_status_and_current_state() {
    let mut f = Fixture::new();
    let cases: Value =
        serde_json::from_str(include_str!("fixtures/events_routes_marks.json")).unwrap();
    for row in cases.as_array().unwrap() {
        let (status, body) = f.call("POST", "/work-marks", Some(row["data"].clone()), false);
        assert_eq!(
            status,
            row["status"].as_u64().unwrap() as u16,
            "{}",
            row["data"]
        );
        assert_eq!(body["error"], row["error"], "{}", row["data"]);
        if status == 409 {
            assert_eq!(body["current"]["revision"], 0);
            assert_eq!(body["current"]["mark"], "none");
        }
    }
    let (status, saved) = f.call("POST", "/work-marks", Some(json!({"scope":"pane","key":"pk","value":{"mark":"frozen","favorite":true},"expectedRevision":0})), false);
    assert_eq!(status, 200);
    assert_eq!(saved["mark"]["updatedAtMs"], NOW);
    assert_eq!(saved["mark"]["favorite"], true);
    let (status, conflict) = f.call(
        "POST",
        "/work-marks",
        Some(json!({"scope":"pane","key":"pk","value":"none","expectedRevision":0})),
        false,
    );
    assert_eq!(status, 409);
    assert_eq!(conflict["current"], saved["mark"]);
}

#[test]
fn turns_use_the_last_two_thousand_sequences_and_preserve_target_order() {
    let mut f = Fixture::new();
    f.seed("old", json!({"source":"test","kind":"prompt_accepted","evidence":"confirmed","paneKey":"old","occurredAtMs":1}));
    for index in 0..2000 {
        f.seed(
            &format!("middle-{index}"),
            json!({"source":"test","kind":"announcement","evidence":"confirmed","occurredAtMs":2}),
        );
    }
    for key in ["z", "a"] {
        f.seed(key, json!({"source":"test","kind":"prompt_accepted","evidence":"confirmed","paneKey":key,"occurredAtMs":3}));
    }
    let (_, body) = f.call(
        "GET",
        "/events/v2?after=999999999999999&turns=1",
        None,
        false,
    );
    assert_eq!(body["events"], json!([]));
    assert_eq!(body["nextAfter"], 999_999_999_999_999_u64);
    assert_eq!(body["latest"], 2003);
    assert!(body["turns"].get("pane:old").is_none());
    assert_eq!(
        body["turns"]
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        vec!["pane:z", "pane:a"]
    );
}

#[test]
fn database_failures_propagate_without_turning_sql_diagnostics_into_json() {
    let mut f = Fixture::new();
    assert_eq!(f.call("GET", "/work-marks", None, false).0, 200);
    let oversized = json!({"source":"fixture","kind":"announcement","occurredAtMs":9_223_372_036_854_775_808_u64});
    assert!(matches!(
        f.routes.handle(
            &f.conn,
            &request("POST", "/events/v2", Some(oversized), true)
        ),
        Err(HandlerError::Failure)
    ));
    assert_eq!(comandos_store::latest_sequence(&f.conn).unwrap(), 0);
    for raw in [
        r#"{"source":"fixture","kind":"announcement","occurredAtMs":18446744073709551616}"#,
        r#"{"source":"fixture","kind":"announcement","receivedAtMs":18446744073709551616}"#,
        r#"{"source":"fixture","kind":"announcement","occurredAtMs":Infinity}"#,
        r#"{"hookEvent":"Stop","occurredAtMs":18446744073709551616}"#,
    ] {
        let input = comandos_core::json::workspace_loads(raw).unwrap();
        assert!(
            matches!(
                f.routes
                    .handle(&f.conn, &request("POST", "/events/v2", Some(input), true)),
                Err(HandlerError::Failure)
            ),
            "{raw}"
        );
        assert_eq!(comandos_store::latest_sequence(&f.conn).unwrap(), 0);
    }
    f.conn
        .execute(
            "INSERT INTO work_marks VALUES('pane','full','none',0,?,0,'user')",
            [i64::MAX],
        )
        .unwrap();
    let exhausted =
        json!({"scope":"pane","key":"full","value":"resolved","expectedRevision":i64::MAX});
    assert!(matches!(
        f.routes.handle(
            &f.conn,
            &request("POST", "/work-marks", Some(exhausted), false)
        ),
        Err(HandlerError::Failure)
    ));
    assert_eq!(
        marks::get_mark(&f.conn, "pane", "full").unwrap()["revision"],
        i64::MAX
    );
    f.conn.execute_batch("DROP TABLE work_marks").unwrap();
    assert!(matches!(
        f.routes
            .handle(&f.conn, &request("GET", "/work-marks", None, false)),
        Err(HandlerError::Failure)
    ));
}

#[test]
fn frozen_original_route_and_parser_observations_match_exactly() {
    let reference: Value =
        serde_json::from_str(include_str!("fixtures/events_routes_reference.json")).unwrap();
    let mut f = Fixture::new();
    let steps = reference["steps"].as_array().unwrap();
    assert_eq!(
        steps.len(),
        8,
        "the sandbox source oracle must generate tracked observations before RED"
    );
    for row in reference["splits"].as_array().unwrap() {
        let parts =
            comandos_core::dashboard_access::request_target_parts(row["target"].as_str().unwrap());
        assert_eq!(
            serde_json::to_value(parts.map(|(path, query)| vec![path, query])).unwrap(),
            row["parts"]
        );
    }
    for step in steps {
        let data = step
            .get("rawData")
            .and_then(Value::as_str)
            .map(|raw| comandos_core::json::workspace_loads(raw).unwrap())
            .or_else(|| step.get("data").cloned());
        let (status, body) = f.call(
            step["method"].as_str().unwrap(),
            step["target"].as_str().unwrap(),
            data,
            step["internal"].as_bool().unwrap(),
        );
        assert_eq!(status, step["status"].as_u64().unwrap() as u16);
        assert_eq!(body, step["body"], "{} {}", step["method"], step["target"]);
    }
    for row in reference["pairs"].as_array().unwrap() {
        assert_eq!(
            serde_json::to_value(comandos_core::dashboard_access::query_pairs(
                row["query"].as_str().unwrap(),
                row["keepBlank"].as_bool().unwrap()
            ))
            .unwrap(),
            row["pairs"]
        );
    }
}
