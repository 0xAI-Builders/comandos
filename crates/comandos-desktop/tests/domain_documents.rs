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
