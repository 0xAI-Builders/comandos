use comandos_store::unified::{self, Mode, Origin};
use serde_json::{Value, json};
use std::{fs, path::PathBuf, process::Command};

struct Home(PathBuf);

impl Home {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!(
            "web-cli-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(p.join(".claude/hooks")).unwrap();
        Self(p)
    }
    fn selection(&self) -> PathBuf {
        self.0.join(".claude/hooks/comandos-web.json")
    }
    fn db(&self) -> PathBuf {
        self.0.join(".local/share/comandos/comandos.sqlite3")
    }
}

impl Drop for Home {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn comandos(home: &Home, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_comandos"))
        .env("HOME", &home.0)
        .env_remove("COMANDOS_DB")
        .args(args)
        .output()
        .unwrap()
}

#[test]
fn web_set_shadow_writes_selection_atomically() {
    let home = Home::new();
    let out = comandos(&home, &["web", "set", "quick-terminal", "shadow"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        fs::read_to_string(home.selection()).unwrap(),
        "{\"on\":[],\"shadow\":[\"quick-terminal\"]}"
    );
}

#[test]
fn web_set_unknown_component_exits_2() {
    let home = Home::new();
    let out = comandos(&home, &["web", "set", "nada", "on"]);
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("componente desconocido: nada"));
}

#[test]
fn web_status_falls_back_to_selection_file_when_front_is_down() {
    let home = Home::new();
    fs::write(
        home.selection(),
        "{\"on\":[\"quick-terminal\"],\"shadow\":[]}",
    )
    .unwrap();
    let out = comandos(&home, &["web", "status"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&out.stdout).trim(),
        "{\"on\":[\"quick-terminal\"],\"shadow\":[]}"
    );
}

const DOC: &str = "hooks/comandos-web.json";
const FILE_SELECTION: &str = "{\"on\":[\"i18n\"],\"shadow\":[\"red\"]}";
const ROW_SELECTION: &str = "{\"on\":[\"prelude\"],\"shadow\":[\"helpers\"]}";

fn configured(home: &Home, mode: Mode) -> rusqlite::Connection {
    let db = unified::open_unified(&home.db()).unwrap();
    unified::doc_put(
        &db,
        DOC,
        "ui-docs",
        ROW_SELECTION.as_bytes(),
        Origin::Unified,
        1,
    )
    .unwrap();
    unified::set_mode(&db, "ui-docs", mode, "private web fixture", 2).unwrap();
    fs::write(home.selection(), FILE_SELECTION).unwrap();
    if mode == Mode::Sealed {
        fs::remove_file(home.selection()).unwrap();
    }
    db
}

#[test]
fn web_set_uses_domain_authority_and_preserves_other_selections_in_every_mode() {
    for mode in [Mode::Legacy, Mode::Mirror, Mode::Unified, Mode::Sealed] {
        let home = Home::new();
        let db = configured(&home, mode);
        let out = comandos(&home, &["web", "set", "quick-terminal", "shadow"]);
        assert!(
            out.status.success(),
            "{mode:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        let expected = if matches!(mode, Mode::Legacy | Mode::Mirror) {
            json!({"on":["i18n"],"shadow":["quick-terminal","red"]})
        } else {
            json!({"on":["prelude"],"shadow":["helpers","quick-terminal"]})
        };
        let expected_bytes = serde_json::to_vec(&expected).unwrap();
        let row = unified::doc_get(&db, DOC).unwrap().unwrap();
        if mode == Mode::Legacy {
            assert_eq!(row.body, ROW_SELECTION.as_bytes());
        } else {
            assert_eq!(row.body, expected_bytes, "{mode:?}: document row");
        }
        if mode == Mode::Sealed {
            assert!(
                !home.selection().exists(),
                "sealed must not resurrect the selection file"
            );
            assert!(
                !home.selection().with_extension("json.lock").exists(),
                "sealed must not recreate the file lock"
            );
        } else {
            assert_eq!(
                fs::read(home.selection()).unwrap(),
                expected_bytes,
                "{mode:?}: legacy counterpart"
            );
        }
    }
}

#[test]
fn web_status_fallback_reads_domain_authority_without_recreating_sealed_file() {
    for mode in [Mode::Mirror, Mode::Unified, Mode::Sealed] {
        let home = Home::new();
        let _db = configured(&home, mode);
        let out = comandos(&home, &["web", "status"]);
        assert!(
            out.status.success(),
            "{mode:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert_eq!(
            String::from_utf8(out.stdout).unwrap().trim(),
            if mode == Mode::Mirror {
                FILE_SELECTION
            } else {
                ROW_SELECTION
            }
        );
        if mode == Mode::Sealed {
            assert!(!home.selection().exists());
        }
    }
}

#[test]
fn web_set_and_status_reject_future_database_without_changing_selection_or_database() {
    let home = Home::new();
    let db = configured(&home, Mode::Mirror);
    db.execute(
        "INSERT INTO schema_migrations VALUES(999999,'future',0)",
        [],
    )
    .unwrap();
    let paths = [
        home.db(),
        PathBuf::from(format!("{}-wal", home.db().display())),
        PathBuf::from(format!("{}-shm", home.db().display())),
        home.selection(),
    ];
    let before: Vec<_> = paths.iter().map(|p| fs::read(p).ok()).collect();
    for args in [
        ["web", "set", "quick-terminal", "on"].as_slice(),
        ["web", "status"].as_slice(),
    ] {
        let out = comandos(&home, args);
        assert_eq!(out.status.code(), Some(1), "{args:?}");
        assert!(String::from_utf8_lossy(&out.stderr).contains("versión más nueva"));
        assert_eq!(
            paths.iter().map(|p| fs::read(p).ok()).collect::<Vec<_>>(),
            before
        );
    }
    let current: Value =
        serde_json::from_str(&fs::read_to_string(home.selection()).unwrap()).unwrap();
    assert_eq!(
        current,
        serde_json::from_str::<Value>(FILE_SELECTION).unwrap()
    );
}

#[test]
fn web_set_waits_for_original_selection_lock_before_read_modify_write() {
    let home = Home::new();
    fs::write(home.selection(), FILE_SELECTION).unwrap();
    let lock_path = home.selection().with_extension("json.lock");
    let guard = comandos_store::files::FileLock::exclusive(&lock_path).unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_comandos"))
        .env("HOME", &home.0)
        .env_remove("COMANDOS_DB")
        .args(["web", "set", "quick-terminal", "on"])
        .spawn()
        .unwrap();
    std::thread::sleep(std::time::Duration::from_millis(200));
    let exited_while_locked = child.try_wait().unwrap().is_some();
    // The previous writer changes another selection while holding the original
    // lock. A correct caller reads after this writer, preserving its update.
    fs::write(home.selection(), ROW_SELECTION).unwrap();
    drop(guard);
    let status = child.wait().unwrap();
    assert!(
        !exited_while_locked,
        "web set bypassed the original file flock"
    );
    assert!(status.success());
    assert_eq!(
        fs::read_to_string(home.selection()).unwrap(),
        "{\"on\":[\"prelude\",\"quick-terminal\"],\"shadow\":[\"helpers\"]}"
    );
}

fn readonly_tree(
    root: &std::path::Path,
) -> std::collections::BTreeMap<PathBuf, (Option<Vec<u8>>, Vec<i128>)> {
    use std::os::unix::fs::MetadataExt;
    fn visit(
        root: &std::path::Path,
        path: &std::path::Path,
        tree: &mut std::collections::BTreeMap<PathBuf, (Option<Vec<u8>>, Vec<i128>)>,
    ) {
        let meta = fs::symlink_metadata(path).unwrap();
        tree.insert(
            path.strip_prefix(root).unwrap().to_owned(),
            (
                meta.is_file().then(|| fs::read(path).unwrap()),
                vec![
                    i128::from(meta.ino()),
                    i128::from(meta.len()),
                    i128::from(meta.mode()),
                    i128::from(meta.mtime()),
                    i128::from(meta.mtime_nsec()),
                    i128::from(meta.ctime()),
                    i128::from(meta.ctime_nsec()),
                ],
            ),
        );
        if meta.is_dir() {
            for entry in fs::read_dir(path).unwrap() {
                visit(root, &entry.unwrap().path(), tree);
            }
        }
    }
    let mut tree = std::collections::BTreeMap::new();
    visit(root, root, &mut tree);
    tree
}

#[test]
fn web_status_fallback_missing_and_cold_mirror_create_no_entries_or_change_source_metadata() {
    for cold in [false, true] {
        let home = Home::new();
        if cold {
            let db = configured(&home, Mode::Mirror);
            db.execute_batch("PRAGMA wal_checkpoint(TRUNCATE)").unwrap();
            drop(db);
            for suffix in ["-wal", "-shm"] {
                let path = PathBuf::from(format!("{}{suffix}", home.db().display()));
                if path.exists() {
                    fs::remove_file(path).unwrap();
                }
            }
        }
        let before = readonly_tree(&home.0);
        let out = comandos(&home, &["web", "status"]);
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert_eq!(
            readonly_tree(&home.0),
            before,
            "cold={cold}: readonly status changed source entries/metadata"
        );
    }
}

#[test]
#[ignore = "private 172MB Mirror document/selection read latency gate; run explicitly"]
fn large_mirror_document_and_selection_reads_ignore_unrelated_payload_unchanged() {
    use std::time::{Duration, Instant};
    let home = Home::new();
    let db = configured(&home, Mode::Mirror);
    db.execute_batch("PRAGMA wal_autocheckpoint=0; CREATE TABLE unrelated_payload(body BLOB); INSERT INTO unrelated_payload VALUES(zeroblob(172000000)); PRAGMA wal_checkpoint(TRUNCATE)").unwrap();
    // Keep a committed WAL so the gate exercises sparse mode admission.
    unified::set_mode(&db, "ui-docs", Mode::Mirror, "private read gate", 3).unwrap();
    let before = readonly_tree(&home.0);
    let start = Instant::now();
    let body = comandos_store::domains::DomainStore { home: &home.0 }
        .document(DOC, "ui-docs", home.selection())
        .read_readonly()
        .unwrap();
    let document_elapsed = start.elapsed();
    let start = Instant::now();
    let selection = comandos_server::dash::web::Selection::load_domain(&home.0, &home.selection());
    let selection_elapsed = start.elapsed();
    assert_eq!(body, Some(FILE_SELECTION.as_bytes().to_vec()));
    assert_eq!(selection.on, ["i18n".to_owned()].into_iter().collect());
    assert_eq!(selection.shadow, ["red".to_owned()].into_iter().collect());
    assert_eq!(readonly_tree(&home.0), before);
    eprintln!(
        "172MB_doc_read_us={} selection_read_us={}",
        document_elapsed.as_micros(),
        selection_elapsed.as_micros()
    );
    assert!(
        document_elapsed < Duration::from_millis(50),
        "document Mirror read took {document_elapsed:?}"
    );
    assert!(
        selection_elapsed < Duration::from_millis(50),
        "selection Mirror read took {selection_elapsed:?}"
    );
}
