//! Fixtures unitarios para la carrera entre instantánea y escritura en espejo.
use super::{MigrateOptions, StepReport, import, migrate, sources};
use crate::{
    domains::{StatusDir, catalog},
    unified::{self, Mode},
};
use std::{
    fs,
    os::unix::fs::{MetadataExt, PermissionsExt},
    path::PathBuf,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "comandos-backfill-race-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(path.join(".claude/hooks/state")).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
        Self(path)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
#[test]
fn backfill_never_overwrites_newer_mirror_write() {
    let h = Fixture::new();
    let db = h.0.join("db/comandos.sqlite3");
    let c = unified::open_unified(&db).unwrap();
    let path = h.0.join(".claude/hooks/state/p--s--1.json");
    fs::write(&path, b"{\"status\":\"done\"}").unwrap();
    fs::File::open(&path)
        .unwrap()
        .set_modified(UNIX_EPOCH + Duration::from_secs(1))
        .unwrap();
    let source = sources::Source {
        path: path.clone(),
        symbolic: "H/state/p--s--1.json".into(),
        spec: *catalog::source("H/state/p--s--1.json").unwrap(),
        db: db.clone(),
    };
    unified::modes::prepare_mirror(&h.0, &c, "session-status", 1).unwrap();
    assert_eq!(
        unified::mode_of(Some(&c), "session-status").unwrap(),
        Mode::Mirror
    );
    let snapshot = sources::read(&path).unwrap();
    let writer = unified::open_unified(&db).unwrap();
    let statuses = StatusDir {
        home: &h.0,
        dir: h.0.join(".claude/hooks/state"),
        domain: "session-status",
    };
    statuses
        .write(
            Some(&writer),
            "p--s--1.json",
            b"{\"status\":\"working\"}",
            2_000_000_000,
        )
        .unwrap();
    let mut step = StepReport {
        domain: "session-status".into(),
        source: source.symbolic.clone(),
        status: "done".into(),
        rows: 0,
        bytes: 0,
        elapsed_ms: 0,
        detail: String::new(),
    };
    let tx = c.unchecked_transaction().unwrap();
    import::apply(&c, &source, &snapshot, 2000, &mut step).unwrap();
    tx.commit().unwrap();
    assert_eq!(step.rows, 0);
    assert_eq!(
        statuses.read(Some(&c), "p--s--1.json").unwrap().unwrap(),
        b"{\"status\":\"working\"}"
    );
    let row: Vec<u8> = c
        .query_row(
            "SELECT body FROM session_status WHERE file_key='p--s--1.json'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(row, b"{\"status\":\"working\"}");
}
#[test]
fn source_replaced_with_control_after_collection_is_rejected() {
    let h = Fixture::new();
    let db = h.0.join("db/comandos.sqlite3");
    let c = unified::open_unified(&db).unwrap();
    drop(c);
    let path = h.0.join(".claude/hooks/state/p--s--1.json");
    fs::write(&path, b"old source").unwrap();
    let collected = sources::collect(
        &h.0,
        &db,
        &[*catalog::source("H/state/p--s--1.json").unwrap()],
    )
    .unwrap();
    fs::remove_file(&path).unwrap();
    fs::hard_link(&db, &path).unwrap();
    assert!(sources::read_source(&h.0, &collected[0]).is_err());
}

// A JSON-only selection must not inspect an unrelated, actively managed SQL source.
fn json_scope_fixture() -> (Fixture, MigrateOptions, PathBuf) {
    let h = Fixture::new();
    let hooks = h.0.join(".claude/hooks");
    fs::set_permissions(&hooks, fs::Permissions::from_mode(0o700)).unwrap();
    fs::write(hooks.join("snippets.json"), b"[\"selected document\"]").unwrap();
    let usage = hooks.join("comandos-usage.sqlite");
    fs::write(&usage, b"deliberately invalid unrelated SQLite source").unwrap();
    fs::set_permissions(&usage, fs::Permissions::from_mode(0o600)).unwrap();
    let opts = MigrateOptions {
        home: h.0.clone(),
        db: h.0.join(".local/share/comandos/comandos.sqlite3"),
        dry_run: true,
        resume: false,
        domains: Some(vec!["ui-docs".into()]),
        now_ms: 1_000_000,
    };
    (h, opts, usage)
}
fn source_identity(path: &std::path::Path) -> (Vec<u8>, u64, u64, u32) {
    let meta = fs::symlink_metadata(path).unwrap();
    assert!(meta.file_type().is_file());
    (fs::read(path).unwrap(), meta.dev(), meta.ino(), meta.mode())
}
#[test]
fn explicit_json_dry_run_does_not_inspect_unselected_sqlite() {
    let (_h, opts, usage) = json_scope_fixture();
    let before = source_identity(&usage);
    let report = migrate(&opts).expect("unselected SQLite must not prevent JSON dry-run");
    assert_eq!(report.usage_move_estimate_ms, None);
    assert!(!report.steps.is_empty());
    assert!(report.steps.iter().all(|step| step.domain == "ui-docs"));
    assert_eq!(source_identity(&usage), before);
    assert!(!sources::suffix(&usage, "-wal").exists());
    assert!(!sources::suffix(&usage, "-shm").exists());
    assert!(!opts.db.exists());
}
#[test]
fn explicit_json_migration_keeps_selection_in_preflight() {
    let (_h, mut opts, usage) = json_scope_fixture();
    opts.dry_run = false;
    let before = source_identity(&usage);
    let report = migrate(&opts).expect("unselected SQLite must not prevent JSON mirror migration");
    assert_eq!(report.usage_move_estimate_ms, None);
    assert!(report.backup_dir.is_some());
    assert!(!report.steps.is_empty());
    assert!(report.steps.iter().all(|step| step.domain == "ui-docs"));
    assert_eq!(source_identity(&usage), before);
    assert!(!sources::suffix(&usage, "-wal").exists());
    assert!(!sources::suffix(&usage, "-shm").exists());
    let conn = unified::open_unified(&opts.db).unwrap();
    assert_eq!(
        unified::mode_of(Some(&conn), "ui-docs").unwrap(),
        Mode::Mirror
    );
    let body: Vec<u8> = conn
        .query_row(
            "SELECT body FROM documents WHERE name='hooks/snippets.json'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(body, b"[\"selected document\"]");
}
#[test]
fn default_migration_still_rejects_invalid_usage_sqlite() {
    let (_h, mut opts, usage) = json_scope_fixture();
    opts.domains = None;
    let before = source_identity(&usage);
    for dry_run in [true, false] {
        opts.dry_run = dry_run;
        assert!(
            migrate(&opts).is_err(),
            "default migration must retain SQL estimate validation"
        );
        assert_eq!(source_identity(&usage), before);
        assert!(!opts.db.exists());
        assert!(!sources::suffix(&usage, "-wal").exists());
        assert!(!sources::suffix(&usage, "-shm").exists());
    }
}
