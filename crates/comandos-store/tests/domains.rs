//! Contratos S2 sobre HOME y bases privados.
use comandos_store::{
    domains::{DocHandle, LogHandle, StatusDir},
    unified::{self, LogName, Mode},
};
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
};
struct Fixture {
    home: PathBuf,
}
impl Fixture {
    fn new(tag: &str) -> Self {
        let home = std::env::temp_dir().join(format!("cmd-domains-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&home);
        fs::create_dir_all(home.join(".local/share/comandos")).unwrap();
        fs::set_permissions(
            home.join(".local/share/comandos"),
            fs::Permissions::from_mode(0o700),
        )
        .unwrap();
        Self { home }
    }
    fn path(&self) -> PathBuf {
        self.home.join(".local/share/comandos/comandos.sqlite3")
    }
    fn db(&self) -> rusqlite::Connection {
        unified::open_unified(&self.path()).unwrap()
    }
    fn doc(&self) -> DocHandle<'_> {
        let file = self.home.join(".claude/hooks/snippets.json");
        comandos_store::domains::DomainStore { home: &self.home }.document(
            "hooks/snippets.json",
            "ui-docs",
            file,
        )
    }
    fn status(&self) -> StatusDir<'_> {
        StatusDir {
            home: &self.home,
            dir: self.home.join(".claude/hooks/state"),
            domain: "session-status",
        }
    }
    fn log(&self) -> LogHandle<'_> {
        let file = self.home.join(".claude/hooks/events.jsonl");
        LogHandle {
            home: &self.home,
            log: LogName::Events,
            lock: file.with_extension("jsonl.lock"),
            file,
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.home);
    }
}
#[test]
fn legacy_writes_only_file() {
    let f = Fixture::new("legacy");
    let db = f.db();
    f.doc().write(Some(&db), b" raw\0\n", 1).unwrap();
    assert_eq!(fs::read(f.doc().file).unwrap(), b" raw\0\n");
    assert!(
        unified::doc_get(&db, "hooks/snippets.json")
            .unwrap()
            .is_none()
    );
    assert_eq!(unified::mode_of(None, "ui-docs").unwrap(), Mode::Legacy);
}
#[test]
fn mirror_writes_file_then_row_same_lock() {
    let f = Fixture::new("mirror-order");
    let db = f.db();
    unified::set_mode(&db, "ui-docs", Mode::Mirror, "test", 1).unwrap();
    db.execute_batch("CREATE TRIGGER reject_write BEFORE INSERT ON documents BEGIN SELECT RAISE(ABORT,'reject'); END;").unwrap();
    assert!(f.doc().write(Some(&db), b"file-first", 2).is_err());
    assert_eq!(fs::read(f.doc().file).unwrap(), b"file-first");
    assert!(
        unified::doc_get(&db, "hooks/snippets.json")
            .unwrap()
            .is_none()
    );
}
#[test]
fn unified_writes_row_then_file() {
    let f = Fixture::new("unified-order");
    let db = f.db();
    unified::set_mode(&db, "ui-docs", Mode::Unified, "test", 1).unwrap();
    fs::create_dir_all(&f.doc().file).unwrap();
    assert!(f.doc().write(Some(&db), b"row-first", 2).is_err());
    assert_eq!(
        unified::doc_get(&db, "hooks/snippets.json")
            .unwrap()
            .unwrap()
            .body,
        b"row-first"
    );
    assert!(db.is_autocommit());
}
#[test]
fn unified_without_db_falls_back_to_file_read() {
    let f = Fixture::new("fallback");
    let db = f.db();
    unified::set_mode(&db, "ui-docs", Mode::Unified, "test", 1).unwrap();
    f.doc().write(Some(&db), b"last", 2).unwrap();
    assert_eq!(f.doc().read(None).unwrap(), Some(b"last".to_vec()));
}
#[test]
fn sealed_without_db_is_error() {
    let f = Fixture::new("sealed");
    let db = f.db();
    for d in ["ui-docs", "session-status", "logs"] {
        unified::set_mode(&db, d, Mode::Sealed, "test", 1).unwrap();
    }
    f.doc().write(Some(&db), b"only-db", 2).unwrap();
    f.status()
        .write(Some(&db), "pane.json", b"status", 2)
        .unwrap();
    f.log().append(Some(&db), b"log").unwrap();
    assert!(!f.doc().file.exists());
    drop(db);
    fs::remove_file(f.path()).unwrap();
    assert!(f.doc().read(None).is_err());
    assert!(f.doc().write(None, b"resurrection", 3).is_err());
    assert!(f.status().read(None, "pane.json").is_err());
    assert!(f.status().list(None).is_err());
    assert!(
        f.status()
            .write(None, "pane.json", b"resurrection", 3)
            .is_err()
    );
    assert!(f.log().tail(None, 1).is_err());
    assert!(f.log().append(None, b"resurrection").is_err());
    assert!(!f.doc().file.exists());
    assert!(!f.status().dir.exists());
    assert!(!f.log().file.exists());
}
#[test]
fn mirror_never_leaves_row_newer_than_file() {
    let f = Fixture::new("concurrent");
    let db = f.db();
    unified::set_mode(&db, "ui-docs", Mode::Mirror, "test", 1).unwrap();
    drop(db);
    let workers: Vec<_> = (0..2)
        .map(|thread| {
            let home = f.home.clone();
            std::thread::spawn(move || {
                let db =
                    unified::open_unified(&home.join(".local/share/comandos/comandos.sqlite3"))
                        .unwrap();
                let file = home.join(".claude/hooks/snippets.json");
                let h = DocHandle {
                    home: &home,
                    name: "hooks/snippets.json",
                    domain: "ui-docs",
                    lock: file.with_extension("json.lock"),
                    file,
                };
                for i in 0..1000 {
                    h.write(Some(&db), format!("{thread}:{i}").as_bytes(), i)
                        .unwrap();
                }
            })
        })
        .collect();
    for w in workers {
        w.join().unwrap();
    }
    let db = f.db();
    assert_eq!(
        fs::read(f.doc().file).unwrap(),
        unified::doc_get(&db, "hooks/snippets.json")
            .unwrap()
            .unwrap()
            .body
    );
    assert_eq!(
        unified::doc_get(&db, "hooks/snippets.json")
            .unwrap()
            .unwrap()
            .revision,
        2000
    );
}
#[test]
fn statuses_logs_respect_all_modes_and_exact_bytes() {
    for mode in [Mode::Legacy, Mode::Mirror, Mode::Unified, Mode::Sealed] {
        let f = Fixture::new(&format!("collections-{mode:?}"));
        let db = f.db();
        for d in ["session-status", "logs"] {
            unified::set_mode(&db, d, mode, "test", 1).unwrap();
        }
        f.status()
            .write(Some(&db), "p.json", b" \0raw ", 8)
            .unwrap();
        assert_eq!(
            f.status().read(Some(&db), "p.json").unwrap(),
            Some(b" \0raw ".to_vec())
        );
        assert_eq!(
            f.status().list(Some(&db)).unwrap(),
            vec![("p.json".into(), b" \0raw ".to_vec())]
        );
        f.log().append(Some(&db), b"one").unwrap();
        f.log().append(Some(&db), b"two\r").unwrap();
        assert_eq!(f.log().tail(Some(&db), 1).unwrap(), vec![b"two\r".to_vec()]);
        assert!(f.log().tail(Some(&db), 0).unwrap().is_empty());
        assert_eq!(f.status().dir.exists(), mode != Mode::Sealed);
        assert!(f.status().write(Some(&db), "../escape", b"bad", 1).is_err());
    }
}
#[test]
fn modes_audit_and_unseal_are_committed_and_guard_is_conservative() {
    let f = Fixture::new("audit");
    let db = f.db();
    unified::set_mode(&db, "ui-docs", Mode::Sealed, "tester", 123).unwrap();
    assert_eq!(
        db.query_row(
            "SELECT changed_by,changed_at_ms FROM domain_modes WHERE domain='ui-docs'",
            [],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))
        )
        .unwrap(),
        ("tester".into(), 123)
    );
    db.execute_batch("BEGIN IMMEDIATE").unwrap();
    assert!(unified::set_mode(&db, "ui-docs", Mode::Legacy, "rollback", 124).is_err());
    db.execute_batch("ROLLBACK").unwrap();
    assert!(f.doc().write(None, b"bad", 0).is_err());
    unified::set_mode(&db, "ui-docs", Mode::Legacy, "explicit", 125).unwrap();
    f.doc().write(None, b"allowed", 0).unwrap();
    assert!(unified::set_mode(&db, "unknown", Mode::Mirror, "test", 0).is_err());
    assert!(
        unified::set_mode(
            &unified::open_unified(Path::new(":memory:")).unwrap(),
            "ui-docs",
            Mode::Sealed,
            "test",
            0
        )
        .is_err()
    );
}
#[test]
fn failed_seal_and_corrupt_guard_fail_closed() {
    let f = Fixture::new("failed-seal");
    let db = f.db();
    db.execute_batch("CREATE TRIGGER reject_mode BEFORE INSERT ON domain_modes BEGIN SELECT RAISE(ABORT,'reject'); END;").unwrap();
    assert!(unified::set_mode(&db, "ui-docs", Mode::Sealed, "test", 1).is_err());
    assert!(f.doc().read(None).is_err());
    db.execute_batch("DROP TRIGGER reject_mode").unwrap();
    unified::set_mode(&db, "ui-docs", Mode::Legacy, "recovery", 2).unwrap();
    unified::set_mode(&db, "ui-docs", Mode::Sealed, "test", 3).unwrap();
    fs::write(
        unified::seal_guard_path(&f.path(), "ui-docs").unwrap(),
        b"corrupt",
    )
    .unwrap();
    assert!(f.doc().read(None).is_err());
    assert!(f.doc().read(Some(&db)).is_err());
}
#[test]
fn writes_wait_on_existing_file_flock_and_mode_changes_wait_for_write() {
    use comandos_store::files::FileLock;
    use std::sync::{Arc, Barrier};
    let f = Fixture::new("existing-flock");
    let db = f.db();
    unified::set_mode(&db, "ui-docs", Mode::Mirror, "test", 1).unwrap();
    let held = FileLock::exclusive(&f.doc().lock).unwrap();
    let barrier = Arc::new(Barrier::new(2));
    let home = f.home.clone();
    let b = barrier.clone();
    let worker = std::thread::spawn(move || {
        let c =
            unified::open_unified(&home.join(".local/share/comandos/comandos.sqlite3")).unwrap();
        let file = home.join(".claude/hooks/snippets.json");
        let h = DocHandle {
            home: &home,
            name: "hooks/snippets.json",
            domain: "ui-docs",
            lock: file.with_extension("json.lock"),
            file,
        };
        b.wait();
        h.write(Some(&c), b"held", 5).unwrap();
    });
    barrier.wait();
    // El escritor debe haber tomado el candado de modo antes de esperar el flock legado.
    let mode_lock = PathBuf::from(format!("{}.domain-modes.lock", f.path().display()));
    let mut writer_holds_mode = false;
    for _ in 0..100 {
        if FileLock::try_exclusive(&mode_lock).unwrap().is_none() {
            writer_holds_mode = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert!(writer_holds_mode);
    let (done, completed) = std::sync::mpsc::channel();
    let path = f.path();
    let change = std::thread::spawn(move || {
        let c = unified::open_unified(&path).unwrap();
        unified::set_mode(&c, "ui-docs", Mode::Sealed, "concurrent", 6).unwrap();
        done.send(()).unwrap();
    });
    assert!(
        completed
            .recv_timeout(std::time::Duration::from_millis(50))
            .is_err()
    );
    assert!(!f.doc().file.exists());
    assert!(
        unified::doc_get(&db, "hooks/snippets.json")
            .unwrap()
            .is_none()
    );
    drop(held);
    worker.join().unwrap();
    change.join().unwrap();
    completed
        .recv_timeout(std::time::Duration::from_secs(1))
        .unwrap();
    assert_eq!(
        unified::mode_of(Some(&db), "ui-docs").unwrap(),
        Mode::Sealed
    );
    assert!(f.doc().read(None).is_err());
    assert_eq!(
        unified::doc_get(&db, "hooks/snippets.json")
            .unwrap()
            .unwrap()
            .body,
        b"held"
    );
}
#[test]
fn concurrent_set_mode_keeps_guard_consistent() {
    let f = Fixture::new("mode-race");
    let db = f.db();
    unified::set_mode(&db, "ui-docs", Mode::Mirror, "test", 1).unwrap();
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(3));
    let workers: Vec<_> = (0..2)
        .map(|i| {
            let home = f.home.clone();
            let b = barrier.clone();
            std::thread::spawn(move || {
                let c = unified::open_unified(&home.join(".local/share/comandos/comandos.sqlite3"))
                    .unwrap();
                b.wait();
                for n in 0..100 {
                    unified::set_mode(
                        &c,
                        "ui-docs",
                        if (i + n) % 2 == 0 {
                            Mode::Sealed
                        } else {
                            Mode::Mirror
                        },
                        "race",
                        n,
                    )
                    .unwrap();
                }
            })
        })
        .collect();
    barrier.wait();
    for w in workers {
        w.join().unwrap();
    }
    unified::set_mode(&db, "ui-docs", Mode::Sealed, "final", 200).unwrap();
    assert_eq!(
        unified::mode_of(Some(&db), "ui-docs").unwrap(),
        Mode::Sealed
    );
    assert!(f.doc().read(None).is_err());
    f.doc().write(Some(&db), b"final", 201).unwrap();
    assert!(!f.doc().file.exists());
}
#[test]
fn crash_after_unseal_commit_before_guard_removal_remains_blocked() {
    let f = Fixture::new("unseal-crash");
    let db = f.db();
    unified::set_mode(&db, "ui-docs", Mode::Sealed, "test", 1).unwrap();
    db.execute(
        "UPDATE domain_modes SET mode='mirror' WHERE domain='ui-docs'",
        [],
    )
    .unwrap();
    assert!(f.doc().read(Some(&db)).is_err());
    assert!(f.doc().write(None, b"bad", 1).is_err());
    unified::set_mode(&db, "ui-docs", Mode::Mirror, "recover-explicit", 2).unwrap();
    f.doc().write(Some(&db), b"good", 3).unwrap();
}
#[test]
fn dual_write_rejects_callers_uncommitted_transaction() {
    let f = Fixture::new("caller-tx");
    let db = f.db();
    for mode in [Mode::Mirror, Mode::Unified, Mode::Sealed] {
        unified::set_mode(&db, "ui-docs", mode, "test", 1).unwrap();
        db.execute_batch("BEGIN IMMEDIATE").unwrap();
        assert!(f.doc().write(Some(&db), b"uncommitted", 2).is_err());
        db.execute_batch("ROLLBACK").unwrap();
        assert!(!f.doc().file.exists());
        assert!(
            unified::doc_get(&db, "hooks/snippets.json")
                .unwrap()
                .is_none()
        );
    }
}
#[test]
fn missing_database_reads_do_not_create_insecure_future_database_parent() {
    let f = Fixture::new("missing-parent");
    fs::remove_dir_all(f.home.join(".local/share/comandos")).unwrap();
    assert_eq!(f.doc().read(None).unwrap(), None);
    let db = f.db();
    assert_eq!(
        unified::mode_of(Some(&db), "ui-docs").unwrap(),
        Mode::Legacy
    );
}
#[test]
fn raw_newer_or_insecure_connection_cannot_change_modes_or_write_files() {
    let f = Fixture::new("newer-mode");
    let db = f.db();
    db.execute_batch("PRAGMA user_version=12").unwrap();
    assert!(unified::set_mode(&db, "ui-docs", Mode::Sealed, "test", 0).is_err());
    assert!(f.doc().write(Some(&db), b"bad", 0).is_err());
    assert!(!f.doc().file.exists());
    assert!(
        !unified::seal_guard_path(&f.path(), "ui-docs")
            .unwrap()
            .exists()
    );
    db.execute_batch("PRAGMA user_version=11").unwrap();
    fs::set_permissions(
        f.path().parent().unwrap(),
        fs::Permissions::from_mode(0o755),
    )
    .unwrap();
    assert!(unified::set_mode(&db, "ui-docs", Mode::Mirror, "test", 0).is_err());
    assert_eq!(
        db.query_row("SELECT count(*) FROM domain_modes", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        0
    );
    assert_eq!(
        fs::metadata(f.path().parent().unwrap())
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o755
    );
}
#[test]
fn configured_override_seal_guard_is_used_without_connection() {
    let f = Fixture::new("override-guard");
    let path = f.home.join("private-override/state.sqlite3");
    let db = unified::open_unified(&path).unwrap();
    for domain in ["ui-docs", "session-status", "logs"] {
        unified::set_mode(&db, domain, Mode::Sealed, "test", 1).unwrap();
    }
    drop(db);
    fs::remove_file(&path).unwrap();
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "override_guard_child"])
        .env("HOME", &f.home)
        .env("COMANDOS_DB", path)
        .env("S2_OVERRIDE_FIXTURE", &f.home)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{} {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
#[test]
fn override_guard_child() {
    let Some(home) = std::env::var_os("S2_OVERRIDE_FIXTURE") else {
        return;
    };
    let home = PathBuf::from(home);
    let file = home.join(".claude/hooks/snippets.json");
    let doc = DocHandle {
        home: &home,
        name: "hooks/snippets.json",
        domain: "ui-docs",
        lock: file.with_extension("json.lock"),
        file,
    };
    let status = StatusDir {
        home: &home,
        dir: home.join(".claude/hooks/state"),
        domain: "session-status",
    };
    let file = home.join(".claude/hooks/events.jsonl");
    let log = LogHandle {
        home: &home,
        log: LogName::Events,
        lock: file.with_extension("jsonl.lock"),
        file,
    };
    assert!(doc.read(None).is_err());
    assert!(doc.write(None, b"bad", 0).is_err());
    assert!(status.list(None).is_err());
    assert!(status.write(None, "x.json", b"bad", 0).is_err());
    assert!(log.tail(None, 1).is_err());
    assert!(log.append(None, b"bad").is_err());
    assert!(!home.join(".claude").exists());
}
