use comandos_store::{
    domains::{DomainStore, LayoutSnapshot},
    unified::{self, Mode},
};
use serde_json::{Value, json};
use std::{
    fs,
    os::unix::fs::DirBuilderExt,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};
static SEQ: AtomicU64 = AtomicU64::new(0);
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let home = std::env::temp_dir().join(format!(
            "comandos-layout-{}-{}",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::Relaxed)
        ));
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&home)
            .unwrap();
        Self(home)
    }
    fn layout(&self) -> LayoutSnapshot<'_> {
        LayoutSnapshot {
            home: &self.0,
            file: self.0.join(".claude/hooks/app-sessions-v2.json"),
        }
    }
    fn path(&self) -> PathBuf {
        self.0.join(".local/share/comandos/comandos.sqlite3")
    }
    fn tabs(&self) -> comandos_store::domains::DocHandle<'_> {
        DomainStore { home: &self.0 }.document(
            "hooks/app-tabs.json",
            "tabs",
            self.0.join(".claude/hooks/app-tabs.json"),
        )
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}
#[test]
fn missing_layout_and_tabs_read_never_create_home_controls() {
    let f = Fixture::new();
    assert_eq!(
        f.layout().read_readonly().unwrap(),
        json!({"version":2,"sessions":{}})
    );
    assert_eq!(f.tabs().read_readonly().unwrap(), None);
    assert_eq!(fs::read_dir(&f.0).unwrap().count(), 0);
}
#[test]
fn layout_generations_follow_all_four_modes_and_watch_gate() {
    for mode in [Mode::Legacy, Mode::Mirror, Mode::Unified, Mode::Sealed] {
        let f = Fixture::new();
        let db = unified::open_unified(&f.path()).unwrap();
        unified::set_mode(&db, "layout", mode, "fixture", 1).unwrap();
        let first = json!({"version":2,"sessions":{},"saved_at":600});
        let next = json!({"version":2,"sessions":{},"saved_at":601});
        assert!(f.layout().write_when(&first, 600, || true).unwrap());
        assert!(!f.layout().write_when(&next, 601, || false).unwrap());
        assert_eq!(f.layout().read_readonly().unwrap(), first);
        assert!(f.layout().write_when(&next, 601, || true).unwrap());
        assert_eq!(f.layout().read_readonly().unwrap(), next);
        let file = f.layout().file;
        if mode == Mode::Sealed {
            assert!(!file.exists());
            assert!(!file.with_extension("json.bak").exists());
        } else {
            let previous: Value =
                serde_json::from_slice(&fs::read(file.with_extension("json.bak")).unwrap())
                    .unwrap();
            assert_eq!(previous, first);
            assert_eq!(
                fs::read_dir(file.with_extension("json.history"))
                    .unwrap()
                    .count(),
                1
            );
        }
    }
}

