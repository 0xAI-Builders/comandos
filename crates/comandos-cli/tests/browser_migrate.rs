use std::{path::PathBuf, process::Command};

fn tempdir() -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "cli-migrate-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn migrate_dry_run_and_apply_rewrite_json_without_leaking_secret() {
    let home = tempdir();
    let cfg = home.join(".claude.json");
    std::fs::write(
        &cfg,
        r#"{"secret":"do-not-print","mcpServers":{"chrome-bg":{"command":"old"},"other":{"env":{"TOKEN":"private"}}}}"#,
    )
    .unwrap();
    let plan = home.join("plan.json");
    let out = Command::new(env!("CARGO_BIN_EXE_comandos"))
        .args([
            "browser",
            "migrate-config",
            "dry-run",
            "--config",
            cfg.to_str().unwrap(),
            "--wrapper",
            "/client",
            "--plan",
            plan.to_str().unwrap(),
        ])
        .env("HOME", &home)
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(!String::from_utf8_lossy(&out.stdout).contains("do-not-print"));
    assert!(
        !std::fs::read_to_string(&plan)
            .unwrap()
            .contains("do-not-print")
    );
    let out = Command::new(env!("CARGO_BIN_EXE_comandos"))
        .args([
            "browser",
            "migrate-config",
            "apply",
            "--plan",
            plan.to_str().unwrap(),
            "--backup-dir",
            home.join("backups").to_str().unwrap(),
        ])
        .env("HOME", &home)
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let rewritten: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(cfg).unwrap()).unwrap();
    assert_eq!(rewritten["mcpServers"]["chrome-bg"]["command"], "/client");
    assert_eq!(rewritten["mcpServers"]["other"]["env"]["TOKEN"], "private");
}
