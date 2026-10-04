use std::{fs, os::unix::fs::symlink, process::Command};

#[test]
fn link_and_rollback_round_trip() {
    let home = std::env::temp_dir().join("comandos-install-test");
    let _ = fs::remove_dir_all(&home);
    fs::create_dir_all(home.join(".local/bin")).unwrap();
    fs::write(home.join("old-target"), "#!/bin/sh\necho old\n").unwrap();
    symlink(
        home.join("old-target"),
        home.join(".local/bin/cc-extensions"),
    )
    .unwrap();
    let bin = env!("CARGO_BIN_EXE_comandos");
    assert!(
        Command::new(bin)
            .args(["install", "--home", home.to_str().unwrap(), "--stage"])
            .status()
            .unwrap()
            .success()
    );
    assert!(home.join(".local/share/comandos/bin/comandos").exists());
    assert!(
        Command::new(bin)
            .args([
                "install",
                "--home",
                home.to_str().unwrap(),
                "--link",
                "cc-extensions"
            ])
            .status()
            .unwrap()
            .success()
    );
    assert_eq!(
        fs::read_link(home.join(".local/bin/cc-extensions")).unwrap(),
        home.join(".local/share/comandos/bin/comandos")
    );
    assert_eq!(
        fs::read_to_string(home.join(".local/share/comandos/rollback/cc-extensions.target"))
            .unwrap()
            .trim(),
        home.join("old-target").to_str().unwrap()
    );
    assert!(
        Command::new(bin)
            .args([
                "install",
                "--home",
                home.to_str().unwrap(),
                "--rollback",
                "cc-extensions"
            ])
            .status()
            .unwrap()
            .success()
    );
    assert_eq!(
        fs::read_link(home.join(".local/bin/cc-extensions")).unwrap(),
        home.join("old-target")
    );
}
