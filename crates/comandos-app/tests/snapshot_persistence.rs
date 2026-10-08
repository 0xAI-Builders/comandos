use comandos_app::{config, guard::WriteGuard, state_files::StateFiles};
use serde_json::json;
use std::{
    fs,
    os::unix::fs::{DirBuilderExt, symlink},
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};
static SEQ: AtomicU64 = AtomicU64::new(0);

#[allow(clippy::disallowed_methods)]
#[test]
fn live_tab_snapshot_uses_its_exact_guarded_lock_without_widening_access() {
    let root = std::env::temp_dir().join(format!(
        "comandos-app-live-tab-snapshot-{}-{}",
        std::process::id(),
        SEQ.fetch_add(1, Ordering::Relaxed)
    ));
    for sub in ["home/.claude/hooks", "run", "tmp"] {
        fs::DirBuilder::new().recursive(true).mode(0o700).create(root.join(sub)).unwrap();
    }
    let env = |name: &str| match name {
        "HOME" => Some(root.join("home")),
        "XDG_RUNTIME_DIR" => Some(root.join("run")),
        "TMPDIR" => Some(root.join("tmp")),
        _ => None,
    }.map(|p| p.to_string_lossy().into_owned());
    let config = config::parse_args(&["--mode".into(), "live".into()], false, &env).unwrap();
    let guard = WriteGuard::from_config(&config, ":private-snapshot");
    let files = StateFiles::new(config.clone(), guard.clone());
    let snapshot = json!({"local":{"agent":"codex","resume_id":"preserved-conversation"}});
    files.write_snapshot_when("app-tabs-snapshot.json", &snapshot, || true).unwrap();
    assert_eq!(files.read("app-tabs-snapshot.json").unwrap(), snapshot);
    let lock = config.hooks_dir().join("app-tabs-snapshot.json.lock");
    assert!(lock.is_file());
    assert!(matches!(guard.open_lock(&config.hooks_dir().join("unrelated.json.lock")),
        Err(comandos_app::guard::GuardError::Outside(_))));
    fs::remove_file(&lock).unwrap();
    let outside = root.join("untouched");
    fs::write(&outside, b"original").unwrap();
    symlink(&outside, &lock).unwrap();
    assert!(files.write_snapshot_when("app-tabs-snapshot.json", &json!({}), || true).is_err());
    assert_eq!(fs::read(&outside).unwrap(), b"original");
    assert_eq!(files.read("app-tabs-snapshot.json").unwrap(), snapshot);
    fs::remove_dir_all(root).unwrap();
}
// These writes construct corruption and symlink fixtures outside the app guard.
#[allow(clippy::disallowed_methods)]
#[test]
fn shared_snapshot_policy_preserves_guard_backup_history_and_stale_gate_without_tmux_or_gtk() {
    let root = std::env::temp_dir().join(format!(
        "comandos-app-snapshot-policy-{}-{}",
        std::process::id(),
        SEQ.fetch_add(1, Ordering::Relaxed)
    ));
    for sub in ["home", "run", "tmp", "data", "config", "cache", "state"] {
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(root.join(sub))
            .unwrap();
    }
    let env = |name: &str| {
        match name {
            "HOME" => Some(root.join("home")),
            "XDG_RUNTIME_DIR" => Some(root.join("run")),
            "TMPDIR" => Some(root.join("tmp")),
            "XDG_DATA_HOME" => Some(root.join("data")),
            "XDG_CONFIG_HOME" => Some(root.join("config")),
            "XDG_CACHE_HOME" => Some(root.join("cache")),
            "XDG_STATE_HOME" => Some(root.join("state")),
            _ => None,
        }
        .map(|p| p.to_string_lossy().into_owned())
    };
    let config = config::parse_args(
        &[
            "--mode".into(),
            "sandbox".into(),
            "--tmux-socket".into(),
            "private".into(),
        ],
        false,
        &env,
    )
    .unwrap();
    let guard = WriteGuard::from_config(&config, ":private");
    guard.create_dir_all(config.hooks_dir(), 0o700).unwrap();
    let files = StateFiles::new(config.clone(), guard);
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
        / 60
        * 60;
    let first = json!({"version":2,"sessions":{},"saved_at":now});
    let second = json!({"version":2,"sessions":{},"saved_at":now+1});
    files
        .write_snapshot_when("app-sessions-v2.json", &first, || true)
        .unwrap();
    files
        .write_snapshot_when("app-sessions-v2.json", &second, || false)
        .unwrap();
    assert_eq!(files.read_session_snapshot(), first);
    files
        .write_snapshot_when("app-sessions-v2.json", &second, || true)
        .unwrap();
    assert_eq!(files.read_session_snapshot(), second);
    assert_eq!(
        fs::read_dir(config.hooks_dir().join("app-sessions-v2.json.history"))
            .unwrap()
            .count(),
        1
    );
    fs::write(
        config
            .hooks_dir()
            .join("app-sessions-v2.json.history/000000000100.json"),
        b"ancient",
    )
    .unwrap();
    let third = json!({"version":2,"sessions":{},"saved_at":now+2});
    files
        .write_snapshot_when("app-sessions-v2.json", &third, || true)
        .unwrap();
    assert!(
        !config
            .hooks_dir()
            .join("app-sessions-v2.json.history/000000000100.json")
            .exists()
    );
    fs::write(config.hooks_dir().join("app-sessions-v2.json"), b"{").unwrap();
    assert_eq!(files.read_session_snapshot(), second);
    assert!(
        files
            .write_snapshot_when("app-sessions-v2.json", &json!({"version":2}), || true)
            .is_err()
    );
    let target = root.join("outside");
    fs::write(&target, b"untouched").unwrap();
    fs::remove_file(config.hooks_dir().join("app-sessions-v2.json")).unwrap();
    symlink(&target, config.hooks_dir().join("app-sessions-v2.json")).unwrap();
    files
        .write_snapshot_when("app-sessions-v2.json", &second, || true)
        .unwrap();
    assert!(
        !fs::symlink_metadata(config.hooks_dir().join("app-sessions-v2.json"))
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(fs::read(target).unwrap(), b"untouched");
    fs::rename(config.hooks_dir(), root.join("old-hooks")).unwrap();
    let outside = root.join("outside-dir");
    fs::create_dir(&outside).unwrap();
    symlink(&outside, config.hooks_dir()).unwrap();
    assert!(
        files
            .write_snapshot_when("app-sessions-v2.json", &third, || true)
            .is_err()
    );
    assert_eq!(fs::read_dir(outside).unwrap().count(), 0);
    fs::remove_dir_all(root).unwrap();
}
