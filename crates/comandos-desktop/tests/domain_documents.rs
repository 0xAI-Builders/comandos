#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]
//! Fachada real con backend de archivos confinado al HOME de cada fixture.
use comandos_desktop::{
    mode::RunMode,
    state_files::{StateConfig, StateFiles, StateGuard},
};
use comandos_store::unified::{self, Mode};
use serde_json::json;
use std::{
    path::{Path, PathBuf},
    sync::atomic::{AtomicUsize, Ordering},
};
static NEXT: AtomicUsize = AtomicUsize::new(0);
#[derive(Clone)]
struct Fixture {
    home: PathBuf,
    hooks: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let home = std::env::temp_dir().join(format!(
            "domain-desktop-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let hooks = home.join("hooks");
        std::fs::create_dir_all(&hooks).unwrap();
        Self { home, hooks }
    }
}
impl StateConfig for Fixture {
    fn mode(&self) -> RunMode {
        RunMode::Sandbox
    }
    fn home(&self) -> &Path {
        &self.home
    }
    fn hooks_dir(&self) -> &Path {
        &self.hooks
    }
}
#[derive(Clone)]
struct Guard(PathBuf);
impl StateGuard for Guard {
    type Error = String;
    fn create_dir_all(&self, p: &Path, _: u32) -> Result<(), String> {
        assert!(p.starts_with(&self.0));
        std::fs::create_dir_all(p).map_err(|e| e.to_string())
    }
    fn open_lock(&self, p: &Path) -> Result<std::fs::File, String> {
        assert!(p.starts_with(&self.0));
        std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(p)
            .map_err(|e| e.to_string())
    }
    fn write_atomic(&self, p: &Path, b: &[u8], _: &str) -> Result<(), String> {
        assert!(p.starts_with(&self.0));
        comandos_store::files::write_atomic(p, b).map_err(|e| e.to_string())
    }
    fn write_atomic_when(
        &self,
        p: &Path,
        b: &[u8],
        prefix: &str,
        allowed: impl Fn() -> bool,
    ) -> Result<(), String> {
        if allowed() {
            self.write_atomic(p, b, prefix)
        } else {
            Ok(())
        }
    }
    fn archive_once(&self, p: &Path, b: &[u8]) -> Result<bool, String> {
        self.write_atomic(p, b, "")?;
        Ok(true)
    }
    fn remove_file(&self, p: &Path) -> Result<(), String> {
        assert!(p.starts_with(&self.0));
        std::fs::remove_file(p).map_err(|e| e.to_string())
    }
}
#[test]
fn snippets_and_app_ui_cas_work_in_every_mode_without_changing_bytes() {
    for mode in [Mode::Legacy, Mode::Mirror, Mode::Unified, Mode::Sealed] {
        let f = Fixture::new();
        let files = StateFiles::new(f.clone(), Guard(f.home.clone()));
        let db = unified::open_unified(&unified::unified_path(&f.home)).unwrap();
        for domain in ["ui-docs", "app-ui"] {
            unified::set_mode(&db, domain, mode, "fixture", 1).unwrap();
        }
        let next = vec![json!({"id":"one","body":"ñ😀\n","unknown":{"keep":true}})];
        assert!(files.write_snippets_when(&[], &next, || true).unwrap());
        assert_eq!(files.read_snippets().unwrap(), next);
        assert!(!files.write_snippets_when(&[], &[], || true).unwrap());
        let ui = json!({"height":300,"unknown":{"keep":true}});
        assert!(
            files
                .write_shelf_when(&serde_json::Value::Null, &ui, || true)
                .unwrap()
        );
        assert_eq!(
            files.read_ui_document("app-extension-shelf.json").unwrap(),
            ui
        );
        for (name, value, domain) in [
            ("snippets.json", serde_json::Value::Array(next), "ui-docs"),
            ("app-extension-shelf.json", ui, "app-ui"),
        ] {
            let exact = comandos_core::json::response_dumps(&value)
                .unwrap()
                .into_bytes();
            if mode != Mode::Legacy {
                let d = unified::doc_get(&db, &format!("hooks/{name}"))
                    .unwrap()
                    .unwrap();
                assert_eq!(d.body, exact);
                assert_eq!(d.domain, domain);
            }
            if mode != Mode::Sealed {
                assert_eq!(std::fs::read(f.hooks.join(name)).unwrap(), exact);
            } else {
                assert!(!f.hooks.join(name).exists());
            }
        }
        drop(db);
        std::fs::remove_dir_all(f.home).unwrap();
    }
}

#[test]
fn app_commands_survive_mode_flip_and_acknowledge_the_delivered_generation() {
    use comandos_desktop::ipc::{IpcConsumer, read_request_domain};
    let f = Fixture::new();
    let path = f.hooks.join("app-focus.json");
    let db = unified::open_unified(&unified::unified_path(&f.home)).unwrap();
    unified::set_mode(&db, "app-commands", Mode::Mirror, "fixture", 1).unwrap();
    let first = br#"{ "session": "before", "unknown": "keep" }"#;
    comandos_store::domains::commands::publish(&f.home, "app-focus.json", first, 2, || {
        Ok(std::fs::write(&path, first)?)
    })
    .unwrap();
    let request = read_request_domain(&f.home, &path).unwrap();
    unified::set_mode(&db, "app-commands", Mode::Unified, "fixture", 3).unwrap();
    let consumer = IpcConsumer::new(RunMode::Sandbox, Guard(f.home.clone()));
    consumer.consume_domain(&f.home, &request).unwrap();
    assert!(read_request_domain(&f.home, &path).is_err());
    for mode in [Mode::Unified, Mode::Sealed] {
        unified::set_mode(&db, "app-commands", mode, "fixture", 4).unwrap();
        let value = json!({"session":"after","unknown":"ñ😀"});
        let files = StateFiles::new(f.clone(), Guard(f.home.clone()));
        files.write("app-focus.json", &value).unwrap();
        let request = read_request_domain(&f.home, &path).unwrap();
        assert_eq!(request.payload, value);
        assert_eq!(
            read_request_domain(&f.home, &path).unwrap(),
            request,
            "peek must not consume"
        );
        consumer.consume_domain(&f.home, &request).unwrap();
        assert!(read_request_domain(&f.home, &path).is_err());
    }
    drop(db);
    std::fs::remove_dir_all(f.home).unwrap();
}
impl comandos_desktop::ipc::IpcGuard for Guard {
    type Error = String;
    fn remove_file(&self, path: &Path) -> Result<(), String> {
        StateGuard::remove_file(self, path)
    }
}

#[test]
fn sealed_cas_keeps_the_authoritative_read_inside_the_write_transaction() {
    let f = Fixture::new();
    let db = unified::open_unified(&unified::unified_path(&f.home)).unwrap();
    unified::set_mode(&db, "app-ui", Mode::Sealed, "fixture", 1).unwrap();
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(4));
    let workers: Vec<_> = (0..4)
        .map(|i| {
            let f = f.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                let files = StateFiles::new(f.clone(), Guard(f.home.clone()));
                barrier.wait();
                files
                    .write_shelf_when(&serde_json::Value::Null, &json!({"winner":i}), || true)
                    .unwrap()
            })
        })
        .collect();
    assert_eq!(
        workers
            .into_iter()
            .map(|w| usize::from(w.join().unwrap()))
            .sum::<usize>(),
        1
    );
    assert!(!f.hooks.join("app-extension-shelf.json").exists());
    drop(db);
    std::fs::remove_dir_all(f.home).unwrap();
}

