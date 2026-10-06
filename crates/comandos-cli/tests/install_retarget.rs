use comandos_cli::install::retarget;
use serde_json::json;
use std::path::Path;

#[test]
fn retarget_replaces_owned_paths_and_removes_obsolete_interpreters() {
    let raw = "{\n  \"foreign\": \"leave me\",\n  \"notify\": [\"bash\", \"/old/repo/adapters/codex-notify.sh\", \"extra\"],\n  \"hook\": \"CC_AGENT=gemini bash /old/repo/adapters/gemini-hooks.sh --event Stop\"\n}\n";
    let rewritten =
        retarget::rewrite(raw, Path::new("/old/repo"), Path::new("/private/home")).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&rewritten).unwrap();
    assert_eq!(
        parsed["notify"],
        json!(["/private/home/.local/bin/codex-notify.sh", "extra"])
    );
    assert_eq!(
        parsed["hook"],
        "CC_AGENT=gemini /private/home/.local/bin/gemini-hooks.sh --event Stop"
    );
    assert!(rewritten.contains("  \"foreign\": \"leave me\",\n"));
    assert_eq!(
        retarget::rewrite(
            &rewritten,
            Path::new("/old/repo"),
            Path::new("/private/home")
        )
        .unwrap(),
        rewritten
    );
}

#[test]
fn retarget_preserves_jsonc_trivia_and_rejects_unknown_native_replacements() {
    let raw = "// keep comment\n{\"command\":\"/old/repo/hooks/cc-notify.sh\", \"manual\":42,}\n";
    let rewritten =
        retarget::rewrite(raw, Path::new("/old/repo"), Path::new("/private/home")).unwrap();
    assert_eq!(
        rewritten,
        "// keep comment\n{\"command\":\"/private/home/.claude/hooks/cc-notify.sh\", \"manual\":42,}\n"
    );
    assert!(
        retarget::rewrite(
            "{\"cmd\":\"/old/repo/bin/not-migrated\"}",
            Path::new("/old/repo"),
            Path::new("/private/home")
        )
        .is_err()
    );
}

#[test]
fn retarget_dry_run_and_backup_preserve_exact_config_bytes_and_permissions() {
    use std::{
        fs,
        os::unix::fs::{PermissionsExt, symlink},
    };
    let root = std::env::temp_dir().join(format!("retarget-home-{}", std::process::id()));
    fs::create_dir(&root).unwrap();
    fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
    fs::create_dir(root.join(".codex")).unwrap();
    let path = root.join(".codex/config.toml");
    let before=b"# retain comment\nnotify = [\"bash\", \"/old/repo/adapters/codex-notify.sh\", \"extra\"]\nforeign = true\n";
    fs::write(&path, before).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o640)).unwrap();
    let report = retarget::apply(&root, Path::new("/old/repo"), true).unwrap();
    assert_eq!(report.len(), 1);
    assert_eq!(fs::read(&path).unwrap(), before);
    assert!(!root.join(".local").exists());
    fs::create_dir_all(root.join(".local/bin")).unwrap();
    let target = root.join("native");
    fs::write(&target, b"private executable fixture").unwrap();
    fs::set_permissions(&target, fs::Permissions::from_mode(0o700)).unwrap();
    symlink(&target, root.join(".local/bin/codex-notify.sh")).unwrap();
    let report = retarget::apply(&root, Path::new("/old/repo"), false).unwrap();
    assert_eq!(report.len(), 1);
    assert_eq!(path.metadata().unwrap().permissions().mode() & 0o777, 0o640);
    assert!(
        fs::read_to_string(&path)
            .unwrap()
            .contains("# retain comment\n")
    );
    let backups = fs::read_dir(root.join(".codex"))
        .unwrap()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_name().to_string_lossy().contains("pre-comandos"))
        .collect::<Vec<_>>();
    assert_eq!(backups.len(), 1);
    assert_eq!(fs::read(backups[0].path()).unwrap(), before);
    assert!(
        retarget::apply(&root, Path::new("/old/repo"), false)
            .unwrap()
            .is_empty()
    );
    fs::remove_dir_all(&root).unwrap();
}
