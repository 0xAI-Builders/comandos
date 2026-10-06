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
