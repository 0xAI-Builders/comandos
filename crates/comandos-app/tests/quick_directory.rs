#![allow(clippy::unwrap_used, clippy::disallowed_methods)]
use comandos_app::{config::parse_args, guard::WriteGuard};
use std::os::unix::fs::{PermissionsExt, symlink};

struct PrivateRoot(std::path::PathBuf);
impl PrivateRoot {
    fn path(&self) -> &std::path::Path {
        &self.0
    }
}
impl Drop for PrivateRoot {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}
struct Fixture(PrivateRoot, comandos_app::config::AppConfig, WriteGuard);
fn fixture() -> Fixture {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let root = PrivateRoot(std::env::temp_dir().join(format!(
        "comandos-quick-directory-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    )));
    std::fs::create_dir(root.path()).unwrap();
    let home = root.path().join("home");
    let run = root.path().join("run");
    let tmp = root.path().join("tmp");
    for path in [&home, &run, &tmp] {
        std::fs::create_dir(path).unwrap();
    }
    std::fs::set_permissions(&run, std::fs::Permissions::from_mode(0o700)).unwrap();
    let env = |key: &str| match key {
        "HOME" => Some(home.display().to_string()),
        "XDG_RUNTIME_DIR" => Some(run.display().to_string()),
        "TMPDIR" => Some(tmp.display().to_string()),
        _ => None,
    };
    let cfg = parse_args(
        &["--tmux-socket".into(), "quick-directory-private".into()],
        false,
        &env,
    )
    .unwrap();
    let guard = WriteGuard::from_config(&cfg, ":private-test");
    Fixture(root, cfg, guard)
}
#[test]
fn post_config_symlink_cannot_reserve_outside_both_sandbox_roots() {
    let Fixture(root, cfg, guard) = fixture();
    let private_home = cfg.sandbox_root().unwrap().join("home");
    guard.create_dir_all(&private_home, 0o700).unwrap();
    let outside = root.path().join("outside-allowlist");
    std::fs::create_dir(&outside).unwrap();
    std::fs::write(outside.join("sentinel"), b"unchanged").unwrap();
    symlink(&outside, private_home.join("codebase")).unwrap();
    let before = std::fs::read_dir(&outside)
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect::<Vec<_>>();
    let now = chrono::DateTime::parse_from_rfc3339("2026-10-05T06:00:00Z").unwrap();
    assert!(
        guard
            .reserve_quick_directory(&private_home.join("codebase/0xJesus/Terminal"), now)
            .is_err()
    );
    let after = std::fs::read_dir(&outside)
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect::<Vec<_>>();
    assert_eq!(before, after);
    assert_eq!(
        std::fs::read(outside.join("sentinel")).unwrap(),
        b"unchanged"
    );
}
#[test]
fn guarded_reservation_keeps_timezone_exclusive_collisions_and_permissions() {
    let Fixture(_root, cfg, guard) = fixture();
    let base = cfg
        .sandbox_root()
        .unwrap()
        .join("home/codebase/0xJesus/Terminal");
    let now = chrono::DateTime::parse_from_rfc3339("2026-10-05T06:00:00Z").unwrap();
    let first = guard.reserve_quick_directory(&base, now).unwrap();
    let second = guard.reserve_quick_directory(&base, now).unwrap();
    assert_eq!(first, base.join("T-2026-10-05-00-00-00"));
    assert_eq!(second, base.join("T-2026-10-05-00-00-00-2"));
    assert_eq!(
        std::fs::metadata(second).unwrap().permissions().mode() & 0o777,
        0o700
    );
}
