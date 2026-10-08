use xtask::retire_check;
#[test]
fn cli_rejects_missing_paths_and_unknown_options_before_scanning() {
    assert!(retire_check::parse(&[]).is_err());
    assert!(retire_check::parse(&["--delete".into(), "lib/old.py".into()]).is_err());
    assert!(retire_check::parse(&["lib/old.py".into(), "--home".into()]).is_err());
}
#[test]
fn cli_keeps_injected_scan_roots_and_json_arguments() {
    let args = retire_check::parse(&[
        "bin/cc-old".into(),
        "--repo".into(),
        "/tmp/retire-repo".into(),
        "--home".into(),
        "/tmp/retire-home".into(),
        "--proc".into(),
        "/tmp/retire-proc".into(),
        "--repo-live".into(),
        "/tmp/live".into(),
        "--json".into(),
    ])
    .expect("valid private roots");
    assert_eq!(args.paths, vec!["bin/cc-old"]);
    assert_eq!(args.options.repo_live, std::path::Path::new("/tmp/live"));
    assert_eq!(args.options.proc, std::path::Path::new("/tmp/retire-proc"));
    assert_eq!(
        args.manifest,
        std::path::Path::new("/tmp/retire-repo/docs/verification/retirement.json")
    );
}

#[test]
fn cli_emits_bounded_json_and_blocks_live_process_without_mutating_sources() {
    use std::{fs, process::Command};
    let root = std::env::temp_dir().join(format!("retire-cli-{}", std::process::id()));
    for p in [
        "repo/lib",
        "repo/docs/verification",
        "home/.claude",
        "proc/42",
        "transient",
    ] {
        fs::create_dir_all(root.join(p)).unwrap();
    }
    let artifact = root.join("repo/lib/old.py");
    fs::write(&artifact, b"pass\n").unwrap();
    fs::write(root.join("repo/docs/verification/retirement.json"), br#"{"version":1,"rows":[{"path":"lib/old.py","rust":"fixture","phase":"fixture","verified_by":[],"status":"pending"}]}"#).unwrap();
    fs::write(
        root.join("home/.claude/settings.json"),
        format!(
            "{{\"command\":\"{}\",\"secret\":\"PRIVATE-CLI-SECRET\"}}",
            artifact.display()
        ),
    )
    .unwrap();
    fs::write(
        root.join("proc/42/cmdline"),
        format!("python3\0{}\0", artifact.display()),
    )
    .unwrap();
    fs::write(root.join("crontab"), b"").unwrap();
    for args in [
        vec!["init", "-q"],
        vec!["add", "."],
        vec![
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "commit",
            "-qm",
            "fixture",
        ],
    ] {
        assert!(
            Command::new("git")
                .args(args)
                .current_dir(root.join("repo"))
                .status()
                .unwrap()
                .success()
        );
    }
    let output = Command::new(env!("CARGO_BIN_EXE_xtask"))
        .arg("retire-check")
        .arg("lib/old.py")
        .arg("--repo")
        .arg(root.join("repo"))
        .arg("--repo-live")
        .arg(root.join("repo"))
        .arg("--home")
        .arg(root.join("home"))
        .arg("--proc")
        .arg(root.join("proc"))
        .arg("--transient")
        .arg(root.join("transient"))
        .arg("--crontab-file")
        .arg(root.join("crontab"))
        .arg("--json")
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(1),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["clean"], false);
    let findings = report["artifacts"][0]["findings"].as_array().unwrap();
    assert!(
        findings
            .iter()
            .any(|r| r["kind"] == "LiveProcess" && r["pid"] == 42)
    );
    assert!(!String::from_utf8_lossy(&output.stdout).contains("PRIVATE-CLI-SECRET"));
    assert_eq!(fs::read(&artifact).unwrap(), b"pass\n");
    fs::remove_dir_all(root).unwrap();
}
