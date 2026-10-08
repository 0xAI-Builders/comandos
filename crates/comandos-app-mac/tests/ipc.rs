#![allow(clippy::unwrap_used)]
use comandos_app_mac::ipc::{Ipc, Kind};
use std::{fs, os::unix::fs::DirBuilderExt};
#[test]
fn seeds_existing_files_never_removes_and_ignores_invalid_changed_json() {
    let root = std::env::temp_dir().join(format!("m4-ipc-{}", std::process::id()));
    fs::DirBuilder::new().mode(0o700).create(&root).unwrap();
    let path = root.join("app-focus.json");
    fs::write(&path, b"{\"session\":\"preexisting\"}").unwrap();
    let mut ipc = Ipc::new(root.clone());
    assert!(ipc.poll().is_empty());
    std::thread::sleep(std::time::Duration::from_millis(3));
    fs::write(&path, b"{\"session\":\"own\"}").unwrap();
    let events = ipc.poll();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].0, Kind::Focus);
    assert_eq!(events[0].1["session"], "own");
    assert!(path.exists());
    assert!(ipc.poll().is_empty());
    fs::write(root.join("app-tab-close.json"), b"bad").unwrap();
    assert!(ipc.poll().is_empty());
    fs::remove_dir_all(root).unwrap();
}
#[test]
fn fifo_and_symlink_are_nonblocking_and_never_consumed() {
    let root = std::env::temp_dir().join(format!("m4-ipc-fifo-{}", std::process::id()));
    fs::DirBuilder::new().mode(0o700).create(&root).unwrap();
    let mut ipc = Ipc::new(root.clone());
    let fifo = root.join("app-tab-open.json");
    nix::unistd::mkfifo(
        &fifo,
        nix::sys::stat::Mode::S_IRUSR | nix::sys::stat::Mode::S_IWUSR,
    )
    .unwrap();
    std::os::unix::fs::symlink(&fifo, root.join("app-tab-close.json")).unwrap();
    let t = std::time::Instant::now();
    assert!(ipc.poll().is_empty());
    assert!(t.elapsed() < std::time::Duration::from_millis(100));
    assert!(fifo.exists());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn original_tab_open_filename_is_consumed_as_open() {
    let root = std::env::temp_dir().join(format!("m4-ipc-contract-{}", std::process::id()));
    fs::DirBuilder::new().mode(0o700).create(&root).unwrap();
    let mut ipc = Ipc::new(root.clone());
    fs::write(root.join("app-tab-open.json"), b"{\"session\":\"owned\"}").unwrap();
    let events = ipc.poll();
    fs::remove_dir_all(root).unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].0, Kind::Open);
}
