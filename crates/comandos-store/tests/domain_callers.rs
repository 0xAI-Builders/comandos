#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]
use comandos_store::{
    domains::caller,
    unified::{self, Mode},
};
use std::{fs, os::unix::fs::DirBuilderExt};
#[test]
fn caller_write_obeys_every_mode_and_never_creates_database() {
    let home = std::env::temp_dir().join(format!("caller-modes-{}", std::process::id()));
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(&home)
        .unwrap();
    let path = unified::unified_path(&home);
    let file = home.join("legacy.json");
    caller::write(
        &home,
        "quota-docs",
        &file.with_extension("lock"),
        || Ok(fs::write(&file, b"legacy")?),
        |db, origin| {
            unified::doc_put(
                db,
                "hooks/agy-quota.json",
                "quota-docs",
                b"legacy",
                origin,
                1,
            )
            .map(|_| ())
        },
    )
    .unwrap();
    assert!(!path.exists());
    let db = unified::open_unified(&path).unwrap();
    for mode in [Mode::Legacy, Mode::Mirror, Mode::Unified, Mode::Sealed] {
        unified::set_mode(&db, "quota-docs", mode, "test", 1).unwrap();
        let body = format!("{mode:?}").into_bytes();
        let before = fs::read(&file).unwrap();
        caller::write(
            &home,
            "quota-docs",
            &file.with_extension("lock"),
            || Ok(fs::write(&file, &body)?),
            |db, origin| {
                unified::doc_put(db, "hooks/agy-quota.json", "quota-docs", &body, origin, 1)
                    .map(|_| ())
            },
        )
        .unwrap();
        assert_eq!(
            fs::read(&file).unwrap(),
            if mode == Mode::Sealed {
                before
            } else {
                body.clone()
            }
        );
        if mode != Mode::Legacy {
            assert_eq!(
                unified::doc_get(&db, "hooks/agy-quota.json")
                    .unwrap()
                    .unwrap()
                    .body,
                body
            );
        }
    }
    fs::remove_dir_all(home).unwrap();
}

#[test]
fn caller_rejects_future_wal_without_mutating_source() {
    let home = std::env::temp_dir().join(format!("caller-future-{}", std::process::id()));
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(&home)
        .unwrap();
    let path = unified::unified_path(&home);
    let db = unified::open_unified(&path).unwrap();
    db.pragma_update(None, "wal_autocheckpoint", 0).unwrap();
    db.pragma_update(None, "user_version", 99).unwrap();
    let wal = std::path::PathBuf::from(format!("{}-wal", path.display()));
    let before = (fs::read(&path).unwrap(), fs::read(&wal).unwrap());
    assert!(caller::CallerAccess::open(&home, "session-status").is_err());
    assert_eq!((fs::read(&path).unwrap(), fs::read(&wal).unwrap()), before);
    drop(db);
    fs::remove_dir_all(home).unwrap();
}
#[test]
#[ignore = "diagnóstico de admisión SQLite; ejecutar en release con --nocapture"]
fn release_caller_200_writes_profile() {
    use comandos_store::files::{FileLock, write_atomic};
    use std::time::Instant;
    let home = std::env::temp_dir().join(format!("caller-profile-{}", std::process::id()));
    let dir = home.join(".claude/hooks/state");
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(&dir)
        .unwrap();
    let path = unified::unified_path(&home);
    let db = unified::open_unified(&path).unwrap();
    unified::set_mode(&db, "session-status", Mode::Mirror, "test", 1).unwrap();
    let file = dir.join("pane.json");
    let lock = file.with_extension("json.lock");
    let body=b"{\n  \"project\": \"p\",\n  \"status\": \"done\",\n  \"detail\": \"answer\",\n  \"cwd\": \"/private\",\n  \"ts\": 2,\n  \"options\": \"\",\n  \"last\": \"\",\n  \"agent\": \"claude\",\n  \"session\": \"s\",\n  \"pane\": \"%1\"\n}\n";
    let mut samples = Vec::new();
    let mut opens = Vec::new();
    let mut writes = Vec::new();
    for _ in 0..200 {
        let start = Instant::now();
        let access = caller::CallerAccess::open(&home, "session-status").unwrap();
        opens.push(start.elapsed());
        let _lock = FileLock::exclusive(&lock).unwrap();
        let write = Instant::now();
        access
            .write(
                || Ok(write_atomic(&file, body)?),
                |db, origin| unified::status_put(db, "pane.json", body, 2_000_000_000, origin),
            )
            .unwrap();
        writes.push(write.elapsed());
        drop(access);
        samples.push(start.elapsed());
    }
    samples.sort();
    opens.sort();
    writes.sort();
    eprintln!(
        "release200 median_total_us={} median_open_us={} median_write_us={}",
        samples[100].as_micros(),
        opens[100].as_micros(),
        writes[100].as_micros()
    );
    // Un único ensayo grande limita el trabajo y revela el coste de copiar el WAL realista.
    db.execute("CREATE TABLE caller_profile_payload(body BLOB)", [])
        .unwrap();
    db.execute(
        "INSERT INTO caller_profile_payload VALUES(zeroblob(172000000))",
        [],
    )
    .unwrap();
    let start = Instant::now();
    let access = caller::CallerAccess::open(&home, "session-status").unwrap();
    eprintln!("172MB_one_open_ms={}", start.elapsed().as_millis());
    drop(access);
    fs::remove_dir_all(home).unwrap();
}

