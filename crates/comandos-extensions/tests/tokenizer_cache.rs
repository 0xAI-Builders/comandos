//! Offline cache entries are untrusted files; a FIFO must never hang the server.
use comandos_extensions::tokenizer::{ENCODING_FILE, offline_counts_at};
use std::{
    fs,
    path::PathBuf,
    process::Command,
    time::{Duration, Instant},
};

#[test]
fn offline_cache_child() {
    let Some(cache) = std::env::var_os("COMANDOS_TEST_TOKENIZER_CACHE") else {
        return;
    };
    assert!(offline_counts_at(&PathBuf::from(cache), &["hello".into()]).is_none());
}

fn check_in_child(cache: &std::path::Path) {
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "offline_cache_child", "--nocapture"])
        .env("COMANDOS_TEST_TOKENIZER_CACHE", cache)
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(status.success());
            return;
        }
        if Instant::now() >= deadline {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("offline cache reader blocked on {}", cache.display());
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn fifo_directory_oversized_and_bad_hash_fail_closed_without_blocking() {
    let root =
        std::env::temp_dir().join(format!("comandos-tokenizer-cache-{}", std::process::id()));
    fs::create_dir(&root).unwrap();
    struct Cleanup(PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    let _cleanup = Cleanup(root.clone());
    let file = root.join(ENCODING_FILE);
    nix::unistd::mkfifo(
        &file,
        nix::sys::stat::Mode::S_IRUSR | nix::sys::stat::Mode::S_IWUSR,
    )
    .unwrap();
    check_in_child(&root);
    fs::remove_file(&file).unwrap();
    fs::create_dir(&file).unwrap();
    check_in_child(&root);
    fs::remove_dir(&file).unwrap();
    fs::File::create(&file).unwrap().set_len(2_000_001).unwrap();
    check_in_child(&root);
    fs::write(&file, "not the verified encoding").unwrap();
    check_in_child(&root);
}
