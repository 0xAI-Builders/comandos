#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]
use std::{
    fs,
    os::unix::fs::{DirBuilderExt, PermissionsExt, symlink},
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::{AtomicUsize, Ordering},
};
static NEXT: AtomicUsize = AtomicUsize::new(0);
struct Home(PathBuf);
impl Home {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!(
            "m5-home-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::DirBuilder::new().mode(0o700).create(&p).unwrap();
        Self(p)
    }
}
impl Drop for Home {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn run(home: &Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_comandos"))
        .args(["install", "--home", home.to_str().unwrap()])
        .args(args)
        .output()
        .unwrap()
}
fn plist(home: &Path) -> PathBuf {
    home.join("Library/LaunchAgents/com.0xai.cc-dash.plist")
}
fn orig(home: &Path) -> PathBuf {
    home.join(".local/share/comandos/rollback/com.0xai.cc-dash.plist.orig")
}
const ORIGINAL: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>Label</key><string>com.0xai.cc-dash</string>
  <key>ProgramArguments</key><array>
    <string>$BIN/cc-dash</string><string>--no-open</string>
  </array>
  <key>EnvironmentVariables</key><dict>
    <!-- launchd no hereda el PATH del shell: sin Homebrew aquí, cc-dash no
         encuentra tmux y /state (y toda acción tmux) muere con conexión vacía. -->
    <key>PATH</key><string>$BIN:/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin</string>
  </dict>
  <key>RunAtLoad</key><true/>
  <key>KeepAlive</key><true/>
</dict></plist>
"#;
#[test]
fn agent_preserves_original_heredoc_and_selects_native_terminal() {
    let script = include_str!("../../../install.sh");
    let heredoc = script
        .split("<<PLIST\n")
        .nth(1)
        .unwrap()
        .split("\nPLIST")
        .next()
        .unwrap();
    assert_eq!(heredoc, ORIGINAL.trim_end_matches('\n'));
    let home = Home::new();
    let r = run(&home.0, &["--darwin-agent", "--no-launchctl"]);
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    let expected = ORIGINAL
        .replace(
            "<string>$BIN/cc-dash</string>",
            &format!(
                "<string>{}/.local/share/comandos/bin/comandos</string><string>dash</string><string>--term</string><string>native</string>",
                home.0.display()
            ),
        )
        .replace("$BIN", &format!("{}/.local/bin", home.0.display()));
    assert_eq!(fs::read_to_string(plist(&home.0)).unwrap(), expected);
    assert_eq!(
        fs::metadata(plist(&home.0)).unwrap().permissions().mode() & 0o777,
        0o600
    );
    assert!(
        run(
            &home.0,
            &["--rollback", "com.0xai.cc-dash.plist", "--no-launchctl"]
        )
        .status
        .success()
    );
    assert!(!plist(&home.0).exists());
}
#[test]
fn failed_launch_load_restores_prior_bytes_and_keeps_first_backup() {
    let home = Home::new();
    fs::create_dir_all(plist(&home.0).parent().unwrap()).unwrap();
    fs::write(plist(&home.0), b"old agent").unwrap();
    let mut calls = Vec::new();
    let mut loads = 0;
    let mut fake = |action: &str, path: &Path| -> Result<(), String> {
        calls.push(action.to_string());
        if action == "load" {
            loads += 1;
            if loads == 1 {
                assert_ne!(fs::read(path).unwrap(), b"old agent");
                return Err("owned load failure".into());
            }
            assert_eq!(fs::read(path).unwrap(), b"old agent");
        }
        Ok(())
    };
    let error =
        comandos_cli::install::darwin::agent_with(&home.0, false, Some(&mut fake)).unwrap_err();
    assert!(error.contains("previous file restored"));
    assert_eq!(calls, ["unload", "load", "unload", "load"]);
    assert_eq!(fs::read(plist(&home.0)).unwrap(), b"old agent");
    assert_eq!(fs::read(orig(&home.0)).unwrap(), b"old agent");
    assert!(
        run(&home.0, &["--darwin-agent", "--no-launchctl"])
            .status
            .success()
    );
    assert_eq!(fs::read(orig(&home.0)).unwrap(), b"old agent");
}
#[test]
fn first_agent_rollback_unloads_before_removing_its_plist() {
    let home = Home::new();
    comandos_cli::install::darwin::agent_with(&home.0, false, None).unwrap();
    let installed = fs::read(plist(&home.0)).unwrap();
    let mut calls = Vec::new();
    let mut unloaded = false;
    let mut fake = |action: &str, path: &Path| -> Result<(), String> {
        calls.push(action.to_string());
        // launchctl unload needs the plist to determine the registered label.
        let bytes = fs::read(path).map_err(|e| e.to_string())?;
        assert_eq!(bytes, installed);
        unloaded = true;
        Ok(())
    };
    comandos_cli::install::darwin::rollback_agent_with(&home.0, false, Some(&mut fake)).unwrap();
    assert_eq!(calls, ["unload"]);
    assert!(
        unloaded,
        "registered service must be stopped before its plist is removed"
    );
    assert!(!plist(&home.0).exists());
}
#[test]
fn failed_agent_unload_preserves_the_current_plist_and_reports_failure() {
    let home = Home::new();
    comandos_cli::install::darwin::agent_with(&home.0, false, None).unwrap();
    let installed = fs::read(plist(&home.0)).unwrap();
    let mut fake = |action: &str, _: &Path| -> Result<(), String> {
        assert_eq!(action, "unload");
        Err("owned unload failure".into())
    };
    let error = comandos_cli::install::darwin::rollback_agent_with(&home.0, false, Some(&mut fake))
        .unwrap_err();
    assert!(error.contains("owned unload failure"));
    assert_eq!(fs::read(plist(&home.0)).unwrap(), installed);
}
#[test]
fn failed_agent_recovery_load_is_reported_with_the_restored_file() {
    let home = Home::new();
    fs::create_dir_all(plist(&home.0).parent().unwrap()).unwrap();
    fs::write(plist(&home.0), b"old agent").unwrap();
    let mut loads = 0;
    let mut fake = |action: &str, _: &Path| -> Result<(), String> {
        if action == "load" {
            loads += 1;
            return Err(if loads == 1 {
                "new load failure"
            } else {
                "old reload failure"
            }
            .into());
        }
        Ok(())
    };
    let error =
        comandos_cli::install::darwin::agent_with(&home.0, false, Some(&mut fake)).unwrap_err();
    assert!(error.contains("new load failure"));
    assert!(error.contains("old reload failure"));
    assert_eq!(fs::read(plist(&home.0)).unwrap(), b"old agent");
}
#[test]
fn concurrent_agent_installs_share_one_original_and_leave_complete_plist() {
    let home = Home::new();
    fs::create_dir_all(plist(&home.0).parent().unwrap()).unwrap();
    fs::write(plist(&home.0), b"first original").unwrap();
    let child = || {
        Command::new(env!("CARGO_BIN_EXE_comandos"))
            .args([
                "install",
                "--home",
                home.0.to_str().unwrap(),
                "--darwin-agent",
                "--no-launchctl",
            ])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap()
    };
    let mut a = child();
    let mut b = child();
    assert!(a.wait().unwrap().success());
    assert!(b.wait().unwrap().success());
    assert_eq!(fs::read(orig(&home.0)).unwrap(), b"first original");
    assert_eq!(
        fs::read_to_string(plist(&home.0)).unwrap(),
        comandos_cli::install::darwin::agent_plist(&home.0).unwrap()
    );
}
#[test]
fn first_app_install_rolls_back_absence_and_modified_app_keeps_previous() {
    let home = Home::new();
    let source = bundle(&home.0, "incoming.app", b"new");
    assert!(
        run(&home.0, &["--app", source.to_str().unwrap()])
            .status
            .success()
    );
    assert!(
        run(&home.0, &["--rollback", "ComandOS.app"])
            .status
            .success()
    );
    assert!(!home.0.join("Applications/ComandOS.app").exists());
    let old = bundle(&home.0.join("Applications"), "ComandOS.app", b"old");
    assert!(
        run(&home.0, &["--app", source.to_str().unwrap()])
            .status
            .success()
    );
    fs::write(old.join("Contents/MacOS/comandos-app-mac"), b"foreign").unwrap();
    assert!(
        !run(&home.0, &["--rollback", "ComandOS.app"])
            .status
            .success()
    );
    assert_eq!(
        fs::read(
            home.0
                .join("Applications/ComandOS.app.previous/Contents/MacOS/comandos-app-mac")
        )
        .unwrap(),
        b"old"
    );
    assert_eq!(
        fs::read(old.join("Contents/MacOS/comandos-app-mac")).unwrap(),
        b"foreign"
    );
}
#[test]
fn first_agent_backup_survives_reinstall_and_rollback_refuses_foreign_edits() {
    let home = Home::new();
    fs::create_dir_all(plist(&home.0).parent().unwrap()).unwrap();
    fs::write(plist(&home.0), b"original").unwrap();
    assert!(
        run(&home.0, &["--darwin-agent", "--no-launchctl"])
            .status
            .success()
    );
    assert!(
        run(&home.0, &["--darwin-agent", "--no-launchctl"])
            .status
            .success()
    );
    assert_eq!(fs::read(orig(&home.0)).unwrap(), b"original");
    assert_eq!(
        fs::metadata(orig(&home.0)).unwrap().permissions().mode() & 0o777,
        0o600
    );
    let installed = fs::read(plist(&home.0)).unwrap();
    fs::write(plist(&home.0), b"foreign").unwrap();
    assert!(
        !run(
            &home.0,
            &["--rollback", "com.0xai.cc-dash.plist", "--no-launchctl"]
        )
        .status
        .success()
    );
    assert_eq!(fs::read(plist(&home.0)).unwrap(), b"foreign");
    fs::write(plist(&home.0), installed).unwrap();
    assert!(
        run(
            &home.0,
            &["--rollback", "com.0xai.cc-dash.plist", "--no-launchctl"]
        )
        .status
        .success()
    );
    assert_eq!(fs::read(plist(&home.0)).unwrap(), b"original");
}
#[test]
fn agent_dry_run_and_linux_default_do_not_create_directories() {
    let home = Home::new();
    assert!(
        run(&home.0, &["--darwin-agent", "--dry-run"])
            .status
            .success()
    );
    assert_eq!(fs::read_dir(&home.0).unwrap().count(), 0);
    if !cfg!(target_os = "macos") {
        assert!(!run(&home.0, &["--darwin-agent"]).status.success());
        assert_eq!(fs::read_dir(&home.0).unwrap().count(), 0);
    }
    assert!(
        !run(&home.0, &["--stage", "--no-launchctl"])
            .status
            .success()
    );
    assert_eq!(fs::read_dir(&home.0).unwrap().count(), 0);
}
fn bundle(parent: &Path, name: &str, bytes: &[u8]) -> PathBuf {
    let p = parent.join(name);
    fs::create_dir_all(p.join("Contents/MacOS")).unwrap();
    fs::create_dir_all(p.join("Contents/Resources")).unwrap();
    fs::write(p.join("Contents/Info.plist"), b"owned plist").unwrap();
    fs::write(p.join("Contents/MacOS/comandos-app-mac"), bytes).unwrap();
    fs::set_permissions(
        p.join("Contents/MacOS/comandos-app-mac"),
        fs::Permissions::from_mode(0o755),
    )
    .unwrap();
    p
}
#[test]
fn app_backup_is_not_overwritten_and_rollback_restores_complete_original() {
    let home = Home::new();
    let original = bundle(&home.0.join("Applications"), "ComandOS.app", b"old");
    let source = bundle(&home.0, "incoming.app", b"new");
    let r = run(&home.0, &["--app", source.to_str().unwrap()]);
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    assert_eq!(
        fs::read(original.join("Contents/MacOS/comandos-app-mac")).unwrap(),
        b"new"
    );
    assert_eq!(
        fs::read(
            home.0
                .join("Applications/ComandOS.app.previous/Contents/MacOS/comandos-app-mac")
        )
        .unwrap(),
        b"old"
    );
    let backup = home
        .0
        .join("Applications/ComandOS.app.previous/Contents/MacOS/comandos-app-mac");
    use std::os::unix::fs::MetadataExt;
    let inode = backup.metadata().unwrap().ino();
    assert!(
        run(&home.0, &["--app", source.to_str().unwrap()])
            .status
            .success()
    );
    assert_eq!(backup.metadata().unwrap().ino(), inode);
    fs::write(
        source.join("Contents/MacOS/comandos-app-mac"),
        b"another release",
    )
    .unwrap();
    assert!(
        !run(&home.0, &["--app", source.to_str().unwrap()])
            .status
            .success()
    );
    assert_eq!(fs::read(&backup).unwrap(), b"old");
    assert!(
        run(&home.0, &["--rollback", "ComandOS.app"])
            .status
            .success()
    );
    assert_eq!(
        fs::read(original.join("Contents/MacOS/comandos-app-mac")).unwrap(),
        b"old"
    );
    assert!(!home.0.join("Applications/ComandOS.app.previous").exists());
}
#[test]
fn app_dry_run_missing_backup_and_symlink_conflicts_leave_existing_data() {
    let home = Home::new();
    let source = bundle(&home.0, "incoming.app", b"new");
    assert!(
        run(&home.0, &["--app", source.to_str().unwrap(), "--dry-run"])
            .status
            .success()
    );
    assert!(!home.0.join("Applications").exists());
    assert!(
        !run(&home.0, &["--rollback", "ComandOS.app"])
            .status
            .success()
    );
    let foreign = home.0.join("foreign");
    fs::create_dir(&foreign).unwrap();
    fs::create_dir(home.0.join("Applications")).unwrap();
    symlink(&foreign, home.0.join("Applications/ComandOS.app")).unwrap();
    assert!(
        !run(&home.0, &["--app", source.to_str().unwrap()])
            .status
            .success()
    );
    assert!(home.0.join("Applications/ComandOS.app").is_symlink());
    assert_eq!(fs::read_dir(&foreign).unwrap().count(), 0);
}

#[test]
fn app_replaces_and_restores_legacy_bundle_with_different_executable() {
    let home = Home::new();
    let legacy = bundle(&home.0.join("Applications"), "ComandOS.app", b"legacy");
    fs::rename(
        legacy.join("Contents/MacOS/comandos-app-mac"),
        legacy.join("Contents/MacOS/cc-app-mac"),
    )
    .unwrap();
    fs::write(legacy.join("Contents/Info.plist"), b"legacy plist").unwrap();
    let source = bundle(&home.0, "incoming.app", b"new");
    let install = run(&home.0, &["--app", source.to_str().unwrap()]);
    assert!(
        install.status.success(),
        "{}",
        String::from_utf8_lossy(&install.stderr)
    );
    let rollback = run(&home.0, &["--rollback", "ComandOS.app"]);
    assert!(
        rollback.status.success(),
        "{}",
        String::from_utf8_lossy(&rollback.stderr)
    );
    assert_eq!(
        fs::read(legacy.join("Contents/MacOS/cc-app-mac")).unwrap(),
        b"legacy"
    );
    assert_eq!(
        fs::read(legacy.join("Contents/Info.plist")).unwrap(),
        b"legacy plist"
    );
    assert!(!legacy.join("Contents/MacOS/comandos-app-mac").exists());
}
