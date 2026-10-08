use comandos_store::{
    domains::caller::CallerAccess,
    unified::{self, Mode, Origin},
};
use std::{fs, os::unix::fs::DirBuilderExt};

#[cfg(target_os = "linux")]
#[test]
fn repeated_runtime_writes_do_not_copy_unrelated_wal_payloads() {
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
    let home = std::env::temp_dir().join(format!("live-write-io-{}", std::process::id()));
    fs::DirBuilder::new().mode(0o700).create(&home).unwrap();
    let db = unified::open_unified(&unified::unified_path(&home)).unwrap();
    db.execute_batch("PRAGMA wal_autocheckpoint=0; CREATE TABLE unrelated_payload(body BLOB); INSERT INTO unrelated_payload VALUES(zeroblob(2097152));").unwrap();
    unified::set_mode(&db, "ui-docs", Mode::Unified, "test", 1).unwrap();
    drop(CallerAccess::open(&home, "ui-docs").unwrap());
    let before = io_bytes();
    for value in 0..6 {
        let body = value.to_string();
        let access = CallerAccess::open(&home, "ui-docs").unwrap();
        access
            .write(
                || Ok(()),
                |writer, origin| {
                    unified::doc_put(
                        writer,
                        "hooks/prefs.json",
                        "ui-docs",
                        body.as_bytes(),
                        origin,
                        value,
                    )
                    .map(|_| ())
                },
            )
            .unwrap();
        assert_eq!(
            unified::doc_get(&db, "hooks/prefs.json")
                .unwrap()
                .unwrap()
                .body,
            body.as_bytes()
        );
    }
    let after = io_bytes();
    let read = after.0 - before.0;
    let written = after.1 - before.1;
    drop(db);
    fs::remove_dir_all(home).unwrap();
    eprintln!("six_runtime_writes_read_bytes={read} written_bytes={written}");
    assert!(read < 512 * 1024, "six tiny writes read {read} bytes");
    assert!(
        written < 512 * 1024,
        "six tiny writes wrote {written} bytes"
    );
}

#[test]
fn reused_writer_observes_external_commits_modes_and_rejects_newer_schema() {
    let home = std::env::temp_dir().join(format!("live-write-authority-{}", std::process::id()));
    fs::DirBuilder::new().mode(0o700).create(&home).unwrap();
    let path = unified::unified_path(&home);
    let db = unified::open_unified(&path).unwrap();
    assert_eq!(
        CallerAccess::open(&home, "ui-docs").unwrap().mode(),
        Mode::Legacy
    );
    // This connection represents a different owner; dropping the runtime
    // access must release its lease before the authority changes.
    unified::doc_put(
        &db,
        "hooks/prefs.json",
        "ui-docs",
        b"external commit",
        Origin::Unified,
        2,
    )
    .unwrap();
    unified::set_mode(&db, "ui-docs", Mode::Sealed, "external writer", 2).unwrap();
    let access = CallerAccess::open(&home, "ui-docs").unwrap();
    assert_eq!(access.mode(), Mode::Sealed);
    assert_eq!(
        access
            .read_document("hooks/prefs.json", &home.join("absent.json"))
            .unwrap()
            .unwrap(),
        b"external commit"
    );
    drop(access);
    db.pragma_update(None, "user_version", 99).unwrap();
    let wal = path.with_file_name("comandos.sqlite3-wal");
    let before = (fs::read(&path).unwrap(), fs::read(&wal).unwrap());
    assert!(CallerAccess::open(&home, "ui-docs").is_err());
    assert_eq!(before, (fs::read(&path).unwrap(), fs::read(&wal).unwrap()));
    drop(db);
    fs::remove_dir_all(home).unwrap();
}
