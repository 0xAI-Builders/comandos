//! Contratos de la base nueva sobre conexiones exclusivamente temporales.
use comandos_store::unified::{self, CommandKind, Generation, LogName, Origin};
use rusqlite::Connection;

#[test]
fn insecure_existing_locations_are_rejected_without_chmod_or_database_creation() {
    use std::{fs, os::unix::fs::PermissionsExt};
    let base = std::env::temp_dir().join(format!(
        "comandos-unified-permissions-{}",
        std::process::id()
    ));
    fs::create_dir_all(&base).unwrap();
    fs::set_permissions(&base, fs::Permissions::from_mode(0o755)).unwrap();
    let path = base.join("state.sqlite3");
    assert!(unified::open_unified(&path).is_err());
    assert!(
        !path.exists(),
        "failed preflight must not create a database"
    );
    assert_eq!(
        fs::metadata(&base).unwrap().permissions().mode() & 0o777,
        0o755
    );
    fs::set_permissions(&base, fs::Permissions::from_mode(0o700)).unwrap();
    let connection = Connection::open(&path).unwrap();
    connection
        .execute_batch("CREATE TABLE marker(value TEXT);INSERT INTO marker VALUES('untouched');")
        .unwrap();
    drop(connection);
    fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
    let bytes = fs::read(&path).unwrap();
    assert!(unified::open_unified(&path).is_err());
    assert_eq!(fs::read(&path).unwrap(), bytes);
    assert_eq!(
        fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o644
    );
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    let reopened = unified::open_unified(&path).unwrap();
    assert_eq!(
        reopened
            .query_row("SELECT value FROM marker", [], |row| row
                .get::<_, String>(0))
            .unwrap(),
        "untouched"
    );
    drop(reopened);
    fs::remove_dir_all(base).unwrap();
}

