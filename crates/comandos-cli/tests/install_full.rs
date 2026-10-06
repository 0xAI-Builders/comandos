use comandos_cli::install::{hooks_register, platform::Platform};
use serde_json::json;
use std::{
    fs,
    os::unix::fs::{PermissionsExt, symlink},
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};
static SEQ: AtomicU64 = AtomicU64::new(0);

#[test]
fn installer_resources_and_aliases_match_frozen_legacy_install_tree() {
    use sha2::{Digest, Sha256};
    let baseline: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/install-legacy-linux.json")).unwrap();
    let home = Home::new();
    let release = home.0.join("release");
    fs::create_dir(&release).unwrap();
    for artifact in ["comandos-app", "comandos-notifyd", "cc-model-proxy"] {
        fs::write(release.join(artifact), b"private native fixture").unwrap();
        fs::set_permissions(release.join(artifact), fs::Permissions::from_mode(0o755)).unwrap();
    }
    let app =
        comandos_cli::install::release::stage_app(&home.0, &release.join("comandos-app")).unwrap();
    fs::create_dir_all(home.0.join(".local/share/comandos/bin")).unwrap();
    fs::write(home.0.join(".local/share/comandos/bin/comandos"), b"native").unwrap();
    let actions = comandos_cli::install::plan::plan(&home.0, Platform::LinuxNative, &release);
    comandos_cli::install::plan::apply_with(&actions, false, &mut |_| Ok(())).unwrap();
    let mut resources = 0;
    let mut aliases = 0;
    let mut frontend = 0;
    let mut remaining = Vec::new();
    for (relative, old) in baseline["entries"].as_object().unwrap() {
        // The separately built native frontend still needs standalone packaging.
        // It is a declared boundary, not a passed parity assertion.
        if relative.starts_with(".claude/hooks/dash") {
            frontend += 1;
            continue;
        }
        if matches!(
            relative.as_str(),
            ".local/bin/cc-app-mac" | ".local/bin/cc_usage.py"
        ) {
            remaining.push(relative.as_str());
            continue;
        }
        let installed = home.0.join(relative);
        if old["kind"] == "dir" {
            assert!(installed.is_dir(), "missing directory {relative}");
        } else if relative.starts_with(".local/bin/")
            || matches!(
                relative.as_str(),
                ".claude/hooks/cc-notify.sh"
                    | ".claude/hooks/cc-status.sh"
                    | ".claude/hooks/cc-usage-tool.sh"
            )
        {
            let name = installed.file_name().unwrap().to_str().unwrap();
            let target = match name {
                "cc-app" => app.path.clone(),
                "cc-notifyd" => release.join("comandos-notifyd"),
                _ => home.0.join(".local/share/comandos/bin/comandos"),
            };
            assert_eq!(
                fs::read_link(&installed).unwrap(),
                target,
                "alias {relative}"
            );
            aliases += 1;
        } else {
            assert!(!installed.is_symlink(), "embedded resource {relative}");
            let bytes = fs::read(&installed).unwrap();
            let normalized = String::from_utf8(bytes.clone())
                .ok()
                .map(|text| text.replace(&home.0.to_string_lossy().to_string(), "{{HOME}}"));
            if relative == ".claude/settings.json" {
                let actual: serde_json::Value =
                    serde_json::from_str(normalized.as_ref().unwrap()).unwrap();
                assert_eq!(actual, old["json"], "Claude hook registration");
            } else {
                let payload = normalized.as_deref().map(str::as_bytes).unwrap_or(&bytes);
                let actual = format!("{:x}", Sha256::digest(payload));
                let expected = old.get("native_sha256").unwrap_or(&old["sha256"]);
                assert_eq!(actual, expected.as_str().unwrap(), "resource {relative}");
            }
            resources += 1;
        }
    }
    assert_eq!(resources, 15);
    assert_eq!(aliases, 27);
    assert_eq!(frontend, 33);
    assert_eq!(remaining.len(), 2);
}
struct Home(PathBuf);
impl Home {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!(
            "install-full-{}-{}",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&p).unwrap();
        fs::set_permissions(&p, fs::Permissions::from_mode(0o700)).unwrap();
        Self(p)
    }
}
impl Drop for Home {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn dry_run_writes_nothing_and_invokes_no_tools() {
    let home = Home::new();
    let actions =
        comandos_cli::install::plan::plan(&home.0, Platform::LinuxNative, &home.0.join("release"));
    let report = comandos_cli::install::plan::apply_with(&actions, true, &mut |_| {
        panic!("preview ran a tool")
    })
    .unwrap();
    assert!(!report.is_empty());
    assert_eq!(fs::read_dir(&home.0).unwrap().count(), 0);
    assert!(report.iter().all(|line| line.len() < 1024));
}

#[test]
fn full_plan_is_idempotent_and_preserves_existing_tmux_and_custom_units() {
    let home = Home::new();
    fs::create_dir_all(home.0.join(".local/share/comandos/bin")).unwrap();
    fs::write(
        home.0.join(".local/share/comandos/bin/comandos"),
        b"installed native fixture",
    )
    .unwrap();
    fs::create_dir_all(home.0.join(".config/systemd/user")).unwrap();
    fs::write(home.0.join("custom-tmux"), b"never reload or rewrite").unwrap();
    symlink(
        home.0.join("custom-tmux"),
        home.0.join(".config/systemd/user/tmux.service"),
    )
    .unwrap();
    fs::write(
        home.0.join(".config/systemd/user/cc-dash.service"),
        b"custom dashboard unit",
    )
    .unwrap();
    let actions =
        comandos_cli::install::plan::plan(&home.0, Platform::LinuxNative, &home.0.join("release"));
    let mut tools = Vec::new();
    comandos_cli::install::plan::apply_with(&actions, false, &mut |action| {
        tools.push(format!("{action:?}"));
        Ok(())
    })
    .unwrap();
    assert_eq!(
        fs::read_link(home.0.join(".config/systemd/user/tmux.service")).unwrap(),
        home.0.join("custom-tmux")
    );
    assert_eq!(
        fs::read(home.0.join("custom-tmux")).unwrap(),
        b"never reload or rewrite"
    );
    assert_eq!(
        fs::read(home.0.join(".config/systemd/user/cc-dash.service")).unwrap(),
        b"custom dashboard unit"
    );
    assert!(
        !tools
            .iter()
            .any(|line| line.contains("restart") || line.contains("start-server"))
    );
    let report = comandos_cli::install::plan::apply_with(&actions, false, &mut |_| Ok(())).unwrap();
    assert!(!report.iter().any(|line| line.starts_with("write ")
        || line.starts_with("link ")
        || line.starts_with("unit ")));
    assert_eq!(
        fs::read_link(home.0.join(".claude/hooks/cc-notify.sh")).unwrap(),
        home.0.join(".local/share/comandos/bin/comandos")
    );
}

#[test]
fn cleanup_and_restore_preserve_bytes_links_modes_and_inodes() {
    use std::os::unix::fs::MetadataExt;
    let home = Home::new();
    fs::create_dir_all(home.0.join(".claude/hooks/old-dir")).unwrap();
    let file = home.0.join(".claude/hooks/old-dir/notify.sh");
    fs::write(&file, b"original bytes\0\xff").unwrap();
    fs::set_permissions(&file, fs::Permissions::from_mode(0o751)).unwrap();
    let inode = file.metadata().unwrap().ino();
    symlink("unrelated target", home.0.join(".claude/hooks/old-link")).unwrap();
    let paths = [
        PathBuf::from(".claude/hooks/old-dir"),
        PathBuf::from(".claude/hooks/old-link"),
    ];
    assert!(
        comandos_cli::install::cleanup::stage(&home.0, &paths, true)
            .unwrap()
            .is_none()
    );
    assert!(!home.0.join(".local").exists());
    let manifest = comandos_cli::install::cleanup::stage(&home.0, &paths, false)
        .unwrap()
        .unwrap();
    assert!(!file.exists());
    comandos_cli::install::cleanup::restore(&home.0, &manifest, true).unwrap();
    assert!(!file.exists());
    comandos_cli::install::cleanup::restore(&home.0, &manifest, false).unwrap();
    assert_eq!(fs::read(&file).unwrap(), b"original bytes\0\xff");
    assert_eq!(file.metadata().unwrap().ino(), inode);
    assert_eq!(file.metadata().unwrap().mode() & 0o777, 0o751);
    assert_eq!(
        fs::read_link(home.0.join(".claude/hooks/old-link")).unwrap(),
        PathBuf::from("unrelated target")
    );
}

#[test]
fn cleanup_restore_refuses_new_user_files_and_manifest_traversal() {
    let home = Home::new();
    fs::write(home.0.join("notify.sh"), b"original").unwrap();
    let manifest =
        comandos_cli::install::cleanup::stage(&home.0, &[PathBuf::from("notify.sh")], false)
            .unwrap()
            .unwrap();
    fs::write(home.0.join("notify.sh"), b"new user file").unwrap();
    assert!(comandos_cli::install::cleanup::restore(&home.0, &manifest, false).is_err());
    assert_eq!(
        fs::read(home.0.join("notify.sh")).unwrap(),
        b"new user file"
    );
    assert!(
        comandos_cli::install::cleanup::stage(&home.0, &[PathBuf::from("../outside")], false)
            .is_err()
    );
}

#[test]
fn platform_detection_matches_linux_wsl_and_darwin() {
    assert_eq!(Platform::detect("Darwin", "", ""), Platform::Darwin);
    assert_eq!(
        Platform::detect("Linux", "6.8-microsoft-WSL2", "ID=\"ubuntu\"\n"),
        Platform::WslUbuntu
    );
    assert_eq!(
        Platform::detect("Linux", "6.8-Microsoft", "ID=debian\n"),
        Platform::LinuxOther
    );
    assert_eq!(
        Platform::detect("Linux", "6.8", "ID=fedora\n"),
        Platform::LinuxNative
    );
}

#[test]
fn claude_hooks_preserve_foreign_entries_and_explicit_timeouts() {
    let input = json!({"theme":"dark","hooks":{
        "Stop":[{"matcher":"*","hooks":[{"type":"command","command":"~/hooks/notify","timeout":77},{"type":"command","command":"foreign"}]}],
        "PostToolUseFailure":[{"hooks":[{"type":"command","command":"/private/hooks/tool"},{"type":"command","command":"foreign"}]}]
    }});
    let output = hooks_register::configure(
        &input,
        "/private",
        "/private/hooks/notify",
        "/private/hooks/tool",
    )
    .unwrap();
    assert_eq!(output["theme"], "dark");
    assert_eq!(output["hooks"]["Stop"][0], input["hooks"]["Stop"][0]);
    assert_eq!(
        output["hooks"]["PostToolUseFailure"][0]["hooks"],
        json!([{"type":"command","command":"foreign"}])
    );
    assert_eq!(output["hooks"]["PreToolUse"][0]["hooks"][0]["timeout"], 5);
    assert_eq!(output["hooks"]["SessionEnd"][0]["hooks"][0]["timeout"], 15);
    assert_eq!(
        hooks_register::configure(
            &output,
            "/private",
            "/private/hooks/notify",
            "/private/hooks/tool"
        )
        .unwrap(),
        output
    );
}

#[test]
fn invalid_hook_shapes_fail_without_changing_input() {
    for input in [
        json!([]),
        json!({"hooks":3}),
        json!({"hooks":{"Stop":"custom"}}),
    ] {
        let before = input.clone();
        assert!(hooks_register::configure(&input, "/private", "notify", "tool").is_err());
        assert_eq!(input, before);
    }
}
