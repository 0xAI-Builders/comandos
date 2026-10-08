#![allow(clippy::unwrap_used)]
use std::io;
#[test]
fn apple_checks_cover_both_consumers_on_each_target() {
    let mut calls = Vec::new();
    xtask::mac::check_with(|args| {
        calls.push(args.iter().map(|s| s.to_string()).collect::<Vec<_>>());
        Ok(0)
    })
    .unwrap();
    assert_eq!(
        calls,
        vec![
            vec![
                "check",
                "--offline",
                "-j2",
                "-p",
                "comandos-app-mac",
                "-p",
                "comandos-cli",
                "--target",
                "aarch64-apple-darwin"
            ],
            vec![
                "check",
                "--offline",
                "-j2",
                "-p",
                "comandos-app-mac",
                "-p",
                "comandos-cli",
                "--target",
                "x86_64-apple-darwin"
            ],
        ]
    );
}
#[test]
fn failed_apple_check_stops_before_next_target_and_preserves_code() {
    let mut calls = 0;
    assert_eq!(
        xtask::mac::check_with(|_| {
            calls += 1;
            Ok(17)
        })
        .unwrap(),
        17
    );
    assert_eq!(calls, 1);
    let err = xtask::mac::check_with(|_| Err(io::Error::other("own missing cargo"))).unwrap_err();
    assert_eq!(err.to_string(), "own missing cargo");
}

#[test]
fn command_entry_executes_owned_cargo_at_workspace_and_rejects_extra_arguments() {
    use std::{
        fs,
        os::unix::fs::{DirBuilderExt, PermissionsExt},
        process::Command,
    };
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    struct Dir(std::path::PathBuf);
    impl Drop for Dir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    let root = Dir(std::env::temp_dir().join(format!(
        "m2-mac-check-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    )));
    fs::DirBuilder::new().mode(0o700).create(&root.0).unwrap();
    let cargo = root.0.join("cargo");
    let log = root.0.join("calls");
    fs::write(
        &cargo,
        "#!/bin/sh\npwd >> \"$M2_CALL_LOG\"\nprintf '%s\\n' \"$*\" >> \"$M2_CALL_LOG\"\nexit 0\n",
    )
    .unwrap();
    fs::set_permissions(&cargo, fs::Permissions::from_mode(0o700)).unwrap();
    let invoke = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_xtask"))
            .args(args)
            .env("CARGO", &cargo)
            .env("M2_CALL_LOG", &log)
            .output()
            .unwrap()
    };
    assert!(invoke(&["mac-check"]).status.success());
    let lines = fs::read_to_string(&log).unwrap();
    let workspace = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .canonicalize()
        .unwrap();
    assert_eq!(
        lines
            .lines()
            .filter(|s| *s == workspace.to_str().unwrap())
            .count(),
        2
    );
    assert_eq!(
        lines
            .lines()
            .filter(|s| s
                .starts_with("check --offline -j2 -p comandos-app-mac -p comandos-cli --target "))
            .count(),
        2
    );
    fs::remove_file(&log).unwrap();
    assert_eq!(invoke(&["mac-check", "unexpected"]).status.code(), Some(2));
    assert!(!log.exists());
}
