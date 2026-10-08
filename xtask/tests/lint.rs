//! La auditoría debe medir archivos rastreados, sin aprobar código por accidente.
use std::{fs, path::PathBuf, process::Command, time::SystemTime};

struct Repo(PathBuf);
impl Repo {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!(
            "comandos-lint-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        assert!(
            Command::new("git")
                .args(["init", "-q"])
                .arg(&dir)
                .status()
                .unwrap()
                .success()
        );
        Self(dir)
    }
    fn file(&self, path: &str, body: &str, tracked: bool) {
        let target = self.0.join(path);
        fs::create_dir_all(target.parent().unwrap()).unwrap();
        fs::write(target, body).unwrap();
        if tracked {
            assert!(
                Command::new("git")
                    .current_dir(&self.0)
                    .args(["add", "--", path])
                    .status()
                    .unwrap()
                    .success()
            );
        }
    }
    fn run(&self, mode: &str) -> std::process::Output {
        Command::new(env!("CARGO_BIN_EXE_xtask"))
            .args(["lint", mode, "--root"])
            .arg(&self.0)
            .output()
            .unwrap()
    }
}
impl Drop for Repo {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn report_detects_tracked_languages_and_ignores_untracked_files() {
    let repo = Repo::new();
    for (path, body) in [
        ("bin/cc-next", "#!/usr/bin/env python3\n"),
        ("bin/cc-centro", "#!/usr/bin/env bash\n"),
        ("bin/cc-browser-remote", "#!/bin/sh\n"),
        ("dash/a.mjs", "export default 1;\n"),
        ("dash/a.tsx", "const x = 1;\n"),
        ("dash/a.html", "<html></html>"),
        ("dash/x.svg", "<svg><script>alert(1)</script></svg>"),
        ("tools/a.rb", "puts 1"),
    ] {
        repo.file(path, body, true);
    }
    repo.file("config/tmux.conf", "# tmux\n", true);
    repo.file("dash/y.svg", "<svg><path d=\"M0\"/></svg>", true);
    repo.file("crates/x/src/lib.rs", "fn f() {}\n", true);
    repo.file("scratch.py", "print(1)", false);
    let output = repo.run("--report");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["total"], 8);
    assert_eq!(report["by_lang"]["Python"], 1);
    assert_eq!(report["by_lang"]["Shell"], 2);
    assert_eq!(report["by_lang"]["JavaScript"], 2);
    assert_eq!(report["violations"].as_array().unwrap().len(), 8);
    assert_eq!(repo.run("--strict").status.code(), Some(1));
}

#[test]
fn allowlist_has_exact_paths_prefixes_and_line_limits() {
    let repo = Repo::new();
    repo.file("adapters/opencode-comandos.js", "a\n\nb\nc\n", true);
    repo.file("vendor/claude-codex/ci/build.sh", "#!/bin/sh\n", true);
    assert!(repo.run("--strict").status.success());
    repo.file("adapters/opencode-comandos.js", "a\nb\nc\nd\n", true);
    repo.file("vendor/claude-codex-evil/build.sh", "#!/bin/sh\n", true);
    let output = repo.run("--report");
    assert!(output.status.success());
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["total"], 2);
}

#[test]
fn scan_failure_never_reports_a_clean_repository() {
    let dir = std::env::temp_dir();
    let output = Command::new(env!("CARGO_BIN_EXE_xtask"))
        .args(["lint", "--strict", "--root"])
        .arg(dir)
        .output()
        .unwrap();
    assert!(!output.status.success());
}
