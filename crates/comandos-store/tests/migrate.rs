use comandos_store::{
    migrate::{MigrateOptions, migrate, verify, verify_backup},
    unified::{self, Mode},
};
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};
struct Home(PathBuf);
impl Home {
    fn new() -> Self {
        let home = std::env::temp_dir().join(format!(
            "comandos-migrate-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&home).unwrap();
        fs::set_permissions(&home, fs::Permissions::from_mode(0o700)).unwrap();
        Self(home)
    }
    fn write(&self, path: &str, body: &[u8]) {
        let path = self.0.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, body).unwrap();
    }
    fn opts(&self) -> MigrateOptions {
        MigrateOptions {
            home: self.0.clone(),
            db: self.0.join("db/comandos.sqlite3"),
            dry_run: false,
            resume: false,
            domains: None,
            now_ms: 1_790_000_000_000,
        }
    }
    fn all() -> Self {
        let h = Self::new();
        for (path, body) in [
            (".claude/hooks/app-tabs.json", "{ \"pane\": \"ñ\" }\n"),
            (
                ".claude/hooks/snippets.json",
                "[\n {\"body\":\"hola\"}\n]\n",
            ),
            (".claude/hooks/webterm-enabled", "1\n"),
            (".claude/hooks/pane-models.txt", "%1 = opus\n"),
            (".claude/hooks/state/p--s--1.json", "{\"status\":\"done\"}"),
            (".claude/hooks/native-processes/123.json", "{\"pid\":123}"),
            (
                ".claude/hooks/events.jsonl",
                "{\"event\":1}\n{\"event\":2}\n{\"partial\":",
            ),
            (".claude/hooks/ui-events.jsonl", "{\"kind\":\"ui\"}\n"),
            (".claude/hooks/focus-queue.jsonl", "{\"pane\":\"%1\"}\n"),
            (
                ".claude/hooks/app-sessions-v2.json",
                "{\"saved_at\":1790000000,\"sessions\":[]}",
            ),
            (
                ".claude/hooks/app-sessions-v2.json.bak",
                "{\"saved_at\":1789999900,\"sessions\":[]}",
            ),
            (
                ".claude/hooks/app-sessions-v2.json.history/001789999900.json",
                "{\"sessions\":[]}",
            ),
            (
                ".claude/hooks/app-sessions-v2.json.history/000000000001.json",
                "{\"old\":true}",
            ),
            (".claude/hooks/app-focus.json", "{\"pane\":\"%1\"}"),
            (
                ".local/state/comandos/extensions/sizes/model.json",
                "{\"size\":42}",
            ),
            (
                ".local/state/comandos/extensions/snapshot.json",
                "{\"tools\":[]}",
            ),
            (".claude/hooks/providers.env", "SECRET=unchanged"),
            (
                ".claude/hooks/operator/actions.sqlite",
                "SQLite-source-pending-S4",
            ),
        ] {
            h.write(path, body.as_bytes());
        }
        h
    }
    fn conn(&self) -> rusqlite::Connection {
        unified::open_unified(&self.opts().db).unwrap()
    }
    fn counts(&self) -> Vec<i64> {
        let c = self.conn();
        [
            "documents",
            "session_status",
            "native_processes",
            "log_lines",
            "layout_snapshots",
            "app_commands",
        ]
        .iter()
        .map(|table| {
            c.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
                .unwrap()
        })
        .collect()
    }
}
impl Drop for Home {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn tree(path: &Path) -> Vec<(PathBuf, String, u32, i64, i64, u64)> {
    use sha2::{Digest, Sha256};
    use std::os::unix::fs::MetadataExt;
    let mut out = vec![];
    for entry in fs::read_dir(path).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            out.extend(tree(&path));
        } else {
            let m = fs::metadata(&path).unwrap();
            out.push((
                path.clone(),
                format!("{:x}", Sha256::digest(fs::read(&path).unwrap())),
                m.mode(),
                m.mtime(),
                m.mtime_nsec(),
                m.ino(),
            ));
        }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

#[test]
fn migrate_is_idempotent_and_preserves_original_formats() {
    let h = Home::all();
    let first = migrate(&h.opts()).unwrap();
    assert!(first.steps.iter().any(|s| s.detail.contains("incompleta")));
    assert!(first.steps.iter().any(|s| s.detail.contains("retención")));
    let counts = h.counts();
    migrate(&h.opts()).unwrap();
    assert_eq!(h.counts(), counts);
    assert_eq!(counts, vec![6, 1, 1, 4, 3, 1]);
    let c = h.conn();
    assert_eq!(
        unified::doc_get(&c, "hooks/app-tabs.json")
            .unwrap()
            .unwrap()
            .body,
        fs::read(h.0.join(".claude/hooks/app-tabs.json")).unwrap()
    );
    assert_eq!(
        unified::mode_of(Some(&c), "session-status").unwrap(),
        Mode::Mirror
    );
    let manifest = first.backup_dir.unwrap().join("manifest.json");
    verify_backup(&manifest).unwrap();
    assert!(
        !fs::read_to_string(manifest)
            .unwrap()
            .contains("actions.sqlite")
    );
}
#[test]
fn dry_run_touches_nothing_in_home() {
    let h = Home::all();
    let before = tree(&h.0);
    let mut opts = h.opts();
    opts.dry_run = true;
    assert!(migrate(&opts).unwrap().backup_dir.is_none());
    assert_eq!(tree(&h.0), before);
}
#[test]
fn resume_after_interruption_commits_sources_and_journal_together() {
    let h = Home::all();
    let c = h.conn();
    c.execute_batch("CREATE TRIGGER fail_import BEFORE INSERT ON native_processes BEGIN SELECT RAISE(ABORT,'injected interruption'); END").unwrap();
    assert!(migrate(&h.opts()).is_err());
    let running: String = c
        .query_row(
            "SELECT run_id FROM migration_runs WHERE status='running'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        c.query_row::<i64, _, _>("SELECT COUNT(*) FROM native_processes", [], |r| r.get(0))
            .unwrap(),
        0
    );
    c.execute_batch("DROP TRIGGER fail_import").unwrap();
    let mut opts = h.opts();
    opts.resume = true;
    let report = migrate(&opts).unwrap();
    assert_eq!(report.run_id, running);
    assert!(report.steps.iter().any(|s| s.status == "skipped"));
    assert_eq!(h.counts(), vec![6, 1, 1, 4, 3, 1]);
}
#[test]
fn backup_manifest_rehash_detects_tampering_and_blocks_resume() {
    let h = Home::all();
    let report = migrate(&h.opts()).unwrap();
    let backup = report.backup_dir.unwrap();
    let manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(backup.join("manifest.json")).unwrap()).unwrap();
    fs::write(
        manifest["entries"][0]["copy"].as_str().unwrap(),
        b"tampered",
    )
    .unwrap();
    assert!(verify_backup(&backup.join("manifest.json")).is_err());
    let c = h.conn();
    c.execute("UPDATE migration_runs SET status='running'", [])
        .unwrap();
    let mut opts = h.opts();
    opts.resume = true;
    assert!(migrate(&opts).is_err());
}
#[test]
fn verify_reports_byte_difference_and_missing_row_by_name() {
    let h = Home::all();
    migrate(&h.opts()).unwrap();
    let c = h.conn();
    h.write(".claude/hooks/snippets.json", b"edited");
    c.execute("DELETE FROM session_status", []).unwrap();
    assert!(
        verify(&h.0, &c, "ui-docs")
            .unwrap()
            .mismatches
            .iter()
            .any(|s| s.contains("hooks/snippets.json"))
    );
    assert!(
        verify(&h.0, &c, "session-status")
            .unwrap()
            .mismatches
            .iter()
            .any(|s| s.contains("p--s--1.json"))
    );
}
#[test]
fn unified_and_sealed_domains_are_never_demoted_or_backfilled() {
    let h = Home::all();
    let c = h.conn();
    unified::doc_put(
        &c,
        "hooks/snippets.json",
        "ui-docs",
        b"authoritative",
        unified::Origin::Unified,
        i64::MAX,
    )
    .unwrap();
    unified::set_mode(&c, "ui-docs", Mode::Sealed, "test", 1).unwrap();
    unified::set_mode(&c, "tabs", Mode::Unified, "test", 1).unwrap();
    migrate(&h.opts()).unwrap();
    assert_eq!(unified::mode_of(Some(&c), "ui-docs").unwrap(), Mode::Sealed);
    assert_eq!(unified::mode_of(Some(&c), "tabs").unwrap(), Mode::Unified);
    assert_eq!(
        unified::doc_get(&c, "hooks/snippets.json")
            .unwrap()
            .unwrap()
            .body,
        b"authoritative"
    );
}

#[test]
fn missing_sealed_database_is_rejected_before_recreating_any_state() {
    let h = Home::all();
    let c = h.conn();
    unified::set_mode(&c, "ui-docs", Mode::Sealed, "test", 1).unwrap();
    drop(c);
    fs::remove_file(h.opts().db).unwrap();
    let before = tree(&h.0);
    assert!(migrate(&h.opts()).is_err());
    assert_eq!(tree(&h.0), before);
}
#[test]
fn future_schema_is_rejected_unchanged_for_real_and_dry_run() {
    let h = Home::all();
    let c = h.conn();
    c.pragma_update(None, "user_version", 12).unwrap();
    drop(c);
    let before = tree(&h.0);
    assert!(migrate(&h.opts()).is_err());
    let mut opts = h.opts();
    opts.dry_run = true;
    assert!(migrate(&opts).is_err());
    assert_eq!(tree(&h.0), before);
}
#[test]
fn dry_run_existing_wal_uses_snapshot_without_source_shm_or_metadata_changes() {
    let h = Home::all();
    let c = h.conn();
    c.execute_batch("PRAGMA wal_autocheckpoint=0").unwrap();
    unified::doc_put(
        &c,
        "hooks/snippets.json",
        "ui-docs",
        b"newest",
        unified::Origin::Mirror,
        i64::MAX,
    )
    .unwrap();
    let before = tree(&h.0);
    let mut opts = h.opts();
    opts.dry_run = true;
    let report = migrate(&opts).unwrap();
    assert!(
        report
            .steps
            .iter()
            .any(|s| s.source == "H/snippets.json" && s.rows == 0)
    );
    assert_eq!(tree(&h.0), before);
}
#[test]
fn verify_detects_extra_layout_logs_and_commands_without_files() {
    let h = Home::all();
    migrate(&h.opts()).unwrap();
    let c = h.conn();
    fs::remove_file(h.0.join(".claude/hooks/app-sessions-v2.json")).unwrap();
    fs::remove_file(h.0.join(".claude/hooks/ui-events.jsonl")).unwrap();
    fs::remove_file(h.0.join(".claude/hooks/app-focus.json")).unwrap();
    assert!(
        verify(&h.0, &c, "layout")
            .unwrap()
            .mismatches
            .iter()
            .any(|s| s.contains("current"))
    );
    assert!(
        verify(&h.0, &c, "logs")
            .unwrap()
            .mismatches
            .iter()
            .any(|s| s.contains("ui-events"))
    );
    assert!(
        verify(&h.0, &c, "app-commands")
            .unwrap()
            .mismatches
            .iter()
            .any(|s| s.contains("focus"))
    );
}
#[test]
fn source_symlinks_and_backup_path_escape_fail_closed() {
    let h = Home::all();
    fs::remove_file(h.0.join(".claude/hooks/snippets.json")).unwrap();
    std::os::unix::fs::symlink(
        h.0.join(".claude/hooks/prefs.json"),
        h.0.join(".claude/hooks/snippets.json"),
    )
    .unwrap();
    let mut opts = h.opts();
    opts.dry_run = true;
    assert!(migrate(&opts).is_err());
    fs::remove_file(h.0.join(".claude/hooks/snippets.json")).unwrap();
    h.write(".claude/hooks/snippets.json", b"original");
    let report = migrate(&h.opts()).unwrap();
    let manifest = report.backup_dir.unwrap().join("manifest.json");
    let mut value: serde_json::Value =
        serde_json::from_slice(&fs::read(&manifest).unwrap()).unwrap();
    value["entries"][0]["copy"] =
        h.0.join(".claude/hooks/snippets.json")
            .to_string_lossy()
            .to_string()
            .into();
    fs::write(&manifest, serde_json::to_vec(&value).unwrap()).unwrap();
    assert!(verify_backup(&manifest).is_err());
}
#[test]
fn resume_reimports_changed_source_and_backs_up_new_source() {
    let h = Home::all();
    let report = migrate(&h.opts()).unwrap();
    let c = h.conn();
    c.execute(
        "UPDATE migration_runs SET status='running' WHERE run_id=?1",
        [&report.run_id],
    )
    .unwrap();
    h.write(".claude/hooks/state/new.json", b"new-file");
    let source = h.0.join(".claude/hooks/snippets.json");
    fs::write(&source, b"changed-after-interruption").unwrap();
    fs::File::open(&source)
        .unwrap()
        .set_modified(UNIX_EPOCH + std::time::Duration::from_secs(2_000_000_000))
        .unwrap();
    let mut opts = h.opts();
    opts.resume = true;
    let resumed = migrate(&opts).unwrap();
    assert_eq!(
        unified::doc_get(&c, "hooks/snippets.json")
            .unwrap()
            .unwrap()
            .body,
        b"changed-after-interruption"
    );
    assert!(
        fs::read_to_string(resumed.backup_dir.unwrap().join("manifest.json"))
            .unwrap()
            .contains("new.json")
    );
}

#[test]
fn journal_failure_rolls_back_source_rows_and_is_resumable() {
    let h = Home::all();
    let c = h.conn();
    c.execute_batch("CREATE TRIGGER fail_receipt BEFORE INSERT ON migration_steps WHEN NEW.source='H/app-focus.json' BEGIN SELECT RAISE(ABORT,'receipt failure'); END").unwrap();
    assert!(migrate(&h.opts()).is_err());
    assert_eq!(
        c.query_row::<i64, _, _>("SELECT COUNT(*) FROM app_commands", [], |r| r.get(0))
            .unwrap(),
        0
    );
    assert_eq!(
        c.query_row::<i64, _, _>(
            "SELECT COUNT(*) FROM migration_steps WHERE source='H/app-focus.json'",
            [],
            |r| r.get(0)
        )
        .unwrap(),
        0
    );
    c.execute_batch("DROP TRIGGER fail_receipt").unwrap();
    let mut opts = h.opts();
    opts.resume = true;
    migrate(&opts).unwrap();
    assert_eq!(h.counts(), vec![6, 1, 1, 4, 3, 1]);
}

#[test]
fn mirror_is_committed_before_backfill_and_shared_file_lock_is_respected() {
    use comandos_store::files::FileLock;
    use std::sync::mpsc;
    let h = Home::all();
    let c = h.conn();
    c.execute_batch("CREATE TRIGGER require_mirror BEFORE INSERT ON session_status WHEN COALESCE((SELECT mode FROM domain_modes WHERE domain='session-status'),'legacy')!='mirror' BEGIN SELECT RAISE(ABORT,'not mirror'); END").unwrap();
    let lock = FileLock::exclusive(&h.0.join(".claude/hooks/snippets.json.lock")).unwrap();
    let opts = h.opts();
    let (sent, received) = mpsc::channel();
    let thread = std::thread::spawn(move || {
        sent.send(migrate(&opts)).unwrap();
    });
    assert!(
        received
            .recv_timeout(std::time::Duration::from_millis(50))
            .is_err()
    );
    drop(lock);
    assert!(
        received
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap()
            .is_ok()
    );
    thread.join().unwrap();
    assert_eq!(
        unified::mode_of(Some(&c), "session-status").unwrap(),
        Mode::Mirror
    );
}

#[test]
fn target_hardlinks_and_controls_are_excluded_from_backup_and_backfill() {
    let h = Home::all();
    let c = h.conn();
    drop(c);
    fs::hard_link(h.opts().db, h.0.join(".claude/hooks/prefs.json")).unwrap();
    let report = migrate(&h.opts()).unwrap();
    assert!(!report.steps.iter().any(|s| s.source == "H/prefs.json"));
    let manifest = fs::read_to_string(report.backup_dir.unwrap().join("manifest.json")).unwrap();
    assert!(!manifest.contains("prefs.json"));
    let c = h.conn();
    assert!(unified::doc_get(&c, "hooks/prefs.json").unwrap().is_none());
}

#[test]
fn status_uses_read_only_snapshot_without_creating_sqlite_sidecars() {
    let h = Home::all();
    migrate(&h.opts()).unwrap();
    let before = tree(&h.0);
    let status = comandos_store::migrate::journal::status(&h.0, &h.opts().db).unwrap();
    assert!(status["domains"].is_array());
    assert_eq!(tree(&h.0), before);
}

#[test]
fn selected_empty_domain_is_prepared_for_future_mirror_writes() {
    let h = Home::new();
    let mut opts = h.opts();
    opts.domains = Some(vec!["ui-docs".into()]);
    migrate(&opts).unwrap();
    let c = h.conn();
    assert_eq!(unified::mode_of(Some(&c), "ui-docs").unwrap(), Mode::Mirror);
    assert_eq!(unified::mode_of(Some(&c), "tabs").unwrap(), Mode::Legacy);
}

#[test]
fn partial_resume_keeps_run_running_until_other_sources_finish() {
    let h = Home::all();
    let c = h.conn();
    c.execute_batch("CREATE TRIGGER stop BEFORE INSERT ON native_processes BEGIN SELECT RAISE(ABORT,'stop'); END").unwrap();
    assert!(migrate(&h.opts()).is_err());
    c.execute_batch("DROP TRIGGER stop").unwrap();
    let mut opts = h.opts();
    opts.resume = true;
    opts.domains = Some(vec!["ui-docs".into()]);
    let report = migrate(&opts).unwrap();
    let status: String = c
        .query_row(
            "SELECT status FROM migration_runs WHERE run_id=?1",
            [&report.run_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(status, "running");
    opts.domains = None;
    migrate(&opts).unwrap();
    let status: String = c
        .query_row(
            "SELECT status FROM migration_runs WHERE run_id=?1",
            [&report.run_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(status, "done");
}
#[test]
fn resume_preserves_copy_left_before_manifest_extension_commit() {
    let h = Home::all();
    let report = migrate(&h.opts()).unwrap();
    let c = h.conn();
    c.execute(
        "UPDATE migration_runs SET status='running' WHERE run_id=?1",
        [&report.run_id],
    )
    .unwrap();
    h.write(".claude/hooks/state/new.json", b"new source");
    let backup = report.backup_dir.unwrap();
    let orphan = backup.join("H/state/new.json");
    fs::write(&orphan, b"orphan copy before manifest commit").unwrap();
    let mut opts = h.opts();
    opts.resume = true;
    migrate(&opts).unwrap();
    assert_eq!(
        fs::read(orphan).unwrap(),
        b"orphan copy before manifest commit"
    );
    verify_backup(&backup.join("manifest.json")).unwrap();
}

#[test]
fn extension_sizes_document_and_directory_variants_are_both_supported() {
    let h = Home::new();
    h.write(".local/state/comandos/extensions/sizes", b"{\"model\":42}");
    let mut opts = h.opts();
    opts.domains = Some(vec!["extensions".into()]);
    migrate(&opts).unwrap();
    let c = h.conn();
    assert_eq!(
        unified::doc_get(&c, "state/extensions/sizes")
            .unwrap()
            .unwrap()
            .body,
        b"{\"model\":42}"
    );
}

#[test]
fn changed_done_source_outside_partial_resume_keeps_run_open() {
    let h = Home::all();
    let c = h.conn();
    c.execute_batch("CREATE TRIGGER stop_late BEFORE INSERT ON documents WHEN NEW.name='hooks/snippets.json' BEGIN SELECT RAISE(ABORT,'late interruption'); END").unwrap();
    assert!(migrate(&h.opts()).is_err());
    let id: String = c
        .query_row(
            "SELECT run_id FROM migration_runs WHERE status='running'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    c.execute_batch("DROP TRIGGER stop_late").unwrap();
    let tabs = h.0.join(".claude/hooks/app-tabs.json");
    fs::write(&tabs, b"changed-tabs").unwrap();
    fs::File::open(&tabs)
        .unwrap()
        .set_modified(UNIX_EPOCH + std::time::Duration::from_secs(2_000_000_000))
        .unwrap();
    let mut opts = h.opts();
    opts.resume = true;
    opts.domains = Some(vec!["ui-docs".into()]);
    migrate(&opts).unwrap();
    let status: String = c
        .query_row(
            "SELECT status FROM migration_runs WHERE run_id=?1",
            [&id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(status, "running");
    assert_ne!(
        unified::doc_get(&c, "hooks/app-tabs.json")
            .unwrap()
            .unwrap()
            .body,
        b"changed-tabs"
    );
    opts.domains = None;
    migrate(&opts).unwrap();
    assert_eq!(
        unified::doc_get(&c, "hooks/app-tabs.json")
            .unwrap()
            .unwrap()
            .body,
        b"changed-tabs"
    );
    assert!(verify(&h.0, &c, "tabs").unwrap().mismatches.is_empty());
}

#[test]
fn partial_resume_keeps_removed_manifest_source_running() {
    let h = Home::new();
    h.write(".claude/hooks/app-tabs.json", b"original");
    h.write(".claude/hooks/snippets.json", b"[]");
    let c = h.conn();
    c.execute_batch("CREATE TRIGGER stop_late BEFORE INSERT ON documents WHEN NEW.name='hooks/snippets.json' BEGIN SELECT RAISE(ABORT,'stop'); END").unwrap();
    assert!(migrate(&h.opts()).is_err());
    c.execute_batch("DROP TRIGGER stop_late").unwrap();
    fs::remove_file(h.0.join(".claude/hooks/app-tabs.json")).unwrap();
    let mut opts = h.opts();
    opts.resume = true;
    opts.domains = Some(vec!["ui-docs".into()]);
    let report = migrate(&opts).unwrap();
    let status = || {
        c.query_row(
            "SELECT status FROM migration_runs WHERE run_id=?1",
            [&report.run_id],
            |r| r.get::<_, String>(0),
        )
        .unwrap()
    };
    assert_eq!(
        status(),
        "running",
        "removed manifest source cannot be verified"
    );
    h.write(".claude/hooks/app-tabs.json", b"original");
    opts.domains = None;
    migrate(&opts).unwrap();
    assert_eq!(status(), "done");
}

#[test]
fn completion_exempts_authoritative_domains_without_resurrecting_files() {
    for mode in [Mode::Unified, Mode::Sealed] {
        let h = Home::new();
        h.write(".claude/hooks/app-tabs.json", b"original");
        h.write(".claude/hooks/snippets.json", b"[]");
        let c = h.conn();
        c.execute_batch("CREATE TRIGGER stop_late BEFORE INSERT ON documents WHEN NEW.name='hooks/snippets.json' BEGIN SELECT RAISE(ABORT,'stop'); END").unwrap();
        assert!(migrate(&h.opts()).is_err());
        c.execute_batch("DROP TRIGGER stop_late").unwrap();
        unified::set_mode(&c, "tabs", mode, "private-test", 100).unwrap();
        let tabs = h.0.join(".claude/hooks/app-tabs.json");
        let lock = h.0.join(".claude/hooks/app-tabs.json.lock");
        fs::remove_file(&tabs).unwrap();
        fs::remove_file(&lock).unwrap();
        let mut opts = h.opts();
        opts.resume = true;
        opts.domains = Some(vec!["ui-docs".into()]);
        let report = migrate(&opts).unwrap();
        assert_eq!(
            c.query_row(
                "SELECT status FROM migration_runs WHERE run_id=?1",
                [&report.run_id],
                |r| r.get::<_, String>(0)
            )
            .unwrap(),
            "done"
        );
        assert_eq!(unified::mode_of(Some(&c), "tabs").unwrap(), mode);
        assert!(!tabs.exists());
        assert!(!lock.exists());
    }
}
