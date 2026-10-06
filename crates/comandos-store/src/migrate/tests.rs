//! Fixtures unitarios para la carrera entre instantánea y escritura en espejo.
use super::{StepReport, import, sources};
use crate::{
    domains::{StatusDir, catalog},
    unified::{self, Mode},
};
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::PathBuf,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "comandos-backfill-race-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(path.join(".claude/hooks/state")).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
        Self(path)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
#[test]
fn backfill_never_overwrites_newer_mirror_write() {
    let h = Fixture::new();
    let db = h.0.join("db/comandos.sqlite3");
    let c = unified::open_unified(&db).unwrap();
    let path = h.0.join(".claude/hooks/state/p--s--1.json");
    fs::write(&path, b"{\"status\":\"done\"}").unwrap();
    fs::File::open(&path)
        .unwrap()
        .set_modified(UNIX_EPOCH + Duration::from_secs(1))
        .unwrap();
    let source = sources::Source {
        path: path.clone(),
        symbolic: "H/state/p--s--1.json".into(),
        spec: *catalog::source("H/state/p--s--1.json").unwrap(),
        db: db.clone(),
    };
    unified::modes::prepare_mirror(&h.0, &c, "session-status", 1).unwrap();
    assert_eq!(
        unified::mode_of(Some(&c), "session-status").unwrap(),
        Mode::Mirror
    );
    let snapshot = sources::read(&path).unwrap();
    let writer = unified::open_unified(&db).unwrap();
    let statuses = StatusDir {
        home: &h.0,
        dir: h.0.join(".claude/hooks/state"),
        domain: "session-status",
    };
    statuses
        .write(
            Some(&writer),
            "p--s--1.json",
            b"{\"status\":\"working\"}",
            2_000_000_000,
        )
        .unwrap();
    let mut step = StepReport {
        domain: "session-status".into(),
        source: source.symbolic.clone(),
        status: "done".into(),
        rows: 0,
        bytes: 0,
        elapsed_ms: 0,
        detail: String::new(),
    };
    let tx = c.unchecked_transaction().unwrap();
    import::apply(&c, &source, &snapshot, 2000, &mut step).unwrap();
    tx.commit().unwrap();
    assert_eq!(step.rows, 0);
    assert_eq!(
        statuses.read(Some(&c), "p--s--1.json").unwrap().unwrap(),
        b"{\"status\":\"working\"}"
    );
    let row: Vec<u8> = c
        .query_row(
            "SELECT body FROM session_status WHERE file_key='p--s--1.json'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(row, b"{\"status\":\"working\"}");
}
#[test]
fn source_replaced_with_control_after_collection_is_rejected() {
    let h = Fixture::new();
    let db = h.0.join("db/comandos.sqlite3");
    let c = unified::open_unified(&db).unwrap();
    drop(c);
    let path = h.0.join(".claude/hooks/state/p--s--1.json");
    fs::write(&path, b"old source").unwrap();
    let collected = sources::collect(
        &h.0,
        &db,
        &[*catalog::source("H/state/p--s--1.json").unwrap()],
    )
    .unwrap();
    fs::remove_file(&path).unwrap();
    fs::hard_link(&db, &path).unwrap();
    assert!(sources::read_source(&h.0, &collected[0]).is_err());
}
