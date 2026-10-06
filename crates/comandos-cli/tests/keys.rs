//! Differential CLI verification with fake SSH; no real network or passwords.
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
    sync::OnceLock,
};

fn fixture_binary() -> &'static Path {
    static BIN: OnceLock<PathBuf> = OnceLock::new();
    BIN.get_or_init(|| {
        let root = std::env::temp_dir().join(format!("comandos-keys-fake-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let source = root.join("fake.rs");
        fs::write(&source, r#"
use std::{io::Write, fs::OpenOptions, path::Path};
fn main() {
    let args: Vec<String> = std::env::args().collect();
    let name = Path::new(&args[0]).file_name().unwrap().to_str().unwrap();
    let mut log = OpenOptions::new().append(true).create(true).open(std::env::var_os("KEYS_TEST_LOG").unwrap()).unwrap();
    writeln!(log, "{} {:?}", name, &args[1..]).unwrap();
    let host = if name == "ssh" { &args[args.len()-2] } else { args.last().unwrap() };
    std::process::exit(if (name == "ssh" && host == "ready") || (name == "ssh-copy-id" && host != "fail") {0} else {1});
}
"#).unwrap();
        let bin = root.join("fake");
        let result = Command::new("rustc").args(["--edition=2024"]).arg(&source).arg("-o").arg(&bin).output().unwrap();
        assert!(result.status.success(), "{}", String::from_utf8_lossy(&result.stderr));
        bin
    }).as_path()
}
struct Home(PathBuf);
impl Home {
    fn new(tag: &str) -> Self {
        let root = std::env::temp_dir().join(format!("comandos-keys-{tag}-{}", std::process::id()));
        fs::create_dir_all(root.join(".ssh")).unwrap();
        fs::create_dir_all(root.join("bin")).unwrap();
        for name in ["ssh", "ssh-copy-id"] {
            std::os::unix::fs::symlink(fixture_binary(), root.join("bin").join(name)).unwrap();
        }
        std::os::unix::fs::symlink(env!("CARGO_BIN_EXE_comandos"), root.join("bin/cc-keys"))
            .unwrap();
        fs::write(root.join(".ssh/id_ed25519.pub"), "fixture-public-key\n").unwrap();
        Self(root)
    }
    fn compare(&self, args: &[&str]) {
        let log = self.0.join("calls");
        let path = format!("{}:/usr/bin:/bin", self.0.join("bin").display());
        let run = |program: &Path, prefix: &[&str]| -> (Output, Vec<u8>) {
            fs::write(&log, b"").unwrap();
            let out = Command::new(program)
                .args(prefix)
                .args(args)
                .env("HOME", &self.0)
                .env("PATH", &path)
                .env("KEYS_TEST_LOG", &log)
                .output()
                .unwrap();
            (out, fs::read(&log).unwrap())
        };
        let original = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../bin/cc-keys");
        let (oracle, oracle_calls) = run(Path::new("/bin/bash"), &[original.to_str().unwrap()]);
        let (native, native_calls) = run(Path::new(env!("CARGO_BIN_EXE_comandos")), &["keys"]);
        assert_eq!(native.status.code(), oracle.status.code());
        assert_eq!(native.stdout, oracle.stdout);
        assert_eq!(native.stderr, oracle.stderr);
        assert_eq!(native_calls, oracle_calls);
        let (alias, alias_calls) = run(&self.0.join("bin/cc-keys"), &[]);
        assert_eq!(alias.status.code(), oracle.status.code());
        assert_eq!(alias.stdout, oracle.stdout);
        assert_eq!(alias.stderr, oracle.stderr);
        assert_eq!(alias_calls, oracle_calls);
    }
}
impl Drop for Home {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn mixed_hosts_preserve_first_alias_wildcards_argv_output_and_pending_exit() {
    let home = Home::new("mixed");
    fs::write(
        home.0.join(".ssh/config"),
        "Host ready ignored-alias\nHOST install\nHost *.example\nHost fail\nHost ?skip\n",
    )
    .unwrap();
    home.compare(&[]);
    home.compare(&[""]);
    let custom = home.0.join("llave pública.pub");
    fs::write(&custom, "fixture").unwrap();
    home.compare(&[custom.to_str().unwrap()]);
}

#[test]
fn missing_key_and_absent_server_configuration_match_original_without_ssh() {
    let home = Home::new("missing");
    home.compare(&[home.0.join("absent.pub").to_str().unwrap()]);
    home.compare(&[]);
    fs::write(home.0.join(".ssh/config"), "Host *.example\n").unwrap();
    home.compare(&[]);
}
