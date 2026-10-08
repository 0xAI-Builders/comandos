use comandos_store::{
    domains::{DomainStore, catalog::DOMAINS},
    unified::{self, Mode},
};
use std::{
    fs,
    os::unix::fs::{MetadataExt, PermissionsExt},
    path::{Path, PathBuf},
};

type Stamp = (PathBuf, u64, u32, i64, i64, Vec<u8>);
fn stamp(root: &Path) -> Vec<Stamp> {
    fn walk(p: &Path, out: &mut Vec<Stamp>) {
        let Ok(m) = p.symlink_metadata() else { return };
        out.push((
            p.to_owned(),
            m.ino(),
            m.mode(),
            m.mtime(),
            m.mtime_nsec(),
            if m.is_file() {
                fs::read(p).unwrap()
            } else {
                vec![]
            },
        ));
        if m.is_dir() {
            for e in fs::read_dir(p).unwrap() {
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
fn modes_snapshot_no_create_and_all_authorities_with_live_wal() {
    let home = std::env::temp_dir().join(format!("doctor-modes-{}", std::process::id()));
    let _ = fs::remove_dir_all(&home);
    let store = DomainStore { home: &home };
    assert!(
        store
            .modes_readonly()
            .unwrap()
            .iter()
            .all(|(_, m)| *m == Mode::Legacy)
    );
    assert!(!home.exists());
    fs::create_dir(&home).unwrap();
    fs::set_permissions(&home, fs::Permissions::from_mode(0o700)).unwrap();
    let path = home.join(".local/share/comandos/comandos.sqlite3");
    let db = unified::open_unified(&path).unwrap();
    for (i, d) in DOMAINS.iter().enumerate() {
        unified::set_mode(
            &db,
            d.name,
            [Mode::Legacy, Mode::Mirror, Mode::Unified, Mode::Sealed][i % 4],
            "fixture",
            1,
        )
        .unwrap();
    }
    let before = stamp(&home);
    let modes = store.modes_readonly().unwrap();
    assert_eq!(stamp(&home), before);
    assert_eq!(modes.len(), DOMAINS.len());
    for (i, (_, mode)) in modes.iter().enumerate() {
        assert_eq!(
            *mode,
            [Mode::Legacy, Mode::Mirror, Mode::Unified, Mode::Sealed][i % 4]
        );
    }
    db.execute_batch("PRAGMA user_version=999999").unwrap();
    let before = stamp(&home);
    assert!(store.modes_readonly().is_err());
    assert_eq!(stamp(&home), before);
    db.execute_batch("PRAGMA user_version=11").unwrap();
    drop(db);
    let lock = PathBuf::from(format!("{}.domain-modes.lock", path.display()));
    let held = fs::File::open(&lock).unwrap();
    held.lock().unwrap();
    let before = stamp(&home);
    assert!(store.modes_readonly().is_err());
    assert_eq!(stamp(&home), before);
    held.unlock().unwrap();
    fs::remove_file(lock).unwrap();
    let before = stamp(&home);
    assert!(store.modes_readonly().is_ok());
    assert_eq!(stamp(&home), before);
    fs::remove_file(&path).unwrap();
    let before = stamp(&home);
    assert!(store.modes_readonly().is_err());
    assert_eq!(stamp(&home), before);
    fs::remove_dir_all(&home).unwrap();
}
