//! Catalogued document collections, original authority and private read-only fixtures.
use comandos_store::{
    domains::DomainStore,
    unified::{self, Mode, Origin},
};
use std::{
    collections::BTreeMap,
    fs,
    os::unix::fs::{DirBuilderExt, MetadataExt, symlink},
    path::{Path, PathBuf},
    sync::atomic::{AtomicUsize, Ordering},
};
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        static N: AtomicUsize = AtomicUsize::new(0);
        let p = std::env::temp_dir().join(format!(
            "document-dir-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        fs::DirBuilder::new().mode(0o700).create(&p).unwrap();
        Self(p)
    }
    fn path(&self) -> PathBuf {
        self.0.join(".local/state/comandos/codex-full-access")
    }
    fn db(&self) -> PathBuf {
        unified::unified_path(&self.0)
    }
    fn collection(&self) -> comandos_store::Result<comandos_store::domains::DocumentDir<'_>> {
        DomainStore { home: &self.0 }.document_dir(
            "codex-reports",
            "STATE/codex-full-access",
            self.path(),
        )
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
type Tree = BTreeMap<PathBuf, (u64, u32, i64, i64, Vec<u8>)>;
fn tree(p: &Path) -> Tree {
    fn walk(p: &Path, out: &mut Tree) {
        let Ok(m) = p.symlink_metadata() else { return };
        out.insert(
            p.to_owned(),
            (
                m.ino(),
                m.mode(),
                m.mtime(),
                m.mtime_nsec(),
                if m.is_file() {
                    fs::read(p).unwrap()
                } else {
                    vec![]
                },
            ),
        );
        if m.is_dir() {
            for e in fs::read_dir(p).unwrap() {
                walk(&e.unwrap().path(), out);
            }
        }
    }
    let mut t = Tree::new();
    walk(p, &mut t);
    t
}
#[test]
fn dynamic_document_name_borrows_only_home_and_name_not_temporary_facade() {
    let f = Fixture::new();
    let name = String::from("state/codex-full-access/1.json");
    let handle =
        { DomainStore { home: &f.0 }.document(&name, "codex-reports", f.path().join("1.json")) };
    assert!(handle.read_readonly().unwrap().is_none());
    assert!(f.path().symlink_metadata().is_err());
}
#[test]
fn missing_collection_and_home_remain_absent_without_controls() {
    let f = Fixture::new();
    let before = tree(&f.0);
    let dir = f.collection().unwrap();
    assert!(dir.list_readonly().unwrap().is_empty());
    assert!(dir.read_readonly("1.json").unwrap().is_none());
    assert_eq!(tree(&f.0), before);
    fs::remove_dir_all(&f.0).unwrap();
    assert!(dir.list_readonly().unwrap().is_empty());
    assert!(!f.0.exists());
}
#[test]
fn all_four_modes_use_catalog_names_and_original_guards_without_read_writes() {
    for mode in [Mode::Legacy, Mode::Mirror, Mode::Unified, Mode::Sealed] {
        let f = Fixture::new();
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(f.path())
            .unwrap();
        fs::write(f.path().join("1.json"), b"legacy").unwrap();
        let db = unified::open_unified(&f.db()).unwrap();
        unified::doc_put(
            &db,
            "state/codex-full-access/2.json",
            "codex-reports",
            b"database",
            Origin::Import,
            10,
        )
        .unwrap();
        for (name, domain) in [
            ("state/closed-panes/3.json", "codex-reports"),
            ("state/codex-full-access/foreign.json", "news-docs"),
            ("state/codex-full-access/nested/4.json", "codex-reports"),
            ("state/codex-full-access/.hidden.json", "codex-reports"),
        ] {
            unified::doc_put(&db, name, domain, b"foreign", Origin::Import, 11).unwrap();
        }
        unified::set_mode(&db, "codex-reports", mode, "fixture", 1).unwrap();
        let before = tree(&f.0);
        let dir = f.collection().unwrap();
        assert_eq!(dir.mode_readonly().unwrap(), mode);
        dir.with_document("2.json", |doc| {
            assert_eq!(doc.name, "state/codex-full-access/2.json");
            assert_eq!(doc.domain, "codex-reports");
            assert_eq!(doc.file, f.path().join("2.json"));
            Ok(())
        })
        .unwrap();
        let rows = dir.list_readonly().unwrap();
        let expected = if matches!(mode, Mode::Unified | Mode::Sealed) {
            ("2.json", b"database".as_slice())
        } else {
            ("1.json", b"legacy".as_slice())
        };
        assert_eq!(rows.len(), 1);
        assert_eq!((&*rows[0].key, rows[0].body.as_slice()), expected);
        assert_eq!(dir.read_readonly(expected.0).unwrap().unwrap(), expected.1);
        assert_eq!(tree(&f.0), before);
        drop(db);
        let before = tree(&f.0);
        assert_eq!(dir.list_readonly().unwrap().len(), 1);
        assert_eq!(tree(&f.0), before);
    }
}
#[test]
fn unknown_catalog_domain_kind_prefix_and_traversal_keys_reject() {
    let f = Fixture::new();
    for (domain, prefix) in [
        ("unknown", "STATE/codex-full-access"),
        ("codex-reports", "STATE/unknown"),
        ("news-docs", "STATE/codex-full-access"),
        ("session-status", "H/state"),
    ] {
        assert!(
            DomainStore { home: &f.0 }
                .document_dir(domain, prefix, f.path())
                .is_err()
        );
    }
    let d = f.collection().unwrap();
    for key in [
        "../1.json",
        "nested/1.json",
        "\\1.json",
        ".hidden.json",
        "1.JSON",
        "1.json\0",
        "",
        "..",
    ] {
        assert!(d.read_readonly(key).is_err());
    }
    assert!(fs::read_dir(&f.0).unwrap().next().is_none());
}
#[test]
fn future_database_and_missing_corrupt_or_mismatched_seal_never_fall_back_to_files() {
    for case in ["future", "missing", "corrupt", "mismatch"] {
        let f = Fixture::new();
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(f.path())
            .unwrap();
        fs::write(f.path().join("1.json"), b"stale").unwrap();
        let db = unified::open_unified(&f.db()).unwrap();
        if case == "future" {
            db.execute_batch("PRAGMA user_version=999999").unwrap();
        } else {
            let guard = unified::seal_guard_path(&f.db(), "codex-reports").unwrap();
            fs::write(
                guard,
                if case == "corrupt" {
                    b"bad".as_slice()
                } else {
                    b"comandos-state-protocol-2:sealed\n"
                },
            )
            .unwrap();
        }
        drop(db);
        if case == "missing" {
            fs::remove_file(f.db()).unwrap();
        }
        let before = tree(&f.0);
        let d = f.collection().unwrap();
        assert!(d.list_readonly().is_err());
        assert!(d.read_readonly("1.json").is_err());
        assert_eq!(tree(&f.0), before);
    }
}
#[test]
fn legacy_control_hardlinks_and_symlinks_are_not_documents() {
    let f = Fixture::new();
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(f.path())
        .unwrap();
    let db = unified::open_unified(&f.db()).unwrap();
    fs::hard_link(f.db(), f.path().join("hard.json")).unwrap();
    let before = tree(&f.0);
    assert!(f.collection().unwrap().list_readonly().is_err());
    assert_eq!(tree(&f.0), before);
    fs::remove_file(f.path().join("hard.json")).unwrap();
    symlink(f.db(), f.path().join("link.json")).unwrap();
    let before = tree(&f.0);
    assert!(f.collection().unwrap().list_readonly().is_err());
    assert!(f.collection().unwrap().read_readonly("link.json").is_err());
    assert_eq!(tree(&f.0), before);
    drop(db);
}
