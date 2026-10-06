//! Failure injection uses only private files and callbacks, never service commands.
use comandos_cli::install::{
    full,
    plan::{self, Action},
    platform::Platform,
    release::{self, WebSource},
};
use std::{
    fs,
    os::unix::fs::{PermissionsExt, symlink},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};
static SEQ: AtomicU64 = AtomicU64::new(0);
struct Home(PathBuf);
impl Home {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!(
            "install-journal-{}-{}",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&p).unwrap();
        Self(p)
    }
}
impl Drop for Home {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn put(path: &Path, bytes: &[u8], mode: u32) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, bytes).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(mode)).unwrap();
}
#[test]
fn failed_action_restores_alias_regular_bytes_symlink_hooks_and_original_backups() {
    let home = Home::new();
    let target = home.0.join(".local/share/comandos/bin/comandos");
    put(&target, b"native", 0o755);
    let alias = home.0.join(".local/bin/test-native");
    put(&alias, b"original alias", 0o751);
    let config = home.0.join(".config/managed.conf");
    let original = home.0.join("original-config");
    put(&original, b"original config", 0o640);
    fs::create_dir_all(config.parent().unwrap()).unwrap();
    symlink(&original, &config).unwrap();
    let hooks = home.0.join(".claude/settings.json");
    put(&hooks, b"{\"custom\":true}\n", 0o640);
    let actions = vec![
        Action::Link {
            name: "test-native".into(),
            at: alias.clone(),
            target,
        },
        Action::Write {
            path: config.clone(),
            bytes: b"replacement".to_vec(),
            mode: 0o600,
        },
        Action::RegisterClaudeHooks(home.0.clone()),
        Action::AgentsSetup(home.0.clone()),
    ];
    let error = plan::apply_with(
        &actions,
        false,
        &mut |_| Err("injected late failure".into()),
    )
    .unwrap_err();
    assert!(error.contains("injected late failure"));
    assert_eq!(fs::read(&alias).unwrap(), b"original alias");
    assert_eq!(
        alias.metadata().unwrap().permissions().mode() & 0o777,
        0o751
    );
    assert_eq!(fs::read_link(&config).unwrap(), original);
    assert_eq!(fs::read(&original).unwrap(), b"original config");
    assert!(!config.with_file_name("managed.conf.pre-comandos").exists());
    assert_eq!(fs::read(hooks).unwrap(), b"{\"custom\":true}\n");
    assert!(
        !home
            .0
            .join(".local/share/comandos/rollback/test-native.target")
            .exists()
    );
    assert!(
        !home
            .0
            .join(".local/share/comandos/rollback/test-native.orig")
            .exists()
    );
}
#[test]
fn full_late_failure_restores_release_previous_app_proxy_and_plan_files_without_pruning() {
    let home = Home::new();
    let source = home.0.join("source");
    let binary = source.join("comandos");
    put(&binary, b"old-main", 0o755);
    let old = release::stage_release(&home.0, &binary, &WebSource::None).unwrap();
    put(&binary, b"before-main", 0o755);
    release::stage_release(&home.0, &binary, &WebSource::None).unwrap();
    let pointer = home.0.join(".local/share/comandos/bin/comandos");
    let old_pointer = fs::read_link(&pointer).unwrap();
    let previous = home.0.join(".local/share/comandos/releases/previous");
    let old_previous = fs::read(&previous).unwrap();
    for i in 0..8 {
        let dir = home
            .0
            .join(".local/share/comandos/releases")
            .join(format!("a{i:011}"));
        put(&dir.join("comandos"), b"retain", 0o755);
    }
    put(&binary, b"next-main", 0o755);
    put(&source.join("comandos-app"), b"next-app", 0o755);
    put(&source.join("cc-model-proxy"), b"next-proxy", 0o755);
    let proxy = home.0.join(".local/bin/cc-model-proxy");
    put(&proxy, b"original proxy alias", 0o751);
    let cfg = home.0.join(".claude/settings.json");
    put(&cfg, b"{\"theme\":\"private\"}\n", 0o640);
    let mut calls = 0;
    let error = full::run_with(
        &[
            "--home".into(),
            home.0.to_str().unwrap().into(),
            "--release".into(),
            source.to_str().unwrap().into(),
        ],
        &mut |action| {
            calls += 1;
            if matches!(action, Action::AgentsSetup(_)) {
                Err("injected agents failure".into())
            } else {
                Ok(())
            }
        },
    )
    .unwrap_err();
    assert!(error.contains("injected agents failure"));
    assert!(calls > 0);
    assert_eq!(fs::read_link(pointer).unwrap(), old_pointer);
    assert_eq!(fs::read(previous).unwrap(), old_previous);
    assert_eq!(fs::read(proxy).unwrap(), b"original proxy alias");
    assert_eq!(fs::read(cfg).unwrap(), b"{\"theme\":\"private\"}\n");
    assert!(
        !home
            .0
            .join(".local/share/comandos/bin/comandos-app")
            .exists()
    );
    assert!(!home.0.join(".local/bin/cc-app").exists());
    assert!(old.path.exists());
    for i in 0..8 {
        assert!(
            home.0
                .join(".local/share/comandos/releases")
                .join(format!("a{i:011}/comandos"))
                .exists()
        );
    }
}
#[test]
fn dry_full_preview_invokes_no_commands_and_creates_no_journal_or_pointer() {
    let home = Home::new();
    let source = home.0.join("source");
    put(&source.join("comandos"), b"native", 0o755);
    assert_eq!(
        full::run_with(
            &[
                "--home".into(),
                home.0.to_str().unwrap().into(),
                "--release".into(),
                source.to_str().unwrap().into(),
                "--dry-run".into()
            ],
            &mut |_| panic!("preview external call")
        )
        .unwrap(),
        0
    );
    assert!(!home.0.join(".local").exists());
    let _ = Platform::LinuxNative;
}
fn journal_path(home: &Path) -> PathBuf {
    fs::read_dir(home.join(".local/share/comandos/install-journals"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .find(|p| p.join("manifest.json").is_file())
        .unwrap()
}
#[test]
fn a_user_edit_during_a_later_phase_is_preserved_and_recovery_journal_retained() {
    let home = Home::new();
    let config = home.0.join(".config/managed.conf");
    put(&config, b"original", 0o640);
    let actions = [
        Action::Write {
            path: config.clone(),
            bytes: b"installed".to_vec(),
            mode: 0o600,
        },
        Action::AgentsSetup(home.0.clone()),
    ];
    let error = plan::apply_with(&actions, false, &mut |_| {
        put(&config, b"user edit", 0o600);
        Err("injected after edit".into())
    })
    .unwrap_err();
    assert!(error.contains("retained recovery journal"));
    assert!(error.contains(config.to_str().unwrap()));
    assert_eq!(fs::read(&config).unwrap(), b"user edit");
    let journal = journal_path(&home.0);
    let args = [
        "--home".into(),
        home.0.to_str().unwrap().into(),
        "--recover-install".into(),
        journal.to_str().unwrap().into(),
    ];
    assert!(full::run_with(&args, &mut |_| panic!("recovery external command")).is_err());
    assert!(journal.exists());
    assert_eq!(fs::read(&config).unwrap(), b"user edit");
    put(&config, b"original", 0o640);
    assert_eq!(
        full::run_with(&args, &mut |_| panic!("recovery external command")).unwrap(),
        0
    );
    assert!(!journal.exists());
}
#[test]
fn termination_worker() {
    let Some(home) = std::env::var_os("COMANDOS_PRIVATE_TERMINATION_WORKER") else {
        return;
    };
    let home = PathBuf::from(home);
    let config = home.join(".config/managed.conf");
    let actions = [
        Action::Write {
            path: config,
            bytes: b"installed".to_vec(),
            mode: 0o600,
        },
        Action::AgentsSetup(home.clone()),
    ];
    let _ = plan::apply_with(&actions, false, &mut |_| {
        fs::write(home.join("ready"), b"ready").unwrap();
        loop {
            std::thread::park_timeout(std::time::Duration::from_millis(100));
        }
    });
}
#[test]
fn terminating_owned_worker_leaves_durable_preimages_recoverable_without_external_commands() {
    let home = Home::new();
    let config = home.0.join(".config/managed.conf");
    put(&config, b"original", 0o640);
    let mut child = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "termination_worker", "--nocapture"])
        .env("COMANDOS_PRIVATE_TERMINATION_WORKER", &home.0)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while !home.0.join("ready").exists() {
        if std::time::Instant::now() > deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("owned worker did not reach durable phase");
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    child.kill().unwrap();
    child.wait().unwrap();
    assert_eq!(fs::read(&config).unwrap(), b"installed");
    let journal = journal_path(&home.0);
    assert_eq!(
        full::run_with(
            &[
                "--home".into(),
                home.0.to_str().unwrap().into(),
                "--recover-install".into(),
                journal.to_str().unwrap().into()
            ],
            &mut |_| panic!("recovery external command")
        )
        .unwrap(),
        0
    );
    assert_eq!(fs::read(&config).unwrap(), b"original");
    assert_eq!(
        config.metadata().unwrap().permissions().mode() & 0o777,
        0o640
    );
    assert!(!journal.exists());
}
#[test]
fn stage_worker() {
    let Some(home) = std::env::var_os("COMANDOS_PRIVATE_STAGE_WORKER") else {
        return;
    };
    let home = PathBuf::from(home);
    fs::write(home.join("stage-started"), b"start").unwrap();
    release::stage_release(&home, &home.join("other-candidate"), &WebSource::None).unwrap();
    fs::write(home.join("stage-finished"), b"finished").unwrap();
}
#[test]
fn standalone_stage_waits_until_full_failure_has_restored_its_pointer() {
    let home = Home::new();
    let source = home.0.join("source");
    put(&source.join("comandos"), b"old", 0o755);
    let old = release::stage_release(&home.0, &source.join("comandos"), &WebSource::None).unwrap();
    put(&source.join("comandos"), b"failed-full", 0o755);
    put(&home.0.join("other-candidate"), b"standalone", 0o755);
    struct Owned(Option<std::process::Child>);
    impl Drop for Owned {
        fn drop(&mut self) {
            if let Some(c) = self.0.as_mut() {
                let _ = c.kill();
                let _ = c.wait();
            }
        }
    }
    let mut owned = Owned(None);
    let error = full::run_with(
        &[
            "--home".into(),
            home.0.to_str().unwrap().into(),
            "--release".into(),
            source.to_str().unwrap().into(),
        ],
        &mut |_| {
            owned.0 = Some(
                std::process::Command::new(std::env::current_exe().unwrap())
                    .args(["--exact", "stage_worker", "--nocapture"])
                    .env("COMANDOS_PRIVATE_STAGE_WORKER", &home.0)
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .spawn()
                    .unwrap(),
            );
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
            while !home.0.join("stage-started").exists() {
                assert!(
                    std::time::Instant::now() < deadline,
                    "private worker did not start"
                );
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            for _ in 0..15 {
                assert!(
                    !home.0.join("stage-finished").exists(),
                    "standalone stage bypassed full-install lock"
                );
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            Err("forced full failure under lock".into())
        },
    )
    .unwrap_err();
    assert!(error.contains("forced full failure"));
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        if let Some(status) = owned.0.as_mut().unwrap().try_wait().unwrap() {
            assert!(status.success());
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "stage worker stayed locked"
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(home.0.join("stage-finished").exists());
    assert_eq!(
        fs::read_to_string(home.0.join(".local/share/comandos/releases/previous")).unwrap(),
        format!("{}\n", old.id)
    );
}
#[test]
fn rollback_never_adopts_the_default_contents_of_a_custom_file_that_was_skipped() {
    let home = Home::new();
    let config = home.0.join(".config/managed.conf");
    put(&config, b"custom", 0o600);
    let actions = [
        Action::WriteIfAbsent {
            path: config.clone(),
            bytes: b"default",
            mode: 0o600,
        },
        Action::AgentsSetup(home.0.clone()),
    ];
    let error = plan::apply_with(&actions, false, &mut |_| {
        put(&config, b"default", 0o600);
        Err("user replaced skipped config".into())
    })
    .unwrap_err();
    assert!(error.contains("retained recovery journal"));
    assert_eq!(fs::read(config).unwrap(), b"default");
    assert!(journal_path(&home.0).exists());
}
