use comandos_runtime::session_operations::{config_history, open_journal};
use comandos_store::{
    migrate::{move_db, spec_for},
    unified,
};
use std::{fs, path::PathBuf};
struct Home(PathBuf);
impl Drop for Home {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
#[test]
fn operations_opener_and_readonly_history_route_without_source_shm_or_home_locks() {
    let home = Home(std::env::temp_dir().join(format!(
            "comandos-s4-history-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        )));
    fs::create_dir(&home.0).unwrap();
    let spec = spec_for(&home.0, "db-operations").unwrap();
    let target = home.0.join(".local/share/comandos/comandos.sqlite3");
    let old = open_journal(&spec.legacy).unwrap();
    old.execute_batch(
        "INSERT INTO session_operations VALUES('op','pane','hash','{}','confirmed',1,'{}','{}',1)",
    )
    .unwrap();
    drop(old);
    move_db(
        &spec,
        &target,
        &home.0.join(".local/share/comandos/backups/test"),
        4000,
    )
    .unwrap();
    let new = open_journal(&spec.legacy).unwrap();
    assert_eq!(new.path(), target.to_str());
    drop(new);
    let before = fs::read(&spec.legacy).unwrap();
    let new_before = fs::read(&target).unwrap();
    assert!(
        config_history(&spec.legacy, "/project", "pane").unwrap()["items"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert_eq!(fs::read(&spec.legacy).unwrap(), before);
    assert_eq!(fs::read(&target).unwrap(), new_before);
    assert!(!PathBuf::from(format!("{}-shm", spec.legacy.display())).exists());
    assert!(!PathBuf::from(format!("{}-shm", target.display())).exists());

    let new = unified::open_unified(&target).unwrap();
    assert_eq!(
        new.query_row("SELECT count(*) FROM session_operations", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        1
    );
}

#[test]
fn cached_runtime_app_writers_refuse_moved_source_before_files_or_rows() {
    let home =
        Home(std::env::temp_dir().join(comandos_runtime::fresh_id("s4-runtime-writers").unwrap()));
    fs::create_dir(&home.0).unwrap();
    let spec = spec_for(&home.0, "db-app-state").unwrap();
    let cached = comandos_store::state::connect(&spec.legacy).unwrap();
    comandos_store::state::migrate(&cached, comandos_store::state::MIGRATIONS, 1.0).unwrap();
    cached.execute_batch("INSERT INTO quick_terminal_requests VALUES('seed','launching','/private','s','p',NULL,1,1,1)").unwrap();
    let target = home.0.join(".local/share/comandos/comandos.sqlite3");
    move_db(
        &spec,
        &target,
        &home.0.join(".local/share/comandos/backups/runtime"),
        4000,
    )
    .unwrap();
    let policy = comandos_runtime::news_editions::default_policy();
    let schedule =
        comandos_runtime::news_editions::schedule_editions(&cached, "2026-10-05", &policy, None);
    let recover = comandos_runtime::news_editions::recover_reading(&cached, 12);
    let reconcile = comandos_runtime::news_editions::reconcile(&cached, 12, &policy);
    let requeue = comandos_runtime::news_editions::requeue_orphans(&cached, 12, &policy);
    let claim_news = comandos_runtime::news_editions::claim_due_job(&cached, 12, &policy);
    let base = home.0.join("terminal-directories");
    let options = comandos_runtime::quick_terminal::Options::new(
        &base,
        chrono::DateTime::parse_from_rfc3339("2026-10-05T10:00:00Z").unwrap(),
    );
    let claim = comandos_runtime::quick_terminal::claim(&cached, "late", &options, &|| 12.0);
    let finish = comandos_runtime::quick_terminal::finish(
        &cached,
        "seed",
        "ready",
        None,
        "/private",
        &|| 12.0,
    );
    let legacy = comandos_runtime::legacy::import_legacy(&cached, &home.0.join("missing-timeline"));
    assert!(
        schedule.is_err()
            && recover.is_err()
            && reconcile.is_err()
            && requeue.is_err()
            && claim_news.is_err()
            && claim.is_err()
            && finish.is_err()
            && legacy.is_err(),
        "schedule={schedule:?}, recover={recover:?}, reconcile={reconcile:?}, requeue={requeue:?}, claim_news={claim_news:?}, claim={claim:?}, finish={finish:?}, legacy={legacy:?}"
    );
    assert!(!base.exists());
    assert!(cached.is_autocommit());
    let state: String = cached
        .query_row(
            "SELECT state FROM quick_terminal_requests WHERE request_id='seed'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(state, "launching");
    let count: i64 = cached
        .query_row("SELECT COUNT(*) FROM news_editions", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 0);
    let count: i64 = cached
        .query_row("SELECT COUNT(*) FROM workspace_meta", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 0);
}
