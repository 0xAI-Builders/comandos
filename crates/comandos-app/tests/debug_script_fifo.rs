#![allow(clippy::unwrap_used, clippy::expect_used, clippy::disallowed_methods)]
use comandos_app::{
    proc::{ProcSpec, run},
    ui::signals::debug_script,
};
use std::{
    os::unix::fs::{DirBuilderExt, MetadataExt},
    path::PathBuf,
    time::{Duration, Instant},
};
struct OwnDir(PathBuf);
impl Drop for OwnDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
#[test]
fn own_fifo_without_writer_is_rejected_without_blocking_open() {
    let root = std::env::temp_dir().join(format!("debug-fifo-{}", std::process::id()));
    std::fs::DirBuilder::new()
        .mode(0o700)
        .create(&root)
        .unwrap();
    let root = OwnDir(root);
    let path = root.0.join("comandos-dbg.js");
    nix::unistd::mkfifo(&path, nix::sys::stat::Mode::from_bits_truncate(0o600)).unwrap();
    let result = run(&ProcSpec {
        program: std::env::current_exe().unwrap().display().to_string(),
        args: vec![
            "--ignored".into(),
            "--exact".into(),
            "own_fifo_helper".into(),
            "--nocapture".into(),
        ],
        stdin: None,
        env: vec![(
            "COMANDOS_OWN_FIFO_PATH".into(),
            path.display().to_string().into(),
        )],
        clear_env: false,
        env_remove: vec![],
        cwd: Some(root.0.clone()),
        timeout: Duration::from_secs(1),
    });
    assert!(
        result.is_ok(),
        "fresh own helper timed out opening FIFO: {result:?}"
    );
    let result = result.unwrap();
    assert_eq!(
        result.code,
        Some(0),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(String::from_utf8_lossy(&result.stdout).contains("OWN_FIFO_REJECTED"));
}
#[test]
#[ignore = "Only called as a fresh owned subprocess with an explicit private FIFO path"]
fn own_fifo_helper() {
    let path = PathBuf::from(std::env::var("COMANDOS_OWN_FIFO_PATH").unwrap());
    let uid = path.parent().unwrap().metadata().unwrap().uid();
    let now = Instant::now();
    assert!(debug_script(&path, uid).is_err());
    assert!(now.elapsed() < Duration::from_millis(500));
    println!("OWN_FIFO_REJECTED");
}
