use comandos_store::state::{self, MIGRATIONS, Migration};
use rusqlite::Connection;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!(
            "comandos-state-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&p).unwrap();
        Self(p)
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn fresh_schema_is_complete_and_second_migration_is_noop() {
    let temp = Temp::new();
    let conn = state::connect(&temp.0.join("nested/state.sqlite3")).unwrap();
    let first = state::migrate(&conn, MIGRATIONS, 123.4).unwrap();
    assert_eq!(first.version, 11);
    assert!(first.backup.is_none());
    assert_eq!(
        conn.query_row("SELECT COUNT(*) FROM schema_migrations", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        11
    );
    assert_eq!(
        conn.query_row("PRAGMA foreign_keys", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        1
    );
    assert_eq!(
        conn.query_row("PRAGMA journal_mode", [], |r| r.get::<_, String>(0))
            .unwrap(),
        "wal"
    );
    assert!(
        state::migrate(&conn, MIGRATIONS, 125.0)
            .unwrap()
            .backup
            .is_none()
    );
}

#[test]
fn upgrades_back_up_original_data_and_keep_backup_private() {
    let temp = Temp::new();
    let conn = state::connect(&temp.0.join("state.sqlite3")).unwrap();
    state::migrate(&conn, &MIGRATIONS[..1], 123.0).unwrap();
    conn.execute("INSERT INTO workspace_meta VALUES('test','keep')", [])
        .unwrap();
    let result = state::migrate(&conn, MIGRATIONS, 124.0).unwrap();
    let path = result.backup.unwrap();
    let backup = Connection::open(&path).unwrap();
    assert_eq!(state::schema_version(&backup).unwrap(), 1);
    assert_eq!(
        backup
            .query_row(
                "SELECT value FROM workspace_meta WHERE key='test'",
                [],
                |r| r.get::<_, String>(0)
            )
            .unwrap(),
        "keep"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
}

#[test]
fn missing_lower_versions_are_applied_and_failed_batch_rolls_back() {
    let temp = Temp::new();
    let conn = state::connect(&temp.0.join("state.sqlite3")).unwrap();
    let high = Migration {
        version: 90,
        name: "high",
        sql: "CREATE TABLE high(x);",
    };
    state::migrate(&conn, &[MIGRATIONS[0], high], 100.0).unwrap();
    state::migrate(&conn, &[MIGRATIONS[0], MIGRATIONS[1], high], 101.0).unwrap();
    assert_eq!(
        conn.query_row("SELECT COUNT(*) FROM schema_migrations", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        3
    );
    let bad = Migration {
        version: 91,
        name: "broken",
        sql: "CREATE TABLE transient(x); THIS IS INVALID;",
    };
    assert!(state::migrate(&conn, &[bad], 101.0).is_err());
    assert!(conn.is_autocommit());
    assert_eq!(state::schema_version(&conn).unwrap(), 90);
    assert_eq!(
        conn.query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE name='transient'",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        0
    );
    assert!(
        std::fs::read_dir(&temp.0)
            .unwrap()
            .filter_map(Result::ok)
            .filter(|e| e.file_name().to_string_lossy().contains("pre-migration"))
            .count()
            >= 2
    );
}

#[test]
fn caller_transaction_is_preserved_when_migration_cannot_start() {
    let temp = Temp::new();
    let conn = state::connect(&temp.0.join("state.sqlite3")).unwrap();
    conn.execute_batch("BEGIN IMMEDIATE").unwrap();
    assert!(state::migrate(&conn, MIGRATIONS, 123.0).is_err());
    assert!(!conn.is_autocommit());
    conn.execute_batch("ROLLBACK").unwrap();
}

#[test]
fn concurrent_first_migrations_recheck_membership_under_the_write_lock() {
    use std::sync::{Arc, Barrier};
    let temp = Temp::new();
    let path = temp.0.join("parallel.sqlite3");
    let connections: Vec<_> = (0..6).map(|_| state::connect(&path).unwrap()).collect();
    let barrier = Arc::new(Barrier::new(6));
    let workers: Vec<_> = connections
        .into_iter()
        .map(|conn| {
            let barrier = Arc::clone(&barrier);
            std::thread::spawn(move || {
                barrier.wait();
                state::migrate(&conn, MIGRATIONS, 123.0).unwrap().version
            })
        })
        .collect();
    for worker in workers {
        assert_eq!(worker.join().unwrap(), 11);
    }
    let conn = state::connect(&path).unwrap();
    assert_eq!(
        conn.query_row("SELECT COUNT(*) FROM schema_migrations", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        11
    );
}