#[test]
fn caller_lease_allows_other_readers_and_blocks_mode_flip_until_drop() {
    use std::sync::mpsc;
    let home = std::env::temp_dir().join(format!("caller-shared-{}", std::process::id()));
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(&home)
        .unwrap();
    let db = unified::open_unified(&unified::unified_path(&home)).unwrap();
    let access = caller::CallerAccess::open(&home, "tabs").unwrap();
    assert_eq!(
        unified::with_readonly_access(&home, "ui-docs", |mode, _| Ok(mode)).unwrap(),
        Mode::Legacy
    );
    let (started_tx, started_rx) = mpsc::channel();
    let (done_tx, done_rx) = mpsc::channel();
    let other = home.clone();
    let worker = std::thread::spawn(move || {
        let db = rusqlite::Connection::open(unified::unified_path(&other)).unwrap();
        started_tx.send(()).unwrap();
        unified::set_mode(&db, "tabs", Mode::Unified, "fixture", 1).unwrap();
        done_tx.send(()).unwrap();
    });
    started_rx.recv().unwrap();
    assert!(
        done_rx
            .recv_timeout(std::time::Duration::from_millis(50))
            .is_err()
    );
    drop(access);
    done_rx
        .recv_timeout(std::time::Duration::from_secs(2))
        .unwrap();
    worker.join().unwrap();
    drop(db);
    fs::remove_dir_all(home).unwrap();
}

#[test]
#[ignore = "gate de latencia con una base privada de 172 MB; ejecutar explícitamente"]
fn large_mirrored_caller_admission_has_bounded_latency() {
    use std::time::{Duration, Instant};
    let home = std::env::temp_dir().join(format!("caller-large-gate-{}", std::process::id()));
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(&home)
        .unwrap();
    let path = unified::unified_path(&home);
    let writer = unified::open_unified(&path).unwrap();
    writer.execute_batch("PRAGMA wal_autocheckpoint=0; CREATE TABLE unrelated_payload(body BLOB); INSERT INTO unrelated_payload VALUES(zeroblob(172000000)); PRAGMA wal_checkpoint(TRUNCATE);").unwrap();
    unified::set_mode(&writer, "session-status", Mode::Mirror, "private gate", 1).unwrap();
    assert!(fs::metadata(&path).unwrap().len() >= 172_000_000);
    let start = Instant::now();
    let access = caller::CallerAccess::open(&home, "session-status").unwrap();
    let elapsed = start.elapsed();
    let mode = access.mode();
    drop(access);
    drop(writer);
    fs::remove_dir_all(home).unwrap();
    assert_eq!(mode, Mode::Mirror);
    eprintln!("172MB_metadata_admission_us={}", elapsed.as_micros());
    assert!(
        elapsed < Duration::from_millis(50),
        "schema admission took {elapsed:?}; unrelated payload must not determine hook latency"
    );
}

#[test]
fn caller_metadata_gate_handles_schema_overflow_indices_and_latest_wal_versions() {
    let home = std::env::temp_dir().join(format!("caller-schema-trees-{}", std::process::id()));
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(&home)
        .unwrap();
    let path = unified::unified_path(&home);
    let writer = unified::open_unified(&path).unwrap();
    writer.execute_batch("PRAGMA wal_autocheckpoint=0; PRAGMA wal_checkpoint(TRUNCATE); CREATE INDEX migration_versions_idx ON schema_migrations(version)").unwrap();
    writer
        .execute(
            "UPDATE schema_migrations SET name=?1 WHERE version=101",
            ["x".repeat(32_768)],
        )
        .unwrap();
    for index in 0..180 {
        writer.execute_batch(&format!("CREATE TABLE schema_growth_{index}(one TEXT,two TEXT,three TEXT,four TEXT,five TEXT,six TEXT,seven TEXT,eight TEXT,nine TEXT,ten TEXT)")).unwrap();
    }
    unified::set_mode(
        &writer,
        "session-status",
        Mode::Mirror,
        "private tree check",
        1,
    )
    .unwrap();
    let access = caller::CallerAccess::open(&home, "session-status").unwrap();
    assert_eq!(access.mode(), Mode::Mirror);
    drop(access);
    writer
        .execute("INSERT INTO schema_migrations VALUES(105,'future',0)", [])
        .unwrap();
    let wal = std::path::PathBuf::from(format!("{}-wal", path.display()));
    let before = (fs::read(&path).unwrap(), fs::read(&wal).unwrap());
    let error = match caller::CallerAccess::open(&home, "session-status") {
        Ok(_) => panic!("accepted a future migration in the WAL"),
        Err(error) => error.to_string(),
    };
    assert!(error.contains("versión más nueva"), "{error}");
    assert_eq!((fs::read(&path).unwrap(), fs::read(&wal).unwrap()), before);
    drop(writer);
    fs::remove_dir_all(home).unwrap();
}
