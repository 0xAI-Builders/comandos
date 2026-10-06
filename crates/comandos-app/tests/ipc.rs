#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]
mod support;

use comandos_app::{
    config::RunMode,
    ipc::{IpcConsumer, IpcKind, read_request},
};
use serde_json::json;
use std::time::SystemTime;
use support::tmux::TestTmux;

#[test]
fn request_kind_comes_from_strict_file_name() {
    let f = TestTmux::for_mode(RunMode::Sandbox).unwrap();
    f.guard.create_dir_all(f.config.hooks_dir(), 0o700).unwrap();
    let path = f.config.hooks_dir().join("app-tab-open.json");
    std::fs::write(&path, br#"{"session":"s1"}"#).unwrap();
    let request = read_request(&path).unwrap();
    assert_eq!(request.kind, IpcKind::TabOpen);
    assert_eq!(request.payload["session"], "s1");
    assert!(read_request(&f.config.hooks_dir().join("random.json")).is_err());
}

#[test]
fn shadow_does_not_consume_ipc() {
    let f = TestTmux::for_mode(RunMode::Shadow).unwrap();
    std::fs::create_dir_all(f.config.hooks_dir()).unwrap();
    let path = f.config.hooks_dir().join("app-command.json");
    let body = comandos_core::json::response_dumps(&json!({"command": "focus"})).unwrap();
    std::fs::write(&path, body.as_bytes()).unwrap();
    let before = std::fs::metadata(&path).unwrap();
    let modified = before.modified().unwrap_or(SystemTime::UNIX_EPOCH);
    let request = read_request(&path).unwrap();
    IpcConsumer::new(f.config.mode(), f.guard.clone())
        .consume(&request)
        .unwrap();
    let after = std::fs::metadata(&path).unwrap();
    assert_eq!(before.len(), after.len());
    assert_eq!(modified, after.modified().unwrap_or(SystemTime::UNIX_EPOCH));
    assert_eq!(std::fs::read_to_string(&path).unwrap(), body);
}

#[test]
fn sandbox_consumes_ipc_with_guard() {
    let f = TestTmux::for_mode(RunMode::Sandbox).unwrap();
    f.guard.create_dir_all(f.config.hooks_dir(), 0o700).unwrap();
    let path = f.config.hooks_dir().join("app-focus.json");
    std::fs::write(&path, br#"{"session":"local"}"#).unwrap();
    let request = read_request(&path).unwrap();
    IpcConsumer::new(f.config.mode(), f.guard.clone())
        .consume(&request)
        .unwrap();
    assert!(!path.exists());
}
