use comandos_cli::install::components;
use std::{
    fs,
    os::unix::fs::{PermissionsExt, symlink},
};
#[test]
fn native_components_survive_source_removal_and_reject_modified_installations() {
    let root = std::env::temp_dir().join(format!("component-{}", std::process::id()));
    fs::create_dir(&root).unwrap();
    fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
    let source = root.join("source");
    fs::write(&source, b"native executable bytes").unwrap();
    fs::set_permissions(&source, fs::Permissions::from_mode(0o755)).unwrap();
    let preview = components::stage(&root, "comandos-notifyd", &source, true).unwrap();
    assert!(!root.join(".local").exists());
    let installed = components::stage(&root, "comandos-notifyd", &source, false).unwrap();
    assert_eq!(installed, preview);
    assert_eq!(
        installed,
        components::stage(&root, "comandos-notifyd", &source, false).unwrap()
    );
    fs::remove_file(&source).unwrap();
    assert_eq!(fs::read(&installed).unwrap(), b"native executable bytes");
    fs::write(&source, b"native executable bytes").unwrap();
    fs::set_permissions(&source, fs::Permissions::from_mode(0o755)).unwrap();
    fs::write(&installed, b"corrupted").unwrap();
    assert!(components::stage(&root, "comandos-notifyd", &source, false).is_err());
    let link = root.join("symlink-source");
    symlink(&source, &link).unwrap();
    assert!(components::stage(&root, "comandos-notifyd", &link, false).is_err());
    assert!(components::stage(&root, "../../outside", &source, true).is_err());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn explicit_release_without_comandos_is_rejected_before_any_install_write() {
    let root = std::env::temp_dir().join(format!("missing-release-{}", std::process::id()));
    fs::create_dir(&root).unwrap();
    let home = root.join("home");
    fs::create_dir(&home).unwrap();
    let source = root.join("source");
    fs::create_dir(&source).unwrap();
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_comandos"))
        .args([
            "install",
            "--home",
            home.to_str().unwrap(),
            "--release",
            source.to_str().unwrap(),
            "--dry-run",
        ])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("missing a native executable"));
    assert_eq!(fs::read_dir(&home).unwrap().count(), 0);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn gateway_build_is_skipped_without_sources_or_with_current_executable() {
    use comandos_cli::install::proxy;
    let root = std::env::temp_dir().join(format!("proxy-build-{}", std::process::id()));
    fs::create_dir(&root).unwrap();
    let source = root.join("vendor");
    let bin = root.join("binary");
    assert!(!proxy::needs_build(&source, &bin).unwrap());
    fs::create_dir_all(source.join("src")).unwrap();
    fs::write(source.join("Cargo.toml"), b"source").unwrap();
    fs::write(source.join("src/main.rs"), b"source").unwrap();
    assert!(proxy::needs_build(&source, &bin).unwrap());
    fs::write(&bin, b"native binary").unwrap();
    fs::set_permissions(&bin, fs::Permissions::from_mode(0o755)).unwrap();
    assert!(!proxy::needs_build(&source, &bin).unwrap());
    std::thread::sleep(std::time::Duration::from_millis(5));
    fs::write(source.join("src/main.rs"), b"newer source").unwrap();
    assert!(proxy::needs_build(&source, &bin).unwrap());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn extension_timer_is_native_and_customized_units_are_preserved() {
    use comandos_cli::install::{extensions, plan, platform::Platform};
    let root = std::env::temp_dir().join(format!("extension-install-{}", std::process::id()));
    fs::create_dir(&root).unwrap();
    let actions = extensions::actions(&root, Platform::LinuxNative);
    let mut calls = 0;
    plan::apply_with(&actions, false, &mut |_| {
        calls += 1;
        Ok(())
    })
    .unwrap();
    assert_eq!(calls, 1);
    let path = root.join(".config/systemd/user/comandos-extensions-sync.service");
    assert!(
        fs::read_to_string(&path)
            .unwrap()
            .contains("bin/comandos ext sync")
    );
    plan::apply_with(&actions, false, &mut |_| panic!("repeated daemon reload")).unwrap();
    fs::write(&path, b"custom unit").unwrap();
    assert!(extensions::apply(&root, Platform::LinuxNative, true).is_err());
    assert_eq!(fs::read(&path).unwrap(), b"custom unit");
    assert!(extensions::actions(&root, Platform::Darwin).is_empty());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn wsl_systemd_edit_preserves_other_sections_and_native_dependencies_omit_python() {
    use comandos_cli::install::wsl;
    let before = "# retain\n[boot]\ncommand=custom\n\n[network]\ngenerateResolvConf=false\n";
    let after = wsl::systemd_config(before).unwrap();
    assert_eq!(
        after,
        "# retain\n[boot]\nsystemd=true\ncommand=custom\n\n[network]\ngenerateResolvConf=false\n"
    );
    assert_eq!(wsl::systemd_config(&after).unwrap(), after);
    assert_eq!(
        wsl::systemd_config("[boot]\n  systemd=false\n").unwrap(),
        "[boot]\n  systemd=true\n"
    );
    assert!(wsl::systemd_config("[boot]\n[boot]\n").is_err());
    assert!(wsl::systemd_config("[boot]\nsystemd=true\n[boot]\n").is_err());
    assert_eq!(
        wsl::systemd_config("[boot]\nsystemd = false # retain\n").unwrap(),
        "[boot]\nsystemd = true # retain\n"
    );
    let packages = wsl::packages("VERSION_CODENAME=noble\n");
    assert!(packages.contains(&"libwebkit2gtk-4.1-0"));
    assert!(!packages.iter().any(|p| p.contains("python")));
}
