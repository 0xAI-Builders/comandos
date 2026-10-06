//! Public writer boundaries on private cached connections, including a mover race.
use comandos_store::{
    migrate::{self, MoveSpec},
    unified, usage, usage_import, usage_read,
};
use rusqlite::{Connection, types::Value as Sql};
use serde_json::{Value, json};
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::PathBuf,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
struct Home(PathBuf);
impl Home {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!(
            "comandos-s4-writers-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&p).unwrap();
        fs::set_permissions(&p, fs::Permissions::from_mode(0o700)).unwrap();
        Self(p)
    }
    fn target(&self) -> PathBuf {
        self.0.join(".local/share/comandos/comandos.sqlite3")
    }
    fn source(&self, domain: &str) -> (MoveSpec, Connection) {
        let spec = migrate::spec_for(&self.0, domain).unwrap();
        let c = if domain == "db-usage" {
            let c = usage::open_usage_db_at(&spec.legacy).unwrap();
            usage::ensure_schema(&c).unwrap();
            c
        } else {
            let c = comandos_store::state::connect(&spec.legacy).unwrap();
            comandos_store::state::migrate(&c, comandos_store::state::MIGRATIONS, 1.0).unwrap();
            c
        };
        (spec, c)
    }
    fn move_source(&self, spec: &MoveSpec) {
        migrate::move_db(
            spec,
            &self.target(),
            &self.0.join(".local/share/comandos/backups/first"),
            4000,
        )
        .unwrap();
    }
}
impl Drop for Home {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn rows(c: &Connection, t: &str) -> Vec<Vec<Sql>> {
    let mut s = c
        .prepare(&format!("SELECT rowid,* FROM {t} ORDER BY rowid"))
        .unwrap();
    let n = s.column_count();
    s.query_map([], |r| (0..n).map(|i| r.get(i)).collect())
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap()
}
fn all(c: &Connection, spec: &MoveSpec) -> Vec<Vec<Vec<Sql>>> {
    spec.tables.iter().map(|(t, _)| rows(c, t)).collect()
}
fn quota() -> Value {
    json!({"id":"late","provider":"claude","account":"private","window":"5h","scope":"test","resets_at":7200,"percent":12,"captured_at":10})
}
#[test]
fn public_quota_and_pane_writers_refuse_cached_moved_source_and_inverse_keeps_reopened_write() {
    let h = Home::new();
    let (spec, cached) = h.source("db-usage");
    h.move_source(&spec);
    let before = all(&cached, &spec);
    let quota_result = usage_read::record_quota_snapshots(&cached, &[quota()], 10);
    let pane_result = usage_read::record_panes(
        &cached,
        &[
            json!({"tmux_session":"late-session","tmux_pane":"late-pane"})
                .as_object()
                .unwrap()
                .clone(),
        ],
    );
    assert!(
        quota_result.is_err() && pane_result.is_err(),
        "quota={quota_result:?}, pane={pane_result:?}"
    );
    assert_eq!(all(&cached, &spec), before);
    assert!(cached.is_autocommit());
    let reopened = usage::open_usage_db_at(&spec.legacy).unwrap();
    assert_eq!(
        usage_read::record_quota_snapshots(&reopened, &[quota()], 10).unwrap(),
        1
    );
    assert_eq!(rows(&cached, "usage_quota_snapshots").len(), 0);
    assert_eq!(rows(&reopened, "usage_quota_snapshots").len(), 1);
    drop(reopened);
    migrate::demote_db(&spec, &h.target()).unwrap();
    assert_eq!(rows(&cached, "usage_quota_snapshots").len(), 1);
}
#[test]
fn public_usage_import_mutators_refuse_cached_moved_source() {
    let h = Home::new();
    let (spec, cached) = h.source("db-usage");
    cached.execute_batch("INSERT INTO usage_turns(id,provider,agent,tmux_session,tmux_pane,pane_pwd,git_root,turn_started_at,turn_finished_at,source,confidence) VALUES('old','claude','claude','s','p','/private','/private',1,2,'hook','exact')").unwrap();
    h.move_source(&spec);
    let before = all(&cached, &spec);
    let config=usage_import::record_session_config(&cached,json!({"tmux_session":"late-session","tmux_pane":"late-pane","harness":"codex","motor":"codex","model":"private","effort":"high"}).as_object().unwrap(),12);
    let prune = usage_import::prune_old_turns(&cached, 2_000_000, 1);
    let reconcile = usage_import::reconcile_orphan_interactions(&cached, 12, 14);
    assert!(
        config.is_err() && prune.is_err() && reconcile.is_err(),
        "config={config:?}, prune={prune:?}, reconcile={reconcile:?}"
    );
    assert_eq!(all(&cached, &spec), before);
    assert!(cached.is_autocommit());
}
#[test]
fn app_state_public_transaction_writers_refuse_cached_schema_sentinel() {
    let h = Home::new();
    let (spec, cached) = h.source("db-app-state");
    cached.execute_batch("INSERT INTO workspace_current VALUES(1,9,'{}',1);INSERT INTO news_editions(id,local_date,slot,timezone,scheduled_at_ms,status) VALUES('private-edition','2026-10-05','am','UTC',1,'published');INSERT INTO news_stories(id,edition_id,position,story_key,category,title,summary_md,body_md) VALUES(17,'private-edition',1,'private','fixture','private','summary','body')").unwrap();
    h.move_source(&spec);
    let before = all(&cached, &spec);
    let workspace = comandos_store::workspace::WorkspaceStore::new(&cached)
        .commit(
            &json!(9),
            &json!({"schema":1,"groups":[],"tabs":{}}),
            "late-workspace",
            "user",
            12.0,
        )
        .map(|_| ())
        .map_err(|e| e.to_string());
    let notification = comandos_store::notifications::record_presence(
        &cached,
        "late-device",
        true,
        true,
        true,
        12,
        &json!("web"),
    )
    .map(|_| ())
    .map_err(|e| e.to_string());
    let news = comandos_store::news::set_saved(&cached, 17, true, 12)
        .map(|_| ())
        .map_err(|e| e.to_string());
    let clock = || 12000;
    let id = || "late-pomodoro".to_owned();
    let timer = comandos_store::pomodoro::PomodoroStore::new(&cached, &clock, &id);
    let pomodoro = timer
        .command(&json!({"requestId":"late-timer","action":"start","targetMs":60000}))
        .map(|_| ())
        .map_err(|e| e.to_string());
    assert!(
        [&workspace, &notification, &news, &pomodoro]
            .iter()
            .all(|r| r.is_err()),
        "workspace={workspace:?}, notification={notification:?}, news={news:?}, pomodoro={pomodoro:?}"
    );
    assert_eq!(all(&cached, &spec), before);
    assert!(cached.is_autocommit());
}
#[test]
fn quota_rechecks_after_waiting_for_mover_despite_successful_earlier_admission() {
    let h = Home::new();
    let (spec, cached) = h.source("db-usage");
    assert!(migrate::move_db::admit_write(&cached).is_ok());
    let target = unified::open_unified(&h.target()).unwrap();
    target.execute_batch("BEGIN IMMEDIATE").unwrap();
    let dest = h.target();
    let backup = h.0.join(".local/share/comandos/backups/race");
    let source = spec.legacy.clone();
    let mover = std::thread::spawn(move || migrate::move_db(&spec, &dest, &backup, 4000));
    let probe = Connection::open(&source).unwrap();
    probe.busy_timeout(Duration::ZERO).unwrap();
    let start = Instant::now();
    loop {
        if probe.execute_batch("BEGIN IMMEDIATE").is_err() {
            break;
        }
        probe.execute_batch("ROLLBACK").unwrap();
        assert!(start.elapsed() < Duration::from_secs(3));
        std::thread::sleep(Duration::from_millis(5));
    }
    let writer =
        std::thread::spawn(move || usage_read::record_quota_snapshots(&cached, &[quota()], 12));
    std::thread::sleep(Duration::from_millis(40));
    assert!(!writer.is_finished());
    target.execute_batch("COMMIT").unwrap();
    mover.join().unwrap().unwrap();
    let outcome = writer.join().unwrap();
    assert!(outcome.is_err(), "post-move public write {outcome:?}");
    assert!(rows(&probe, "usage_quota_snapshots").is_empty());
    assert!(rows(&target, "usage_quota_snapshots").is_empty());
}
#[test]
fn public_import_batch_intrinsically_refuses_moved_source_even_if_caller_admits() {
    struct Roots;
    impl usage_import::GitRoots for Roots {
        fn root(&self, path: &str) -> String {
            path.into()
        }
    }
    let h = Home::new();
    let (spec, cached) = h.source("db-usage");
    let projects = h.0.join(".claude/projects/private");
    fs::create_dir_all(&projects).unwrap();
    let transcript = json!({"type":"assistant","timestamp":"2026-10-05T10:00:00Z","cwd":"/private","sessionId":"s-a","requestId":"req1","message":{"id":"msg1","model":"claude-fable-5","usage":{"input_tokens":5,"output_tokens":7}}});
    fs::write(
        projects.join("conversation.jsonl"),
        format!("{transcript}\n"),
    )
    .unwrap();
    h.move_source(&spec);
    let before = all(&cached, &spec);
    let zone = chrono_tz::UTC;
    let plan = usage_import::ImportPlan {
        now: 1_791_200_000,
        max_age_days: 21,
        claude_max_files: None,
        codex_max_files: None,
        home: &h.0,
        claude_projects_main: None,
        opencode_db: h.0.join("missing-opencode.db"),
        zone: &zone,
        admit: &|_| true,
        cancelled: &|| false,
        big_lines: usage_import::BigLines::Skip,
    };
    let result = usage_import::record_local_claude_jsonl(
        &cached,
        &plan,
        &h.0.join(".claude/projects"),
        "main",
        &mut usage_import::ImportSeen::default(),
        &Roots,
    );
    assert!(
        matches!(result, Err(usage_import::ImportError::Refused)),
        "{result:?}"
    );
    assert_eq!(all(&cached, &spec), before);
    assert!(cached.is_autocommit());
}

fn event(id: &str) -> Value {
    json!({"eventId":id,"source":"hook:codex","kind":"turn_completed","evidence":"confirmed","correlation":"source","turnId":id,"conversationId":"c","harness":"codex","occurredAtMs":1000,"receivedAtMs":1001})
}
#[test]
fn direct_event_and_focus_helpers_refuse_moved_source_without_finishing_caller_transaction() {
    let h = Home::new();
    let (spec, cached) = h.source("db-app-state");
    comandos_store::append_event(&cached, &event("seed"), 1000, "seed", "seed-receipt").unwrap();
    let policy = comandos_core::focus::policy_v1();
    comandos_store::focus::ensure_policy(&cached, &policy, 0).unwrap();
    h.move_source(&spec);
    let before = all(&cached, &spec);
    let tx =
        rusqlite::Transaction::new_unchecked(&cached, rusqlite::TransactionBehavior::Immediate)
            .unwrap();
    let append =
        comandos_store::append_event(&cached, &event("late"), 1000, "late", "late-receipt");
    let claim = comandos_store::claim_delivery(&cached, "seed", "private", "private", 12);
    let ensure = comandos_store::focus::ensure_policy(&cached, &policy, 12);
    let award = comandos_store::focus::award(
        &cached,
        &json!({"blockId":"late","mode":"focus","status":"completed","activeMs":60000,"startedAtMs":1000,"endedAtMs":61000,"provenance":"measured"}),
        &policy,
        61000,
    );
    assert!(
        append.is_err() && claim.is_err() && ensure.is_err() && award.is_err(),
        "append={append:?}, claim={claim:?}, ensure={ensure:?}, award={award:?}"
    );
    assert!(!cached.is_autocommit());
    assert_eq!(all(&cached, &spec), before);
    tx.rollback().unwrap();
    assert!(cached.is_autocommit());
}
#[test]
fn usage_change_and_schema_initialization_refuse_moved_source() {
    let h = Home::new();
    let (spec, cached) = h.source("db-usage");
    h.move_source(&spec);
    let before = all(&cached, &spec);
    let change = usage::record_change(
        &cached,
        json!({"after_model":"private"}).as_object().unwrap(),
        12,
    );
    let schema = usage::ensure_schema(&cached);
    assert!(
        change.is_err() && schema.is_err(),
        "change={change:?}, schema={schema:?}"
    );
    assert_eq!(all(&cached, &spec), before);
    assert!(cached.is_autocommit());
}
#[test]
fn late_news_completions_refuse_moved_source() {
    let h = Home::new();
    let (spec, cached) = h.source("db-app-state");
    cached.execute_batch("INSERT INTO news_editions(id,local_date,slot,timezone,scheduled_at_ms,status) VALUES('private-edition','2026-10-05','am','UTC',1,'published');INSERT INTO news_stories(id,edition_id,position,story_key,category,title,summary_md,body_md) VALUES(17,'private-edition',1,'private','fixture','private','summary','body');INSERT INTO news_chat(id,story_id,edition_id,role,state,text,created_at_ms) VALUES(1,17,'private-edition','assistant','pending','',1),(2,17,'private-edition','assistant','pending','',2);INSERT INTO news_sources(id,url,original_url,title,origin,category,discovered_at_ms,fetch_status) VALUES(1,'https://private.invalid/1','https://private.invalid/1','private','private','test',1,'ok');INSERT INTO news_translations(source_id,lang,state,created_at_ms,updated_at_ms) VALUES(1,'es','running',1,1),(1,'en','running',1,1)").unwrap();
    h.move_source(&spec);
    let before = all(&cached, &spec);
    let done = comandos_store::news::finish_chat(&cached, 1, "late", None, "private");
    let failed = comandos_store::news::fail_chat(&cached, 2, "late-error");
    let translated =
        comandos_store::news::finish_translation(&cached, 1, "es", "late", "[]", "private", 12);
    let failed_translation =
        comandos_store::news::fail_translation(&cached, 1, "en", "late-error", 12);
    assert!(
        done.is_err() && failed.is_err() && translated.is_err() && failed_translation.is_err(),
        "done={done:?}, failed={failed:?}, translated={translated:?}, failed_translation={failed_translation:?}"
    );
    assert_eq!(all(&cached, &spec), before);
    assert!(cached.is_autocommit());
}
#[test]
fn late_optional_profile_schema_cannot_resurrect_table_on_moved_source() {
    let h = Home::new();
    let (spec, cached) = h.source("db-app-state");
    h.move_source(&spec);
    let result = comandos_store::session_profiles::list_profiles(&cached);
    assert!(result.is_err(), "{result:?}");
    let count: i64 = cached
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE name='session_profiles'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 0);
    assert!(cached.is_autocommit());
}

