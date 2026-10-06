use std::path::{Path, PathBuf};

struct FakeProc {
    root: PathBuf,
}

impl FakeProc {
    fn new() -> Self {
        let root =
            std::env::temp_dir().join(format!("cb-proc-{}-{}", std::process::id(), unique()));
        std::fs::create_dir_all(&root).unwrap();
        Self { root }
    }

    fn path(&self) -> &Path {
        &self.root
    }

    fn add(&self, pid: i32, ppid: i32, start: u64, comm: &str, argv: &[&str]) {
        let dir = self.root.join(pid.to_string());
        std::fs::create_dir_all(&dir).unwrap();
        let mut fields = vec!["S".to_owned(), ppid.to_string()];
        for index in 2..20 {
            fields.push(if index == 19 {
                start.to_string()
            } else {
                "0".to_owned()
            });
        }
        std::fs::write(
            dir.join("stat"),
            format!("{pid} ({comm}) {}", fields.join(" ")),
        )
        .unwrap();
        std::fs::write(dir.join("cmdline"), argv.join("\0")).unwrap();
    }
}

impl Drop for FakeProc {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn unique() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos()
}

#[test]
fn identity_parses_after_last_paren() {
    let root = FakeProc::new();
    root.add(42, 1, 9001, "comm with ) paren", &["node", "x"]);
    assert_eq!(
        comandos_browser::proc_scan::process_identity(root.path(), 42),
        Some((9001, 1))
    );
}

#[test]
fn owned_includes_profile_matches_and_descendants() {
    let root = FakeProc::new();
    let profile = std::path::Path::new("/s/session-a");
    root.add(10, 1, 100, "node", &["node", "mcp"]);
    root.add(
        20,
        1,
        200,
        "chrome",
        &["chrome", "--user-data-dir=/s/session-a"],
    );
    root.add(21, 20, 201, "chrome", &["chrome", "--type=renderer"]);
    root.add(
        30,
        1,
        300,
        "chrome",
        &["chrome", "--user-data-dir=/s/session-b"],
    );
    let owned = comandos_browser::proc_scan::owned_processes(root.path(), 10, profile);
    assert_eq!(owned.keys().copied().collect::<Vec<_>>(), vec![10, 20, 21]);
}
