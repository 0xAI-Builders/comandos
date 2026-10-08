use comandos_store::unified::{self, Mode, Origin};
use std::{fs, os::unix::fs::DirBuilderExt};

#[test]
fn runtime_reads_survive_a_concurrent_payload_commit_without_changing_authority() {
    let home = std::env::temp_dir().join(format!("live-read-{}", std::process::id()));
    fs::DirBuilder::new().mode(0o700).create(&home).unwrap();
    let db = unified::open_unified(&unified::unified_path(&home)).unwrap();
    for mode in [Mode::Mirror, Mode::Unified, Mode::Sealed] {
        unified::set_mode(&db, "ui-docs", mode, "test", 1).unwrap();
        let result =
            comandos_store::domains::caller::read(&home, "ui-docs", |observed, _reader| {
                // Another SQLite connection commits while this runtime reader owns
                // its mode lease. A payload commit is not a change of authority.
                unified::doc_put(&db, "hooks/prefs.json", "ui-docs", b"{}", Origin::Mirror, 2)?;
                Ok(observed)
            });
        assert_eq!(result.unwrap(), mode);
        assert_eq!(unified::mode_of(Some(&db), "ui-docs").unwrap(), mode);
    }
    drop(db);
    fs::remove_dir_all(home).unwrap();
}

#[test]
fn runtime_reader_yields_when_a_mode_change_owns_the_lease() {
    let home = std::env::temp_dir().join(format!("live-mode-busy-{}", std::process::id()));
    fs::DirBuilder::new().mode(0o700).create(&home).unwrap();
    let path = unified::unified_path(&home);
    let db = unified::open_unified(&path).unwrap();
    let lock = comandos_store::files::FileLock::exclusive(
        &path.with_file_name("comandos.sqlite3.domain-modes.lock"),
    )
    .unwrap();
    let result = comandos_store::domains::caller::read(&home, "ui-docs", |_, _| Ok(()));
    assert!(matches!(result, Err(comandos_store::Error::ModeBusy)));
    drop(lock);
    drop(db);
    fs::remove_dir_all(home).unwrap();
}