#[test]
fn caller_owned_deferred_snapshot_cannot_upgrade_to_stale_write_after_move() {
    let h = Home::new();
    let (spec, cached) = h.source("db-app-state");
    let tx = cached.unchecked_transaction().unwrap();
    migrate::move_db::admit_write(&cached).unwrap();
    h.move_source(&spec);
    let result = comandos_store::append_event(
        &cached,
        &event("late-snapshot"),
        1000,
        "late-snapshot",
        "snapshot-receipt",
    );
    assert!(result.is_err(), "{result:?}");
    assert!(!cached.is_autocommit());
    tx.rollback().unwrap();
    assert!(rows(&cached, "events").is_empty());
    assert!(migrate::move_db::admit_write(&cached).is_err());
    let new = unified::open_unified(&h.target()).unwrap();
    assert!(rows(&new, "events").is_empty());
}
#[test]
fn public_usage_writers_reject_future_target_schema_without_mutation() {
    let h = Home::new();
    let (spec, cached) = h.source("db-usage");
    h.move_source(&spec);
    drop(cached);
    let reopened = usage::open_usage_db_at(&spec.legacy).unwrap();
    let before = all(&reopened, &spec);
    reopened
        .execute_batch("INSERT INTO schema_migrations VALUES(105,'future',1)")
        .unwrap();
    assert!(usage_read::record_quota_snapshots(&reopened, &[quota()], 12).is_err());
    assert!(
        usage::record_change(
            &reopened,
            json!({"after_model":"late"}).as_object().unwrap(),
            12
        )
        .is_err()
    );
    assert!(
        usage_import::record_session_config(
            &reopened,
            json!({"tmux_session":"late","tmux_pane":"late"})
                .as_object()
                .unwrap(),
            12
        )
        .is_err()
    );
    assert_eq!(all(&reopened, &spec), before);
    assert!(reopened.is_autocommit());
}

#[test]
fn explicit_schema_migration_cannot_write_cached_source_marker() {
    let h = Home::new();
    let (spec, cached) = h.source("db-app-state");
    h.move_source(&spec);
    let custom = comandos_store::state::Migration {
        version: 90,
        name: "custom",
        sql: "CREATE TABLE must_not_exist(x);",
    };
    assert!(comandos_store::state::migrate(&cached, &[custom], 12.0).is_err());
    let exists: i64 = cached
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE name='must_not_exist'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(exists, 0);
    assert!(cached.is_autocommit());
}
