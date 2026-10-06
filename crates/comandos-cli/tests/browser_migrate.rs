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

#[test]
fn migration_review_regressions_match_python311() {
    use comandos_cli::browser::migrate::{transform_json, transform_toml};
    let oracle = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../lib/browser_config_migration.py"
    );
    for (kind, text) in [
        (
            "json",
            r#"{"mcpServers":{"chrome-bg":{},"one":{"command":"a"},"chrome-current":{},"two":{"command":"b"},"three":{"command":"c"}}}"#,
        ),
        (
            "json",
            r#"{"mcpServers":{"chrome-bg":{}},"nested":{"x":1,"x":2}}"#,
        ),
        (
            "json",
            r#"{"enabledPlugins":{"chrome-devtools-mcp@chrome-devtools-plugins":true},"mcpServers":{"chrome-bg":{},"claude-in-chrome":{"command":"keep"}},"projects":{"/private":{"mcpServers":{"chrome-devtools":{}}}}}"#,
        ),
        (
            "toml",
            "memo = \"\"\"\n[mcp_servers.chrome-bg]\nkeep\n\"\"\"\n[ mcp_servers . 'chrome-devtools' ]\ncommand = 'old'\n[mcp_servers.other]\ncommand = 'keep'\n",
        ),
        (
            "toml",
            "[mcp_servers.chrome-bg]\ncommand = '/client'\nargs = []\n",
        ),
        (
            "toml",
            "mcp_servers = { 'chrome-bg' = {command = 'old'} }\n",
        ),
        ("toml", "broken = [\n"),
    ] {
        let output = Command::new("/usr/bin/python3.11").args(["-c", "import importlib.util,sys; s=importlib.util.spec_from_file_location('oracle',sys.argv[1]); m=importlib.util.module_from_spec(s); s.loader.exec_module(m); print(getattr(m,'transform_'+sys.argv[2])(sys.stdin.read(),'/client'),end='')", oracle, kind]).stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped()).spawn().unwrap();
        use std::io::Write;
        let mut child = output;
        child
            .stdin
            .take()
            .unwrap()
            .write_all(text.as_bytes())
            .unwrap();
        let out = child.wait_with_output().unwrap();
        let rust = if kind == "json" {
            transform_json(text, "/client")
        } else {
            transform_toml(text, "/client")
        };
        assert_eq!(rust.is_ok(), out.status.success(), "{kind}: {text}");
        if let Ok(rust) = rust {
            assert_eq!(rust.as_bytes(), out.stdout, "{kind}: {text}");
        }
    }
}

#[test]
fn same_path_plan_and_alias_preserve_original() {
    let home = tempdir();
    let cfg = home.join("config.json");
    let text = r#"{"mcpServers":{"chrome-bg":{"command":"old"}}}"#;
    std::fs::create_dir(home.join("branch")).unwrap();
    for plan in [
        cfg.clone(),
        home.join("branch/../config.json"),
        home.join("plan-link"),
    ] {
        std::fs::write(&cfg, text).unwrap();
        if plan.file_name().unwrap() == "plan-link" {
            std::os::unix::fs::symlink(&cfg, &plan).unwrap();
        }
        let out = Command::new(env!("CARGO_BIN_EXE_comandos"))
            .args(["browser", "migrate-config", "dry-run", "--config"])
            .arg(&cfg)
            .args(["--plan"])
            .arg(&plan)
            .env("HOME", &home)
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(1));
        assert_eq!(std::fs::read_to_string(&cfg).unwrap(), text);
    }
}

