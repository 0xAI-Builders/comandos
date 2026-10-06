use comandos_cli::codex::reports::Reports;
use comandos_store::unified::{self, Mode, Origin};
use serde_json::{Value, json};
use std::{
    fs,
    os::unix::fs::{DirBuilderExt, PermissionsExt},
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        static N: AtomicUsize = AtomicUsize::new(0);
        let p = std::env::temp_dir().join(format!(
            "codex-reports-c5-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        fs::DirBuilder::new().mode(0o700).create(&p).unwrap();
        Self(p)
    }
    fn dir(&self) -> PathBuf {
        self.0.join(".local/state/comandos/codex-full-access")
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn report() -> Value {
    json!({"plans":[],"results":[]})
}
#[test]
fn four_modes_write_through_owned_authority_and_sealed_never_recreates_legacy() {
    for mode in [Mode::Legacy, Mode::Mirror, Mode::Unified, Mode::Sealed] {
        let f = Fixture::new();
        if mode != Mode::Legacy {
            let db = unified::open_unified(&unified::unified_path(&f.0)).unwrap();
            unified::set_mode(&db, "codex-reports", mode, "private", 1).unwrap();
            drop(db);
        }
        let reports = Reports::new(&f.0, f.dir()).unwrap();
        reports.save("100.json", &report(), 100).unwrap();
        assert_eq!(
            reports.read_path(&f.dir().join("100.json")).unwrap(),
            report()
        );
        if mode == Mode::Sealed {
            assert!(!f.dir().exists());
        } else {
            assert_eq!(
                fs::metadata(f.dir()).unwrap().permissions().mode() & 0o777,
                0o700
            );
            assert_eq!(
                fs::metadata(f.dir().join("100.json"))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }
        let db = unified::unified_path(&f.0);
        if mode == Mode::Legacy {
            assert!(!db.exists());
        } else {
            let db = unified::open_unified(&db).unwrap();
            assert_eq!(
                unified::doc_get(&db, "state/codex-full-access/100.json")
                    .unwrap()
                    .unwrap()
                    .domain,
                "codex-reports"
            );
        }
    }
}
#[test]
fn latest_sql_same_millisecond_uses_numeric_filename_tie_and_never_legacy() {
    let f = Fixture::new();
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(f.dir())
        .unwrap();
    fs::write(f.dir().join("999999.json"), b"legacy must not win").unwrap();
    let db = unified::open_unified(&unified::unified_path(&f.0)).unwrap();
    for key in ["9.json", "10.json"] {
        unified::doc_put(
            &db,
            &format!("state/codex-full-access/{key}"),
            "codex-reports",
            serde_json::to_string(&report()).unwrap().as_bytes(),
            Origin::Import,
            7,
        )
        .unwrap();
    }
    unified::set_mode(&db, "codex-reports", Mode::Unified, "private", 1).unwrap();
    drop(db);
    let r = Reports::new(&f.0, f.dir()).unwrap();
    assert_eq!(r.latest().unwrap().0, f.dir().join("10.json"));
    assert!(r.read_path(&f.dir().join("999999.json")).is_err());
}
#[test]
fn malformed_duplicate_results_unknown_path_and_future_schema_reject_without_writes() {
    let f = Fixture::new();
    let r = Reports::new(&f.0, f.dir()).unwrap();
    assert!(r.latest().is_err());
    assert!(!f.dir().exists());
    assert!(r.read_path(&PathBuf::from("relative.json")).is_err());
    assert!(r.read_path(&f.0.join("other.json")).is_err());
    assert!(
        r.save(
            "1.json",
            &json!({"plans":[{"pane":"%1"},{"pane":"%1"}],"results":[]}),
            1
        )
        .is_err()
    );
    assert!(!f.dir().exists());
    let db = unified::open_unified(&unified::unified_path(&f.0)).unwrap();
    db.pragma_update(None, "user_version", 999999).unwrap();
    drop(db);
    assert!(r.save("1.json", &report(), 1).is_err());
    assert!(!f.dir().exists());
}
#[test]
fn release_receipt_is_separate_from_full_access_and_does_not_claim_new_turn() {
    let f = Fixture::new();
    let r = Reports::new(&f.0, f.dir()).unwrap();
    r.save("1.json", &report(), 1).unwrap();
    let release = json!({"operation":"thread-release","plans":[{"sid":"11111111-1111-1111-1111-111111111111","home":f.0.join("account"),"binary":f.0.join("bin/codex"),"transcript":f.0.join("account/sessions/thread.jsonl")}],"results":[{"sid":"11111111-1111-1111-1111-111111111111","status":"released"}]});
    r.save("2.json", &release, 2).unwrap();
    assert_eq!(r.latest().unwrap().0, f.dir().join("1.json"));
    assert_eq!(r.read_path(&f.dir().join("2.json")).unwrap(), release);
}
#[test]
fn existing_nonprivate_report_directory_fails_without_chmod_or_report() {
    let f = Fixture::new();
    fs::create_dir_all(f.dir()).unwrap();
    fs::set_permissions(f.dir(), fs::Permissions::from_mode(0o755)).unwrap();
    let r = Reports::new(&f.0, f.dir()).unwrap();
    assert!(r.save("1.json", &report(), 1).is_err());
    assert_eq!(
        fs::metadata(f.dir()).unwrap().permissions().mode() & 0o777,
        0o755
    );
    assert!(!f.dir().join("1.json").exists());
}
