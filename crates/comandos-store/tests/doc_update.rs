use comandos_store::{
    domains::DomainStore,
    unified::{self, Mode},
};
use serde_json::{Value, json};
use std::{
    fs,
    os::unix::fs::{DirBuilderExt, PermissionsExt},
    path::{Path, PathBuf},
    sync::{Arc, Barrier},
};
struct Fixture(PathBuf);
impl Fixture {
    fn new(tag: &str) -> Self {
        let p = std::env::temp_dir().join(format!("doc-rmw-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&p);
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(p.join(".local/share/comandos"))
            .unwrap();
        Self(p)
    }
    fn db(&self) -> rusqlite::Connection {
        unified::open_unified(&self.0.join(".local/share/comandos/comandos.sqlite3")).unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn doc(home: &Path) -> comandos_store::domains::DocHandle<'_> {
    DomainStore { home }.document(
        "hooks/acp-panes.json",
        "ui-docs",
        home.join(".claude/hooks/acp-panes.json"),
    )
}
#[test]
fn atomic_updates_preserve_concurrent_panes_in_all_modes() {
    for mode in [Mode::Legacy, Mode::Mirror, Mode::Unified, Mode::Sealed] {
        let f = Fixture::new(&format!("{mode:?}"));
        let db = f.db();
        if mode != Mode::Legacy {
            unified::set_mode(&db, "ui-docs", mode, "private", 1).unwrap();
        }
        drop(db);
        let barrier = Arc::new(Barrier::new(4));
        let workers: Vec<_> = (0..4)
            .map(|n| {
                let home = f.0.clone();
                let b = barrier.clone();
                std::thread::spawn(move || {
                    b.wait();
                    for i in 0..12 {
                        doc(&home)
                            .update_owned(i, |old| {
                                let mut value: Value = old
                                    .map(|b| serde_json::from_slice(b).unwrap())
                                    .unwrap_or(json!({}));
                                value[format!("%{n}")] = json!({"round":i});
                                Ok(Some(serde_json::to_vec(&value).unwrap()))
                            })
                            .unwrap();
                    }
                })
            })
            .collect();
        for w in workers {
            w.join().unwrap();
        }
        let result: Value =
            serde_json::from_slice(&doc(&f.0).read_readonly().unwrap().unwrap()).unwrap();
        assert_eq!(result.as_object().unwrap().len(), 4);
        for n in 0..4 {
            assert_eq!(result[format!("%{n}")]["round"], 11);
        }
        if mode == Mode::Sealed {
            assert!(!doc(&f.0).file.exists());
            assert!(!doc(&f.0).lock.exists());
        } else {
            assert_eq!(
                serde_json::from_slice::<Value>(&fs::read(doc(&f.0).file).unwrap()).unwrap(),
                result
            );
        }
    }
}
#[test]
fn legacy_missing_db_is_not_created_and_rejected_callback_preserves_bytes() {
    let f = Fixture::new("missing");
    let h = doc(&f.0);
    h.update_owned(1, |_| Ok(Some(b"{\"other\":1}".to_vec())))
        .unwrap();
    assert!(!unified::unified_path(&f.0).exists());
    fs::set_permissions(&h.file, fs::Permissions::from_mode(0o640)).unwrap();
    assert!(
        h.update_owned(2, |_| Err(comandos_store::Error::Validation(
            "bad metadata".into()
        )))
        .is_err()
    );
    assert_eq!(fs::read(&h.file).unwrap(), b"{\"other\":1}");
    assert_eq!(
        fs::metadata(&h.file).unwrap().permissions().mode() & 0o777,
        0o640
    );
}
#[test]
fn stale_sealed_guard_and_future_database_reject_before_document_lock() {
    for kind in ["sealed", "future"] {
        let f = Fixture::new(kind);
        let db = f.db();
        if kind == "sealed" {
            unified::set_mode(&db, "ui-docs", Mode::Sealed, "private", 1).unwrap();
            drop(db);
            fs::remove_file(unified::unified_path(&f.0)).unwrap();
        } else {
            db.pragma_update(None, "user_version", 999).unwrap();
            drop(db);
        }
        let h = doc(&f.0);
        assert!(h.update_owned(1, |_| Ok(Some(b"{}".to_vec()))).is_err());
        assert!(!h.file.exists());
        assert!(!h.lock.exists());
    }
}

#[test]
fn typed_mode_busy_deadline_and_cancellation_do_not_run_callback_or_create_documents() {
    use std::{
        cell::Cell,
        sync::atomic::{AtomicBool, Ordering},
        time::{Duration, Instant},
    };
    let f = Fixture::new("admission");
    let db = f.db();
    drop(db);
    let path =
        f.0.join(".local/share/comandos/comandos.sqlite3.domain-modes.lock");
    let lock = comandos_store::files::FileLock::exclusive(&path).unwrap();
    assert!(matches!(
        doc(&f.0).read_readonly(),
        Err(comandos_store::Error::ModeBusy)
    ));
    let calls = Cell::new(0);
    let start = Instant::now();
    let result = doc(&f.0).update_owned_when(
        1,
        Duration::from_millis(25),
        || true,
        |_| {
            calls.set(calls.get() + 1);
            Ok(Some(b"{}".to_vec()))
        },
    );
    assert!(matches!(result, Err(comandos_store::Error::ModeBusy)));
    assert!(start.elapsed() < Duration::from_secs(1));
    assert_eq!(calls.get(), 0);
    let allowed = Arc::new(AtomicBool::new(true));
    let flag = allowed.clone();
    let trigger = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(10));
        flag.store(false, Ordering::Relaxed);
    });
    assert!(
        !doc(&f.0)
            .update_owned_when(
                1,
                Duration::from_secs(1),
                || allowed.load(Ordering::Relaxed),
                |_| {
                    calls.set(calls.get() + 1);
                    Ok(Some(b"{}".to_vec()))
                }
            )
            .unwrap()
    );
    trigger.join().unwrap();
    assert_eq!(calls.get(), 0);
    assert!(!doc(&f.0).file.exists());
    assert!(!doc(&f.0).lock.exists());
    drop(lock);
    let absent = f.0.join("absent");
    assert!(
        !doc(&absent)
            .update_owned_when(
                1,
                Duration::ZERO,
                || false,
                |_| panic!("cancelled callback must never run")
            )
            .unwrap()
    );
    assert!(!absent.exists());
}
