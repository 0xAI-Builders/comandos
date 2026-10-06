use std::{
    collections::BTreeMap,
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Command, Output},
};
fn tempdir() -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "ce-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&dir).unwrap();
    dir
}
fn command(home: &Path, state: &Path, rust: bool, args: &[&str]) -> Output {
    let mut cmd = if rust {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_comandos"));
        cmd.args(["browser", "expose"]);
        cmd
    } else {
        let mut cmd = Command::new("/usr/bin/python3.11");
        cmd.arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../bin/cc-browser-expose"
        ));
        cmd
    };
    cmd.args(args)
        .env("HOME", home)
        .env(
            "PATH",
            std::env::join_paths([
                home.join("bin"),
                PathBuf::from("/usr/bin"),
                PathBuf::from("/bin"),
            ])
            .unwrap(),
        )
        .env("CC_BROWSER_FORWARD_DIR", state)
        .env("FAKE_SSH_LOG", home.join("ssh.log"))
        .output()
        .unwrap()
}
fn snapshot(root: &Path) -> BTreeMap<String, (Vec<u8>, u32)> {
    if !root.exists() {
        return BTreeMap::new();
    }
    fs::read_dir(root)
        .unwrap()
        .map(|entry| {
            let path = entry.unwrap().path();
            (
                path.file_name().unwrap().to_string_lossy().into_owned(),
                (
                    fs::read(&path).unwrap(),
                    fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                ),
            )
        })
        .collect()
}
fn setup(state: &Path, files: &[(&str, &str)], mode: u32) {
    if state.exists() {
        fs::remove_dir_all(state).unwrap();
    }
    fs::create_dir(state).unwrap();
    fs::set_permissions(state, fs::Permissions::from_mode(mode)).unwrap();
    for (name, text) in files {
        let p = state.join(name);
        fs::write(&p, text).unwrap();
        fs::set_permissions(p, fs::Permissions::from_mode(0o600)).unwrap();
    }
}
#[test]
fn expose_differential_exit_text_state_and_ssh_argv() {
    let home = tempdir();
    fs::create_dir(home.join("bin")).unwrap();
    let out = Command::new("rustc")
        .args([
            "--edition=2024",
            concat!(env!("CARGO_MANIFEST_DIR"), "/tests/support/fake_ssh.rs"),
            "-o",
        ])
        .arg(home.join("bin/ssh"))
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let mapping =
        "{\"version\": 1, \"host\": \"macmini\", \"local_port\": 3000, \"remote_port\": 13000}\n";
    let mapping2 =
        "{\"version\": 1, \"host\": \"macmini\", \"local_port\": 3001, \"remote_port\": 13001}\n";
    type Scenario<'a> = (&'a [&'a str], Vec<(&'a str, &'a str)>, u32);
    let scenarios: Vec<Scenario<'_>> = vec![
        (&["start", "3000", "13000"], vec![], 0o700),
        (
            &["start", "3000", "13000"],
            vec![("13000.json", mapping), ("13000.sock", "")],
            0o700,
        ),
        (
            &["start", "3002", "13000"],
            vec![("13000.json", mapping)],
            0o700,
        ),
        (&["start", "3000", "13000"], vec![("13000.sock", "")], 0o700),
        (&["stop", "13000"], vec![], 0o700),
        (
            &["stop", "13000"],
            vec![("13000.json", mapping), ("13000.sock", "")],
            0o700,
        ),
        (&["status"], vec![], 0o700),
        (
            &["status"],
            vec![
                ("13000.json", mapping),
                ("13000.sock", ""),
                ("13001.json", mapping2),
            ],
            0o700,
        ),
        (&["start", "3000", "13000"], vec![], 0o755),
        (&["start", "80"], vec![], 0o700),
        (&["status", "extra"], vec![], 0o700),
        (&["start", "abc"], vec![], 0o700),
    ];
    let state = home.join("state");
    for (args, files, mode) in scenarios {
        setup(&state, &files, mode);
        let _ = fs::remove_file(home.join("ssh.log"));
        let python = command(&home, &state, false, args);
        let expected = snapshot(&state);
        let log = fs::read(home.join("ssh.log")).unwrap_or_default();
        setup(&state, &files, mode);
        let _ = fs::remove_file(home.join("ssh.log"));
        let rust = command(&home, &state, true, args);
        assert_eq!(rust.status.code(), python.status.code(), "{args:?}");
        assert_eq!(rust.stdout, python.stdout, "{args:?}");
        assert_eq!(rust.stderr, python.stderr, "{args:?}");
        assert_eq!(snapshot(&state), expected, "{args:?}");
        assert_eq!(
            fs::read(home.join("ssh.log")).unwrap_or_default(),
            log,
            "{args:?}"
        );
        assert_eq!(
            fs::metadata(&state).unwrap().permissions().mode() & 0o777,
            mode
        );
    }
    let long = home.join("x".repeat(90));
    setup(&long, &[], 0o700);
    let python = command(&home, &long, false, &["start", "3000"]);
    setup(&long, &[], 0o700);
    let rust = command(&home, &long, true, &["start", "3000"]);
    assert_eq!(
        (rust.status.code(), rust.stdout, rust.stderr),
        (python.status.code(), python.stdout, python.stderr)
    );
    let target = home.join("target");
    setup(&target, &[], 0o755);
    let link = home.join("link");
    std::os::unix::fs::symlink(&target, &link).unwrap();
    let python = command(&home, &link, false, &["start", "3000"]);
    let rust = command(&home, &link, true, &["start", "3000"]);
    assert_eq!(
        (rust.status.code(), rust.stdout, rust.stderr),
        (python.status.code(), python.stdout, python.stderr)
    );
    assert_eq!(
        fs::metadata(&target).unwrap().permissions().mode() & 0o777,
        0o755
    );
    fs::remove_dir_all(home).unwrap();
}

