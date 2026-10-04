use comandos_core::{focus as cf, pomodoro as cp};
use comandos_store::{
    focus as f,
    pomodoro::{self as p, Error, PomodoroStore},
    state,
};
use rusqlite::Connection;
use serde_json::{Value, json};
use std::{
    cell::Cell,
    path::PathBuf,
    sync::{
        Arc, Barrier,
        atomic::{AtomicUsize, Ordering},
    },
};
const MIN: i64 = 60_000;
const T0: i64 = 2_000_000_000_000;
struct Db {
    path: PathBuf,
    conn: Connection,
}
impl Db {
    fn new() -> Self {
        static N: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "pomodoro-{}-{}.sqlite3",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        let conn = state::connect(&path).unwrap();
        state::migrate(&conn, state::MIGRATIONS, T0 as f64 / 1000.0).unwrap();
        Self { path, conn }
    }
}
impl Drop for Db {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
        let _ = std::fs::remove_file(format!("{}-wal", self.path.display()));
        let _ = std::fs::remove_file(format!("{}-shm", self.path.display()));
    }
}
fn start(s: &PomodoroStore<'_>, rid: &str, minutes: i64) -> Value {
    s.command(&json!({"requestId":rid,"expectedRevision":s.snapshot().unwrap()["revision"],"action":"start","targetMs":minutes*MIN,"project":"ComandOS","sessionKey":"s","paneKey":"p"})).unwrap()
}
fn act(s: &PomodoroStore<'_>, rid: &str, action: &str) -> Value {
    s.command(&json!({"requestId":rid,"expectedRevision":s.snapshot().unwrap()["revision"],"action":action})).unwrap()
}
fn code(e: Error) -> String {
    match e {
        Error::Domain(e) => e.code,
        other => panic!("unexpected {other}"),
    }
}
fn count(conn: &Connection, table: &str) -> i64 {
    conn.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
        .unwrap()
}
#[test]
fn confirmed_start_snapshot_and_nonmutating_read() {
    let db = Db::new();
    let now = Cell::new(T0);
    let clock = || now.get();
    let id = || "a".into();
    let s = PomodoroStore::new(&db.conn, &clock, &id);
    let out = start(&s, "r", 25);
    assert_eq!(out["revision"], 1);
    assert_eq!(out["block"]["deadlineMs"], T0 + 25 * MIN);
    assert_eq!(s.next_deadline_ms().unwrap(), Some(T0 + 25 * MIN));
    now.set(T0 + 30 * MIN);
    assert_eq!(s.snapshot().unwrap()["block"]["status"], "running");
    assert_eq!(count(&db.conn, "pomodoro_records"), 0);
}
#[test]
fn pause_resume_extend_cancel_measures_actual_work() {
    let db = Db::new();
    let now = Cell::new(T0);
    let clock = || now.get();
    let id = || "a".into();
    let s = PomodoroStore::new(&db.conn, &clock, &id);
    start(&s, "r", 25);
    now.set(T0 + 2 * MIN);
    assert_eq!(act(&s, "p", "pause")["block"]["activeMs"], 2 * MIN);
    assert_eq!(s.next_deadline_ms().unwrap(), None);
    now.set(T0 + 62 * MIN);
    assert_eq!(
        act(&s, "res", "resume")["block"]["deadlineMs"],
        T0 + 85 * MIN
    );
    now.set(T0 + 63 * MIN);
    let e = s
        .command(&json!({"requestId":"ex","action":"extend","deltaMs":5*MIN,"project":"Other"}))
        .unwrap();
    assert_eq!(e["block"]["targetMs"], 30 * MIN);
    assert_eq!(e["block"]["project"], "ComandOS");
    now.set(T0 + 64 * MIN);
    assert_eq!(act(&s, "c", "cancel")["block"]["activeMs"], 4 * MIN);
    let r = p::records(&db.conn, None, None, None).unwrap();
    assert_eq!(r[0]["activeMs"], 4 * MIN);
    assert_eq!(r[0]["status"], "cancelled");
}
#[test]
fn validation_codes_control_chars_and_target_limits() {
    let db = Db::new();
    let clock = || T0;
    let id = || "a".into();
    let s = PomodoroStore::new(&db.conn, &clock, &id);
    for target in [
        json!(0),
        json!(30_000),
        json!(181 * MIN),
        json!("25"),
        Value::Null,
        json!(true),
    ] {
        assert_eq!(
            code(
                s.command(&json!({"requestId":"bad","action":"start","targetMs":target}))
                    .unwrap_err()
            ),
            "invalid_targetMs"
        );
    }
    for field in ["project", "sessionKey", "paneKey"] {
        let mut r = json!({"requestId":"bad","action":"start","targetMs":MIN});
        r[field] = json!("a\u{1}b");
        assert_eq!(code(s.command(&r).unwrap_err()), format!("invalid_{field}"));
    }
    assert_eq!(start(&s, "ok", 180)["block"]["targetMs"], 180 * MIN);
}
#[test]
fn replay_ignores_revision_preserves_original_block_and_digest() {
    let db = Db::new();
    let now = Cell::new(T0);
    let clock = || now.get();
    let id = || "a".into();
    let s = PomodoroStore::new(&db.conn, &clock, &id);
    let req = json!({"requestId":"r","expectedRevision":0,"action":"start","targetMs":25*MIN,"project":"México 😊","extra":{"z":1,"a":1.0}});
    let first = s.command(&req).unwrap();
    act(&s, "p", "pause");
    now.set(T0 + 100);
    let mut req = req;
    req["expectedRevision"] = json!(999);
    let again = s.command(&req).unwrap();
    assert_eq!(again["block"], first["block"]);
    assert_eq!(again["revision"], 1);
    assert_eq!(again["serverNowMs"], T0 + 100);
    assert_eq!(again["replayed"], true);
    req["targetMs"] = json!(MIN);
    assert_eq!(code(s.command(&req).unwrap_err()), "request_reused");
}
#[test]
fn settlement_commits_before_invalid_command_and_only_direct_revision_survives() {
    let db = Db::new();
    let now = Cell::new(T0);
    let clock = || now.get();
    let ids = Cell::new(0);
    let id = || {
        ids.set(ids.get() + 1);
        format!("b{}", ids.get())
    };
    let s = PomodoroStore::new(&db.conn, &clock, &id);
    start(&s, "a", 1);
    now.set(T0 + MIN);
    let err = s
        .command(&json!({"requestId":"bad","action":"start","expectedRevision":1,"targetMs":0}))
        .unwrap_err();
    assert_eq!(code(err), "invalid_targetMs");
    assert_eq!(s.snapshot().unwrap()["block"]["status"], "completed");
    assert_eq!(count(&db.conn, "pomodoro_records"), 1);
    let next = s
        .command(&json!({"requestId":"next","action":"start","expectedRevision":1,"targetMs":MIN}))
        .unwrap();
    assert_eq!(next["revision"], 3);
    act(&s, "p", "pause");
    assert_eq!(
        code(
            s.command(&json!({"requestId":"stale","action":"cancel","expectedRevision":3}))
                .unwrap_err()
        ),
        "stale_revision"
    );
}
#[test]
fn reward_event_callback_failure_rolls_back_and_retry_is_exactly_once() {
    let db = Db::new();
    let now = Cell::new(T0);
    let clock = || now.get();
    let id = || "a".into();
    let policy = cf::policy_v1();
    f::ensure_policy(&db.conn, &policy, T0).unwrap();
    let reward = |c: &Connection, r: &Value, t: i64| {
        assert!(!c.is_autocommit());
        f::award(c, r, &policy, t)?;
        Ok(())
    };
    let fail = Cell::new(true);
    let emit = |c: &Connection, e: &Value| {
        assert!(!c.is_autocommit());
        comandos_store::append_event(c, e, now.get() as u64, "unused", "receipt")?;
        if fail.replace(false) {
            return Err(comandos_store::Error::Validation("crash".into()).into());
        }
        Ok(())
    };
    let mut s = PomodoroStore::new(&db.conn, &clock, &id);
    s.rewards = Some(&reward);
    s.emit = Some(&emit);
    start(&s, "r", 25);
    now.set(T0 + 25 * MIN);
    assert!(s.settle_due(None).is_err());
    assert_eq!(s.snapshot().unwrap()["block"]["status"], "running");
    for t in [
        "pomodoro_records",
        "focus_rewards",
        "events",
        "event_receipts",
    ] {
        assert_eq!(count(&db.conn, t), 0);
    }
    assert_eq!(s.settle_due(None).unwrap(), ["a"]);
    assert!(s.settle_due(None).unwrap().is_empty());
    for t in ["pomodoro_records", "focus_rewards", "events"] {
        assert_eq!(count(&db.conn, t), 1);
    }
    assert_eq!(
        f::ledger_progress(&db.conn, &policy, now.get()).unwrap()["xp"],
        250
    );
}
#[test]
fn caller_transaction_is_rejected_before_timer_mutation() {
    let db = Db::new();
    let clock = || T0;
    let id = || "a".into();
    let s = PomodoroStore::new(&db.conn, &clock, &id);
    db.conn
        .execute_batch("BEGIN IMMEDIATE; INSERT INTO focus_policies VALUES ('sentinel','{}',1)")
        .unwrap();
    assert!(
        s.command(&json!({"requestId":"r","action":"start","targetMs":25*MIN}))
            .is_err()
    );
    assert!(s.settle_due(None).is_err());
    assert!(!db.conn.is_autocommit());
    assert_eq!(count(&db.conn, "focus_policies"), 1);
    assert_eq!(s.snapshot().unwrap()["revision"], 0);
    db.conn.execute_batch("ROLLBACK").unwrap();
    assert_eq!(count(&db.conn, "focus_policies"), 0);
}
#[test]
fn disk_reopen_and_parallel_completion_keep_one_record() {
    let db = Db::new();
    let clock = || T0;
    let id = || "a".into();
    start(&PomodoroStore::new(&db.conn, &clock, &id), "r", 1);
    let barrier = Arc::new(Barrier::new(3));
    let handles = (0..3)
        .map(|_| {
            let path = db.path.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                let conn = state::connect(&path).unwrap();
                let clock = || T0 + MIN + 1;
                let id = || "unused".into();
                let s = PomodoroStore::new(&conn, &clock, &id);
                assert_eq!(s.snapshot().unwrap()["block"]["status"], "running");
                barrier.wait();
                s.settle_due(None).unwrap().len()
            })
        })
        .collect::<Vec<_>>();
    assert_eq!(
        handles
            .into_iter()
            .map(|h| h.join().unwrap())
            .sum::<usize>(),
        1
    );
    assert_eq!(count(&db.conn, "pomodoro_records"), 1);
}
#[test]
fn legacy_import_report_separates_measured_and_planned_and_adopted() {
    let db = Db::new();
    let now = Cell::new(T0);
    let clock = || now.get();
    let id = || "a".into();
    let s = PomodoroStore::new(&db.conn, &clock, &id);
    let legacy = json!({"blockId":"live","until":(T0+10*MIN)as f64/1000.0,"startedAt":(T0-15*MIN)as f64/1000.0,"mins":25,"project":"Lola"});
    assert!(s.import_legacy_focus(&legacy, None).unwrap());
    assert!(!s.import_legacy_focus(&legacy, None).unwrap());
    let rows = vec![
        json!({"id":"live","started_at_ms":T0,"planned_minutes":25}),
        json!({"id":"old","started_at_ms":T0,"planned_minutes":25,"status":"completed","project":"MRP"}),
        json!({"id":"unknown","started_at_ms":T0,"planned_minutes":50,"status":"running","project":"MRP"}),
    ];
    assert_eq!(s.import_legacy_history(&rows, None).unwrap(), 2);
    assert_eq!(s.import_legacy_history(&rows, None).unwrap(), 0);
    now.set(T0 + 10 * MIN);
    s.settle_due(None).unwrap();
    let report =
        p::focus_report(&db.conn, T0 - 15 * MIN, T0 + 30 * MIN, None, cp::REPORT_TZ).unwrap();
    assert_eq!(report["measured"]["activeMs"], 25 * MIN);
    assert_eq!(report["legacy"]["blocks"], 2);
    assert_eq!(report["legacy"]["plannedMs"], 25 * MIN);
    assert_eq!(report["projects"], json!(["Lola", "MRP"]));
}
#[test]
fn policy_activation_no_retroactive_versions_and_single_level_notice() {
    let db = Db::new();
    let now = Cell::new(T0);
    let clock = || now.get();
    let ids = Cell::new(0);
    let id = || {
        ids.set(ids.get() + 1);
        format!("b{}", ids.get())
    };
    let policy = cf::policy_v1();
    assert_eq!(f::ensure_policy(&db.conn, &policy, T0).unwrap(), T0);
    assert_eq!(f::ensure_policy(&db.conn, &policy, T0 + 1).unwrap(), T0);
    let reward = |c: &Connection, r: &Value, t: i64| {
        f::award(c, r, &policy, t)?;
        Ok(())
    };
    let mut s = PomodoroStore::new(&db.conn, &clock, &id);
    s.rewards = Some(&reward);
    for i in 0..5 {
        start(&s, &format!("r{i}"), 25);
        now.set(now.get() + 25 * MIN);
        s.settle_due(None).unwrap();
    }
    let out = f::ledger_progress(&db.conn, &policy, now.get()).unwrap();
    assert_eq!(out["xp"], 1250);
    assert_eq!(out["lastLevelUp"]["blockId"], "b4");
    assert_eq!(out["lastLevelUp"]["level"], 2);
    assert_eq!(
        db.conn
            .query_row(
                "SELECT COUNT(*) FROM focus_rewards WHERE level_reached IS NOT NULL",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
        1
    );
    let mut trial = policy.clone();
    trial["policyVersion"] = json!("trial");
    trial["xpPerMinute"] = json!(1);
    f::ensure_policy(&db.conn, &trial, T0).unwrap();
    let records = p::records(&db.conn, None, None, None).unwrap();
    assert_eq!(
        f::award(&db.conn, &records[0], &trial, now.get())
            .unwrap()
            .unwrap()["xp"],
        25
    );
    assert!(
        f::award(&db.conn, &records[0], &trial, now.get())
            .unwrap()
            .is_none()
    );
    assert_eq!(
        f::ledger_progress(&db.conn, &trial, now.get()).unwrap()["xp"],
        25
    );
    let mut later = policy.clone();
    later["policyVersion"] = json!("later");
    f::ensure_policy(&db.conn, &later, now.get() + 1).unwrap();
    assert!(
        f::award(&db.conn, &records[0], &later, now.get())
            .unwrap()
            .is_none()
    );
}
#[test]
fn fixed_clock_commands_reports_digest_and_ledger_match_python_reference() {
    let fixture: Value = serde_json::from_str(include_str!("pomodoro_focus_fixture.json")).unwrap();
    let db = Db::new();
    let now = Cell::new(T0);
    let clock = || now.get();
    let ids = Cell::new(0);
    let id = || {
        ids.set(ids.get() + 1);
        format!("b{}", ids.get())
    };
    let policy = cf::policy_v1();
    f::ensure_policy(&db.conn, &policy, T0).unwrap();
    let reward = |c: &Connection, r: &Value, t: i64| {
        f::award(c, r, &policy, t)?;
        Ok(())
    };
    let mut s = PomodoroStore::new(&db.conn, &clock, &id);
    s.rewards = Some(&reward);
    for (i, step) in fixture["steps"].as_array().unwrap().iter().enumerate() {
        now.set(step["now"].as_i64().unwrap());
        let actual = match s.command(&step["request"]) {
            Ok(out) => out,
            Err(Error::Domain(e)) => json!({"status":e.status,"payload":e.payload()}),
            Err(e) => panic!("step {i}: {e}"),
        };
        assert_eq!(actual, step["result"], "command step {i}");
        assert_eq!(s.snapshot().unwrap(), step["snapshot"], "snapshot step {i}");
    }
    assert_eq!(
        s.import_legacy_history(fixture["history"].as_array().unwrap(), None)
            .unwrap(),
        fixture["imported"].as_u64().unwrap() as usize
    );
    assert_eq!(
        json!(p::records(&db.conn, None, None, None).unwrap()),
        fixture["records"]
    );
    for r in fixture["reports"].as_array().unwrap() {
        assert_eq!(
            p::focus_report(
                &db.conn,
                r["from"].as_i64().unwrap(),
                r["to"].as_i64().unwrap(),
                r["project"].as_str(),
                r["timezone"].as_str().unwrap()
            )
            .unwrap(),
            r["report"]
        );
    }
    assert_eq!(
        f::ledger_progress(&db.conn, &policy, now.get()).unwrap(),
        fixture["progress"]
    );
    for d in fixture["digests"].as_array().unwrap() {
        assert_eq!(
            PomodoroStore::digest(&d["request"]).unwrap(),
            d["digest"].as_str().unwrap()
        );
    }
}
#[test]
fn legacy_sqlite_overflow_rolls_back_entire_batch_instead_of_skipping() {
    let db = Db::new();
    let clock = || T0;
    let id = || "a".into();
    let s = PomodoroStore::new(&db.conn, &clock, &id);
    for field in ["planned_minutes", "started_at_ms", "ended_at_ms"] {
        let mut bad =
            json!({"id":"overflow","started_at_ms":T0,"planned_minutes":25,"status":"completed"});
        bad[field] = serde_json::from_str("99999999999999999999999999999999999999").unwrap();
        let rows = [
            json!({"id":"before","started_at_ms":T0,"planned_minutes":25}),
            bad,
        ];
        assert!(
            s.import_legacy_history(&rows, None).is_err(),
            "field {field}"
        );
        assert_eq!(count(&db.conn, "pomodoro_records"), 0);
    }
}
#[test]
fn due_completion_survives_request_insert_trigger_failure() {
    let db = Db::new();
    let now = Cell::new(T0);
    let clock = || now.get();
    let ids = Cell::new(0);
    let id = || {
        ids.set(ids.get() + 1);
        format!("b{}", ids.get())
    };
    let s = PomodoroStore::new(&db.conn, &clock, &id);
    start(&s, "a", 1);
    db.conn.execute_batch("CREATE TRIGGER no_request BEFORE INSERT ON pomodoro_requests BEGIN SELECT RAISE(ABORT,'request crash'); END").unwrap();
    now.set(T0 + MIN);
    assert!(
        s.command(&json!({"requestId":"b","action":"start","expectedRevision":1,"targetMs":MIN}))
            .is_err()
    );
    assert_eq!(s.snapshot().unwrap()["revision"], 2);
    assert_eq!(s.snapshot().unwrap()["block"]["status"], "completed");
    assert_eq!(count(&db.conn, "pomodoro_blocks"), 1);
    assert_eq!(count(&db.conn, "pomodoro_records"), 1);
    db.conn.execute_batch("DROP TRIGGER no_request").unwrap();
    assert_eq!(
        s.command(&json!({"requestId":"b","action":"start","expectedRevision":1,"targetMs":MIN}))
            .unwrap()["revision"],
        3
    );
}
#[test]
fn request_expiration_is_strict_and_existing_expired_replays_before_prune() {
    let db = Db::new();
    let now = Cell::new(T0);
    let clock = || now.get();
    let ids = Cell::new(0);
    let id = || {
        ids.set(ids.get() + 1);
        format!("b{}", ids.get())
    };
    let s = PomodoroStore::new(&db.conn, &clock, &id);
    let first = start(&s, "first", 1);
    now.set(T0 + cp::REQUEST_TTL_MS);
    start(&s, "boundary", 1);
    assert_eq!(
        db.conn
            .query_row(
                "SELECT COUNT(*) FROM pomodoro_requests WHERE request_id='first'",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
        1
    );
    now.set(now.get() + 1);
    let req = json!({"requestId":"first","expectedRevision":0,"action":"start","targetMs":MIN,"project":"ComandOS","sessionKey":"s","paneKey":"p"});
    assert_eq!(s.command(&req).unwrap()["block"], first["block"]);
    act(&s, "pause", "pause");
    assert_eq!(
        db.conn
            .query_row(
                "SELECT COUNT(*) FROM pomodoro_requests WHERE request_id='first'",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
        0
    );
}
#[test]
fn report_limits_history_ties_and_projects_order_and_empty_range() {
    let db = Db::new();
    let clock = || T0;
    let id = || "a".into();
    let s = PomodoroStore::new(&db.conn, &clock, &id);
    let rows=(0..205).map(|i|json!({"id":format!("b{i:03}"),"started_at_ms":T0,"planned_minutes":25,"project":if i%2==0{"Zulu"}else{"Alpha"},"status":"completed"})).collect::<Vec<_>>();
    s.import_legacy_history(&rows, None).unwrap();
    let r = p::focus_report(&db.conn, T0 - 1, T0 + 450 * 86_400_000, None, cp::REPORT_TZ).unwrap();
    assert_eq!(r["history"].as_array().unwrap().len(), 200);
    assert_eq!(r["byDay"].as_array().unwrap().len(), 400);
    assert_eq!(r["history"][0]["blockId"], "b000");
    assert_eq!(r["history"][199]["blockId"], "b199");
    assert_eq!(r["projects"], json!(["Alpha", "Zulu"]));
    assert_eq!(r["byProject"][0]["project"], "Zulu");
    assert!(p::focus_report(&db.conn, T0, T0, None, cp::REPORT_TZ).is_err());
    assert!(p::focus_report(&db.conn, T0, T0 + MIN, None, "Invalid/Zone").is_err());
}
#[test]
fn clock_overflow_fails_transactionally_and_huge_expected_revision_conflicts() {
    let db = Db::new();
    let clock = || i64::MAX - 100;
    let id = || "a".into();
    let s = PomodoroStore::new(&db.conn, &clock, &id);
    assert!(
        s.command(&json!({"requestId":"overflow","action":"start","targetMs":MIN}))
            .is_err()
    );
    assert_eq!(count(&db.conn, "pomodoro_blocks"), 0);
    assert_eq!(count(&db.conn, "pomodoro_requests"), 0);
    let huge: Value = serde_json::from_str("99999999999999999999999999999999999999999999").unwrap();
    assert_eq!(
        code(
            s.command(
                &json!({"requestId":"huge","action":"start","expectedRevision":huge,"targetMs":MIN})
            )
            .unwrap_err()
        ),
        "stale_revision"
    );
}
#[test]
fn simultaneous_starts_and_same_request_retries_preserve_one_authority() {
    for same in [false, true] {
        let db = Db::new();
        let barrier = Arc::new(Barrier::new(4));
        let handles=(0..4).map(|i|{let path=db.path.clone();let barrier=barrier.clone();std::thread::spawn(move||{let conn=state::connect(&path).unwrap();let clock=||T0;let id=||format!("b{i}");let s=PomodoroStore::new(&conn,&clock,&id);barrier.wait();s.command(&json!({"requestId":if same{"same".to_string()}else{format!("r{i}")},"expectedRevision":0,"action":"start","targetMs":MIN}))})}).collect::<Vec<_>>();
        let results = handles
            .into_iter()
            .map(|h| h.join().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(count(&db.conn, "pomodoro_blocks"), 1);
        if same {
            let results = results.into_iter().map(|r| r.unwrap()).collect::<Vec<_>>();
            assert_eq!(results.iter().filter(|r| r["replayed"] == false).count(), 1);
            for r in &results {
                assert_eq!(r["block"]["blockId"], results[0]["block"]["blockId"]);
            }
        } else {
            assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
            for result in results.into_iter().filter_map(Result::err) {
                assert_eq!(code(result), "stale_revision");
            }
        }
    }
}
#[test]
fn break_and_cancel_reward_policy_and_sanitized_legacy_event_identifiers() {
    let db = Db::new();
    let now = Cell::new(T0);
    let clock = || now.get();
    let ids = Cell::new(0);
    let id = || {
        ids.set(ids.get() + 1);
        format!("b{}", ids.get())
    };
    let policy = cf::policy_v1();
    f::ensure_policy(&db.conn, &policy, T0).unwrap();
    let reward = |c: &Connection, r: &Value, t: i64| {
        f::award(c, r, &policy, t)?;
        Ok(())
    };
    let emit = |c: &Connection, e: &Value| {
        assert_eq!(e["projectKey"], Value::Null);
        assert_eq!(e["sessionKey"], Value::Null);
        assert_eq!(e["excerpt"], "1 min de foco · sin proyecto");
        comandos_store::append_event(c, e, now.get() as u64, "unused", "receipt")?;
        Ok(())
    };
    let mut s = PomodoroStore::new(&db.conn, &clock, &id);
    s.rewards = Some(&reward);
    s.command(&json!({"requestId":"break","action":"start","mode":"break","targetMs":MIN}))
        .unwrap();
    now.set(T0 + MIN);
    s.settle_due(None).unwrap();
    assert_eq!(count(&db.conn, "focus_rewards"), 0);
    start(&s, "focus", 25);
    now.set(now.get() + 12 * MIN + 59_999);
    act(&s, "cancel", "cancel");
    assert_eq!(
        f::ledger_progress(&db.conn, &policy, now.get()).unwrap()["xp"],
        120
    );
    let cancelled = p::records(&db.conn, None, None, None).unwrap()[0].clone();
    let mut trial = policy.clone();
    trial["policyVersion"] = json!("trial");
    trial["countCancelledActive"] = json!(false);
    f::ensure_policy(&db.conn, &trial, T0).unwrap();
    assert!(
        f::award(&db.conn, &cancelled, &trial, now.get())
            .unwrap()
            .is_none()
    );
    s.emit = Some(&emit);
    start(&s, "bad-stored", 1);
    db.conn
        .execute_batch(
            "UPDATE pomodoro_blocks SET project='p'||char(9)||'q',session_key='s'||char(1)||'x'",
        )
        .unwrap();
    now.set(now.get() + MIN);
    s.settle_due(None).unwrap();
    assert_eq!(count(&db.conn, "events"), 1);
}
#[test]
fn reward_callback_and_record_trigger_errors_retry_without_any_partial_write() {
    for trigger in [false, true] {
        let db = Db::new();
        let now = Cell::new(T0);
        let clock = || now.get();
        let id = || "a".into();
        let policy = cf::policy_v1();
        f::ensure_policy(&db.conn, &policy, T0).unwrap();
        let fail = Cell::new(!trigger);
        let reward = |c: &Connection, r: &Value, t: i64| {
            f::award(c, r, &policy, t)?;
            if fail.replace(false) {
                return Err(comandos_store::Error::Validation("reward crash".into()).into());
            }
            Ok(())
        };
        let mut s = PomodoroStore::new(&db.conn, &clock, &id);
        s.rewards = Some(&reward);
        start(&s, "r", 1);
        if trigger {
            db.conn.execute_batch("CREATE TRIGGER no_record BEFORE INSERT ON pomodoro_records BEGIN SELECT RAISE(ABORT,'record crash'); END").unwrap();
        }
        now.set(T0 + MIN);
        assert!(s.settle_due(None).is_err());
        assert_eq!(s.snapshot().unwrap()["revision"], 1);
        assert_eq!(s.snapshot().unwrap()["block"]["status"], "running");
        assert_eq!(count(&db.conn, "pomodoro_records"), 0);
        assert_eq!(count(&db.conn, "focus_rewards"), 0);
        if trigger {
            db.conn.execute_batch("DROP TRIGGER no_record").unwrap();
        }
        assert_eq!(s.settle_due(None).unwrap(), ["a"]);
        assert_eq!(count(&db.conn, "focus_rewards"), 1);
    }
}
#[test]
fn callback_unwind_rolls_back_owned_transaction() {
    let db = Db::new();
    let now = Cell::new(T0);
    let clock = || now.get();
    let id = || "a".into();
    let fail = Cell::new(true);
    let emit = |_: &Connection, _: &Value| -> p::Result<()> {
        assert!(!fail.replace(false), "simulated unwind");
        Ok(())
    };
    let mut s = PomodoroStore::new(&db.conn, &clock, &id);
    s.emit = Some(&emit);
    start(&s, "r", 1);
    now.set(T0 + MIN);
    assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| s.settle_due(None))).is_err());
    assert!(db.conn.is_autocommit());
    assert_eq!(s.snapshot().unwrap()["revision"], 1);
    assert_eq!(count(&db.conn, "pomodoro_records"), 0);
    assert_eq!(s.settle_due(None).unwrap(), ["a"]);
}
#[test]
fn expired_malformed_active_duplicate_legacy_and_numeric_import_boundaries() {
    let db = Db::new();
    let clock = || T0;
    let id = || "a".into();
    let s = PomodoroStore::new(&db.conn, &clock, &id);
    for data in [
        Value::Null,
        json!({"until":"soon"}),
        json!({"until":(T0-1000)as f64/1000.0,"startedAt":1,"mins":25}),
        json!({"until":(T0+MIN)as f64/1000.0,"startedAt":1,"mins":181}),
    ] {
        assert!(!s.import_legacy_focus(&data, None).unwrap());
    }
    start(&s, "r", 25);
    assert!(
        !s.import_legacy_focus(
            &json!({"until":(T0+MIN)as f64/1000.0,"startedAt":1,"mins":25}),
            None
        )
        .unwrap()
    );
    let rows = [
        json!({"id":"unicode","started_at_ms":"+٢_٠٠٠","planned_minutes":"٢_٥","ended_at_ms":12.9,"status":"completed"}),
        json!({"id":"bad","started_at_ms":"1__2","planned_minutes":25}),
        json!({"id":"negative","started_at_ms":0,"planned_minutes":"-999999999999999999999999999999999999999","status":"completed"}),
    ];
    assert_eq!(s.import_legacy_history(&rows, None).unwrap(), 2);
    let records = p::records(&db.conn, None, None, None).unwrap();
    assert_eq!(records[0]["startedAtMs"], 2000);
    assert_eq!(records[0]["plannedMs"], 25 * MIN);
    assert_eq!(records[0]["endedAtMs"], 12);
    assert_eq!(records[1]["plannedMs"], 0);
}
#[test]
fn legacy_live_out_of_range_timestamps_error_without_mutation() {
    let db = Db::new();
    let clock = || T0;
    let id = || "a".into();
    let s = PomodoroStore::new(&db.conn, &clock, &id);
    for field in ["until", "startedAt"] {
        let mut data =
            json!({"until":(T0+MIN)as f64/1000.0,"startedAt":(T0-MIN)as f64/1000.0,"mins":1});
        data[field] = json!(1e30);
        assert!(s.import_legacy_focus(&data, None).is_err(), "field {field}");
        assert_eq!(count(&db.conn, "pomodoro_blocks"), 0);
    }
}
#[test]
fn sqlite_request_cutoff_underflow_aborts_command_instead_of_silently_skipping_prune() {
    let db = Db::new();
    let clock = || i64::MIN;
    let id = || "a".into();
    let s = PomodoroStore::new(&db.conn, &clock, &id);
    assert!(
        s.command(&json!({"requestId":"underflow","action":"start","targetMs":MIN}))
            .is_err()
    );
    assert_eq!(count(&db.conn, "pomodoro_blocks"), 0);
    assert_eq!(s.snapshot().unwrap()["revision"], 0);
}
#[test]
fn digest_keeps_python_overflow_nonfinite_and_deeper_request_bytes() {
    use sha2::{Digest, Sha256};
    let overflow:Value=serde_json::from_str(r#"{"requestId":"overflow","action":"start","targetMs":60000,"extra":1e9999,"project":"México 😊"}"#).unwrap();
    let expected = r#"{"action": "start", "extra": Infinity, "project": "M\u00e9xico \ud83d\ude0a", "targetMs": 60000}"#;
    assert_eq!(
        PomodoroStore::digest(&overflow).unwrap(),
        format!("{:x}", Sha256::digest(expected.as_bytes()))
    );
    let mut deep = json!(null);
    for _ in 0..256 {
        deep = json!([deep]);
    }
    let request = json!({"requestId":"deep","action":"start","targetMs":MIN,"extra":deep});
    let expected = format!(
        "{{\"action\": \"start\", \"extra\": {}null{}, \"targetMs\": 60000}}",
        "[".repeat(256),
        "]".repeat(256)
    );
    assert_eq!(
        PomodoroStore::digest(&request).unwrap(),
        format!("{:x}", Sha256::digest(expected.as_bytes()))
    );
    let mut beyond = Value::Null;
    for _ in 0..1001 {
        beyond = Value::Array(vec![beyond]);
    }
    let mut fields = serde_json::Map::new();
    fields.insert("action".into(), json!("start"));
    fields.insert("extra".into(), beyond);
    let beyond = Value::Object(fields);
    assert!(
        PomodoroStore::digest(&beyond)
            .unwrap_err()
            .to_string()
            .contains("nesting limit")
    );
}
#[test]
fn legacy_live_float_strings_accept_python_underscores_and_unicode_digits() {
    for (until, started) in [
        ("2_000_000_060", "2_000_000_000"),
        ("٢٠٠٠٠٠٠٠٦٠", "٢٠٠٠٠٠٠٠٠٠"),
    ] {
        let db = Db::new();
        let clock = || T0;
        let id = || "a".into();
        let s = PomodoroStore::new(&db.conn, &clock, &id);
        assert!(
            s.import_legacy_focus(&json!({"until":until,"startedAt":started,"mins":1}), None)
                .unwrap()
        );
        assert_eq!(s.snapshot().unwrap()["block"]["deadlineMs"], T0 + MIN);
    }
}
#[test]
fn consolidated_numeric_requests_and_legacy_catch_scopes_match_python() {
    let fixture: Value =
        serde_json::from_str(include_str!("pomodoro_focus_numeric_fixture.json")).unwrap();
    for case in fixture.as_array().unwrap() {
        let db = Db::new();
        let clock = || T0;
        let id = || "a".into();
        let s = PomodoroStore::new(&db.conn, &clock, &id);
        if case["prestart"] == true {
            s.command(&json!({"requestId":"start","action":"start","targetMs":25*MIN}))
                .unwrap();
        }
        let data = comandos_core::json::workspace_loads(case["raw"].as_str().unwrap()).unwrap();
        let result = match case["kind"].as_str().unwrap() {
            "live" => s.import_legacy_focus(&data, None).map(|r| json!(r)),
            "history" => s
                .import_legacy_history(data.as_array().unwrap(), None)
                .map(|r| json!(r)),
            "command" => s.command(&data),
            _ => unreachable!(),
        };
        let actual = match result {
            Ok(r) => json!({"ok":true,"result":r}),
            Err(Error::Domain(e)) => {
                json!({"ok":false,"domain":{"status":e.status,"payload":e.payload()}})
            }
            Err(_) => json!({"ok":false}),
        };
        let mut expected = case["expected"].clone();
        expected.as_object_mut().unwrap().remove("exception");
        assert_eq!(actual, expected, "numeric case {}", case["name"]);
        assert_eq!(
            s.snapshot().unwrap(),
            case["snapshot"],
            "numeric snapshot {}",
            case["name"]
        );
        assert_eq!(
            json!(p::records(&db.conn, None, None, None).unwrap()),
            case["records"],
            "numeric records {}",
            case["name"]
        );
    }
}

#[test]
fn decimal_activation_awards_and_huge_end_rolls_back_caller_batch() {
    let db = Db::new();
    let policy = cf::policy_v1();
    f::ensure_policy(&db.conn, &policy, T0).unwrap();
    let block = json!({"blockId":"decimal","mode":"focus","status":"completed","activeMs":MIN,"startedAtMs":T0,"endedAtMs":(T0+MIN) as f64,"provenance":"measured"});
    assert_eq!(
        f::award(&db.conn, &block, &policy, T0 + MIN)
            .unwrap()
            .unwrap()["xp"],
        10
    );
    assert_eq!(
        db.conn
            .query_row("SELECT ended_at_ms FROM focus_rewards", [], |r| r
                .get::<_, i64>(0))
            .unwrap(),
        T0 + MIN
    );
    assert_eq!(f::ledger_progress(&db.conn, &policy, T0).unwrap()["xp"], 10);
    {
        let tx = db.conn.unchecked_transaction().unwrap();
        let mut b = block.clone();
        b["blockId"] = json!("before-huge");
        assert!(f::award(&tx, &b, &policy, T0).unwrap().is_some());
        b["blockId"] = json!("huge");
        b["endedAtMs"] =
            serde_json::from_str("9999999999999999999999999999999999999999999999999999999999")
                .unwrap();
        assert!(matches!(
            f::award(&tx, &b, &policy, T0),
            Err(comandos_store::Error::Validation(_))
        ));
    }
    assert_eq!(count(&db.conn, "focus_rewards"), 1);
    assert!(db.conn.is_autocommit());
}

#[test]
fn unicode_container_text_persists_through_command_and_legacy_import() {
    let db = Db::new();
    let clock = || T0;
    let id = || "a".into();
    let s = PomodoroStore::new(&db.conn, &clock, &id);
    let out = s.command(&json!({"requestId":"unicode","action":"start","targetMs":MIN,"project":["a\u{2028}b"],"sessionKey":["x\u{a0}y"],"paneKey":["p\u{200d}q"]})).unwrap();
    assert_eq!(out["block"]["project"], "['a\\u2028b']");
    assert_eq!(out["block"]["sessionKey"], "['x\\xa0y']");
    assert_eq!(out["block"]["paneKey"], "['p\\u200dq']");
    let rows = [
        json!({"id":["id\u{e0001}"],"status":"completed","started_at_ms":T0,"planned_minutes":1,"project":["a\u{2028}b"],"tmux_session":["x\u{a0}y"],"tmux_pane":["p\u{200d}q"]}),
    ];
    assert_eq!(s.import_legacy_history(&rows, None).unwrap(), 1);
    let records = p::records(&db.conn, None, None, None).unwrap();
    assert_eq!(records[0]["blockId"], "['id\\U000e0001']");
    assert_eq!(records[0]["project"], out["block"]["project"]);
    assert_eq!(records[0]["sessionKey"], out["block"]["sessionKey"]);
    assert_eq!(records[0]["paneKey"], out["block"]["paneKey"]);
}
