#![allow(clippy::unwrap_used)]
use comandos_app_mac::files::MacFiles;
use comandos_store::{
    domains::DomainStore,
    unified::{self, Mode},
};
use serde_json::json;
use std::{fs, os::unix::fs::DirBuilderExt, path::PathBuf};
struct Fixture(PathBuf);
impl Fixture {
    fn new(tag: &str) -> Self {
        let p = std::env::temp_dir().join(format!("mac-m4-files-{tag}-{}", std::process::id()));
        fs::DirBuilder::new().mode(0o700).create(&p).unwrap();
        Self(p)
    }
    fn files(&self) -> MacFiles {
        MacFiles::new(self.0.clone(), self.0.join(".claude/hooks")).unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
#[test]
fn cold_mac_reads_do_not_create_database_hooks_or_control_files() {
    let f = Fixture::new("cold");
    assert!(f.files().load_saved().unwrap().is_empty());
    assert!(f.files().metadata().unwrap().is_empty());
    assert_eq!(fs::read_dir(&f.0).unwrap().count(), 0);
}
#[test]
fn save_and_history_use_authoritative_store_in_all_four_modes() {
    for mode in [Mode::Legacy, Mode::Mirror, Mode::Unified, Mode::Sealed] {
        let f = Fixture::new(&format!("{mode:?}"));
        let db = unified::open_unified(&unified::unified_path(&f.0)).unwrap();
        if mode != Mode::Legacy {
            unified::set_mode(&db, "tabs", mode, "own", 1).unwrap();
        }
        drop(db);
        let files = f.files();
        files.load_saved().unwrap();
        assert!(
            files
                .save_tabs(&json!({"one":"One","two":"Two"}), &|| true, 1)
                .unwrap()
        );
        let history: Vec<_> = (0..84)
            .map(|n| json!({"session":format!("s{n}"),"label":"own"}))
            .collect();
        let hdoc = DomainStore { home: &f.0 }.document(
            "hooks/app-tabs-history.json",
            "tabs",
            f.0.join(".claude/hooks/app-tabs-history.json"),
        );
        hdoc.update_owned(2, |_| Ok(Some(serde_json::to_vec(&history).unwrap())))
            .unwrap();
        files
            .archive(&json!({"session":"s83","label":"First"}), &|| true, 3)
            .unwrap();
        files
            .archive(&json!({"session":"s1","label":"New"}), &|| true, 4)
            .unwrap();
        let h = DomainStore { home: &f.0 }
            .document(
                "hooks/app-tabs-history.json",
                "tabs",
                f.0.join(".claude/hooks/app-tabs-history.json"),
            )
            .read_readonly()
            .unwrap()
            .unwrap();
        let h: serde_json::Value = serde_json::from_slice(&h).unwrap();
        assert_eq!(h.as_array().unwrap().len(), 80);
        assert_eq!(h[0]["label"], "New");
        assert_eq!(
            h.as_array()
                .unwrap()
                .iter()
                .filter(|v| v["session"] == "s1")
                .count(),
            1
        );
        assert_eq!(
            files.load_saved().unwrap(),
            vec![("one".into(), json!("One")), ("two".into(), json!("Two"))]
        );
        if mode == Mode::Sealed {
            assert!(!f.0.join(".claude/hooks").exists());
        }
    }
}
#[test]
fn stale_compare_and_swap_or_cancelled_owner_never_overwrites_other_writer() {
    let f = Fixture::new("cas");
    let files = f.files();
    files.load_saved().unwrap();
    assert!(
        !files
            .save_tabs(&json!({"no":"write"}), &|| false, 1)
            .unwrap()
    );
    let doc = DomainStore { home: &f.0 }.document(
        "hooks/app-tabs.json",
        "tabs",
        f.0.join(".claude/hooks/app-tabs.json"),
    );
    doc.update_owned(2, |_| Ok(Some(b"{\"external\":\"Keep\"}".to_vec())))
        .unwrap();
    assert!(
        files
            .save_tabs(&json!({"stale":"Bad"}), &|| true, 3)
            .is_err()
    );
    assert_eq!(
        doc.read_readonly().unwrap().unwrap(),
        b"{\"external\":\"Keep\"}"
    );
}
#[test]
fn moved_or_stale_seal_authority_rejects_before_tab_document_lock() {
    for kind in ["moved", "seal"] {
        let f = Fixture::new(kind);
        let db = unified::open_unified(&unified::unified_path(&f.0)).unwrap();
        if kind == "moved" {
            db.pragma_update(None, "user_version", unified::MOVED_SENTINEL)
                .unwrap();
            drop(db);
        } else {
            unified::set_mode(&db, "tabs", Mode::Sealed, "own", 1).unwrap();
            drop(db);
            fs::remove_file(unified::unified_path(&f.0)).unwrap();
        }
        let files = f.files();
        assert!(files.load_saved().is_err());
        assert!(files.save_tabs(&json!({"bad":"Bad"}), &|| true, 1).is_err());
        assert!(!f.0.join(".claude/hooks").exists());
    }
}

#[test]
fn cold_legacy_save_keeps_database_and_mode_control_absent() {
    let f = Fixture::new("legacy-write");
    let files = f.files();
    files.load_saved().unwrap();
    assert!(files.save_tabs(&json!({"own":"Own"}), &|| true, 1).unwrap());
    assert!(!unified::unified_path(&f.0).exists());
    let doc = DomainStore { home: &f.0 }.document(
        "hooks/app-tabs.json",
        "tabs",
        f.0.join(".claude/hooks/app-tabs.json"),
    );
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&doc.read_readonly().unwrap().unwrap())
            .unwrap(),
        json!({"own":"Own"})
    );
}
