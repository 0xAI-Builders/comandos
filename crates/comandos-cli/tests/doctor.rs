//! Actual Bash oracle in a private tree; all diagnostic tools are private Rust fakes.
use std::{
    fs,
    io::Write,
    os::unix::fs::{PermissionsExt, symlink},
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
    sync::{
        OnceLock,
        atomic::{AtomicUsize, Ordering},
    },
};

fn fake() -> &'static Path {
    static BIN: OnceLock<PathBuf> = OnceLock::new();
    BIN.get_or_init(|| {
        let root = std::env::temp_dir().join(format!("doctor-fake-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
        let source = root.join("fake.rs");
        fs::write(&source, r#"
use std::{fs, io::Write, path::Path};
fn main() {
 let args: Vec<String> = std::env::args().collect();
 let name = Path::new(&args[0]).file_name().unwrap().to_str().unwrap();
 let root = std::env::var_os("DOCTOR_FIXTURE").unwrap();
 let root = Path::new(&root);
 let mut log = fs::OpenOptions::new().append(true).create(true).open(root.join("calls")).unwrap();
 writeln!(log,"{} {:?}",name,&args[1..]).unwrap();
 if name=="curl" && let Some(i)=args.iter().position(|a|a=="-o") { fs::write(&args[i+1],b"private fake model\n").unwrap(); }
 let output = fs::read(root.join(format!("{name}.out"))).unwrap_or_default();
 std::io::stdout().write_all(&output).unwrap();
 let error = fs::read(root.join(format!("{name}.err"))).unwrap_or_default();
 std::io::stderr().write_all(&error).unwrap();
 let code = fs::read_to_string(root.join(format!("{name}.code"))).ok().and_then(|s|s.trim().parse().ok()).unwrap_or(0);
 std::process::exit(code);
}
"#).unwrap();
        let bin = root.join("fake");
        let output = Command::new("rustc").args(["--edition=2024"]).arg(source).arg("-o").arg(&bin).output().unwrap();
        assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
        bin
    }).as_path()
}
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        static N: AtomicUsize = AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!(
            "doctor-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        for path in [
            "home",
            "data",
            "config",
            "cache",
            "state",
            "run",
            "tmp",
            "bin",
            "oracle/bin",
            "oracle/lib",
        ] {
            fs::create_dir_all(root.join(path)).unwrap();
            fs::set_permissions(root.join(path), fs::Permissions::from_mode(0o700)).unwrap();
        }
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
        let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        fs::copy(
            repo.join("bin/cc-doctor"),
            root.join("oracle/bin/cc-doctor"),
        )
        .unwrap();
        fs::copy(
            repo.join("lib/platform.sh"),
            root.join("oracle/lib/platform.sh"),
        )
        .unwrap();
        // Only shell utilities needed by the unchanged oracle. No ambient tool PATH.
        for name in [
            "readlink",
            "dirname",
            "sed",
            "awk",
            "grep",
            "seq",
            "head",
            "tr",
            "mkdir",
            "mktemp",
            "rm",
            "mv",
            "chmod",
            "sha256sum",
        ] {
            symlink(
                Path::new("/usr/bin").join(name),
                root.join("bin").join(name),
            )
            .unwrap();
        }
        symlink(env!("CARGO_BIN_EXE_comandos"), root.join("bin/cc-doctor")).unwrap();
        fs::write(
            root.join("os-release"),
            "ID=ubuntu\nVERSION_CODENAME=noble\n",
        )
        .unwrap();
        fs::write(root.join("kernel"), "fixture kernel\n").unwrap();
        Self(root)
    }
    fn tool(&self, name: &str, output: &str, code: i32) {
        let path = self.0.join("bin").join(name);
        if path.symlink_metadata().is_err() {
            symlink(fake(), path).unwrap();
        }
        fs::write(self.0.join(format!("{name}.out")), output).unwrap();
        fs::write(self.0.join(format!("{name}.code")), code.to_string()).unwrap();
    }
    fn write(&self, path: &str, bytes: impl AsRef<[u8]>) {
        let path = self.0.join("home").join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
    }
    fn run(&self, oracle: bool, alias: bool, args: &[&str], input: &str) -> Output {
        fs::write(self.0.join("calls"), b"").unwrap();
        let program = if oracle {
            PathBuf::from("/bin/bash")
        } else if alias {
            self.0.join("bin/cc-doctor")
        } else {
            PathBuf::from(env!("CARGO_BIN_EXE_comandos"))
        };
        let mut cmd = Command::new(program);
        if oracle {
            cmd.arg(self.0.join("oracle/bin/cc-doctor"));
        } else if !alias {
            cmd.arg("doctor");
        }
        cmd.args(args)
            .env_clear()
            .env("HOME", self.0.join("home"))
            .env("PATH", self.0.join("bin"))
            .env("XDG_DATA_HOME", self.0.join("data"))
            .env("XDG_CONFIG_HOME", self.0.join("config"))
            .env("XDG_CACHE_HOME", self.0.join("cache"))
            .env("XDG_STATE_HOME", self.0.join("state"))
            .env("XDG_RUNTIME_DIR", self.0.join("run"))
            .env("TMPDIR", self.0.join("tmp"))
            .env("TMP", self.0.join("tmp"))
            .env("TEMP", self.0.join("tmp"))
            .env("CC_MOCK_UNAME", "Linux")
            .env("CC_MOCK_OSRELEASE_FILE", self.0.join("kernel"))
            .env("CC_MOCK_OS_RELEASE_FILE", self.0.join("os-release"))
            .env("CC_MOCK_SYSTEMD_STATE", "running")
            .env("CC_MOCK_DPKG_MISSING", "pulseaudio-utils")
            .env("CC_MOCK_APT", "1")
            .env("DOCTOR_FIXTURE", &self.0)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        for entry in fs::read_dir(&self.0).unwrap().flatten() {
            if let Some(key) = entry
                .file_name()
                .to_str()
                .and_then(|s| s.strip_prefix("env-"))
                .map(str::to_owned)
            {
                let value = fs::read_to_string(entry.path()).unwrap();
                if value == "<unset>" {
                    cmd.env_remove(key);
                } else {
                    cmd.env(key, value);
                }
            }
        }
        let mut child = cmd.spawn().unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(input.as_bytes())
            .unwrap();
        child.wait_with_output().unwrap()
    }
    fn equal(&self, args: &[&str], expected: i32) {
        let oracle = self.run(true, false, args, "");
        for alias in [false, true] {
            let native = self.run(false, alias, args, "");
            let expected_stdout = if args.contains(&"--json") && !args.contains(&"--help") {
                String::from_utf8_lossy(&oracle.stdout)
                    .lines()
                    .skip(1)
                    .map(|s| format!("{s}\n"))
                    .collect::<String>()
                    .into_bytes()
            } else {
                oracle.stdout.clone()
            };
            assert_eq!(
                native.stdout,
                expected_stdout,
                "{}",
                String::from_utf8_lossy(&native.stderr)
            );
            assert_eq!(native.stderr, oracle.stderr);
            assert_eq!(native.status.code(), Some(expected));
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn help_and_unknown_arguments_match_actual_bash_including_order() {
    let f = Fixture::new();
    for args in [
        &["--help"][..],
        &["audio", "-h"],
        &["--fix", "--json", "--help"],
    ] {
        f.equal(args, 0);
    }
    for args in [
        &["bogus"][..],
        &["--help", "bogus"],
        &["audio", "--"],
        &["", "-h"],
    ] {
        let code = if args[0] == "--help" { 0 } else { 2 };
        f.equal(args, code);
    }
}
#[test]
fn audio_stdout_matches_original_and_fail_is_nonzero() {
    let f = Fixture::new();
    let oracle = f.run(true, false, &["audio"], "");
    assert_eq!(
        oracle.status.code(),
        Some(0),
        "Original swallows its summary FAIL status"
    );
    assert!(String::from_utf8_lossy(&oracle.stdout).contains("1 FAIL"));
    f.equal(&["audio"], 1);
    f.equal(&["--json", "audio"], 1);
}
#[test]
fn audio_last_section_wins_and_regular_symlink_model_matches() {
    let f = Fixture::new();
    f.tool("pw-play", "", 0);
    f.tool("piper", "", 0);
    f.write(
        ".claude/hooks/cc-notify.conf",
        "PIPER_VOICE='voz privada'=ignored\n",
    );
    f.write("models/voice", b"private fixture");
    fs::create_dir_all(f.0.join("home/.local/share/piper-voices")).unwrap();
    symlink(
        f.0.join("home/models/voice"),
        f.0.join("home/.local/share/piper-voices/voz privada.onnx"),
    )
    .unwrap();
    f.equal(&["core", "audio"], 0);
    f.equal(&["audio", "--json"], 0);
}
#[test]
fn fix_declined_and_explicit_blank_accepts_match_actual_bash() {
    let f = Fixture::new();
    for input in ["n\n", "\n\n"] {
        let oracle = f.run(true, false, &["audio", "--fix"], input);
        let native = f.run(false, false, &["audio", "--fix"], input);
        assert_eq!(native.stdout, oracle.stdout);
        assert_eq!(native.stderr, oracle.stderr);
        assert_eq!(native.status.code(), Some(1));
    }
}

fn records(out: &Output) -> Vec<serde_json::Value> {
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter(|s| s.starts_with('{'))
        .map(|s| serde_json::from_str(s).unwrap())
        .collect()
}
#[test]
fn core_keeps_live_checks_and_checks_native_release_alias_units_and_modes_readonly() {
    let f = Fixture::new();
    for (name, out) in [
        ("tmux", "tmux fixture\n"),
        ("jq", "jq-fixture\n"),
        ("python3", "Python fixture\n"),
        ("grok", "grok fixture\nsecond line\n"),
        ("curl", ""),
        ("ss", "LISTEN 0 0 127.0.0.1:4778 0.0.0.0:*\n"),
    ] {
        f.tool(name, out, 0);
    }
    f.write(".grok/auth.json", "private token never displayed");
    f.write(".grok/hooks/comandos.json", r#"{"StopCancelled":{}}"#);
    f.write(".claude/hooks/cc-notify.sh", "native hook fixture");
    fs::set_permissions(
        f.0.join("home/.claude/hooks/cc-notify.sh"),
        fs::Permissions::from_mode(0o700),
    )
    .unwrap();
    let oracle = f.run(true, false, &["core", "--json"], "");
    let oracle: Vec<_> = records(&oracle)
        .into_iter()
        .filter(|r| r["check"] != "python3")
        .collect();
    let native = f.run(false, false, &["core", "--json"], "");
    let rows = records(&native);
    let kept: Vec<_> = rows
        .iter()
        .filter(|r| oracle.iter().any(|o| o["check"] == r["check"]))
        .cloned()
        .collect();
    assert_eq!(kept, oracle);
    assert!(!rows.iter().any(|r| r["check"] == "python3"));
    assert!(
        rows.iter()
            .any(|r| r["check"] == "release activa" && r["status"] == "fail")
    );
    assert!(
        rows.iter()
            .any(|r| r["check"] == "dominio session-status" && r["detail"] == "legacy")
    );
    assert_eq!(native.status.code(), Some(1));
    let release = comandos_cli::install::release::stage_release(
        &f.0.join("home"),
        fake(),
        &comandos_cli::install::release::WebSource::None,
    )
    .unwrap();
    let staged = f.0.join("home/.local/share/comandos/bin/comandos");
    fs::remove_file(f.0.join("home/.claude/hooks/cc-notify.sh")).unwrap();
    symlink(&staged, f.0.join("home/.claude/hooks/cc-notify.sh")).unwrap();
    fs::create_dir_all(f.0.join("home/.local/bin")).unwrap();
    symlink(&staged, f.0.join("home/.local/bin/cc-doctor")).unwrap();
    fs::create_dir_all(f.0.join("config/systemd/user")).unwrap();
    fs::write(
        f.0.join("config/systemd/user/fixture.service"),
        format!("[Service]\nExecStart={} dash --no-open\n", staged.display()),
    )
    .unwrap();
    let native = f.run(false, false, &["core", "--json"], "");
    let rows = records(&native);
    assert!(
        rows.iter()
            .any(|r| r["check"] == "release activa" && r["status"] == "pass")
    );
    assert!(
        rows.iter()
            .any(|r| r["check"] == format!("release {}", release.id) && r["status"] == "pass")
    );
    assert!(
        rows.iter()
            .any(|r| r["check"] == "alias cc-doctor" && r["status"] == "pass")
    );
    assert!(
        rows.iter()
            .any(|r| r["check"] == "unidad fixture.service" && r["status"] == "pass")
    );
    assert_eq!(native.status.code(), Some(0));
    assert!(
        !f.0.join("home/.local/share/comandos/comandos.sqlite3")
            .exists()
    );
    assert!(
        !f.0.join("home/.local/share/comandos/comandos.sqlite3.domain-modes.lock")
            .exists()
    );
}
#[test]
fn native_desktop_uses_gtk3_webkit41_libraries_from_private_sdk_and_web_manifest() {
    let f = Fixture::new();
    let libs = f.0.join("sdk/lib");
    fs::create_dir_all(&libs).unwrap();
    fs::write(libs.join("libgtk-3.so.0"), "private ABI fixture").unwrap();
    fs::write(libs.join("libwebkit2gtk-4.1.so.0"), "private ABI fixture").unwrap();
    f.tool("pkg-config", libs.to_str().unwrap(), 0);
    f.tool("wmctrl", "", 0);
    f.tool("fc-list", "JetBrainsMono Nerd Font Mono\n", 0);
    let out = f.run(false, false, &["desktop", "--json"], "");
    let rows = records(&out);
    assert!(
        rows.iter()
            .any(|r| r["check"] == "GTK 3" && r["status"] == "pass")
    );
    assert!(
        rows.iter()
            .any(|r| r["check"] == "WebKit2GTK 4.1" && r["status"] == "pass")
    );
    assert!(!rows.iter().any(|r| {
        r["check"]
            .as_str()
            .is_some_and(|s| s.contains("VTE") || s.starts_with("GTK 4") || s.contains("xterm"))
    }));
    let calls = fs::read_to_string(f.0.join("calls")).unwrap();
    assert!(!calls.contains("python3"));
    assert!(calls.contains("gtk+-3.0") && calls.contains("webkit2gtk-4.1"));
}
#[test]
fn original_eof_installs_but_native_skips_without_an_answer() {
    let f = Fixture::new();
    let original = f.run(true, false, &["audio", "--fix"], "");
    assert!(
        String::from_utf8_lossy(&original.stdout).contains("[mock apt install pulseaudio-utils]")
    );
    let native = f.run(false, false, &["audio", "--fix"], "");
    assert!(String::from_utf8_lossy(&native.stdout).contains("  skip\n"));
    assert!(!String::from_utf8_lossy(&native.stdout).contains("[mock apt"));
    assert!(!String::from_utf8_lossy(&native.stderr).contains("Faltan paquetes"));
    assert!(fs::read(f.0.join("calls")).unwrap().is_empty());
}
#[test]
fn json_escapes_actual_bad_version_output_and_json_fix_has_only_json_records() {
    let f = Fixture::new();
    f.tool("grok", "fixture \"quoted\"\\version\ttext\n", 0);
    let oracle = f.run(true, false, &["core", "--json"], "");
    let invalid = String::from_utf8_lossy(&oracle.stdout)
        .lines()
        .find(|line| line.contains("Grok Build"))
        .unwrap()
        .to_owned();
    assert!(serde_json::from_str::<serde_json::Value>(&invalid).is_err());
    let native = f.run(false, false, &["core", "--json"], "");
    assert!(
        String::from_utf8_lossy(&native.stdout)
            .lines()
            .all(|line| serde_json::from_str::<serde_json::Value>(line).is_ok())
    );
    assert!(
        records(&native)
            .iter()
            .any(|r| r["check"] == "Grok Build"
                && r["detail"] == "fixture \"quoted\"\\version\ttext")
    );
    let native = f.run(false, false, &["audio", "--json", "--fix"], "n\n");
    assert!(
        String::from_utf8_lossy(&native.stdout)
            .lines()
            .all(|line| serde_json::from_str::<serde_json::Value>(line).is_ok())
    );
    assert!(
        records(&native)
            .iter()
            .any(|r| r["section"] == "Arreglos" && r["status"] == "skip")
    );
}
#[test]
fn typed_voice_fix_does_not_execute_configuration_as_shell() {
    let f = Fixture::new();
    f.tool("pw-play", "", 0);
    f.tool("piper", "", 0);
    f.write(
        ".claude/hooks/cc-notify.conf",
        "PIPER_VOICE=unknown;printf injected >$HOME/injected\n",
    );
    let oracle = f.run(true, false, &["audio", "--fix"], "\n");
    assert!(String::from_utf8_lossy(&oracle.stdout).contains("voz 'unknown' desconocida"));
    assert_eq!(fs::read(f.0.join("home/injected")).unwrap(), b"injected");
    fs::remove_file(f.0.join("home/injected")).unwrap();
    let native = f.run(false, false, &["audio", "--fix"], "\n");
    assert!(
        String::from_utf8_lossy(&native.stdout)
            .contains("voz 'unknown;printf injected >$HOME/injected' desconocida")
    );
    assert!(!f.0.join("home/injected").exists());
    assert_eq!(native.status.code(), Some(1));
}
#[test]
fn private_download_checksum_failure_matches_actual_oracle_and_keeps_model_absent() {
    let f = Fixture::new();
    f.tool("pw-play", "", 0);
    f.tool("piper", "", 0);
    f.tool("curl", "private curl output\n", 0);
    fs::write(f.0.join("curl.err"), "private progress/error\n").unwrap();
    let oracle = f.run(true, false, &["audio", "--fix"], "\n");
    let native = f.run(false, false, &["audio", "--fix"], "\n");
    assert_eq!(native.stdout, oracle.stdout);
    assert_eq!(native.stderr, oracle.stderr);
    assert_eq!(native.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&native.stdout).contains("Checksum mismatch"));
    assert!(
        !f.0.join("home/.local/share/piper-voices/es_MX-ald-medium.onnx")
            .exists()
    );
    assert_eq!(fs::read_dir(f.0.join("tmp")).unwrap().count(), 0);
}

#[test]
fn desktop_retained_rows_match_actual_oracle_with_wsl_and_native_libraries_missing() {
    let f = Fixture::new();
    f.tool("python3", "", 0);
    f.tool("wmctrl", "", 0);
    f.tool("fc-list", "fixture font fallback\n", 0);
    f.tool("wslview", "", 0);
    fs::write(f.0.join("kernel"), "fixture Microsoft kernel\n").unwrap();
    fs::write(f.0.join("env-DISPLAY"), ":fixture").unwrap();
    fs::write(f.0.join("env-WAYLAND_DISPLAY"), "fixture-wayland").unwrap();
    let oracle = f.run(true, false, &["desktop", "--json"], "");
    let oracle: Vec<_> = records(&oracle)
        .into_iter()
        .filter(|r| {
            ![
                "GTK 3",
                "VTE 2.91",
                "WebKit2",
                "xterm.js bundle",
                "opentype.js (ligas)",
            ]
            .contains(&r["check"].as_str().unwrap())
        })
        .collect();
    let native = f.run(false, false, &["desktop", "--json"], "");
    let kept: Vec<_> = records(&native)
        .into_iter()
        .filter(|r| oracle.iter().any(|o| o["check"] == r["check"]))
        .collect();
    assert_eq!(kept, oracle);
    assert_eq!(native.status.code(), Some(1));
    let calls = fs::read_to_string(f.0.join("calls")).unwrap();
    assert!(!calls.contains("python3"));
    fs::write(f.0.join("env-CC_MOCK_UNAME"), "Darwin").unwrap();
    f.equal(&["desktop"], 0);
    f.equal(&["desktop", "--json"], 0);
}
#[test]
fn remote_preserves_tailscale_and_replaces_ttyd_with_native_endpoints() {
    let f = Fixture::new();
    f.tool("tailscale", "", 0);
    f.tool("ttyd", "", 0);
    f.tool("curl", "", 0);
    f.tool("comandos", "", 0);
    fs::write(f.0.join("env-CC_WEBTERM_PATH_PORT"), "32145").unwrap();
    fs::write(f.0.join("env-CC_WEBTERM_PORT"), "32146").unwrap();
    let oracle = f.run(true, false, &["remote", "--json"], "");
    let original_calls = fs::read_to_string(f.0.join("calls")).unwrap();
    let native = f.run(false, true, &["remote", "--json"], "");
    let rows = records(&native);
    assert_eq!(
        rows.iter().find(|r| r["check"] == "tailscale"),
        records(&oracle).iter().find(|r| r["check"] == "tailscale")
    );
    assert!(!rows.iter().any(|r| r["check"] == "ttyd"));
    assert!(
        rows.iter()
            .any(|r| r["check"] == "webterm Rust" && r["status"] == "pass")
    );
    assert_eq!(
        fs::read_to_string(f.0.join("calls")).unwrap(),
        original_calls
    );
    f.tool("curl", "", 22);
    f.tool("tailscale", "", 1);
    let native = f.run(false, false, &["remote", "--json"], "");
    assert_eq!(
        records(&native)
            .iter()
            .filter(|r| r["status"] == "warn")
            .count(),
        3
    );
    assert_eq!(native.status.code(), Some(0));
    fs::write(f.0.join("env-CC_WEBTERM_PORT"), "1@example.invalid/").unwrap();
    let _ = f.run(false, false, &["remote"], "");
    assert!(
        !fs::read_to_string(f.0.join("calls"))
            .unwrap()
            .contains("example.invalid")
    );
}
#[test]
fn malformed_release_wrong_alias_checkout_unit_and_d9_remnants_are_visible_without_writes() {
    let f = Fixture::new();
    let release = comandos_cli::install::release::stage_release(
        &f.0.join("home"),
        fake(),
        &comandos_cli::install::release::WebSource::None,
    )
    .unwrap();
    fs::write(
        release.path.parent().unwrap().join("manifest.json"),
        "{malformed",
    )
    .unwrap();
    f.write(".local/bin/cc-next", "legacy script\n");
    fs::set_permissions(
        f.0.join("home/.local/bin/cc-next"),
        fs::Permissions::from_mode(0o700),
    )
    .unwrap();
    f.write(".claude/hooks/telegram.env", "private never parsed\n");
    fs::create_dir_all(f.0.join("config/systemd/user")).unwrap();
    let source = f.0.join("unit-source");
    fs::write(
        &source,
        "[Service]\nExecStart=/usr/bin/python3 %h/codebase/0xJesus/ComandOS/bin/cc-dash\n",
    )
    .unwrap();
    symlink(source, f.0.join("config/systemd/user/legacy.service")).unwrap();
    let before = tree(&f.0.join("home"));
    let out = f.run(false, false, &["core", "--json"], "");
    assert_eq!(tree(&f.0.join("home")), before);
    let rows = records(&out);
    for name in ["release activa", "alias cc-next", "unidad legacy.service"] {
        assert!(
            rows.iter()
                .any(|r| r["check"] == name && r["status"] == "fail"),
            "{rows:?}"
        );
    }
    assert!(rows.iter().any(|r| r["check"] == "restos D9"
        && r["status"] == "warn"
        && r["detail"].as_str().unwrap().contains("telegram.env")));
    assert_eq!(out.status.code(), Some(1));
}
type Stamp = (PathBuf, u64, u32, i64, i64, Vec<u8>);
fn tree(root: &Path) -> Vec<Stamp> {
    use std::os::unix::fs::MetadataExt;
    fn walk(path: &Path, out: &mut Vec<Stamp>) {
        let Ok(m) = path.symlink_metadata() else {
            return;
        };
        let bytes = if m.is_file() {
            fs::read(path).unwrap()
        } else if m.file_type().is_symlink() {
            fs::read_link(path)
                .unwrap()
                .to_string_lossy()
                .as_bytes()
                .to_vec()
        } else {
            vec![]
        };
        out.push((
            path.to_owned(),
            m.ino(),
            m.mode(),
            m.mtime(),
            m.mtime_nsec(),
            bytes,
        ));
        if m.is_dir() {
            for e in fs::read_dir(path).unwrap().flatten() {
                walk(&e.path(), out);
            }
        }
    }
    let mut out = vec![];
    walk(root, &mut out);
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}
#[test]
fn doctor_modes_show_original_sealed_authority_and_reject_future_wal_readonly() {
    let f = Fixture::new();
    let home = f.0.join("home");
    let path = home.join(".local/share/comandos/comandos.sqlite3");
    let db = comandos_store::unified::open_unified(&path).unwrap();
    comandos_store::unified::set_mode(
        &db,
        "session-status",
        comandos_store::unified::Mode::Sealed,
        "fixture",
        1,
    )
    .unwrap();
    let before = tree(&home);
    let out = f.run(false, false, &["core", "--json"], "");
    assert_eq!(tree(&home), before);
    assert!(
        records(&out)
            .iter()
            .any(|r| r["check"] == "dominio session-status" && r["detail"] == "sealed")
    );
    db.execute_batch("PRAGMA user_version=999999").unwrap();
    let before = tree(&home);
    let out = f.run(false, false, &["core", "--json"], "");
    assert_eq!(tree(&home), before);
    assert!(
        records(&out)
            .iter()
            .any(|r| r["check"] == "modos de dominio" && r["status"] == "fail")
    );
    drop(db);
}
#[test]
fn alias_to_frozen_release_is_rejected_and_relative_link_to_active_bin_is_accepted() {
    let f = Fixture::new();
    let release = comandos_cli::install::release::stage_release(
        &f.0.join("home"),
        fake(),
        &comandos_cli::install::release::WebSource::None,
    )
    .unwrap();
    fs::create_dir_all(f.0.join("home/.local/bin")).unwrap();
    let alias = f.0.join("home/.local/bin/cc-next");
    symlink(&release.path, &alias).unwrap();
    let out = f.run(false, false, &["core", "--json"], "");
    assert!(
        records(&out)
            .iter()
            .any(|r| r["check"] == "alias cc-next" && r["status"] == "fail")
    );
    fs::remove_file(&alias).unwrap();
    symlink("../share/comandos/bin/comandos", &alias).unwrap();
    let out = f.run(false, false, &["core", "--json"], "");
    assert!(
        records(&out)
            .iter()
            .any(|r| r["check"] == "alias cc-next" && r["status"] == "pass")
    );
}
#[test]
fn systemd_effective_service_execstart_resets_and_continuations_are_checked() {
    let f = Fixture::new();
    fs::create_dir_all(f.0.join("config/systemd/user")).unwrap();
    let repo = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap();
    for (name, body) in [
        (
            "reset.service",
            format!(
                "[Service]\nExecStart=/usr/bin/python3 {}/bin/cc-dash\nExecStart=\nExecStart=/usr/bin/native\n",
                repo.display()
            ),
        ),
        (
            "split.service",
            format!(
                "[Service]\nExecStart = /usr/bin/python3 \\\n  {}/old-app.py\n",
                repo.display()
            ),
        ),
        (
            "other-section.service",
            format!(
                "[NotAService]\nExecStart={}/bin/cc-dash\n[Service]\nExecStart=/usr/bin/native\n",
                repo.display()
            ),
        ),
    ] {
        fs::write(f.0.join("config/systemd/user").join(name), body).unwrap();
    }
    let out = f.run(false, false, &["core", "--json"], "");
    let rows = records(&out);
    for (name, status) in [
        ("reset.service", "pass"),
        ("split.service", "fail"),
        ("other-section.service", "pass"),
    ] {
        assert!(
            rows.iter()
                .any(|r| r["check"] == format!("unidad {name}") && r["status"] == status),
            "{rows:?}"
        );
    }
}

#[test]
fn confirmed_apt_fix_preserves_output_and_argument_vectors_with_fake_sudo() {
    let f = Fixture::new();
    f.tool("sudo", "private apt output\n", 0);
    fs::write(f.0.join("env-CC_MOCK_APT"), "<unset>").unwrap();
    let oracle = f.run(true, false, &["audio", "--fix"], "\n\n");
    let calls = fs::read(f.0.join("calls")).unwrap();
    let native = f.run(false, false, &["audio", "--fix"], "\n\n");
    assert_eq!(native.stdout, oracle.stdout);
    assert_eq!(native.stderr, oracle.stderr);
    assert_eq!(fs::read(f.0.join("calls")).unwrap(), calls);
    assert_eq!(native.status.code(), Some(1));
}
