//! The maintained Bash command and native CLI share a fake tmux boundary.
//! The double only records argv; it never creates a PTY or runs an agent.
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
    sync::OnceLock,
};

fn fake_tmux() -> &'static Path {
    static BIN: OnceLock<PathBuf> = OnceLock::new();
    BIN.get_or_init(|| {
        let dir = std::env::temp_dir().join(format!("comandos-x-fake-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let source = dir.join("fake.rs");
        fs::write(&source, r#"
use std::{fs::OpenOptions, io::Write};
fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut log = OpenOptions::new().create(true).append(true).open(std::env::var_os("X_TEST_LOG").unwrap()).unwrap();
    writeln!(log, "{:?}", args).unwrap();
    let mode = std::env::var("X_TEST_MODE").unwrap_or_default();
    let code = match args[0].as_str() {
        "list-sessions" if mode == "empty" => 1,
        "list-sessions" => { println!("  fixture  [conectada]"); 0 },
        "has-session" if mode != "existing" => 1,
        "kill-session" if mode == "kill-fails" => 23,
        "attach" | "switch-client" if mode == "attach-fails" => 17,
        "new-session" | "new-window" if mode == "new-fails" => 19,
        _ => 0,
    };
    std::process::exit(code);
}
"#).unwrap();
        let binary = dir.join("fake");
        let out = Command::new("rustc")
            .args(["--edition=2024"])
            .arg(source).arg("-o").arg(&binary).output().unwrap();
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        binary
    }).as_path()
}

struct Home(PathBuf);
impl Home {
    fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("comandos-x-{tag}-{}", std::process::id()));
        fs::create_dir_all(dir.join("bin")).unwrap();
        fs::create_dir_all(dir.join(".claude/hooks")).unwrap();
        fs::create_dir_all(dir.join("codebase/team/Alpha.Test:Case")).unwrap();
        fs::create_dir_all(dir.join("codebase/Other.Space")).unwrap();
        fs::create_dir_all(dir.join("codebase/team/Leaf Space")).unwrap();
        fs::create_dir_all(dir.join("codebase/team/DöC")).unwrap();
        fs::create_dir_all(dir.join("codebase/team/Literal[One]")).unwrap();
        fs::create_dir_all(dir.join("codebase/team/Plain?Mark")).unwrap();
        fs::create_dir_all(dir.join("codebase/node_modules/Hidden")).unwrap();
        fs::create_dir_all(dir.join("codebase/team/deeper/TooDeep")).unwrap();
        std::os::unix::fs::symlink(fake_tmux(), dir.join("bin/tmux")).unwrap();
        std::os::unix::fs::symlink(env!("CARGO_BIN_EXE_comandos"), dir.join("bin/ccx")).unwrap();
        Self(dir)
    }
    fn compare(&self, args: &[&str], mode: &str, agent: Option<&str>, in_tmux: bool) {
        let log = self.0.join("calls");
        let path = format!("{}:/usr/bin:/bin", self.0.join("bin").display());
        let run = |program: &Path, prefix: &[&str]| -> (Output, Vec<u8>) {
            fs::write(&log, b"").unwrap();
            let mut command = Command::new(program);
            command
                .args(prefix)
                .args(args)
                .current_dir(&self.0)
                .env("HOME", &self.0)
                .env("PATH", &path)
                .env("SHELL", "/bin/fixture-shell")
                .env("X_TEST_LOG", &log)
                .env("X_TEST_MODE", mode)
                .env("LC_ALL", "C.UTF-8")
                .env_remove("TMUX")
                .env_remove("CCX_AGENT");
            if let Some(agent) = agent {
                command.env("CCX_AGENT", agent);
            }
            if in_tmux {
                command.env("TMUX", "/private/fixture/socket,1,0");
            }
            let out = command.output().unwrap();
            (out, fs::read(&log).unwrap())
        };
        let original = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../bin/ccx");
        let (oracle, oracle_calls) = run(Path::new("/bin/bash"), &[original.to_str().unwrap()]);
        let alias = self.0.join("bin/ccx");
        for (program, prefix) in [
            (Path::new(env!("CARGO_BIN_EXE_comandos")), vec!["x"]),
            (alias.as_path(), vec![]),
        ] {
            let (native, calls) = run(program, &prefix);
            assert_eq!(native.status.code(), oracle.status.code(), "args={args:?}");
            assert_eq!(native.stdout, oracle.stdout, "args={args:?}");
            // The Bash no-match branch emits an incidental arithmetic warning:
            // `grep -c` prints 0 and `|| echo 0` appends a second 0. Native
            // search keeps its deliberate message/exit and removes that bug.
            if oracle.status.code() == Some(1)
                && oracle_calls.is_empty()
                && oracle.stdout.starts_with(b"No encontre '")
                && oracle
                    .stderr
                    .ends_with(b"[: 0\n0: integer expression expected\n")
            {
                assert!(native.stderr.is_empty());
            } else {
                assert_eq!(native.stderr, oracle.stderr, "args={args:?}");
            }
            assert_eq!(calls, oracle_calls, "args={args:?}");
        }
    }
}
impl Drop for Home {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn lists_and_exact_kill_keep_output_and_exit_codes_without_a_server() {
    let home = Home::new("lists");
    for mode in ["empty", "existing"] {
        home.compare(&[], mode, None, false);
    }
    home.compare(&["kill", "Other.Space"], "", None, false);
    home.compare(&["kill", "Other.Space"], "kill-fails", None, false);
}

#[test]
fn search_depth_fuzzy_ambiguity_globs_and_direct_paths_match_original() {
    let home = Home::new("search");
    for args in [
        vec!["alpha.test:case"],
        vec!["Leaf"],
        vec!["*test*"],
        vec!["Space"],
        vec!["Hidden"],
        vec!["TooDeep"],
        vec!["absent"],
        vec!["codebase/team/Leaf Space"],
        vec!["dÖc"],
        vec!["D?c"],
        vec!["[D]ö[C]"],
        vec!["[!A]öC"],
        vec!["[[:alpha:]]öC"],
    ] {
        home.compare(&args, "", None, false);
    }
    home.compare(&["Other.Space"], "existing", None, true);
    home.compare(&["Other.Space"], "attach-fails", None, false);
    home.compare(&["Other.Space"], "new-fails", None, false);
}

#[test]
fn literal_brackets_escaped_wildcards_and_logical_symlink_paths_match_original() {
    let home = Home::new("literal");
    std::os::unix::fs::symlink(
        home.0.join("codebase/team/Leaf Space"),
        home.0.join("alias.with.dot"),
    )
    .unwrap();
    for name in [
        "Literal[[]One]",
        "Plain\\?Mark",
        "[A-D]öC",
        "alias.with.dot",
        "./alias.with.dot",
    ] {
        home.compare(&[name], "", None, false);
    }
}

#[test]
fn harness_flag_environment_last_config_and_overrides_match_original() {
    let home = Home::new("harness");
    fs::write(home.0.join(".claude/hooks/cc-notify.conf"),
        "# ignored\n AGENT_DEFAULT = grok\nAGENT_DEFAULT='custom.mpm'\nAGENT_LAUNCH_CUSTOM_MPM = 'fixture --old'\nAGENT_LAUNCH_CUSTOM_MPM = \"fixture --last $LITERAL\"\nAGENT_LAUNCH_CLAUDE = 'fixture builtin'\nSECRET=never-export\n").unwrap();
    home.compare(&["Other.Space"], "", None, false);
    home.compare(&["Other.Space"], "", Some("gemini"), false);
    home.compare(&["-a", "claude", "Other.Space"], "", Some("grok"), false);
    for harness in ["grok", "codex", "opencode", "agy", "gemini", "custom"] {
        home.compare(&["-a", harness, "Other.Space"], "", None, false);
    }
}

#[test]
fn kill_uses_only_the_injected_tmux_and_exact_target() {
    use comandos_cli::x::{Context, Tmux, run};
    use std::cell::RefCell;
    struct Spy(RefCell<Vec<Vec<String>>>);
    impl Tmux for Spy {
        fn call(&self, args: &[&str], quiet: bool) -> i32 {
            assert!(!quiet);
            self.0
                .borrow_mut()
                .push(args.iter().map(|arg| arg.to_string()).collect());
            23
        }
        fn finish(&self, _: &[&str]) -> i32 {
            panic!("kill never attaches");
        }
    }
    let home = Home::new("injected");
    let ctx = Context {
        home: home.0.clone(),
        cwd: home.0.clone(),
        agent: None,
        in_tmux: false,
    };
    let spy = Spy(RefCell::new(vec![]));
    let mut out = vec![];
    assert_eq!(
        run(
            &["kill".into(), "project:with space;kill-server".into()],
            &ctx,
            &spy,
            &mut out
        )
        .unwrap(),
        23
    );
    assert!(out.is_empty());
    assert_eq!(
        *spy.0.borrow(),
        vec![vec![
            "kill-session",
            "-t",
            "=project:with space;kill-server"
        ]]
    );
}
