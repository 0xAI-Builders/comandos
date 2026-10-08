//! A dedicated extension executable stays immutable across ordinary CLI upgrades.
use comandos_cli::install::full;
use std::{
    fs,
    os::unix::fs::{PermissionsExt, symlink},
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
};

static SEQ: AtomicU64 = AtomicU64::new(0);
fn private_action(action: &comandos_cli::install::plan::Action) -> Result<(), String> {
    if let comandos_cli::install::plan::Action::ExtensionOperation {
        home,
        journal,
        operation,
        quiescent,
    } = action
    {
        let result = comandos_cli::install::extension_worker::execute(home, journal, operation);
        *quiescent.borrow_mut() = true;
        return result;
    }
    Ok(())
}
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "comandos-install-extensions-{}-{}",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
        Self(path)
    }
    fn alias(&self) -> PathBuf {
        self.0.join(".local/bin/cc-extensions")
    }
    fn candidate(&self) -> PathBuf {
        let path = self.0.join("candidate");
        executable(&path, b"private standalone fixture; never executed");
        path
    }
    fn install(&self, args: &[&str]) -> bool {
        Command::new(env!("CARGO_BIN_EXE_comandos"))
            .args(["install", "--home", self.0.to_str().unwrap()])
            .args(args)
            .env_remove("COMANDOS_WEB_SOURCE")
            .output()
            .unwrap()
            .status
            .success()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn executable(path: &Path, bytes: &[u8]) {
    fs::write(path, bytes).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

#[test]
fn dedicated_extensions_survive_source_removal_relink_and_full_cli_upgrade() {
    let f = Fixture::new();
    let source = f.candidate();
    let expected = fs::read(&source).unwrap();
    assert!(f.install(&["--extensions-bin", source.to_str().unwrap()]));
    let target = fs::read_link(f.alias()).unwrap();
    assert!(target.starts_with(f.0.join(".local/share/comandos/components/comandos-extensions")));
    assert_eq!(target.file_name().unwrap(), "comandos-extensions");
    let manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(target.parent().unwrap().join("manifest.json")).unwrap())
            .unwrap();
    assert_eq!(manifest["artifact"], "comandos-extensions");
    assert_eq!(
        manifest["sha256"],
        target
            .parent()
            .unwrap()
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
    );
    fs::remove_file(source).unwrap();
    assert_eq!(fs::read(f.alias()).unwrap(), expected);
    // --link must retain a verified dedicated target even before a CLI stage.
    assert!(f.install(&["--link", "cc-extensions"]));
    assert_eq!(fs::read_link(f.alias()).unwrap(), target);

    let release = f.0.join("release");
    fs::create_dir(&release).unwrap();
    executable(
        &release.join("comandos"),
        b"private CLI upgrade; never executed",
    );
    let args = vec![
        "--home".into(),
        f.0.to_string_lossy().into_owned(),
        "--release".into(),
        release.to_string_lossy().into_owned(),
    ];
    assert_eq!(full::run_with(&args, &mut private_action).unwrap(), 0);
    assert_eq!(fs::read_link(f.alias()).unwrap(), target);
    assert_eq!(fs::read(f.alias()).unwrap(), expected);
}

#[test]
fn full_installer_uses_dedicated_payload_when_release_supplies_it() {
    let f = Fixture::new();
    let release = f.0.join("release");
    fs::create_dir(&release).unwrap();
    executable(
        &release.join("comandos"),
        b"private CLI fixture; never executed",
    );
    executable(
        &release.join("comandos-extensions"),
        b"private dedicated payload; never executed",
    );
    let args = vec![
        "--home".into(),
        f.0.to_string_lossy().into_owned(),
        "--release".into(),
        release.to_string_lossy().into_owned(),
    ];
    assert_eq!(full::run_with(&args, &mut private_action).unwrap(), 0);
    let target = fs::read_link(f.alias()).unwrap();
    assert!(target.starts_with(f.0.join(".local/share/comandos/components/comandos-extensions")));
    fs::remove_dir_all(release).unwrap();
    assert_eq!(
        fs::read(f.alias()).unwrap(),
        b"private dedicated payload; never executed"
    );
}

#[test]
fn tampered_dedicated_component_is_not_silently_replaced_by_multicall() {
    let f = Fixture::new();
    let source = f.candidate();
    assert!(f.install(&["--extensions-bin", source.to_str().unwrap()]));
    let target = fs::read_link(f.alias()).unwrap();
    executable(&target, b"modified component; never executed");
    assert!(!f.install(&["--link", "cc-extensions"]));
    assert_eq!(fs::read_link(f.alias()).unwrap(), target);
    let release = f.0.join("release");
    fs::create_dir(&release).unwrap();
    executable(
        &release.join("comandos"),
        b"private CLI fixture; never executed",
    );
    let args = vec![
        "--home".into(),
        f.0.to_string_lossy().into_owned(),
        "--release".into(),
        release.to_string_lossy().into_owned(),
    ];
    assert!(full::run_with(&args, &mut private_action).is_err());
    assert_eq!(fs::read_link(f.alias()).unwrap(), target);
}

#[test]
fn preview_is_read_only_and_original_alias_has_a_reversible_receipt() {
    let f = Fixture::new();
    let source = f.candidate();
    fs::create_dir_all(f.alias().parent().unwrap()).unwrap();
    let original = f.0.join("previous-cli");
    symlink(&original, f.alias()).unwrap();
    assert!(f.install(&["--extensions-bin", source.to_str().unwrap(), "--dry-run"]));
    assert_eq!(fs::read_link(f.alias()).unwrap(), original);
    assert!(!f.0.join(".local/share/comandos").exists());
    assert!(f.install(&["--extensions-bin", source.to_str().unwrap()]));
    assert!(f.install(&["--rollback", "cc-extensions"]));
    assert_eq!(fs::read_link(f.alias()).unwrap(), original);
}
