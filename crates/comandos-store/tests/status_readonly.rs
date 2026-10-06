//! Read authority from the original DB path without creating source artifacts.
use comandos_store::{
    domains::StatusDir,
    unified::{self, Mode, Origin},
};
use std::{
    fs,
    os::unix::fs::{DirBuilderExt, MetadataExt},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        static ID: AtomicU64 = AtomicU64::new(0);
        let home = std::env::temp_dir().join(format!(
            "status-readonly-{}-{}",
            std::process::id(),
            ID.fetch_add(1, Ordering::Relaxed)
        ));
        fs::DirBuilder::new().mode(0o700).create(&home).unwrap();
        Self(home)
    }
    fn dbpath(&self) -> PathBuf {
        self.0.join(".local/share/comandos/comandos.sqlite3")
    }
    fn status(&self) -> StatusDir<'_> {
        StatusDir {
            home: &self.0,
            dir: self.0.join(".claude/hooks/state"),
            domain: "session-status",
        }
    }
    fn legacy(&self) {
        fs::create_dir_all(&self.status().dir).unwrap();
        fs::write(self.status().dir.join("z.json"), b"legacy").unwrap();
    }
    fn check(&self) -> comandos_store::Result<Vec<(String, Vec<u8>)>> {
        let before = tree(&self.0);
        let result = self.status().list_readonly();
        assert_eq!(tree(&self.0), before);
        result
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
type Stamp = (PathBuf, u64, u32, i64, i64, Vec<u8>);
fn tree(root: &Path) -> Vec<Stamp> {
    fn walk(path: &Path, out: &mut Vec<Stamp>) {
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
                walk(&e.unwrap().path(), out);
            }
        }
    }
    let mut out = vec![];
    walk(root, &mut out);
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}
#[test]
fn missing_database_or_home_stays_absent() {
    let f = Fixture::new();
    assert!(f.check().unwrap().is_empty());
    f.legacy();
    assert_eq!(
        f.check().unwrap(),
        vec![("z.json".into(), b"legacy".to_vec())]
    );
    fs::remove_dir_all(&f.0).unwrap();
    assert!(f.check().unwrap().is_empty());
}
#[test]
fn snapshot_obeys_modes_including_live_wal_and_sealed_original_guard() {
    for mode in [Mode::Legacy, Mode::Mirror, Mode::Unified, Mode::Sealed] {
        let f = Fixture::new();
        f.legacy();
        let db = unified::open_unified(&f.dbpath()).unwrap();
        unified::status_put(&db, "a.json", b"db", 1, Origin::Import).unwrap();
        for key in [".hidden.json", "wrong.JSON", "not-json"] {
            unified::status_put(&db, key, b"ignored", 1, Origin::Import).unwrap();
        }
        unified::set_mode(&db, "session-status", mode, "test", 1).unwrap();
        let expected = if matches!(mode, Mode::Unified | Mode::Sealed) {
            vec![("a.json".into(), b"db".to_vec())]
        } else {
            vec![("z.json".into(), b"legacy".to_vec())]
        };
        assert_eq!(f.check().unwrap(), expected);
        drop(db);
        assert_eq!(f.check().unwrap(), expected);
        let mut lock = f.dbpath().into_os_string();
        lock.push(".domain-modes.lock");
        fs::remove_file(PathBuf::from(lock)).unwrap();
        assert_eq!(
            f.check().unwrap(),
            expected,
            "absent mode lock must not be created"
        );
        if mode == Mode::Sealed {
            fs::remove_file(f.dbpath()).unwrap();
            assert!(f.check().is_err());
        }
    }
}
#[test]
fn guard_mismatch_future_wal_and_source_permissions_fail_closed_without_writes() {
    let f = Fixture::new();
    f.legacy();
    let db = unified::open_unified(&f.dbpath()).unwrap();
    let mut guard = f.dbpath().into_os_string();
    guard.push(".sealed-session-status");
    let guard = PathBuf::from(guard);
    fs::write(&guard, b"comandos-state-protocol-2:sealed\n").unwrap();
    assert!(f.check().is_err());
    fs::write(&guard, b"bad").unwrap();
    assert!(f.check().is_err());
    fs::remove_file(&guard).unwrap();
    db.execute_batch("PRAGMA user_version=999999").unwrap();
    assert!(f.check().is_err());
    db.execute_batch("PRAGMA user_version=11").unwrap();
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(f.dbpath(), fs::Permissions::from_mode(0o644)).unwrap();
    assert!(f.check().is_err());
}
#[test]
fn existing_mode_lock_is_readonly_and_contended_lock_is_rejected() {
    let f = Fixture::new();
    f.legacy();
    let db = unified::open_unified(&f.dbpath()).unwrap();
    unified::set_mode(&db, "session-status", Mode::Mirror, "test", 1).unwrap();
    drop(db);
    let mut lock = f.dbpath().into_os_string();
    lock.push(".domain-modes.lock");
    let held = fs::File::open(PathBuf::from(lock)).unwrap();
    held.lock().unwrap();
    assert!(f.check().is_err());
    held.unlock().unwrap();
    assert!(f.check().is_ok());
}
