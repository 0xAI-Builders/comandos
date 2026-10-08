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

#[test]
fn no_snapshot_before_restore_finishes() {
    use comandos_app::restore::{RestoreCoordinator, RestoreResult};
    let f = TestTmux::for_mode(RunMode::Sandbox).unwrap();
    f.guard.create_dir_all(f.config.hooks_dir(), 0o700).unwrap();
    let files = StateFiles::new(f.config.clone(), f.guard.clone());
    let original = json!({"version":2,"sessions":{}});
    files.write("app-sessions-v2.json", &original).unwrap();
    let mut coordinator = RestoreCoordinator::default();
    coordinator.begin();
    let candidate = json!({"version":2,"sessions":{},"saved_at":42});
    files
        .write_snapshot_when("app-sessions-v2.json", &candidate, || coordinator.ready())
        .unwrap();
    assert_eq!(files.read("app-sessions-v2.json").unwrap(), original);
    coordinator.finish(RestoreResult::Complete);
    files
        .write_snapshot_when("app-sessions-v2.json", &candidate, || coordinator.ready())
        .unwrap();
    assert_eq!(files.read("app-sessions-v2.json").unwrap(), candidate);
}

#[test]
fn stale_tab_write_is_discarded_inside_shared_lock() {
    let f = TestTmux::for_mode(RunMode::Sandbox).unwrap();
    f.guard.create_dir_all(f.config.hooks_dir(), 0o700).unwrap();
    let files = StateFiles::new(f.config.clone(), f.guard.clone());
    files.write_tabs(&json!({"term-new":"New"})).unwrap();
    files
        .write_tabs_when(&json!({"term-old":"Old"}), || false)
        .unwrap();
    assert_eq!(
        files.read("app-tabs.json").unwrap(),
        json!({"term-new":"New"})
    );
}

#[test]
fn state_files_lock_child() {
    if std::env::var("COMANDOS_LOCK_CHILD").ok().as_deref() != Some("1") {
        return;
    }
    let cfg = comandos_app::config::parse_args(
        &[
            "--mode".into(),
            "sandbox".into(),
            "--tmux-socket".into(),
            "t".into(),
        ],
        false,
        &|key| std::env::var(key).ok(),
    )
    .unwrap();
    let guard = comandos_app::guard::WriteGuard::from_config(&cfg, ":fixture");
    let state = StateFiles::new(cfg.clone(), guard);
    std::fs::write(cfg.hooks_dir().join("child-ready"), b"ready").unwrap();
    state
        .write_tabs(&json!({"local":"Local","term-child":"Child 日本"}))
        .unwrap();
}

#[test]
fn tabs_write_holds_shared_lock_between_processes() {
    use nix::fcntl::{Flock, FlockArg};
    use std::{
        process::Command,
        time::{Duration, Instant},
    };
    let f = TestTmux::for_mode(RunMode::Sandbox).unwrap();
    f.guard.create_dir_all(f.config.hooks_dir(), 0o700).unwrap();
    let state = StateFiles::new(f.config.clone(), f.guard.clone());
    state.write_tabs(&json!({"term-before":"Before"})).unwrap();
    let lock = Flock::lock(
        f.guard
            .open_lock(&f.config.hooks_dir().join("app-tabs.json.lock"))
            .unwrap(),
        FlockArg::LockExclusive,
    )
    .unwrap();
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "state_files_lock_child", "--nocapture"])
        .env_clear()
        .envs(f.env.iter().cloned())
        .env("PATH", "/usr/bin:/bin")
        .env("COMANDOS_LOCK_CHILD", "1")
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(2);
    while !f.config.hooks_dir().join("child-ready").exists() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(f.config.hooks_dir().join("child-ready").exists());
    std::thread::sleep(Duration::from_millis(40));
    assert!(child.try_wait().unwrap().is_none());
    assert_eq!(
        state.read("app-tabs.json").unwrap(),
        json!({"term-before":"Before"})
    );
    drop(lock);
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        if let Some(exit) = child.try_wait().unwrap() {
            assert!(exit.success());
            break;
        }
        if Instant::now() >= deadline {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("child did not acquire released flock");
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(
        state.read("app-tabs.json").unwrap(),
        json!({"local":"Local","term-child":"Child 日本"})
    );
    assert!(
        std::fs::read_dir(f.config.hooks_dir())
            .unwrap()
            .filter_map(Result::ok)
            .all(|entry| !entry.file_name().to_string_lossy().ends_with(".tmp"))
    );
}

#[test]
fn tab_history_keeps_latest_identity_and_caps_eighty_entries() {
    let f = TestTmux::for_mode(RunMode::Sandbox).unwrap();
    f.guard.create_dir_all(f.config.hooks_dir(), 0o700).unwrap();
    let state = StateFiles::new(f.config.clone(), f.guard.clone());
    state
        .write(
            "app-tabs-history.json",
            &json!(
                (0..85)
                    .map(|i| json!({"session":format!("term-{i}"),"label":"old"}))
                    .collect::<Vec<_>>()
            ),
        )
        .unwrap();
    let entry = json!({"session":"term-8","label":"nuevo 日本","cwd":"/private","agent":"codex","reason":"closed","ts":42});
    state.archive_tab(&entry).unwrap();
    let result = state.read("app-tabs-history.json").unwrap();
    assert_eq!(result.as_array().unwrap().len(), 80);
    assert_eq!(result[0], entry);
    assert_eq!(
        result
            .as_array()
            .unwrap()
            .iter()
            .filter(|row| row["session"] == "term-8")
            .count(),
        1
    );
}

#[test]
fn session_snapshot_backup_recovers_corruption_and_archives_once_per_minute() {
    let f = TestTmux::for_mode(RunMode::Sandbox).unwrap();
    f.guard.create_dir_all(f.config.hooks_dir(), 0o700).unwrap();
    let state = StateFiles::new(f.config.clone(), f.guard.clone());
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let first = json!({"version":2,"sessions":{},"saved_at":now});
    let second = json!({"version":2,"sessions":{},"saved_at":now+1});
    state
        .write_snapshot_when("app-sessions-v2.json", &first, || true)
        .unwrap();
    state
        .write_snapshot_when("app-sessions-v2.json", &second, || true)
        .unwrap();
    let history = f.config.hooks_dir().join("app-sessions-v2.json.history");
    let archived: Vec<_> = std::fs::read_dir(&history)
        .unwrap()
        .map(Result::unwrap)
        .collect();
    assert_eq!(archived.len(), 1);
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&std::fs::read(archived[0].path()).unwrap())
            .unwrap(),
        first
    );
    assert_eq!(state.read_session_snapshot(), second);
    std::fs::write(f.config.hooks_dir().join("app-sessions-v2.json"), b"{").unwrap();
    assert_eq!(state.read_session_snapshot(), first);
    assert!(
        state
            .write_snapshot_when("app-sessions-v2.json", &json!({"version":2}), || true)
            .is_err()
    );
    assert_eq!(state.read_session_snapshot(), first);
}
