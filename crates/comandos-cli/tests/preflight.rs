//! Procesos y releases simulados; nunca consulta /proc real.
use comandos_cli::{
    install::manifest::{STATE_PROTOCOL, release_protocol},
    state::preflight::{can_unify, domain_writers},
};
use std::{fs, os::unix::fs::symlink, path::PathBuf};
struct Fixture {
    root: PathBuf,
    home: PathBuf,
    repo: PathBuf,
    proc: PathBuf,
}
impl Fixture {
    fn new(tag: &str) -> Self {
        let root = std::env::temp_dir().join(format!("cmd-preflight-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let home = root.join("home");
        let repo = root.join("repo");
        let proc = root.join("proc");
        for d in [&home, &repo, &proc] {
            fs::create_dir_all(d).unwrap();
        }
        Self {
            root,
            home,
            repo,
            proc,
        }
    }
    fn release(&self, id: &str, protocol: Option<u32>) -> PathBuf {
        let r = self.home.join(".local/share/comandos/releases").join(id);
        fs::create_dir_all(&r).unwrap();
        fs::write(r.join("comandos"), b"private").unwrap();
        if let Some(p) = protocol {
            fs::write(
                r.join("manifest.json"),
                format!("{{\"state_protocol\":{p}}}"),
            )
            .unwrap();
        }
        r
    }
    fn process(&self, pid: u32, exe: &std::path::Path, argv: &[&str]) {
        let d = self.proc.join(pid.to_string());
        fs::create_dir_all(&d).unwrap();
        symlink(exe, d.join("exe")).unwrap();
        symlink(&self.repo, d.join("cwd")).unwrap();
        fs::write(d.join("cmdline"), format!("{}\0", argv.join("\0"))).unwrap();
    }
    fn check(&self, domain: &str) -> Result<(), Vec<String>> {
        can_unify(&domain_writers(&self.proc, &self.home, &self.repo, domain))
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}
#[test]
fn old_release_blocks_only_its_domains() {
    let f = Fixture::new("old");
    let r = f.release("old", None);
    f.process(1234, &r.join("comandos"), &["comandos", "dash"]);
    let reasons = f.check("tabs").unwrap_err();
    assert!(reasons[0].contains("1234"));
    assert!(reasons[0].contains("protocolo 0"));
    assert!(f.check("session-status").is_ok());
    assert_eq!(release_protocol(&r), 0);
}
#[test]
fn capable_writers_pass_and_future_protocol_fails_closed() {
    let f = Fixture::new("capable");
    let r = f.release("new", Some(STATE_PROTOCOL));
    f.process(100, &r.join("comandos"), &["comandos", "dash"]);
    assert!(f.check("tabs").is_ok());
    fs::write(r.join("manifest.json"), "{\"state_protocol\":3}").unwrap();
    assert!(f.check("tabs").is_err());
}
#[test]
fn python_cc_app_blocks_tabs_but_not_logs() {
    let f = Fixture::new("python");
    let script = f.repo.join("bin/cc-app");
    f.process(
        777,
        &PathBuf::from("/usr/bin/python3"),
        &["python3", script.to_str().unwrap()],
    );
    assert!(f.check("tabs").unwrap_err()[0].contains("python"));
    assert!(f.check("logs").is_ok());
}
#[test]
fn domain_python_registry_is_not_global() {
    for (script, yes, no) in [
        ("cc-acp", "ui-docs", "tabs"),
        ("cc-session-snapshot", "layout", "logs"),
        ("cc-extensions", "extensions", "tabs"),
        ("cc_usage.py", "db-usage", "logs"),
    ] {
        let f = Fixture::new(script);
        f.process(
            1,
            &PathBuf::from("/usr/bin/python3"),
            &["python3", f.repo.join("bin").join(script).to_str().unwrap()],
        );
        assert!(f.check(yes).is_err(), "{script}");
        assert!(f.check(no).is_ok(), "{script}");
    }
}
#[test]
fn unreadable_and_ambiguous_processes_fail_closed() {
    let f = Fixture::new("broken");
    fs::create_dir_all(f.proc.join("42")).unwrap();
    assert!(f.check("tabs").is_err());
    fs::remove_dir_all(f.proc.join("42")).unwrap();
    f.process(43, &PathBuf::from("/unknown/comandos"), &["comandos"]);
    assert!(f.check("tabs").is_err());
    assert!(f.check("unknown").is_err());
    fs::remove_dir_all(&f.proc).unwrap();
    assert!(f.check("tabs").is_err());
}
#[test]
fn relative_python_script_uses_injected_cwd() {
    let f = Fixture::new("relative");
    f.process(
        8,
        &PathBuf::from("/usr/bin/python3"),
        &["python3", "bin/cc-app"],
    );
    assert!(f.check("tabs").is_err());
    assert!(f.check("logs").is_ok());
}
#[test]
fn capable_release_with_ambiguous_argv_still_blocks() {
    let f = Fixture::new("ambiguous-capable");
    let r = f.release("new", Some(STATE_PROTOCOL));
    f.process(9, &r.join("comandos"), &["comandos"]);
    assert!(f.check("tabs").is_err());
}
#[test]
fn malformed_manifest_and_missing_cmdline_block() {
    let f = Fixture::new("malformed");
    let r = f.release("new", Some(STATE_PROTOCOL));
    f.process(10, &r.join("comandos"), &["comandos", "dash"]);
    fs::write(r.join("manifest.json"), b"bad").unwrap();
    assert!(f.check("tabs").is_err());
    fs::write(r.join("manifest.json"), b"{\"state_protocol\":2}").unwrap();
    fs::remove_file(f.proc.join("10/cmdline")).unwrap();
    assert!(f.check("tabs").is_err());
}
#[test]
fn app_executables_and_hook_aliases_use_their_release_manifest() {
    let f = Fixture::new("aliases");
    let r = f.release("new", Some(STATE_PROTOCOL));
    f.process(11, &r.join("comandos-app"), &["comandos-app"]);
    f.process(12, &r.join("comandos"), &["cc-status.sh"]);
    assert!(f.check("tabs").is_ok());
    assert!(f.check("session-status").is_ok());
    assert!(f.check("db-usage").is_ok());
}

#[test]
fn unknown_python_entrypoint_is_not_masked_by_known_data_argument() {
    let f = Fixture::new("unknown-data");
    let unknown = f.repo.join("bin/unknown.py");
    let known = f.repo.join("bin/cc-app");
    f.process(
        123,
        &PathBuf::from("/usr/bin/python3"),
        &[
            "python3",
            unknown.to_str().unwrap(),
            known.to_str().unwrap(),
        ],
    );
    assert!(f.check("tabs").is_err());
    assert!(f.check("logs").is_err());
}

#[test]
fn known_python_entrypoint_does_not_treat_data_paths_as_writers() {
    let f = Fixture::new("known-data");
    let known = f.repo.join("bin/cc-app");
    let unrelated = f.repo.join("bin/cc_usage.py");
    f.process(
        123,
        &PathBuf::from("/usr/bin/python3"),
        &[
            "python3",
            "-W",
            "default",
            known.to_str().unwrap(),
            unrelated.to_str().unwrap(),
        ],
    );
    assert!(f.check("tabs").is_err());
    assert!(f.check("logs").is_ok());
    assert!(f.check("db-usage").is_ok());
}

#[test]
fn dynamic_python_entrypoints_cannot_be_masked_by_known_data() {
    for option in ["-m", "-c", "-"] {
        let f = Fixture::new(&format!("dynamic-{option}"));
        let known = f.repo.join("bin/cc-app");
        let mut argv = vec!["python3", option];
        if option != "-" {
            argv.push("unknown_entrypoint");
        }
        argv.push(known.to_str().unwrap());
        f.process(123, &PathBuf::from("/usr/bin/python3"), &argv);
        assert!(f.check("logs").is_err());
    }
}
#[test]
fn external_python_script_does_not_become_repo_writer_from_data() {
    let f = Fixture::new("external-data");
    let known = f.repo.join("bin/cc-app");
    f.process(
        123,
        &PathBuf::from("/usr/bin/python3"),
        &["python3", "/private/external.py", known.to_str().unwrap()],
    );
    assert!(f.check("tabs").is_ok());
    assert!(f.check("logs").is_ok());
}