#[test]
fn minute_collision_preserves_first_archive_and_prunes_only_on_new_archive() {
    for mode in [Mode::Legacy, Mode::Mirror, Mode::Unified, Mode::Sealed] {
        let f = Fixture::new();
        let db = unified::open_unified(&f.path()).unwrap();
        unified::set_mode(&db, "layout", mode, "fixture", 1).unwrap();
        let first = json!({"version":2,"sessions":{},"saved_at":740045});
        let second = json!({"version":2,"sessions":{},"saved_at":740105});
        let third = json!({"version":2,"sessions":{},"saved_at":740165});
        let sentinel = json!({"version":2,"sessions":{},"saved_at":740040,"sentinel":"earliest"});
        f.layout().write_when(&first, 740045, || true).unwrap();
        if mode != Mode::Sealed {
            let history = f.layout().file.with_extension("json.history");
            fs::DirBuilder::new().mode(0o700).create(&history).unwrap();
            fs::write(history.join("000000740040.json"), sentinel.to_string()).unwrap();
            fs::write(history.join("000000000100.json"), b"ancient").unwrap();
        }
        if mode != Mode::Legacy {
            unified::layout_put(
                &db,
                unified::Generation::Minute,
                740040,
                sentinel.to_string().as_bytes(),
            )
            .unwrap();
            unified::layout_put(&db, unified::Generation::Minute, 100, b"ancient").unwrap();
        }
        f.layout().write_when(&second, 740105, || true).unwrap();
        if mode != Mode::Sealed {
            assert!(
                f.layout()
                    .file
                    .with_extension("json.history")
                    .join("000000000100.json")
                    .exists()
            );
        }
        if mode != Mode::Legacy {
            assert_eq!(
                db.query_row(
                    "SELECT COUNT(*) FROM layout_snapshots WHERE generation='minute' AND stamp=100",
                    [],
                    |r| r.get::<_, i64>(0)
                )
                .unwrap(),
                1
            );
        }
        f.layout().write_when(&third, 740165, || true).unwrap();
        if mode != Mode::Sealed {
            let history = f.layout().file.with_extension("json.history");
            assert!(!history.join("000000000100.json").exists());
            assert_eq!(
                serde_json::from_slice::<Value>(
                    &fs::read(history.join("000000740040.json")).unwrap()
                )
                .unwrap(),
                sentinel
            );
            assert_eq!(
                serde_json::from_slice::<Value>(
                    &fs::read(history.join("000000740100.json")).unwrap()
                )
                .unwrap(),
                second
            );
        }
        if mode != Mode::Legacy {
            assert_eq!(
                db.query_row(
                    "SELECT COUNT(*) FROM layout_snapshots WHERE generation='minute' AND stamp=100",
                    [],
                    |r| r.get::<_, i64>(0)
                )
                .unwrap(),
                0
            );
            let bytes: Vec<u8> = db
                .query_row(
                    "SELECT body FROM layout_snapshots WHERE generation='minute' AND stamp=740040",
                    [],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(serde_json::from_slice::<Value>(&bytes).unwrap(), sentinel);
        }
    }
}

type TreeStamp = (PathBuf, u64, u32, i64, i64, Vec<u8>);
fn tree(root: &std::path::Path) -> Vec<TreeStamp> {
    use std::os::unix::fs::MetadataExt;
    fn walk(path: &std::path::Path, out: &mut Vec<TreeStamp>) {
        let Ok(m) = fs::symlink_metadata(path) else {
            return;
        };
        let bytes = if m.is_file() {
            fs::read(path).unwrap()
        } else {
            vec![]
        };
        out.push((
            path.to_owned(),
            m.ino(),
            m.mode(),
            m.mtime(),
            m.mtime_nsec(),
            bytes,
        ));
        if m.is_dir() {
            for e in fs::read_dir(path).unwrap() {
                walk(&e.unwrap().path(), out)
            }
        }
    }
    let mut out = vec![];
    walk(root, &mut out);
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}
fn read_unchanged(f: &Fixture) -> comandos_store::Result<(Value, Option<Vec<u8>>)> {
    let before = tree(&f.0);
    let result = (|| Ok((f.layout().read_readonly()?, f.tabs().read_readonly()?)))();
    assert_eq!(
        tree(&f.0),
        before,
        "readonly must preserve bytes/inodes/modes/mtime and not create controls"
    );
    result
}
#[test]
fn readonly_layout_tabs_obey_modes_live_wal_and_current_previous_fallback() {
    for mode in [Mode::Legacy, Mode::Mirror, Mode::Unified, Mode::Sealed] {
        let f = Fixture::new();
        fs::create_dir_all(f.layout().file.parent().unwrap()).unwrap();
        let legacy = json!({"version":2,"sessions":{},"saved_at":100});
        fs::write(&f.layout().file, legacy.to_string()).unwrap();
        fs::write(&f.tabs().file, b"legacy tabs").unwrap();
        let db = unified::open_unified(&f.path()).unwrap();
        let authoritative = json!({"version":2,"sessions":{},"saved_at":200});
        unified::layout_put(
            &db,
            unified::Generation::Current,
            200,
            authoritative.to_string().as_bytes(),
        )
        .unwrap();
        unified::doc_put(
            &db,
            "hooks/app-tabs.json",
            "tabs",
            b"db tabs",
            unified::Origin::Import,
            200,
        )
        .unwrap();
        for domain in ["layout", "tabs"] {
            unified::set_mode(&db, domain, mode, "fixture", 1).unwrap();
        }
        let expected = if matches!(mode, Mode::Unified | Mode::Sealed) {
            (authoritative.clone(), Some(b"db tabs".to_vec()))
        } else {
            (legacy.clone(), Some(b"legacy tabs".to_vec()))
        };
        assert_eq!(read_unchanged(&f).unwrap(), expected);
        if matches!(mode, Mode::Unified | Mode::Sealed) {
            unified::layout_put(
                &db,
                unified::Generation::Previous,
                190,
                legacy.to_string().as_bytes(),
            )
            .unwrap();
            unified::layout_put(&db, unified::Generation::Current, 201, b"invalid").unwrap();
            assert_eq!(read_unchanged(&f).unwrap().0, legacy);
        } else {
            fs::write(
                f.layout().file.with_extension("json.bak"),
                authoritative.to_string(),
            )
            .unwrap();
            fs::write(&f.layout().file, b"invalid").unwrap();
            assert_eq!(read_unchanged(&f).unwrap().0, authoritative);
        }
        drop(db);
        let lock = comandos_store::snapshot_files::suffix(&f.path(), ".domain-modes.lock");
        fs::remove_file(lock).unwrap();
        read_unchanged(&f).unwrap();
        assert!(!comandos_store::snapshot_files::suffix(&f.path(), ".domain-modes.lock").exists());
    }
}
#[test]
fn readonly_future_schema_stale_seal_permissions_and_mode_contention_fail_closed() {
    use std::os::unix::fs::PermissionsExt;
    let f = Fixture::new();
    let db = unified::open_unified(&f.path()).unwrap();
    let seal = comandos_store::snapshot_files::suffix(&f.path(), ".sealed-layout");
    for bytes in [b"comandos-state-protocol-2:sealed\n".as_slice(), b"bad"] {
        fs::write(&seal, bytes).unwrap();
        assert!(read_unchanged(&f).is_err());
    }
    fs::remove_file(&seal).unwrap();
    db.execute_batch("PRAGMA user_version=999999").unwrap();
    assert!(read_unchanged(&f).is_err());
    assert!(
        f.layout()
            .write_when(&json!({"version":2,"sessions":{}}), 1, || true)
            .is_err()
    );
    db.execute_batch("PRAGMA user_version=11").unwrap();
    fs::set_permissions(f.path(), fs::Permissions::from_mode(0o644)).unwrap();
    assert!(read_unchanged(&f).is_err());
    fs::set_permissions(f.path(), fs::Permissions::from_mode(0o600)).unwrap();
    unified::set_mode(&db, "layout", Mode::Unified, "fixture", 1).unwrap();
    drop(db);
    let held = fs::File::open(comandos_store::snapshot_files::suffix(
        &f.path(),
        ".domain-modes.lock",
    ))
    .unwrap();
    held.lock().unwrap();
    assert!(read_unchanged(&f).is_err());
    held.unlock().unwrap();
    read_unchanged(&f).unwrap();
    let db = unified::open_unified(&f.path()).unwrap();
    unified::set_mode(&db, "layout", Mode::Sealed, "fixture", 1).unwrap();
    drop(db);
    fs::remove_file(f.path()).unwrap();
    assert!(read_unchanged(&f).is_err());
    let before = tree(&f.0);
    assert!(
        f.layout()
            .write_when(&json!({"version":2,"sessions":{}}), 1, || true)
            .is_err()
    );
    assert_eq!(tree(&f.0), before);
}
#[test]
fn lost_watch_identity_at_transaction_boundary_cannot_publish_unified_generations() {
    for mode in [Mode::Unified, Mode::Sealed] {
        let f = Fixture::new();
        let db = unified::open_unified(&f.path()).unwrap();
        unified::set_mode(&db, "layout", mode, "fixture", 1).unwrap();
        let first = json!({"version":2,"sessions":{},"saved_at":600});
        let next = json!({"version":2,"sessions":{},"saved_at":660});
        f.layout().write_when(&first, 600, || true).unwrap();
        let mut checks = 0;
        assert!(
            !f.layout()
                .write_when(&next, 660, || {
                    checks += 1;
                    checks < 3
                })
                .unwrap()
        );
        assert!(checks >= 3);
        assert_eq!(f.layout().read_readonly().unwrap(), first);
        if mode == Mode::Unified {
            assert_eq!(
                serde_json::from_slice::<Value>(&fs::read(&f.layout().file).unwrap()).unwrap(),
                first
            );
            assert!(!f.layout().file.with_extension("json.bak").exists());
        }
    }
}

#[test]
fn moved_database_is_original_authority_for_readonly_and_sealed_writes() {
    use std::process::Command;
    if let Some(root) = std::env::var_os("COMANDOS_LAYOUT_CHILD") {
        let root = PathBuf::from(root);
        let home = root.join("home");
        let layout = LayoutSnapshot {
            home: &home,
            file: home.join(".claude/hooks/app-sessions-v2.json"),
        };
        let tabs = DomainStore { home: &home }.document(
            "hooks/app-tabs.json",
            "tabs",
            home.join(".claude/hooks/app-tabs.json"),
        );
        let before = tree(&root);
        assert_eq!(layout.read_readonly().unwrap()["saved_at"], json!(700));
        assert_eq!(tabs.read_readonly().unwrap(), Some(b"moved tabs".to_vec()));
        assert_eq!(tree(&root), before);
        let before_home = tree(&home);
        assert!(
            layout
                .write_when(
                    &json!({"version":2,"sessions":{},"saved_at":760}),
                    760,
                    || true
                )
                .unwrap()
        );
        assert_eq!(layout.read_readonly().unwrap()["saved_at"], json!(760));
        assert_eq!(tree(&home), before_home);
        assert!(!home.join(".local").exists());
        return;
    }
    let f = Fixture::new();
    let home = f.0.join("home");
    fs::DirBuilder::new().mode(0o700).create(&home).unwrap();
    fs::create_dir_all(home.join(".claude/hooks")).unwrap();
    fs::write(
        home.join(".claude/hooks/app-sessions-v2.json"),
        br#"{"version":2,"sessions":{},"saved_at":100}"#,
    )
    .unwrap();
    fs::write(home.join(".claude/hooks/app-tabs.json"), b"legacy tabs").unwrap();
    let moved = f.0.join("moved/state.sqlite3");
    let db = unified::open_unified(&moved).unwrap();
    unified::layout_put(
        &db,
        unified::Generation::Current,
        700,
        br#"{"version":2,"sessions":{},"saved_at":700}"#,
    )
    .unwrap();
    unified::doc_put(
        &db,
        "hooks/app-tabs.json",
        "tabs",
        b"moved tabs",
        unified::Origin::Import,
        700,
    )
    .unwrap();
    for domain in ["layout", "tabs"] {
        unified::set_mode(&db, domain, Mode::Sealed, "fixture", 1).unwrap();
    }
    drop(db);
    let mut child = Command::new(std::env::current_exe().unwrap());
    child
        .env_clear()
        .arg("--exact")
        .arg("moved_database_is_original_authority_for_readonly_and_sealed_writes")
        .arg("--nocapture")
        .env("COMANDOS_LAYOUT_CHILD", &f.0)
        .env("COMANDOS_DB", &moved)
        .env("HOME", &home);
    for (key, dir) in [
        ("XDG_DATA_HOME", "data"),
        ("XDG_CONFIG_HOME", "config"),
        ("XDG_CACHE_HOME", "cache"),
        ("XDG_STATE_HOME", "state"),
        ("XDG_RUNTIME_DIR", "run"),
        ("TMPDIR", "tmp"),
    ] {
        let path = f.0.join(dir);
        fs::DirBuilder::new().mode(0o700).create(&path).unwrap();
        child.env(key, path);
    }
    let result = child.output().unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let db = unified::open_unified(&moved).unwrap();
    let bytes: Vec<u8> = db
        .query_row(
            "SELECT body FROM layout_snapshots WHERE generation='current'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        serde_json::from_slice::<Value>(&bytes).unwrap()["saved_at"],
        json!(760)
    );
}

#[test]
fn absent_home_read_and_invalid_layout_candidate_do_not_publish() {
    let f = Fixture::new();
    let home = f.0.join("absent-home");
    let layout = LayoutSnapshot {
        home: &home,
        file: home.join(".claude/hooks/app-sessions-v2.json"),
    };
    let doc = DomainStore { home: &home }.document(
        "hooks/app-tabs.json",
        "tabs",
        home.join(".claude/hooks/app-tabs.json"),
    );
    assert_eq!(
        layout.read_readonly().unwrap(),
        json!({"version":2,"sessions":{}})
    );
    assert_eq!(doc.read_readonly().unwrap(), None);
    assert!(!home.exists());
    for mode in [Mode::Legacy, Mode::Mirror, Mode::Unified, Mode::Sealed] {
        let f = Fixture::new();
        let db = unified::open_unified(&f.path()).unwrap();
        unified::set_mode(&db, "layout", mode, "fixture", 1).unwrap();
        let current = json!({"version":2,"sessions":{},"saved_at":600});
        f.layout().write_when(&current, 600, || true).unwrap();
        assert!(
            f.layout()
                .write_when(&json!({"version":2,"sessions":{"local":{}}}), 660, || true)
                .is_err()
        );
        assert_eq!(f.layout().read_readonly().unwrap(), current);
        assert!(!f.layout().file.with_extension("json.bak").exists());
        assert!(!f.layout().file.with_extension("json.history").exists());
    }
}

#[cfg(target_os = "linux")]
#[test]
fn repeated_live_layout_saves_do_not_copy_unrelated_wal_payloads() {
    fn io_bytes() -> (u64, u64) {
        let io = fs::read_to_string("/proc/thread-self/io").unwrap();
        let count = |name: &str| {
            io.lines()
                .find_map(|line| line.strip_prefix(name))
                .unwrap()
                .parse::<u64>()
                .unwrap()
        };
        (count("rchar: "), count("wchar: "))
    }
    let f = Fixture::new();
    let db = unified::open_unified(&f.path()).unwrap();
    db.execute_batch("PRAGMA wal_autocheckpoint=0; CREATE TABLE unrelated_payload(body BLOB); INSERT INTO unrelated_payload VALUES(zeroblob(2097152));").unwrap();
    unified::set_mode(&db, "layout", Mode::Sealed, "fixture", 1).unwrap();
    let snapshot = |stamp| json!({"version":2,"sessions":{},"saved_at":stamp});
    f.layout().write_when(&snapshot(600), 600, || true).unwrap();
    let before = io_bytes();
    for stamp in 601..607 {
        let value = snapshot(stamp);
        f.layout().write_when(&value, stamp, || true).unwrap();
        assert_eq!(f.layout().read_live().unwrap(), value);
    }
    let after = io_bytes();
    let read = after.0 - before.0;
    let written = after.1 - before.1;
    eprintln!("six_layout_saves_read_bytes={read} written_bytes={written}");
    assert!(read < 512 * 1024, "six tiny layout saves read {read} bytes");
    assert!(
        written < 512 * 1024,
        "six tiny layout saves wrote {written} bytes"
    );
}