#[test]
fn ssh_output_is_drained_and_deadline_kills_and_reaps_the_fake_child() {
    use std::time::{Duration, Instant};
    let home = tempdir();
    fs::create_dir(home.join("bin")).unwrap();
    let compiled = Command::new("rustc")
        .args([
            "--edition=2024",
            concat!(env!("CARGO_MANIFEST_DIR"), "/tests/support/fake_ssh.rs"),
            "-o",
        ])
        .arg(home.join("bin/ssh"))
        .output()
        .unwrap();
    assert!(compiled.status.success());
    for rust in [false, true] {
        let state = home.join(if rust { "rust" } else { "python" });
        let mut cmd = if rust {
            let mut cmd = Command::new(env!("CARGO_BIN_EXE_comandos"));
            cmd.args(["browser", "expose"]);
            cmd
        } else {
            let mut cmd = Command::new("/usr/bin/python3.11");
            cmd.arg(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../bin/cc-browser-expose"
            ));
            cmd
        };
        let before = Instant::now();
        let output = cmd
            .args(["start", "3000", "13000"])
            .env("HOME", &home)
            .env(
                "PATH",
                std::env::join_paths([
                    home.join("bin"),
                    PathBuf::from("/usr/bin"),
                    PathBuf::from("/bin"),
                ])
                .unwrap(),
            )
            .env("CC_BROWSER_FORWARD_DIR", &state)
            .env("FAKE_SSH_LOG", home.join("ssh.log"))
            .env("FAKE_SSH_FLOOD", "1")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(before.elapsed() < Duration::from_secs(3));
        fs::remove_dir_all(state).unwrap();
    }
    let before = Instant::now();
    let runs = [false, true]
        .into_iter()
        .map(|rust| {
            let home = home.clone();
            std::thread::spawn(move || {
                let state = home.join(if rust { "rust" } else { "python" });
                let pid_file = home.join(if rust { "rust.pid" } else { "python.pid" });
                let mut cmd = if rust {
                    let mut cmd = Command::new(env!("CARGO_BIN_EXE_comandos"));
                    cmd.args(["browser", "expose"]);
                    cmd
                } else {
                    let mut cmd = Command::new("/usr/bin/python3.11");
                    cmd.arg(concat!(
                        env!("CARGO_MANIFEST_DIR"),
                        "/../../bin/cc-browser-expose"
                    ));
                    cmd
                };
                let output = cmd
                    .args(["start", "3000", "13000"])
                    .env("HOME", &home)
                    .env(
                        "PATH",
                        std::env::join_paths([
                            home.join("bin"),
                            PathBuf::from("/usr/bin"),
                            PathBuf::from("/bin"),
                        ])
                        .unwrap(),
                    )
                    .env("CC_BROWSER_FORWARD_DIR", &state)
                    .env("FAKE_SSH_LOG", home.join("ssh.log"))
                    .env("FAKE_SSH_STALL", "1")
                    .env("FAKE_SSH_PID_FILE", &pid_file)
                    .output()
                    .unwrap();
                assert_eq!(output.status.code(), Some(1));
                assert!(!state.join("13000.json").exists());
                let pid = fs::read_to_string(pid_file).unwrap();
                assert!(!std::path::Path::new(&format!("/proc/{pid}")).exists());
                output
            })
        })
        .collect::<Vec<_>>();
    let outputs = runs
        .into_iter()
        .map(|run| run.join().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(outputs[0].stdout, outputs[1].stdout);
    assert_eq!(outputs[0].stderr, outputs[1].stderr);
    assert!(before.elapsed() >= Duration::from_secs(15));
    assert!(before.elapsed() < Duration::from_secs(18));
    fs::remove_dir_all(home).unwrap();
}

#[test]
fn expose_help_matches_argparse_without_creating_state_or_invoking_ssh() {
    let home = tempdir();
    let state = home.join("absent-state");
    for args in [
        vec!["--help"],
        vec!["-h"],
        vec!["start", "--help"],
        vec!["start", "-h"],
        vec!["stop", "--help"],
        vec!["stop", "-h"],
        vec!["status", "--help"],
        vec!["status", "-h"],
    ] {
        let py = command(&home, &state, false, &args);
        let rs = command(&home, &state, true, &args);
        assert_eq!(
            (rs.status.code(), rs.stdout, rs.stderr),
            (py.status.code(), py.stdout, py.stderr),
            "{args:?}"
        );
        assert!(!state.exists());
        assert!(!home.join("ssh.log").exists());
    }
    fs::remove_dir_all(home).unwrap();
}
