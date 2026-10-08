//! Byte preservation and compatibility with the existing flock protocol.
use comandos_store::files::{FileLock, write_atomic};
use std::{fs, os::unix::fs::PermissionsExt, path::PathBuf};

struct Temp(PathBuf);
impl Temp {
    fn new(tag: &str) -> Self {
        let path =
            std::env::temp_dir().join(format!("comandos-store-files-{tag}-{}", std::process::id()));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn atomic_bytes_preserve_non_utf8_body_permissions_and_leave_no_temporary() {
    let dir = Temp::new("bytes");
    let path = dir.0.join("document");
    write_atomic(&path, b"\0\xff\xfe\n").unwrap();
    assert_eq!(fs::read(&path).unwrap(), b"\0\xff\xfe\n");
    assert_eq!(
        fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o600
    );
    fs::set_permissions(&path, fs::Permissions::from_mode(0o640)).unwrap();
    write_atomic(&path, b"second body").unwrap();
    assert_eq!(fs::read(&path).unwrap(), b"second body");
    assert_eq!(
        fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o640
    );
    assert_eq!(fs::read_dir(&dir.0).unwrap().count(), 1);
}

#[test]
fn domain_lock_uses_exact_existing_flock_file_without_second_suffix() {
    let dir = Temp::new("lock");
    let path = dir.0.join("app-tabs.json.lock");
    let original = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .unwrap();
    original.lock().unwrap();
    assert!(FileLock::try_exclusive(&path).unwrap().is_none());
    drop(original);
    let native = FileLock::exclusive(&path).unwrap();
    let second = fs::OpenOptions::new().write(true).open(&path).unwrap();
    assert!(matches!(
        second.try_lock(),
        Err(fs::TryLockError::WouldBlock)
    ));
    drop(native);
    second.try_lock().unwrap();
    assert!(!dir.0.join("app-tabs.json.lock.lock").exists());
}