fn migration_command(home: &std::path::Path, rust: bool, args: &[&str]) -> std::process::Output {
    let mut cmd = if rust {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_comandos"));
        cmd.args(["browser", "migrate-config"]);
        cmd
    } else {
        let mut cmd = Command::new("/usr/bin/python3.11");
        cmd.arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../lib/browser_config_migration.py"
        ));
        cmd
    };
    cmd.args(args)
        .env("HOME", home)
        .env_remove("CLAUDE_CONFIG_DIR")
        .env_remove("CODEX_HOME")
        .env_remove("GROK_HOME")
        .output()
        .unwrap()
}
fn json_output(out: &std::process::Output) -> serde_json::Value {
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).unwrap()
}
#[test]
fn migration_plans_apply_bytes_backups_manifest_and_permissions_match_python311() {
    use std::os::unix::fs::PermissionsExt;
    let home = tempdir();
    let cfg = home.join("claude.json");
    let toml = home.join("config.toml");
    let originals = [
        r#"{"secret":"do-not-print","enabledPlugins":{"chrome-devtools-mcp@chrome-devtools-plugins":true,"claude-in-chrome@official":true},"mcpServers":{"chrome-bg":{"command":"old"},"chrome-devtools":{"command":"old2"},"claude-in-chrome":{"command":"personal"}},"projects":{"/project":{"mcpServers":{"chrome-current":{"command":"old"}}}}}"#,
        "# retained comment\napi_key = \"private\"\n[mcp_servers.\"chrome-bg\"]\ncommand=\"old\"\n[mcp_servers.\"chrome-bg\".env]\nTOKEN=\"old\"\n[mcp_servers.other]\ncommand = \"keep\" # keep formatting\n",
    ];
    let paths = [&cfg, &toml];
    for (path, text) in paths.iter().zip(originals) {
        std::fs::write(path, text).unwrap();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o1640)).unwrap();
    }
    let oracle_plan = home.join("oracle.plan");
    let rust_plan = home.join("rust.plan");
    let backup_dir = home.join("backups");
    let args = |plan: &std::path::Path| {
        vec![
            "dry-run".to_owned(),
            "--config".into(),
            cfg.display().to_string(),
            "--config".into(),
            toml.display().to_string(),
            "--wrapper".into(),
            "/client".into(),
            "--plan".into(),
            plan.display().to_string(),
        ]
    };
    let py_args = args(&oracle_plan);
    let rs_args = args(&rust_plan);
    let py = json_output(&migration_command(
        &home,
        false,
        &py_args.iter().map(String::as_str).collect::<Vec<_>>(),
    ));
    let rs = json_output(&migration_command(
        &home,
        true,
        &rs_args.iter().map(String::as_str).collect::<Vec<_>>(),
    ));
    assert_eq!(py["files"], rs["files"]);
    let py_plan: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&oracle_plan).unwrap()).unwrap();
    let rs_plan: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&rust_plan).unwrap()).unwrap();
    assert_eq!(py_plan, rs_plan);
    assert_eq!(
        std::fs::metadata(&rust_plan).unwrap().permissions().mode() & 0o777,
        0o600
    );
    let py = json_output(&migration_command(
        &home,
        false,
        &[
            "apply",
            "--plan",
            oracle_plan.to_str().unwrap(),
            "--backup-dir",
            backup_dir.to_str().unwrap(),
        ],
    ));
    let expected = paths
        .iter()
        .map(|path| std::fs::read(path).unwrap())
        .collect::<Vec<_>>();
    let py_manifest_path = std::path::Path::new(py["backups"][0].as_str().unwrap())
        .parent()
        .unwrap()
        .join("manifest.json");
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(py_manifest_path).unwrap()).unwrap();
    for file in manifest["files"].as_array_mut().unwrap() {
        file["backup"] = std::path::Path::new(file["backup"].as_str().unwrap())
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .into();
    }
    for (path, text) in paths.iter().zip(originals) {
        std::fs::write(path, text).unwrap();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o1640)).unwrap();
    }
    json_output(&migration_command(
        &home,
        true,
        &rs_args.iter().map(String::as_str).collect::<Vec<_>>(),
    ));
    let rs = json_output(&migration_command(
        &home,
        true,
        &[
            "apply",
            "--plan",
            rust_plan.to_str().unwrap(),
            "--backup-dir",
            backup_dir.to_str().unwrap(),
        ],
    ));
    assert_eq!(py["changed_files"], rs["changed_files"]);
    assert_eq!(
        paths
            .iter()
            .map(|path| std::fs::read(path).unwrap())
            .collect::<Vec<_>>(),
        expected
    );
    for (backup, text) in rs["backups"].as_array().unwrap().iter().zip(originals) {
        let backup = std::path::Path::new(backup.as_str().unwrap());
        assert_eq!(std::fs::read(backup).unwrap(), text.as_bytes());
        assert_eq!(
            std::fs::metadata(backup).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
    for path in paths {
        assert_eq!(
            std::fs::metadata(path).unwrap().permissions().mode() & 0o7777,
            0o1640
        );
    }
    let manifest_path = std::path::Path::new(rs["backups"][0].as_str().unwrap())
        .parent()
        .unwrap()
        .join("manifest.json");
    let mut actual: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&manifest_path).unwrap()).unwrap();
    for file in actual["files"].as_array_mut().unwrap() {
        file["backup"] = std::path::Path::new(file["backup"].as_str().unwrap())
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .into();
    }
    assert_eq!(actual, manifest);
    assert_eq!(
        std::fs::metadata(manifest_path)
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    assert_eq!(
        std::fs::metadata(&backup_dir).unwrap().permissions().mode() & 0o777,
        0o700
    );
    let unchanged = json_output(&migration_command(
        &home,
        true,
        &rs_args.iter().map(String::as_str).collect::<Vec<_>>(),
    ));
    assert!(
        unchanged["files"]
            .as_array()
            .unwrap()
            .iter()
            .all(|file| file["changed"] == false)
    );
    std::fs::remove_dir_all(home).unwrap();
}

