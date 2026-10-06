//! Original desktop command contracts with fake window/process tools only.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
    sync::OnceLock,
};

fn fake_binary() -> &'static Path {
    static BIN: OnceLock<PathBuf> = OnceLock::new();
    BIN.get_or_init(|| {
        let root = std::env::temp_dir().join(format!("comandos-raise-tools-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let source = root.join("fake.rs");
        fs::write(&source, r#"
use std::{fs::OpenOptions, io::Write, path::Path};
fn main() {
    let args: Vec<String> = std::env::args().collect();
    let name = Path::new(&args[0]).file_name().unwrap().to_str().unwrap();
    let mut log = OpenOptions::new().create(true).append(true).open(std::env::var_os("RAISE_TEST_LOG").unwrap()).unwrap();
    writeln!(log, "{name} {:?}", &args[1..]).unwrap();
    if name == "wmctrl" {
        println!("window tool stdout");
        eprintln!("window tool stderr");
        std::process::exit(if args.last().unwrap() == &std::env::var("RAISE_TEST_WINDOW").unwrap_or_default() {0} else {1});
    }
    if name == "setsid" { println!("launch stdout"); eprintln!("launch stderr"); }
    else { println!("next stdout"); eprintln!("next stderr"); }
    std::process::exit(std::env::var("RAISE_TEST_EXIT").unwrap_or_else(|_| "0".into()).parse().unwrap());
}
"#).unwrap();
        let binary = root.join("fake");
        let result = Command::new("rustc").arg("--edition=2024").arg(&source).arg("-o").arg(&binary).output().unwrap();
        assert!(result.status.success(), "{}", String::from_utf8_lossy(&result.stderr));
        binary
    }).as_path()
}

struct Fixture(PathBuf);
impl Fixture {
    fn new(tag: &str) -> Self {
        let root =
            std::env::temp_dir().join(format!("comandos-raise-{tag}-{}", std::process::id()));
        fs::create_dir_all(root.join("bin")).unwrap();
        fs::create_dir_all(root.join(".local/bin")).unwrap();
        for name in ["wmctrl", "setsid"] {
            std::os::unix::fs::symlink(fake_binary(), root.join("bin").join(name)).unwrap();
        }
        for name in ["cc-next", "comandos"] {
            std::os::unix::fs::symlink(fake_binary(), root.join(".local/bin").join(name)).unwrap();
        }
        for name in ["cc-centro", "cc-term"] {
            std::os::unix::fs::symlink(env!("CARGO_BIN_EXE_comandos"), root.join("bin").join(name))
                .unwrap();
        }
        Self(root)
    }

    fn compare(&self, target: &str, window: &str, exit: i32, extra: &[&str]) {
        let alias = if target == "app" {
            "cc-centro"
        } else {
            "cc-term"
        };
        let original = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../bin")
            .join(alias);
        let log = self.0.join("calls");
        let run = |program: &Path, prefix: &[&str]| -> (Output, String) {
            fs::write(&log, "").unwrap();
            let result = Command::new(program)
                .args(prefix)
                .args(extra)
                .env("HOME", &self.0)
                .env("PATH", self.0.join("bin"))
                .env("XDG_DATA_HOME", self.0.join(".local/share"))
                .env("XDG_CONFIG_HOME", self.0.join(".config"))
                .env("XDG_CACHE_HOME", self.0.join(".cache"))
                .env("XDG_STATE_HOME", self.0.join(".local/state"))
                .env("XDG_RUNTIME_DIR", self.0.join("run"))
                .env("RAISE_TEST_LOG", &log)
                .env("RAISE_TEST_WINDOW", window)
                .env("RAISE_TEST_EXIT", exit.to_string())
                .output()
                .unwrap();
            (result, fs::read_to_string(&log).unwrap())
        };
        let (oracle, oracle_calls) = run(Path::new("/bin/bash"), &[original.to_str().unwrap()]);
        let expected_calls = oracle_calls
            .replace("/.local/bin/cc-app", "/.local/bin/comandos-app")
            .replace("cc-next []", "comandos [\"next\"]");
        for (program, prefix) in [
            (
                Path::new(env!("CARGO_BIN_EXE_comandos")).to_path_buf(),
                vec!["raise", target],
            ),
            (self.0.join("bin").join(alias), vec![]),
        ] {
            let (native, native_calls) = run(&program, &prefix);
            assert_eq!(
                native.status.code(),
                oracle.status.code(),
                "{target}/{window}/{exit}"
            );
            assert_eq!(native.stdout, oracle.stdout, "{target}/{window}/{exit}");
            assert_eq!(native.stderr, oracle.stderr, "{target}/{window}/{exit}");
            assert_eq!(native_calls, expected_calls, "{target}/{window}/{exit}");
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn app_reuses_window_or_launches_once_with_suppressed_process_output() {
    let fixture = Fixture::new("app");
    fixture.compare("app", "comandos", 0, &[]);
    fixture.compare("app", "absent", 0, &[]);
    fixture.compare("app", "absent", 19, &["ignored", "argument"]);
    fs::remove_file(fixture.0.join("bin/wmctrl")).unwrap();
    fixture.compare("app", "absent", 0, &[]);
    fs::remove_file(fixture.0.join("bin/setsid")).unwrap();
    fixture.compare("app", "absent", 0, &[]);
}

#[test]
fn non_executable_launcher_preserves_shell_failure_status() {
    let fixture = Fixture::new("permission");
    fs::remove_file(fixture.0.join("bin/setsid")).unwrap();
    fs::write(fixture.0.join("bin/setsid"), "unexecutable fixture").unwrap();
    fixture.compare("app", "absent", 0, &[]);
}

#[test]
fn term_preserves_priority_and_executes_native_next_with_exit_and_output() {
    let fixture = Fixture::new("term");
    for window in ["kitty", "comandos", "tilix", "absent"] {
        fixture.compare("term", window, 0, &[]);
    }
    fixture.compare("term", "absent", 23, &["ignored"]);
    fs::remove_file(fixture.0.join("bin/wmctrl")).unwrap();
    fixture.compare("term", "absent", 0, &[]);
}
