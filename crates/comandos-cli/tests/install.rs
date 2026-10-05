use std::{
    fs,
    os::unix::fs::{PermissionsExt, symlink},
    path::{Path, PathBuf},
    process::{Command, ExitStatus},
};

fn temp_home() -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let home =
        std::env::temp_dir().join(format!("comandos-install-{}-{nanos}", std::process::id()));
    fs::create_dir_all(home.join(".local/bin")).unwrap();
    home
}

fn install(home: &Path, args: &[&str]) -> ExitStatus {
    Command::new(env!("CARGO_BIN_EXE_comandos"))
        .args(["install", "--home", home.to_str().unwrap()])
        .args(args)
        // Herméticas: sin `web/` de ningún entorno.
        .env_remove("COMANDOS_WEB_SOURCE")
        .status()
        .unwrap()
}

fn staged(home: &Path) -> PathBuf {
    home.join(".local/share/comandos/bin/comandos")
}

fn record(home: &Path, name: &str) -> String {
    fs::read_to_string(home.join(format!(".local/share/comandos/rollback/{name}.target"))).unwrap()
}

#[test]
fn link_and_rollback_round_trip() {
    let home = temp_home();
    fs::write(home.join("old-target"), "#!/bin/sh\necho old\n").unwrap();
    symlink(
        home.join("old-target"),
        home.join(".local/bin/cc-extensions"),
    )
    .unwrap();
    assert!(install(&home, &["--stage"]).success());
    assert!(staged(&home).exists());
    assert!(install(&home, &["--link", "cc-extensions"]).success());
    assert_eq!(
        fs::read_link(home.join(".local/bin/cc-extensions")).unwrap(),
        staged(&home)
    );
    assert_eq!(
        record(&home, "cc-extensions").trim(),
        format!("LINK:{}", home.join("old-target").display())
    );
    assert!(install(&home, &["--rollback", "cc-extensions"]).success());
    assert_eq!(
        fs::read_link(home.join(".local/bin/cc-extensions")).unwrap(),
        home.join("old-target")
    );
    fs::remove_dir_all(&home).unwrap();
}

#[test]
fn staged_binary_is_executable() {
    let home = temp_home();
    assert!(install(&home, &["--stage"]).success());
    assert_ne!(
        fs::metadata(staged(&home)).unwrap().permissions().mode() & 0o111,
        0
    );
    fs::remove_dir_all(&home).unwrap();
}

#[test]
fn regular_file_is_backed_up_and_restored() {
    let home = temp_home();
    let hook = home.join(".claude/hooks/cc-notify.sh");
    fs::create_dir_all(hook.parent().unwrap()).unwrap();
    fs::write(&hook, b"#!/bin/sh\necho real\n").unwrap();
    fs::set_permissions(&hook, fs::Permissions::from_mode(0o750)).unwrap();
    assert!(install(&home, &["--stage"]).success());
    assert!(install(&home, &["--link", "cc-notify.sh"]).success());
    // Los hooks viven en ~/.claude/hooks y el original queda respaldado.
    assert_eq!(fs::read_link(&hook).unwrap(), staged(&home));
    let orig = home.join(".local/share/comandos/rollback/cc-notify.sh.orig");
    assert!(orig.exists());
    assert_eq!(
        record(&home, "cc-notify.sh").trim(),
        format!("FILE:{}", orig.display())
    );
    assert!(install(&home, &["--rollback", "cc-notify.sh"]).success());
    assert_eq!(fs::read(&hook).unwrap(), b"#!/bin/sh\necho real\n");
    assert_eq!(
        fs::metadata(&hook).unwrap().permissions().mode() & 0o777,
        0o750
    );
    fs::remove_dir_all(&home).unwrap();
}

#[test]
fn absent_path_rollback_removes_symlink() {
    let home = temp_home();
    assert!(install(&home, &["--stage"]).success());
    assert!(install(&home, &["--link", "cc-new"]).success());
    assert_eq!(record(&home, "cc-new").trim(), "ABSENT");
    assert!(install(&home, &["--rollback", "cc-new"]).success());
    assert!(fs::symlink_metadata(home.join(".local/bin/cc-new")).is_err());
    fs::remove_dir_all(&home).unwrap();
}

#[test]
fn second_link_keeps_original_record() {
    let home = temp_home();
    fs::write(home.join("old-target"), "x").unwrap();
    symlink(home.join("old-target"), home.join(".local/bin/cc-x")).unwrap();
    assert!(install(&home, &["--stage"]).success());
    assert!(install(&home, &["--link", "cc-x"]).success());
    let first = record(&home, "cc-x");
    assert!(install(&home, &["--link", "cc-x"]).success());
    assert_eq!(record(&home, "cc-x"), first);
    fs::remove_dir_all(&home).unwrap();
}

#[test]
fn invalid_name_is_rejected() {
    let home = temp_home();
    assert!(install(&home, &["--stage"]).success());
    assert!(!install(&home, &["--link", "a/b"]).success());
    assert!(!install(&home, &["--link", ".."]).success());
    assert!(!install(&home, &["--stage", "--link", "x"]).success());
    assert!(!home.join(".local/share/comandos/rollback/a").exists());
    fs::remove_dir_all(&home).unwrap();
}
