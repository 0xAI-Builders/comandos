//! Actual Bash differential with private Windows tool doubles, never Windows.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]
use comandos_cli::winstart::{Config, Outcome, run};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::OnceLock,
};

fn tools() -> &'static Path {
    static BIN: OnceLock<PathBuf> = OnceLock::new();
    BIN.get_or_init(|| {
        let root = std::env::temp_dir().join(format!("comandos-winstart-tools-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let source = root.join("tools.rs");
        fs::write(&source, r#"
use std::{env, fs, io::Write, path::Path};
fn main() {
 let a: Vec<String> = env::args().collect();
 let exe = Path::new(&a[0]); let name = exe.file_name().unwrap().to_str().unwrap();
 let executable = env::current_exe().unwrap();
 let root = executable.parent().unwrap().parent().unwrap();
 if name == "grep" { std::process::exit(if fs::read_to_string(root.join("release")).unwrap().to_lowercase().contains("microsoft") {0} else {1}); }
 if name == "wslpath" {
  if root.join("bad-wslpath").exists() { std::process::exit(8); }
  let folder = if a[1].ends_with("Local") {"local"} else {"roaming"};
  println!("{}", root.join(folder).display()); return;
 }
 let mut log = fs::OpenOptions::new().create(true).append(true).open(root.join("powershell.log")).unwrap();
 writeln!(log, "{}", a[3]).unwrap();
 if a[3].contains("Write($env:APPDATA)") {
  if root.join("query-warning").exists() {eprintln!("app warning");}
  if !root.join("empty-appdata").exists() { print!("C:\\Users\\Fixture\\AppData\\Roaming\r\n"); }
  if root.join("repeated-crlf").exists() {print!("\r\n");} return;
 }
 if a[3].contains("Write($env:LOCALAPPDATA)") { print!("C:\\Users\\Fixture\\AppData\\Local\r\n"); if root.join("repeated-crlf").exists() {print!("\r\n");} return; }
 if root.join("bad-create").exists() { eprintln!("fixture PowerShell error"); std::process::exit(9); }
 let properties: Vec<&str> = a[3].lines().map(str::trim).filter(|x| !x.is_empty()).collect();
 fs::write(root.join("properties"), properties.join("\n")).unwrap();
}
"#).unwrap();
        let binary = root.join("tools");
        let r = Command::new("rustc").args(["--edition=2024"]).arg(&source).arg("-o").arg(&binary).output().unwrap();
        assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
        binary
    }).as_path()
}

struct Fixture(PathBuf);
impl Fixture {
    fn new(tag: &str) -> Self {
        let root =
            std::env::temp_dir().join(format!("comandos-winstart-{tag}-{}", std::process::id()));
        for sub in ["bin", "repo/bin", "repo/dash"] {
            fs::create_dir_all(root.join(sub)).unwrap();
        }
        let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        fs::copy(
            repo.join("bin/cc-winstart"),
            root.join("repo/bin/cc-winstart"),
        )
        .unwrap();
        fs::copy(
            repo.join("dash/comandos.ico"),
            root.join("repo/dash/comandos.ico"),
        )
        .unwrap();
        for name in ["grep", "powershell.exe", "wslpath"] {
            fs::copy(tools(), root.join("bin").join(name)).unwrap();
        }
        for name in ["readlink", "dirname", "tr", "mkdir", "cp", "rm"] {
            std::os::unix::fs::symlink(
                Path::new("/usr/bin").join(name),
                root.join("bin").join(name),
            )
            .unwrap();
        }
        fs::write(root.join("release"), "6.8.0-microsoft-standard-WSL2").unwrap();
        Self(root)
    }

    fn compare(&self, args: &[&str], distro: &str) -> Outcome {
        let log = self.0.join("powershell.log");
        let link = self
            .0
            .join("roaming/Microsoft/Windows/Start Menu/Programs")
            .join(distro)
            .join(format!("ComandOS ({distro}).lnk"));
        let before_link = fs::read(&link).ok();
        fs::write(&log, "").unwrap();
        let oracle = Command::new("/bin/bash")
            .arg(self.0.join("repo/bin/cc-winstart"))
            .args(args)
            .env_clear()
            .env("HOME", &self.0)
            .env("USER", "fixture")
            .env("WSL_DISTRO_NAME", distro)
            .env("PATH", self.0.join("bin"))
            .output()
            .unwrap();
        let original_log = fs::read_to_string(&log).unwrap();
        let original_properties = fs::read_to_string(self.0.join("properties")).ok();
        if args.first() == Some(&"--uninstall") {
            if let Some(bytes) = before_link {
                fs::write(&link, bytes).unwrap();
            }
        } else {
            let _ = fs::remove_file(self.0.join("local/ComandOS/comandos.ico"));
        }
        let _ = fs::remove_file(self.0.join("properties"));
        fs::write(&log, "").unwrap();
        let config = Config {
            kernel_release: fs::read_to_string(self.0.join("release")).unwrap(),
            distro: distro.into(),
            user: "fixture".into(),
            home: self.0.clone(),
            path: self.0.join("bin").into_os_string(),
        };
        let native = run(
            &config,
            &args.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
        );
        assert_eq!(native.code, oracle.status.code().unwrap());
        assert_eq!(native.stdout, String::from_utf8(oracle.stdout).unwrap());
        assert_eq!(native.stderr, String::from_utf8(oracle.stderr).unwrap());
        assert_eq!(fs::read_to_string(&log).unwrap(), original_log);
        assert_eq!(
            fs::read_to_string(self.0.join("properties")).ok(),
            original_properties
        );
        native
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn installs_idempotently_and_preserves_shortcut_properties() {
    let f = Fixture::new("install");
    f.compare(&[], "Ubuntu Fixture");
    assert_eq!(
        fs::read(f.0.join("local/ComandOS/comandos.ico")).unwrap(),
        fs::read(f.0.join("repo/dash/comandos.ico")).unwrap()
    );
    f.compare(&["ignored", "extra"], "Ubuntu Fixture");
}

#[test]
fn uninstall_removes_only_shortcut_and_is_idempotent() {
    let f = Fixture::new("uninstall");
    let sm =
        f.0.join("roaming/Microsoft/Windows/Start Menu/Programs/Ubuntu");
    fs::create_dir_all(&sm).unwrap();
    let link = sm.join("ComandOS (Ubuntu).lnk");
    fs::write(&link, b"private shortcut").unwrap();
    let other = sm.join("Other.lnk");
    fs::write(&other, b"other").unwrap();
    f.compare(&["--uninstall"], "Ubuntu");
    assert!(!link.exists());
    assert_eq!(fs::read(&other).unwrap(), b"other");
    f.compare(&["--uninstall"], "Ubuntu");
}

#[test]
fn rejects_non_wsl_missing_tool_empty_paths_and_creation_failure() {
    let f = Fixture::new("errors");
    fs::write(f.0.join("release"), "6.8-linux").unwrap();
    f.compare(&["--uninstall"], "Ubuntu");
    fs::write(f.0.join("release"), "MICROSOFT WSL").unwrap();
    fs::remove_file(f.0.join("bin/powershell.exe")).unwrap();
    f.compare(&[], "Ubuntu");
    fs::copy(tools(), f.0.join("bin/powershell.exe")).unwrap();
    fs::write(f.0.join("empty-appdata"), b"").unwrap();
    f.compare(&[], "Ubuntu");
    fs::remove_file(f.0.join("empty-appdata")).unwrap();
    fs::write(f.0.join("bad-wslpath"), b"").unwrap();
    f.compare(&[], "Ubuntu");
    fs::remove_file(f.0.join("bad-wslpath")).unwrap();
    fs::write(f.0.join("bad-create"), b"").unwrap();
    f.compare(&[], "Ubuntu");
}

#[test]
fn command_and_alias_reject_real_non_wsl_host_without_writing_home() {
    let release = fs::read_to_string("/proc/sys/kernel/osrelease").unwrap();
    if release.to_ascii_lowercase().contains("microsoft") {
        return;
    }
    let f = Fixture::new("binary-gate");
    let alias = f.0.join("bin/cc-winstart");
    std::os::unix::fs::symlink(env!("CARGO_BIN_EXE_comandos"), &alias).unwrap();
    for (program, args) in [
        (
            PathBuf::from(env!("CARGO_BIN_EXE_comandos")),
            vec!["winstart", "--uninstall"],
        ),
        (alias, vec!["--uninstall"]),
    ] {
        let result = Command::new(program)
            .args(args)
            .env_clear()
            .env("HOME", &f.0)
            .env("PATH", f.0.join("bin"))
            .output()
            .unwrap();
        assert_eq!(result.status.code(), Some(1));
        assert!(result.stdout.is_empty());
        assert_eq!(
            String::from_utf8(result.stderr).unwrap(),
            "cc-winstart: este script solo aplica en WSL2.\n"
        );
        assert!(!f.0.join("powershell.log").exists());
        assert!(!f.0.join("roaming").exists());
        assert!(!f.0.join("local").exists());
    }
}

#[test]
fn windows_query_repeated_crlf_matches_shell_cleanup() {
    let f = Fixture::new("crlf");
    fs::write(f.0.join("repeated-crlf"), b"").unwrap();
    f.compare(&[], "Ubuntu");
}

#[test]
fn preserves_prior_query_warning_on_lookup_and_local_failures() {
    for tag in ["warn-tool", "warn-file"] {
        let f = Fixture::new(tag);
        fs::write(f.0.join("query-warning"), b"").unwrap();
        if tag == "warn-tool" {
            fs::remove_file(f.0.join("bin/wslpath")).unwrap();
        } else {
            fs::create_dir_all(f.0.join("local")).unwrap();
            fs::write(f.0.join("local/ComandOS"), b"blocker").unwrap();
        }
        let config = Config {
            kernel_release: "microsoft".into(),
            distro: "Ubuntu".into(),
            user: "fixture".into(),
            home: f.0.clone(),
            path: f.0.join("bin").into_os_string(),
        };
        let result = run(&config, &[]);
        assert_ne!(result.code, 0);
        assert!(
            result.stderr.starts_with("app warning\n"),
            "{}",
            result.stderr
        );
        assert!(!f.0.join("properties").exists());
    }
}

#[test]
fn rejects_distro_path_components_before_external_calls_or_writes() {
    let f = Fixture::new("invalid-distro");
    for distro in ["../escape", "/absolute", "..", "C:\\escape", "bad\nname"] {
        let config = Config {
            kernel_release: "microsoft".into(),
            distro: distro.into(),
            user: "fixture".into(),
            home: f.0.clone(),
            path: f.0.join("bin").into_os_string(),
        };
        let result = run(&config, &[]);
        assert_eq!(result.code, 1, "{distro:?}");
        assert!(!f.0.join("powershell.log").exists(), "{distro:?}");
        assert!(!f.0.join("local").exists(), "{distro:?}");
    }
}