#[test]
fn layout_publication_preserves_app_policy_and_guard_in_every_mode() {
    for mode in [Mode::Legacy, Mode::Mirror, Mode::Unified, Mode::Sealed] {
        let f = Fixture::new();
        let files = StateFiles::new(f.clone(), Guard(f.home.clone()));
        let db = unified::open_unified(&unified::unified_path(&f.home)).unwrap();
        unified::set_mode(&db, "layout", mode, "fixture", 1).unwrap();
        let first = json!({"version":2,"sessions":{},"unknown":"ñ😀"});
        let second = json!({"version":2,"sessions":{},"unknown":"second"});
        files
            .write_snapshot_when("app-sessions-v2.json", &first, || false)
            .unwrap();
        assert!(!f.hooks.join("app-sessions-v2.json").exists());
        files
            .write_snapshot_when("app-sessions-v2.json", &first, || true)
            .unwrap();
        files
            .write_snapshot_when("app-sessions-v2.json", &second, || true)
            .unwrap();
        assert_eq!(files.read_session_snapshot(), second);
        let exact = comandos_store::snapshot_files::plan(
            &second,
            None,
            0,
            comandos_store::snapshot_files::Policy::App,
        )
        .unwrap()
        .current;
        if mode != Mode::Legacy {
            let bytes: Vec<u8> = db
                .query_row(
                    "SELECT body FROM layout_snapshots WHERE generation='current'",
                    [],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(bytes, exact);
        }
        if mode == Mode::Sealed {
            assert!(!f.hooks.join("app-sessions-v2.json").exists());
            assert!(!f.hooks.join("app-tabs.json.lock").exists());
        } else {
            assert_eq!(
                std::fs::read(f.hooks.join("app-sessions-v2.json")).unwrap(),
                exact
            );
            assert!(f.hooks.join("app-tabs.json.lock").exists());
        }
        drop(db);
        std::fs::remove_dir_all(f.home).unwrap();
    }
}

#[test]
fn ipc_batch_matches_individual_peeks_in_every_mode_and_preserves_invalid_siblings() {
    use comandos_desktop::ipc::{read_request_domain, read_requests_domain};
    for mode in [Mode::Legacy, Mode::Mirror, Mode::Unified, Mode::Sealed] {
        let f = Fixture::new();
        let db = unified::open_unified(&unified::unified_path(&f.home)).unwrap();
        unified::set_mode(&db, "app-commands", mode, "fixture", 1).unwrap();
        let names = [
            "app-focus.json",
            "app-tab-open.json",
            "app-tab-close.json",
            "app-command.json",
        ];
        let bodies: [&[u8]; 4] = [
            br#"{ "session": "s1", "nested": {"unknown":true}, "n": 9007199254740993 }"#,
            br#"{"session":"s2"}"#,
            b"not json",
            br#"{"command":"keep"}"#,
        ];
        for (name, body) in names.iter().zip(bodies) {
            let path = f.hooks.join(name);
            comandos_store::domains::commands::publish(&f.home, name, body, 2, || {
                Ok(std::fs::write(&path, body)?)
            })
            .unwrap();
        }
        let mut paths: Vec<_> = names.iter().map(|name| f.hooks.join(name)).collect();
        paths.insert(2, f.hooks.join("ignored.json"));
        paths.push(f.hooks.join("app-tab-active.json"));
        std::fs::write(paths.last().unwrap(), br#"{"session":"legacy-active"}"#).unwrap();
        let expected: Vec<_> = paths
            .iter()
            .filter_map(|path| read_request_domain(&f.home, path).ok())
            .collect();
        assert_eq!(expected.len(), 4);
        assert_eq!(read_requests_domain(&f.home, &paths).unwrap(), expected);
        assert_eq!(
            read_requests_domain(&f.home, &paths).unwrap(),
            expected,
            "peek never consumes"
        );
        for (name, body) in names.iter().zip(bodies) {
            let (got_mode, row) = comandos_store::domains::commands::peek(&f.home, name).unwrap();
            assert_eq!(got_mode, mode);
            if matches!(mode, Mode::Unified | Mode::Sealed) {
                assert_eq!(row.unwrap().1, body);
            }
            if mode != Mode::Sealed {
                assert_eq!(std::fs::read(f.hooks.join(name)).unwrap(), body);
            }
        }
        unified::set_mode(&db, "app-commands", Mode::Unified, "fixture", 3).unwrap();
        assert_eq!(
            read_requests_domain(&f.home, &paths)
                .unwrap()
                .iter()
                .filter(|r| r.stamp.0 == u64::MAX)
                .count(),
            if mode == Mode::Legacy { 0 } else { 3 }
        );
        drop(db);
        std::fs::remove_dir_all(f.home).unwrap();
    }
}

#[test]
fn ipc_batch_absent_database_keeps_legacy_bytes_and_does_not_create_controls() {
    use comandos_desktop::ipc::{read_request_domain, read_requests_domain};
    let f = Fixture::new();
    let paths: Vec<_> = [
        "app-focus.json",
        "app-tab-open.json",
        "app-tab-close.json",
        "app-command.json",
    ]
    .iter()
    .map(|n| f.hooks.join(n))
    .collect();
    let bytes = br#"{ "session": "private", "unknown": [1,2,3] }"#;
    std::fs::write(&paths[0], bytes).unwrap();
    let before: Vec<_> = std::fs::read_dir(&f.home)
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert_eq!(
        read_requests_domain(&f.home, &paths).unwrap(),
        vec![read_request_domain(&f.home, &paths[0]).unwrap()]
    );
    assert_eq!(std::fs::read(&paths[0]).unwrap(), bytes);
    let after: Vec<_> = std::fs::read_dir(&f.home)
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert_eq!(before, after);
    std::fs::remove_dir_all(f.home).unwrap();
}

#[test]
fn ipc_batch_lease_excludes_mode_flip_and_rejects_transient_authority() {
    let f = Fixture::new();
    let names = [
        "app-focus.json",
        "app-tab-open.json",
        "app-tab-close.json",
        "app-command.json",
    ];
    let path = unified::unified_path(&f.home);
    let db = unified::open_unified(&path).unwrap();
    unified::set_mode(&db, "app-commands", Mode::Unified, "fixture", 1).unwrap();
    let lock = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(format!("{}.domain-modes.lock", path.display()))
        .unwrap();
    comandos_store::domains::commands::with_peeked(&f.home, &names, |mode, rows| {
        assert_eq!(mode, Mode::Unified);
        assert_eq!(rows.len(), 4);
        assert!(rows.into_iter().all(|r| r.unwrap().is_none()));
        assert!(
            matches!(lock.try_lock(), Err(std::fs::TryLockError::WouldBlock)),
            "entire batch holds mode exclusion"
        );
    })
    .unwrap();
    lock.try_lock().unwrap();
    lock.unlock().unwrap();
    unified::set_mode(&db, "app-commands", Mode::Sealed, "fixture", 2).unwrap();
    assert_eq!(
        comandos_store::domains::commands::with_peeked(&f.home, &names, |mode, _| mode).unwrap(),
        Mode::Sealed
    );
    drop(db);
    std::fs::remove_dir_all(f.home).unwrap();
    let f = Fixture::new();
    let path = unified::unified_path(&f.home);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::File::open(path.parent().unwrap())
        .unwrap()
        .set_times(std::fs::FileTimes::new().set_modified(std::time::UNIX_EPOCH))
        .unwrap();
    assert!(
        comandos_store::domains::commands::with_peeked(&f.home, &names, |mode, _| {
            assert_eq!(mode, Mode::Legacy);
            std::fs::write(&path, b"transient authority").unwrap();
            std::fs::remove_file(&path).unwrap();
        })
        .is_err(),
        "whole batch revalidates authority after its callback"
    );
    std::fs::remove_dir_all(f.home).unwrap();
}
