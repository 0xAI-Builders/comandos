//! Exercise the actual production arm method without a display or a live PTY.
#![allow(
    clippy::disallowed_methods,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing
)]
use std::{
    fs,
    path::PathBuf,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

struct Probe(PathBuf);
impl Drop for Probe {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn pty_reads_cannot_postpone_paint_until_a_cancellable_timer_fires() {
    let source = include_str!("../src/term/view.rs");
    let start = source.find("    fn arm(self: &Rc<Self>)").unwrap();
    let end = start
        + source[start..]
            .find("    fn tick(self: &Rc<Self>)")
            .unwrap();
    let probe = include_str!("support/term_arm.rs.txt")
        .replace("// ACTUAL_ARM", &source[start..end])
        .replace(
            "// ACTUAL_SCHEDULE",
            include_str!("../src/term/schedule.rs"),
        );
    let id = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("comandos-arm-{}-{id}", std::process::id()));
    fs::create_dir(&dir).unwrap();
    let _cleanup = Probe(dir.clone());
    let source = dir.join("probe.rs");
    let binary = dir.join("probe");
    fs::write(&source, probe).unwrap();
    let build = Command::new("rustc")
        .args(["--edition=2024", "-C", "opt-level=1", "-C", "strip=symbols"])
        .arg(&source)
        .arg("-o")
        .arg(&binary)
        .output()
        .unwrap();
    assert!(
        build.status.success(),
        "{}",
        String::from_utf8_lossy(&build.stderr)
    );
    let run = Command::new(&binary).output().unwrap();
    assert!(
        run.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    assert!(String::from_utf8_lossy(&run.stdout).contains("first=1 repeated=1 hidden=0"));
}
