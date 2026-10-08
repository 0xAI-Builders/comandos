use comandos_runtime::platform::{Host, runtime_path};
use std::path::Path;
#[test]
fn runtime_fallback_is_darwin_tmp_uid_and_linux_keeps_absent_xdg() {
    assert_eq!(
        runtime_path(Host::Macos, None, Some(Path::new("/own/tmp")), 42).unwrap(),
        Some("/own/tmp/comandos-42".into())
    );
    assert_eq!(
        runtime_path(
            Host::Macos,
            Some(Path::new("/own/xdg")),
            Some(Path::new("/own/tmp")),
            42
        )
        .unwrap(),
        Some("/own/xdg".into())
    );
    assert_eq!(
        runtime_path(Host::Linux, None, Some(Path::new("/own/tmp")), 42).unwrap(),
        None
    );
    assert!(runtime_path(Host::Macos, None, Some(Path::new("relative")), 42).is_err());
}

#[test]
fn private_runtime_directory_is_readonly_until_requested_and_releases_resources() {
    use comandos_runtime::platform::prepare_runtime_directory;
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
    let root = std::env::temp_dir().join(format!("m1-runtime-{}", std::process::id()));
    std::fs::DirBuilder::new()
        .mode(0o700)
        .create(&root)
        .unwrap();
    let path = root.join("comandos-own");
    prepare_runtime_directory(Host::Macos, Some(&path), false).unwrap();
    assert!(!path.exists());
    prepare_runtime_directory(Host::Macos, Some(&path), true).unwrap();
    assert_eq!(
        path.symlink_metadata().unwrap().permissions().mode() & 0o7777,
        0o700
    );
    prepare_runtime_directory(Host::Macos, Some(&path), false).unwrap();
    let alias = root.join("alias");
    std::os::unix::fs::symlink(&path, &alias).unwrap();
    assert!(prepare_runtime_directory(Host::Macos, Some(&alias), false).is_err());
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert!(prepare_runtime_directory(Host::Macos, Some(&path), true).is_err());
    let absent = root.join("linux-no-new-effects");
    prepare_runtime_directory(Host::Linux, Some(&absent), true).unwrap();
    assert!(!absent.exists());
    std::fs::remove_dir_all(&root).unwrap();
}
