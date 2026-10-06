use comandos_store::{migrate, unified};
use std::{fs, path::PathBuf};
struct Fixture {
    home: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let home = std::env::temp_dir().join(format!(
            "state-lifecycle-{}-{}",
            std::process::id(),
            migrate::journal::new_id(0).unwrap()
        ));
        fs::create_dir_all(home.join(".claude/hooks")).unwrap();
        fs::write(home.join(".claude/hooks/snippets.json"), b"[1]\n").unwrap();
        migrate::migrate(&migrate::MigrateOptions {
            home: home.clone(),
            db: unified::unified_path(&home),
            dry_run: false,
            resume: false,
            domains: Some(vec!["ui-docs".into()]),
            now_ms: 1,
        })
        .unwrap();
        Self { home }
    }
    fn db(&self) -> rusqlite::Connection {
        unified::open_unified(&unified::unified_path(&self.home)).unwrap()
    }
    fn verifies(&self) {
        let c = self.db();
        for now in [2, 43_200_002, 86_400_002] {
            let r = migrate::verify(&self.home, &c, "ui-docs").unwrap();
            migrate::journal::record_verify(&c, &r, now).unwrap();
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.home).unwrap();
    }
}
#[test]
fn flip_refuses_without_three_verifies_and_without_24_hours() {
    let f = Fixture::new();
    let c = f.db();
    assert!(migrate::lifecycle::flip(&f.home, &c, "ui-docs", 86_400_003).is_err());
    for now in [2, 3, 4] {
        let r = migrate::verify(&f.home, &c, "ui-docs").unwrap();
        migrate::journal::record_verify(&c, &r, now).unwrap();
    }
    assert!(migrate::lifecycle::flip(&f.home, &c, "ui-docs", 86_400_003).is_err());
    assert_eq!(
        unified::mode_of(Some(&c), "ui-docs").unwrap(),
        unified::Mode::Mirror
    );
}
#[test]
fn flip_demote_preserves_latest_mirror_bytes() {
    let f = Fixture::new();
    f.verifies();
    let c = f.db();
    migrate::lifecycle::flip(&f.home, &c, "ui-docs", 86_400_003).unwrap();
    assert_eq!(
        unified::mode_of(Some(&c), "ui-docs").unwrap(),
        unified::Mode::Unified
    );
    migrate::lifecycle::demote(&f.home, &c, "ui-docs", 86_400_004).unwrap();
    assert_eq!(
        fs::read(f.home.join(".claude/hooks/snippets.json")).unwrap(),
        b"[1]\n"
    );
}
#[test]
fn seal_refuses_before_seven_days_and_export_restores_bytes() {
    let f = Fixture::new();
    f.verifies();
    let c = f.db();
    migrate::lifecycle::flip(&f.home, &c, "ui-docs", 86_400_003).unwrap();
    assert!(migrate::lifecycle::seal(&f.home, &c, "ui-docs", 86_400_004).is_err());
    migrate::lifecycle::seal(&f.home, &c, "ui-docs", 691_200_003).unwrap();
    assert!(!f.home.join(".claude/hooks/snippets.json").exists());
    assert!(migrate::lifecycle::demote(&f.home, &c, "ui-docs", 691_200_004).is_err());
    migrate::lifecycle::export_legacy(&f.home, &c, "ui-docs", 691_200_005).unwrap();
    assert_eq!(
        fs::read(f.home.join(".claude/hooks/snippets.json")).unwrap(),
        b"[1]\n"
    );
    assert_eq!(
        unified::mode_of(Some(&c), "ui-docs").unwrap(),
        unified::Mode::Unified
    );
}
#[test]
fn rollback_restores_backup_and_keeps_database() {
    let f = Fixture::new();
    let c = f.db();
    let id: String = c
        .query_row(
            "SELECT run_id FROM migration_runs WHERE backup_dir<>'' LIMIT 1",
            [],
            |r| r.get(0),
        )
        .unwrap();
    drop(c);
    fs::write(f.home.join(".claude/hooks/snippets.json"), b"[2]\n").unwrap();
    let report =
        migrate::lifecycle::rollback(&f.home, &unified::unified_path(&f.home), &id, 90_000_000)
            .unwrap();
    assert!(report.archived_db.exists());
    assert!(!unified::unified_path(&f.home).exists());
    assert_eq!(
        fs::read(f.home.join(".claude/hooks/snippets.json")).unwrap(),
        b"[1]\n"
    );
}
#[test]
fn failed_verify_resets_flip_window() {
    let f = Fixture::new();
    f.verifies();
    let c = f.db();
    let mut report = migrate::verify(&f.home, &c, "ui-docs").unwrap();
    report.mismatches.push("changed".into());
    migrate::journal::record_verify(&c, &report, 86_400_003).unwrap();
    assert!(migrate::lifecycle::flip(&f.home, &c, "ui-docs", 86_400_004).is_err());
}
#[test]
fn rollback_restores_sqlite_before_move_even_after_unified_writes() {
    let f = Fixture::new();
    let path = unified::unified_path(&f.home);
    let c = f.db();
    let id: String = c
        .query_row(
            "SELECT run_id FROM migration_runs WHERE backup_dir<>'' LIMIT 1",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let spec = migrate::spec_for(&f.home, "db-operator").unwrap();
    fs::create_dir_all(spec.legacy.parent().unwrap()).unwrap();
    let old = rusqlite::Connection::open(&spec.legacy).unwrap();
    old.execute_batch("CREATE TABLE actions(id TEXT PRIMARY KEY,tool TEXT,status TEXT,detail TEXT,created REAL,updated REAL);INSERT INTO actions VALUES('original','browser','confirmed',X'00FF',1.25,NULL)").unwrap();
    drop(old);
    migrate::move_db(
        &spec,
        &path,
        &f.home.join(".local/share/comandos/backups/move-operator"),
        4000,
    )
    .unwrap();
    c.execute_batch("INSERT INTO operator_actions VALUES('new','browser','confirmed','{}',2,NULL)")
        .unwrap();
    drop(c);
    migrate::lifecycle::rollback(&f.home, &path, &id, migrate::journal::now_ms().unwrap()).unwrap();
    let old = rusqlite::Connection::open(&spec.legacy).unwrap();
    assert_eq!(
        old.query_row("SELECT COUNT(*) FROM actions", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        1
    );
}
#[test]
fn export_restores_an_empty_imported_log() {
    let f = Fixture::new();
    let file = f.home.join(".claude/hooks/events.jsonl");
    fs::write(&file, b"").unwrap();
    let path = unified::unified_path(&f.home);
    migrate::migrate(&migrate::MigrateOptions {
        home: f.home.clone(),
        db: path,
        dry_run: false,
        resume: false,
        domains: Some(vec!["logs".into()]),
        now_ms: 1,
    })
    .unwrap();
    let c = f.db();
    unified::set_mode(&c, "logs", unified::Mode::Unified, "fixture", 1).unwrap();
    migrate::lifecycle::seal(&f.home, &c, "logs", 604_800_001).unwrap();
    migrate::lifecycle::export_legacy(&f.home, &c, "logs", 604_800_002).unwrap();
    assert_eq!(fs::read(file).unwrap(), b"");
}