#[test]
fn unified_contains_released_schemas_and_both_version_marks() {
    let c = unified::open_unified(std::path::Path::new(":memory:")).unwrap();
    assert_eq!(
        c.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        comandos_store::usage::SCHEMA_VERSION
    );
    assert_eq!(
        c.query_row("SELECT MAX(version) FROM schema_migrations", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        104
    );
    for table in [
        "documents",
        "session_status",
        "native_processes",
        "log_lines",
        "layout_snapshots",
        "app_commands",
        "news_radar_events",
        "operator_actions",
        "session_operations",
        "session_operation_events",
        "usage_turns",
        "pomodoro_state",
    ] {
        assert_eq!(
            c.query_row(
                "SELECT count(*) FROM sqlite_master WHERE type='table' AND name=?1",
                [table],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
            1,
            "{table}"
        );
    }
}

#[test]
fn older_import_never_overwrites_a_mirrored_write_or_changes_raw_bytes() {
    let c = unified::open_unified(std::path::Path::new(":memory:")).unwrap();
    let body = b" {\"value\": 1.00, \"duplicate\": 1, \"duplicate\": 2} \n";
    assert_eq!(
        unified::doc_put(
            &c,
            "hooks/snippets.json",
            "ui-docs",
            body,
            Origin::Mirror,
            2000
        )
        .unwrap(),
        1
    );
    assert!(!unified::doc_put_if_newer(&c, "hooks/snippets.json", "ui-docs", b"[]", 1000).unwrap());
    assert!(!unified::doc_put_if_newer(&c, "hooks/snippets.json", "ui-docs", b"[]", 2000).unwrap());
    assert_eq!(
        unified::doc_get(&c, "hooks/snippets.json")
            .unwrap()
            .unwrap()
            .body,
        body
    );
    assert!(unified::doc_put_if_newer(&c, "hooks/snippets.json", "ui-docs", b"[]", 3000).unwrap());
    assert_eq!(
        unified::doc_get(&c, "hooks/snippets.json")
            .unwrap()
            .unwrap()
            .revision,
        2
    );
    unified::status_put(&c, "pane", b"new", 2000, Origin::Mirror).unwrap();
    assert!(!unified::status_put_if_newer(&c, "pane", b"old", 1000).unwrap());
    assert_eq!(
        c.query_row(
            "SELECT body FROM session_status WHERE file_key='pane'",
            [],
            |r| r.get::<_, Vec<u8>>(0)
        )
        .unwrap(),
        b"new"
    );
}

#[test]
fn logs_are_ordered_per_log_and_commands_are_consumed_exactly_once() {
    let c = unified::open_unified(std::path::Path::new(":memory:")).unwrap();
    assert_eq!(
        unified::log_append(&c, LogName::Events, b"first").unwrap(),
        1
    );
    assert_eq!(
        unified::log_append(&c, LogName::UiEvents, b"other").unwrap(),
        1
    );
    assert_eq!(
        unified::log_append(&c, LogName::Events, b"second").unwrap(),
        2
    );
    assert_eq!(
        unified::log_tail(&c, LogName::Events, 1).unwrap(),
        vec![b"second".to_vec()]
    );
    assert!(
        unified::log_tail(&c, LogName::Events, 0)
            .unwrap()
            .is_empty()
    );
    let id = unified::command_push(&c, CommandKind::Focus, b"raw", 100).unwrap();
    assert_eq!(
        unified::command_take(&c, CommandKind::Focus, 101).unwrap(),
        Some((id, b"raw".to_vec()))
    );
    assert_eq!(
        unified::command_take(&c, CommandKind::Focus, 102).unwrap(),
        None
    );
    unified::layout_put(&c, Generation::Current, 10, b"keep").unwrap();
    unified::layout_put(&c, Generation::Minute, 10, b"old").unwrap();
    unified::layout_put(&c, Generation::Minute, 20, b"new").unwrap();
    assert_eq!(unified::layout_prune_minutes(&c, 15).unwrap(), 1);
    assert_eq!(
        c.query_row("SELECT count(*) FROM layout_snapshots", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        2
    );
}

#[test]
fn opening_a_newer_database_fails_without_downgrading_it() {
    let p = std::env::temp_dir().join(format!(
        "comandos-unified-future-{}.sqlite3",
        std::process::id()
    ));
    let c = Connection::open(&p).unwrap();
    c.execute_batch("PRAGMA user_version=12; CREATE TABLE marker(value TEXT);INSERT INTO marker VALUES ('untouched');").unwrap();
    drop(c);
    assert!(unified::open_unified(&p).is_err());
    let c = Connection::open(&p).unwrap();
    assert_eq!(
        c.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        12
    );
    assert_eq!(
        c.query_row("SELECT value FROM marker", [], |r| r.get::<_, String>(0))
            .unwrap(),
        "untouched"
    );
    assert_eq!(
        c.query_row(
            "SELECT count(*) FROM sqlite_master WHERE name='schema_migrations'",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        0
    );
    drop(c);
    std::fs::remove_file(p).unwrap();
}

#[test]
fn reopen_is_idempotent_private_and_preserves_documents() {
    use std::os::unix::fs::PermissionsExt;
    let dir = std::env::temp_dir().join(format!("comandos-unified-reopen-{}", std::process::id()));
    let path = dir.join("state.sqlite3");
    let c = unified::open_unified(&path).unwrap();
    unified::doc_put(
        &c,
        "hooks/prefs.json",
        "ui-docs",
        b" [1.00] ",
        Origin::Mirror,
        100,
    )
    .unwrap();
    drop(c);
    let c = unified::open_unified(&path).unwrap();
    assert_eq!(
        c.query_row("SELECT count(*) FROM schema_migrations", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        16
    );
    assert_eq!(
        unified::doc_get(&c, "hooks/prefs.json")
            .unwrap()
            .unwrap()
            .body,
        b" [1.00] "
    );
    assert_eq!(
        std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o600
    );
    drop(c);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn concurrent_consumers_never_take_the_same_command_twice() {
    let dir = std::env::temp_dir().join(format!(
        "comandos-unified-concurrent-{}",
        std::process::id()
    ));
    let path = dir.join("state.sqlite3");
    let c = unified::open_unified(&path).unwrap();
    for _ in 0..100 {
        unified::command_push(&c, CommandKind::Open, b"body", 100).unwrap();
    }
    drop(c);
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(3));
    let workers: Vec<_> = (0..2)
        .map(|_| {
            let path = path.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                let c = unified::open_unified(&path).unwrap();
                barrier.wait();
                let mut ids = Vec::new();
                while let Some((id, body)) =
                    unified::command_take(&c, CommandKind::Open, 101).unwrap()
                {
                    assert_eq!(body, b"body");
                    ids.push(id);
                }
                ids
            })
        })
        .collect();
    barrier.wait();
    let mut ids: Vec<_> = workers
        .into_iter()
        .flat_map(|w| w.join().unwrap())
        .collect();
    ids.sort_unstable();
    assert_eq!(ids, (1..=100).collect::<Vec<_>>());
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn unknown_schema_revision_is_rejected_before_any_new_migration() {
    let p = std::env::temp_dir().join(format!(
        "comandos-unified-unknown-{}.sqlite3",
        std::process::id()
    ));
    let c = Connection::open(&p).unwrap();
    c.execute_batch("CREATE TABLE schema_migrations(version INTEGER PRIMARY KEY,name TEXT,applied_at REAL);INSERT INTO schema_migrations VALUES(105,'future',0);").unwrap();
    drop(c);
    assert!(unified::open_unified(&p).is_err());
    let c = Connection::open(&p).unwrap();
    assert_eq!(
        c.query_row("SELECT count(*) FROM schema_migrations", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        1
    );
    assert_eq!(
        c.query_row(
            "SELECT count(*) FROM sqlite_master WHERE name='documents'",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        0
    );
    drop(c);
    std::fs::remove_file(p).unwrap();
}
