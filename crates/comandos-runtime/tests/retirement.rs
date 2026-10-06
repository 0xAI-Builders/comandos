#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]
use comandos_runtime::retirement::{self, CleanupCandidate, FindingKind, Manifest, Options};
use serde_json::json;
use std::{
    path::{Path, PathBuf},
    sync::atomic::{AtomicUsize, Ordering},
};
static NEXT: AtomicUsize = AtomicUsize::new(0);
struct Fixture {
    root: PathBuf,
    opts: Options,
}
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "retirement-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        for p in [
            "repo/lib",
            "repo/bin",
            "repo/crates/example/tests",
            "home/.local/bin",
            "home/.claude/hooks",
            "proc",
            "transient",
        ] {
            std::fs::create_dir_all(root.join(p)).unwrap();
        }
        std::fs::write(
            root.join("repo/crates/example/tests/replacement.rs"),
            "#[test]\nfn replacement_works() {}\n",
        )
        .unwrap();
        std::fs::write(root.join("repo/lib/target.py"), "def main(): pass\n").unwrap();
        let opts = Options {
            repo: root.join("repo"),
            repo_live: root.join("repo"),
            home: root.join("home"),
            proc: root.join("proc"),
            transient: root.join("transient"),
            crontab: String::new(),
            commit: "fixture".into(),
            tracked: Some(vec![
                "lib/target.py".into(),
                "crates/example/tests/replacement.rs".into(),
            ]),
        };
        Self { root, opts }
    }
    fn manifest(&self, status: &str) -> Manifest {
        Manifest::from_value(&json!({"version":1,"rows":[{"path":"lib/target.py","rust":"example replacement","phase":"fixture","verified_by":["example::replacement::replacement_works"],"verification":{"commit":"fixture","passed":true},"status":status}]})).unwrap()
    }
    fn check(&self, status: &str) -> retirement::Report {
        retirement::check(
            &self.opts,
            &self.manifest(status),
            &["lib/target.py".into()],
        )
        .unwrap()
    }
    fn write(&self, relative: &str, body: &[u8]) {
        let path = self.root.join(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, body).unwrap();
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.root).unwrap();
    }
}
#[test]
fn clean_when_nothing_uses_it_and_pending_is_never_cleanup_eligible() {
    let f = Fixture::new();
    assert!(f.check("pending").clean());
    let selected = retirement::cleanup_selection(
        &f.opts,
        &f.check("pending"),
        &[CleanupCandidate {
            artifact: "lib/target.py".into(),
            relative_path: PathBuf::from(".local/bin/old"),
        }],
    );
    assert!(selected.paths.is_empty());
    assert!(!selected.denied.is_empty());
}
#[test]
fn retire_check_flags_live_importer_process() {
    let mut f = Fixture::new();
    f.write("repo/bin/launcher", b"from middle import serve\n");
    f.write("repo/lib/middle.py", b"import target\n");
    f.opts
        .tracked
        .as_mut()
        .unwrap()
        .extend(["bin/launcher".into(), "lib/middle.py".into()]);
    f.write(
        "proc/42/cmdline",
        format!(
            "python3\0{}\0serve\0x\0",
            f.opts.repo.join("bin/launcher").display()
        )
        .as_bytes(),
    );
    let report = retirement::check(
        &f.opts,
        &f.manifest("pending"),
        &[
            "lib/target.py".into(),
            "lib/middle.py".into(),
            "bin/launcher".into(),
        ],
    )
    .unwrap();
    assert!(
        report.artifacts[0]
            .findings
            .iter()
            .any(|r| r.kind == FindingKind::LiveImporter)
    );
}
#[test]
fn flags_home_link_into_repo_and_cleanup_discounts_only_its_own_link() {
    let f = Fixture::new();
    let link = f.opts.home.join(".local/bin/old");
    std::os::unix::fs::symlink(f.opts.repo_live.join("lib/target.py"), &link).unwrap();
    std::fs::remove_file(f.opts.repo.join("lib/target.py")).unwrap();
    let report = f.check("retired");
    assert!(
        report.artifacts[0]
            .findings
            .iter()
            .any(|r| r.kind == FindingKind::HomeLink)
    );
    let selected = retirement::cleanup_selection(
        &f.opts,
        &report,
        &[CleanupCandidate {
            artifact: "lib/target.py".into(),
            relative_path: PathBuf::from(".local/bin/old"),
        }],
    );
    assert_eq!(selected.paths, vec![PathBuf::from(".local/bin/old")]);
    assert!(
        link.symlink_metadata().is_ok(),
        "selection must never mutate the source"
    );
}
#[test]
fn flags_transient_unit_tmux_and_agent_config_reference() {
    let f = Fixture::new();
    let path = f.opts.repo_live.join("lib/target.py").display().to_string();
    f.write(
        "transient/old.service",
        format!("[Service]\nExecStart=python3 {path}\n").as_bytes(),
    );
    f.write("home/.tmux.conf", b"run-shell lib/target.py\n");
    f.write(
        "home/.codex/config.toml",
        format!("notify = ['{path}']\n").as_bytes(),
    );
    let report = f.check("pending");
    for kind in [
        FindingKind::Unit,
        FindingKind::TmuxConf,
        FindingKind::AgentConfig,
    ] {
        assert!(
            report.artifacts[0].findings.iter().any(|r| r.kind == kind),
            "{kind:?}"
        );
    }
}
#[test]
fn missing_replacement_test_is_reported_and_unknown_process_scan_fails_closed() {
    let mut f = Fixture::new();
    std::fs::remove_file(f.opts.repo.join("crates/example/tests/replacement.rs")).unwrap();
    assert!(
        f.check("retired").artifacts[0]
            .findings
            .iter()
            .any(|r| r.kind == FindingKind::MissingReplacement)
    );
    f.opts.proc = f.root.join("missing-proc");
    assert!(retirement::check(&f.opts, &f.manifest("retired"), &["lib/target.py".into()]).is_err());
}
#[test]
fn cleanup_refuses_escape_sensitive_roots_and_stale_or_absent_verification() {
    let f = Fixture::new();
    let report = f.check("retired");
    for path in [
        "../outside",
        "/outside",
        ".claude/hooks/dash",
        ".claude/hooks/extensions-venv",
        ".claude/hooks/providers.env",
    ] {
        let selected = retirement::cleanup_selection(
            &f.opts,
            &report,
            &[CleanupCandidate {
                artifact: "lib/target.py".into(),
                relative_path: Path::new(path).into(),
            }],
        );
        assert!(selected.paths.is_empty(), "{path}");
    }
    let mut manifest = f.manifest("retired");
    manifest.rows[0].verification.as_mut().unwrap().commit = "old".into();
    let report = retirement::check(&f.opts, &manifest, &["lib/target.py".into()]).unwrap();
    assert!(!report.clean());
}

#[test]
fn relative_python_entrypoint_uses_injected_process_cwd_and_never_prints_secrets() {
    let mut f = Fixture::new();
    f.write("repo/bin/launcher", b"import target\n");
    f.opts.tracked.as_mut().unwrap().push("bin/launcher".into());
    f.write("proc/7/cmdline", b"python3\0bin/launcher\0serve\0");
    std::os::unix::fs::symlink(&f.opts.repo_live, f.opts.proc.join("7/cwd")).unwrap();
    f.write(
        "home/.claude/settings.json",
        format!(
            "{{\"secret\":\"PRIVATE-NEVER-OUTPUT\",\"command\":\"{}\"}}",
            f.opts.repo_live.join("lib/target.py").display()
        )
        .as_bytes(),
    );
    let report = retirement::check(
        &f.opts,
        &f.manifest("pending"),
        &["lib/target.py".into(), "bin/launcher".into()],
    )
    .unwrap();
    assert!(
        report.artifacts[0]
            .findings
            .iter()
            .any(|r| r.kind == FindingKind::LiveImporter && r.pid == Some(7))
    );
    assert!(!report.json().to_string().contains("PRIVATE-NEVER-OUTPUT"));
}
