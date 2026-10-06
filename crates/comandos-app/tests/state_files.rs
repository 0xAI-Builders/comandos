#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::disallowed_methods
)]
mod support;

use comandos_app::{config::RunMode, state_files::StateFiles};
use serde_json::json;
use support::tmux::TestTmux;

#[test]
fn absent_and_corrupt_files_are_distinct() {
    let f = TestTmux::for_mode(RunMode::Sandbox).unwrap();
    f.guard.create_dir_all(f.config.hooks_dir(), 0o700).unwrap();
    let files = StateFiles::new(f.config.clone(), f.guard.clone());
    assert_eq!(
        files.read("app-tabs.json").unwrap(),
        serde_json::Value::Null
    );
    std::fs::write(f.config.hooks_dir().join("app-tabs.json"), b"{").unwrap();
    assert!(files.read("app-tabs.json").is_err());
}

#[test]
fn tabs_write_uses_shared_sidecar_lock_and_exact_json() {
    let f = TestTmux::for_mode(RunMode::Sandbox).unwrap();
    f.guard.create_dir_all(f.config.hooks_dir(), 0o700).unwrap();
    let files = StateFiles::new(f.config.clone(), f.guard.clone());
    let value = json!({"local": "Local", "ñ": "andú"});
    files.write_tabs(&value).unwrap();
    assert_eq!(files.read("app-tabs.json").unwrap(), value);
    assert!(f.config.hooks_dir().join("app-tabs.json.lock").exists());
    let leftovers: Vec<_> = std::fs::read_dir(f.config.hooks_dir())
        .unwrap()
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.file_name().to_string_lossy().ends_with(".tmp"))
        .collect();
    assert!(leftovers.is_empty());
}

#[test]
fn only_global_constraint_file_names_are_accepted() {
    let f = TestTmux::for_mode(RunMode::Sandbox).unwrap();
    f.guard.create_dir_all(f.config.hooks_dir(), 0o700).unwrap();
    let files = StateFiles::new(f.config.clone(), f.guard.clone());
    assert!(files.write("app-tabs.json", &json!({})).is_ok());
    assert!(files.write("../app-tabs.json", &json!({})).is_err());
    assert!(files.write("not-allowed.json", &json!({})).is_err());
}