#[test]
fn discovery_home_options_provider_variants_accounts_profiles_match_python311() {
    let home = tempdir();
    for path in [
        ".claude/settings.json",
        ".claude-work/accounts/a/claude.json",
        ".codex/accounts/work/config.toml",
        ".grok_personal/profiles/p/config.toml",
        "unrelated/config.toml",
    ] {
        let path = home.join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, b"").unwrap();
    }
    let py = migration_command(
        &home,
        false,
        &[
            "discover",
            "--home",
            home.to_str().unwrap(),
            "--no-process-env",
        ],
    );
    let rs = migration_command(
        &home,
        true,
        &[
            "discover",
            "--home",
            home.to_str().unwrap(),
            "--no-process-env",
        ],
    );
    assert_eq!(rs.status.code(), py.status.code());
    assert_eq!(rs.stdout, py.stdout);
    assert_eq!(rs.stderr, py.stderr);
    assert!(!String::from_utf8_lossy(&rs.stdout).contains("unrelated"));
    std::fs::remove_dir_all(home).unwrap();
}

#[test]
fn symlink_input_and_concurrent_config_change_are_rejected_without_mutation() {
    let home = tempdir();
    let cfg = home.join("config.json");
    let link = home.join("link.json");
    let plan = home.join("plan.json");
    let original = r#"{"mcpServers":{"chrome-bg":{}}}"#;
    std::fs::write(&cfg, original).unwrap();
    std::os::unix::fs::symlink(&cfg, &link).unwrap();
    for rust in [false, true] {
        let out = migration_command(
            &home,
            rust,
            &[
                "dry-run",
                "--config",
                link.to_str().unwrap(),
                "--wrapper",
                "/client",
                "--plan",
                plan.to_str().unwrap(),
            ],
        );
        assert_eq!(out.status.code(), Some(1));
        assert!(!plan.exists());
        assert_eq!(std::fs::read_to_string(&cfg).unwrap(), original);
    }
    json_output(&migration_command(
        &home,
        true,
        &[
            "dry-run",
            "--config",
            cfg.to_str().unwrap(),
            "--wrapper",
            "/client",
            "--plan",
            plan.to_str().unwrap(),
        ],
    ));
    std::fs::write(&cfg, b"{\"concurrent\":true}").unwrap();
    for rust in [false, true] {
        let out = migration_command(
            &home,
            rust,
            &[
                "apply",
                "--plan",
                plan.to_str().unwrap(),
                "--backup-dir",
                home.join("backups").to_str().unwrap(),
            ],
        );
        assert_eq!(out.status.code(), Some(1));
        assert_eq!(std::fs::read(&cfg).unwrap(), b"{\"concurrent\":true}");
        assert!(!home.join("backups").exists());
    }
    std::fs::remove_dir_all(home).unwrap();
}
