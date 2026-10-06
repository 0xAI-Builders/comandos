//! Native transport fakes only: no daemon, listener, sudo or personal configuration.
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs,
    os::unix::fs::{MetadataExt, PermissionsExt, symlink},
    path::{Path, PathBuf},
    process::{Command, Output},
    sync::{
        OnceLock,
        atomic::{AtomicUsize, Ordering},
    },
};

fn fake() -> &'static Path {
    static BIN: OnceLock<PathBuf> = OnceLock::new();
    BIN.get_or_init(|| {
        let root = std::env::temp_dir().join(format!("mobile-tool-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
        let src = root.join("fake.rs");
        fs::write(&src, r#"
use std::{fs,io::{Read,Write},path::Path};
fn main() {
 let args:Vec<String>=std::env::args().collect();
 let name=Path::new(&args[0]).file_name().unwrap().to_str().unwrap();
 let root=std::env::var_os("MOBILE_FIXTURE").unwrap();let root=Path::new(&root);
 let words=&args[1..];
 let mut input=String::new();
 if words==["serve","set-raw"] || ["grep","head","sed","tr"].contains(&name) {std::io::stdin().read_to_string(&mut input).unwrap();}
 let mut log=fs::OpenOptions::new().create(true).append(true).open(root.join("calls")).unwrap();
 // JSON strings, encoded without external libraries by a native fixture.
 fn quote(s:&str)->String {let mut o=String::from("\"");for c in s.chars(){match c{'\"'=>o.push_str("\\\""),'\\'=>o.push_str("\\\\"),'\n'=>o.push_str("\\n"),'\r'=>o.push_str("\\r"),'\t'=>o.push_str("\\t"),c if c<' '=>o.push_str(&format!("\\u{:04x}",c as u32)),_=>o.push(c)}}o.push('"');o}
 writeln!(log,"{{\"name\":{},\"argv\":[{}],\"stdin\":{}}}",quote(name),words.iter().map(|s|quote(s)).collect::<Vec<_>>().join(","),quote(&input)).unwrap();
 if root.join("overflow").exists() {std::io::stderr().write_all(&vec![b'x';2*1024*1024]).unwrap();return;}
 if root.join("sleep").exists() {std::thread::sleep(std::time::Duration::from_secs(60));return;}
 let mut code=0;
 match name {
  "tailscale" if words==["status","--json"]=>print!("{}",fs::read_to_string(root.join("status.json")).unwrap()),
  "tailscale" if words==["serve","status","--json"]=>{
   let n=fs::read_to_string(root.join("reads")).ok().and_then(|s|s.parse::<u32>().ok()).unwrap_or(0)+1;
   fs::write(root.join("reads"),n.to_string()).unwrap();
   if n==2 && root.join("drift.json").exists(){fs::copy(root.join("drift.json"),root.join("node.json")).unwrap();}
   print!("{}",fs::read_to_string(root.join("node.json")).unwrap());
  },
  "tailscale" if words==["serve","set-raw"]=>{
   if root.join("apply-fail").exists(){eprintln!("fake permission denied");code=1;}
   else{fs::write(root.join("node.json"),input).unwrap();}
  },
  "tailscale" if words==["status"]=>print!("100.64.0.2 node fixture@ linux active\n"),
  "tailscale" if words==["serve","status"]=>{if !root.join("oracle-off").exists(){print!("https://node.fixture.ts.net (tailnet only)\n|-- / proxy http://127.0.0.1:4777\n");}},
  "tailscale" if words.first().is_some_and(|s|s=="serve") && words.get(1).is_some_and(|s|s.starts_with("--"))=>{
   if words.last().is_some_and(|s|s=="off"){fs::write(root.join("oracle-off"),b"1").unwrap();}
  },
  "curl"=>{if root.join("front-off").exists(){code=7;}else{print!("{{\"ok\":true}}\n");}},
  "qrencode"=>{if root.join("qr-fail").exists(){code=9;}else{print!("FAKE QR\n");}},
  "cat"=>{std::io::stdout().write_all(&fs::read(&words[0]).unwrap()).unwrap();},
  "chmod"=>{use std::os::unix::fs::PermissionsExt;assert!(Path::new(&words[1]).starts_with(root));fs::set_permissions(&words[1],fs::Permissions::from_mode(u32::from_str_radix(&words[0],8).unwrap())).unwrap();},
  "grep" if words.first().is_some_and(|s|s=="-oE")=>println!("\"DNSName\":\"node.fixture.ts.net.\""),
  "grep"=>{if !["4777","4779","4780"].iter().any(|p|input.contains(p)){code=1;}},
  "head"=>{let n=words.first().and_then(|s|s.strip_prefix('-')).and_then(|s|s.parse::<usize>().ok()).unwrap();for line in input.lines().take(n){println!("{line}");}},
  "sed"=>{let s=if words[0].contains(".*"){input.trim().split('"').nth(3).unwrap()}else{input.trim().trim_end_matches('.')};println!("{s}");},
  "tr"=>print!("{input}"),
  _=>{eprintln!("unexpected fake command: {name} {words:?}");code=99;}
 }
 std::process::exit(code);
}
"#).unwrap();
        let bin = root.join("fake");
        let out = Command::new("rustc").arg("--edition=2024").arg(src).arg("-o").arg(&bin).output().unwrap();
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        bin
    }).as_path()
}

struct Fixture {
    root: PathBuf,
    home: PathBuf,
}
type Snapshot = BTreeMap<PathBuf, (u64, u32, i64, i64, Vec<u8>)>;
fn evidence(tag: &str, rust: &Fixture, native: &Output, bash: &Fixture, legacy: &Output) {
    if let Some(dir) = std::env::var_os("MOBILE_TEST_EVIDENCE") {
        let dir = PathBuf::from(dir);
        fs::create_dir_all(&dir).unwrap();
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)).unwrap();
        let output =
            |o: &Output| json!({"exit":o.status.code(),"stdout":o.stdout,"stderr":o.stderr});
        fs::write(
            dir.join(format!("{tag}.json")),
            serde_json::to_vec_pretty(&json!({
                "native":{"output":output(native),"calls":rust.calls()},
                "bash":{"output":output(legacy),"calls":bash.calls()},
            "initial_node":{},
            "native_token_bytes":fs::read(rust.token()).ok(),
            "bash_token_bytes":fs::read(bash.token()).ok()
            }))
            .unwrap(),
        )
        .unwrap();
    }
}
impl Fixture {
    fn new(node: &[u8]) -> Self {
        static N: AtomicUsize = AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!(
            "mobile-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        for dir in [
            "home with ' quote",
            "config",
            "data",
            "state",
            "cache",
            "runtime",
            "tmp",
            "bin",
        ] {
            fs::create_dir_all(root.join(dir)).unwrap();
            fs::set_permissions(root.join(dir), fs::Permissions::from_mode(0o700)).unwrap();
        }
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
        let home = root.join("home with ' quote");
        fs::write(root.join("node.json"), node).unwrap();
        fs::write(root.join("status.json"),serde_json::to_vec(&json!({"BackendState":"Running","Self":{"DNSName":"node.fixture.ts.net."},"CertDomains":["node.fixture.ts.net"]})).unwrap()).unwrap();
        for name in ["tailscale", "curl", "qrencode"] {
            symlink(fake(), root.join("bin").join(name)).unwrap();
        }
        symlink(env!("CARGO_BIN_EXE_comandos"), root.join("bin/cc-mobile")).unwrap();
        Self { root, home }
    }
    fn run(&self, args: &[&str]) -> Output {
        self.command(false, args).output().unwrap()
    }
    fn command(&self, alias: bool, args: &[&str]) -> Command {
        let mut cmd = Command::new(if alias {
            self.root.join("bin/cc-mobile")
        } else {
            PathBuf::from(env!("CARGO_BIN_EXE_comandos"))
        });
        if !alias {
            cmd.arg("mobile");
        }
        cmd.args(args)
            .env("HOME", &self.home)
            .env("PATH", self.root.join("bin"))
            .env("MOBILE_FIXTURE", &self.root)
            .env("XDG_CONFIG_HOME", self.root.join("config"))
            .env("XDG_DATA_HOME", self.root.join("data"))
            .env("XDG_STATE_HOME", self.root.join("state"))
            .env("XDG_CACHE_HOME", self.root.join("cache"))
            .env("XDG_RUNTIME_DIR", self.root.join("runtime"))
            .env("TMPDIR", self.root.join("tmp"))
            .env("TMP", self.root.join("tmp"))
            .env("TEMP", self.root.join("tmp"))
            .env_remove("DISPLAY")
            .env_remove("WAYLAND_DISPLAY")
            .env_remove("TMUX")
            .env_remove("CC_PORT")
            .env_remove("CC_WEBTERM_PORT")
            .env_remove("CC_WEBTERM_PATH_PORT");
        cmd
    }
    fn oracle(&self, args: &[&str]) -> Output {
        let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../bin/cc-mobile");
        let frozen = self.root.join("cc-mobile-original");
        fs::copy(source, &frozen).unwrap();
        for name in ["cat", "chmod", "grep", "head", "sed", "tr"] {
            if self.root.join("bin").join(name).symlink_metadata().is_err() {
                symlink(fake(), self.root.join("bin").join(name)).unwrap();
            }
        }
        let mut command = self.command(false, &[]);
        // Reuse only the private environment; replace the program and argv.
        let vars = command
            .get_envs()
            .map(|(k, v)| (k.to_os_string(), v.map(|v| v.to_os_string())))
            .collect::<Vec<_>>();
        command = Command::new("/bin/bash");
        command.arg(frozen).args(args);
        for (k, v) in vars {
            if let Some(v) = v {
                command.env(k, v);
            } else {
                command.env_remove(k);
            }
        }
        command.output().unwrap()
    }
    fn token(&self) -> PathBuf {
        self.home.join(".claude/hooks/dash-token")
    }
    fn set_token(&self, body: &[u8]) {
        fs::create_dir_all(self.token().parent().unwrap()).unwrap();
        fs::write(self.token(), body).unwrap();
        fs::set_permissions(self.token(), fs::Permissions::from_mode(0o600)).unwrap();
    }
    fn calls(&self) -> Vec<Value> {
        fs::read_to_string(self.root.join("calls"))
            .unwrap_or_default()
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect()
    }
    fn applied(&self) -> Vec<Value> {
        self.calls()
            .into_iter()
            .filter(|c| c["argv"] == json!(["serve", "set-raw"]))
            .collect()
    }
    fn config(&self) -> Value {
        serde_json::from_slice(&fs::read(self.root.join("node.json")).unwrap()).unwrap()
    }
    fn backup(&self) -> PathBuf {
        let root = self.home.join(".local/share/comandos/backups");
        let dirs = fs::read_dir(root)
            .unwrap()
            .map(|e| e.unwrap().path())
            .collect::<Vec<_>>();
        assert_eq!(dirs.len(), 1);
        dirs[0].join("tailscale-serve.json")
    }
    fn snapshot(&self) -> Snapshot {
        fn walk(root: &Path, p: &Path, out: &mut Snapshot) {
            let m = p.symlink_metadata().unwrap();
            let bytes = if m.file_type().is_symlink() {
                use std::os::unix::ffi::OsStrExt;
                fs::read_link(p).unwrap().as_os_str().as_bytes().to_vec()
            } else if m.is_file() {
                fs::read(p).unwrap()
            } else {
                Vec::new()
            };
            out.insert(
                p.strip_prefix(root).unwrap().to_owned(),
                (m.ino(), m.mode(), m.mtime(), m.mtime_nsec(), bytes),
            );
            if m.is_dir() {
                for e in fs::read_dir(p).unwrap() {
                    walk(root, &e.unwrap().path(), out);
                }
            }
        }
        let mut out = Snapshot::new();
        walk(&self.home, &self.home, &mut out);
        out
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).unwrap();
    }
}
fn ok(out: &Output) {
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn single_front_keeps_other_routes_ports_services_and_exact_private_backup() {
    let before=br#"{ "TCP":{"443":{"HTTPS":true},"9443":{"HTTPS":true}}, "Web":{"node.fixture.ts.net:443":{"Handlers":{"/reports":{"Proxy":"http://127.0.0.1:6000"}}},"node.fixture.ts.net:9443":{"Handlers":{"/":{"Text":"other service"}}}},"Services":{"svc:other":{"Tun":true}} }\n"#;
    // Preserve actual JSON bytes, including whitespace and trailing newline.
    let before = before.strip_suffix(b"\\n").unwrap();
    let f = Fixture::new(before);
    f.set_token(b"fixture-token");
    ok(&f.run(&["on"]));
    assert_eq!(fs::read(f.backup()).unwrap(), before);
    assert_eq!(f.backup().metadata().unwrap().mode() & 0o777, 0o600);
    assert_eq!(
        f.backup().parent().unwrap().metadata().unwrap().mode() & 0o777,
        0o700
    );
    assert_eq!(
        f.config()["Web"]["node.fixture.ts.net:443"]["Handlers"]["/"],
        json!({"Proxy":"http://127.0.0.1:4777"})
    );
    assert_eq!(f.config()["Services"], json!({"svc:other":{"Tun":true}}));
    assert_eq!(
        f.config()["Web"]["node.fixture.ts.net:443"]["Handlers"]["/reports"]["Proxy"],
        "http://127.0.0.1:6000"
    );
    assert_eq!(f.config()["TCP"]["9443"], json!({"HTTPS":true}));
    let text = String::from_utf8(f.run(&["status"]).stdout).unwrap();
    assert!(text.contains("Tailscale:\n") && text.contains("Serve:\n"));
}

#[test]
fn default_and_alias_on_share_url_token_and_optional_qr_argv() {
    let f = Fixture::new(b"{}");
    f.set_token(b"fixture-token");
    let out = f.command(true, &[]).output().unwrap();
    ok(&out);
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(text.contains("https://node.fixture.ts.net/?token=fixture-token"));
    let qr = f
        .calls()
        .into_iter()
        .find(|c| c["name"] == "qrencode")
        .unwrap();
    assert_eq!(
        qr["argv"],
        json!([
            "-t",
            "ANSIUTF8",
            "https://node.fixture.ts.net/?token=fixture-token"
        ])
    );
    assert!(
        !f.calls()
            .iter()
            .any(|c| c["name"] == "ttyd" || c["name"] == "cc-webterm")
    );
    assert_eq!(f.applied().len(), 1);
}

#[test]
fn dry_run_never_creates_token_backup_state_or_serve_mutation() {
    let f = Fixture::new(b"{}");
    let before = f.snapshot();
    let out = f.run(&["on", "--dry-run"]);
    ok(&out);
    assert_eq!(f.snapshot(), before);
    assert!(f.applied().is_empty());
    assert!(!f.calls().iter().any(|c| c["name"] == "qrencode"));
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(
        text.contains("rollback")
            && text.contains("tailscale-serve.json")
            && text.contains(f.home.to_str().unwrap())
    );
    let f = Fixture::new(br#"{"TCP":{"443":{"HTTPS":true}},"Web":{"node.fixture.ts.net:443":{"Handlers":{"/":{"Proxy":"http://127.0.0.1:4777"}}}}}"#);
    let before = f.snapshot();
    let out = f.run(&["on", "--dry-run"]);
    ok(&out);
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(text.contains("no requiere backup"));
    assert!(!text.contains("rollback"));
    assert_eq!(f.snapshot(), before);
    assert!(f.applied().is_empty());
}

#[test]
fn funnel_conflict_malformed_or_unknown_schema_fail_before_token_and_writes() {
    for bytes in [
        br#"{"AllowFunnel":{"node.fixture.ts.net:443":true}}"#.as_slice(),
        br#"{"TCP":{"443":{"TCPForward":"127.0.0.1:6000"}}}"#,
        br#"{"Web":{"node.fixture.ts.net:443":{"Handlers":{"/":{"Text":"unrelated"}}}}}"#,
        br#"{"Future":{"new":true}}"#,
        b"invalid JSON",
    ] {
        let f = Fixture::new(bytes);
        let before = f.snapshot();
        assert_eq!(f.run(&["on"]).status.code(), Some(1));
        assert_eq!(f.snapshot(), before);
        assert!(f.applied().is_empty());
    }
}

#[test]
fn on_and_off_remove_only_owned_legacy_routes_without_publishing_old_ports() {
    let f=Fixture::new(br#"{"TCP":{"443":{"HTTPS":true},"8443":{"HTTPS":true}},"Web":{"node.fixture.ts.net:443":{"Handlers":{"/":{"Proxy":"http://127.0.0.1:4777"},"/term":{"Proxy":"http://127.0.0.1:4780/term"},"/other":{"Text":"keep"}}},"node.fixture.ts.net:8443":{"Handlers":{"/":{"Proxy":"http://127.0.0.1:4779"}}}}}"#);
    f.set_token(b"fixture-token");
    ok(&f.run(&["on"]));
    assert!(f.config()["Web"].get("node.fixture.ts.net:8443").is_none());
    assert!(
        f.config()["Web"]["node.fixture.ts.net:443"]["Handlers"]
            .get("/term")
            .is_none()
    );
    ok(&f.run(&["off"]));
    assert_eq!(
        f.config()["Web"]["node.fixture.ts.net:443"]["Handlers"],
        json!({"/other":{"Text":"keep"}})
    );
    assert_eq!(f.config()["TCP"]["443"], json!({"HTTPS":true}));
    assert!(
        f.calls()
            .iter()
            .all(|c| c["argv"] != json!(["serve", "reset"]))
    );
}

#[test]
fn rollback_uses_exact_node_json_stdin_and_rejects_unrelated_config_drift() {
    let original = b"{\n  \"Services\": {\"svc:other\":{\"Tun\":true}}\n}\n";
    let f = Fixture::new(original);
    f.set_token(b"fixture-token");
    ok(&f.run(&["on"]));
    let backup = f.backup();
    let before = f.snapshot();
    ok(&f.run(&["rollback", backup.to_str().unwrap(), "--dry-run"]));
    assert_eq!(f.snapshot(), before);
    ok(&f.run(&["rollback", backup.to_str().unwrap()]));
    assert_eq!(fs::read(f.root.join("node.json")).unwrap(), original);
    ok(&f.run(&["on"]));
    let mut current = f.config();
    current["Services"]["svc:added"] = json!({"Tun":true});
    fs::write(
        f.root.join("node.json"),
        serde_json::to_vec(&current).unwrap(),
    )
    .unwrap();
    let calls = f.applied().len();
    assert_eq!(
        f.run(&["rollback", backup.to_str().unwrap()]).status.code(),
        Some(1)
    );
    assert_eq!(f.applied().len(), calls);
    assert_eq!(f.config()["Services"]["svc:added"], json!({"Tun":true}));
}

#[test]
fn serve_failure_keeps_exact_backup_and_fails_without_retry() {
    let f = Fixture::new(b"{}");
    f.set_token(b"fixture-token");
    fs::write(f.root.join("apply-fail"), b"1").unwrap();
    let out = f.run(&["on"]);
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(fs::read(f.backup()).unwrap(), b"{}");
    assert_eq!(f.applied().len(), 1);
    assert!(String::from_utf8_lossy(&out.stderr).contains("rollback"));
}

#[test]
fn config_drift_between_preflight_and_mutation_preserves_new_services() {
    let f = Fixture::new(b"{}");
    f.set_token(b"fixture-token");
    fs::write(
        f.root.join("drift.json"),
        br#"{"Services":{"svc:added":{"Tun":true}}}"#,
    )
    .unwrap();
    assert_eq!(f.run(&["on"]).status.code(), Some(1));
    assert!(f.applied().is_empty());
    assert_eq!(f.config()["Services"]["svc:added"], json!({"Tun":true}));
}

#[test]
fn missing_https_capability_foreground_and_failed_health_do_not_create_token() {
    for node in [
        br#"{"Foreground":{"owned":{"TCP":{"443":{"HTTPS":true}}}}}"#.as_slice(),
        b"{}",
    ] {
        let f = Fixture::new(node);
        if node == b"{}" {
            fs::write(f.root.join("status.json"),br#"{"BackendState":"Running","Self":{"DNSName":"node.fixture.ts.net."},"CertDomains":[]}"#).unwrap();
        }
        let before = f.snapshot();
        assert_eq!(f.run(&["on"]).status.code(), Some(1));
        assert_eq!(f.snapshot(), before);
        assert!(f.applied().is_empty());
    }
    let f = Fixture::new(b"{}");
    fs::write(f.root.join("front-off"), b"1").unwrap();
    let before = f.snapshot();
    assert_eq!(f.run(&["on"]).status.code(), Some(1));
    assert_eq!(f.snapshot(), before);
    assert!(f.applied().is_empty());
}

#[test]
fn token_and_backup_symlinks_are_rejected_without_touching_the_target() {
    let f = Fixture::new(b"{}");
    fs::create_dir_all(f.token().parent().unwrap()).unwrap();
    let outside = f.root.join("outside");
    fs::write(&outside, b"keep bytes").unwrap();
    symlink(&outside, f.token()).unwrap();
    let before = f.snapshot();
    assert_eq!(f.run(&["on"]).status.code(), Some(1));
    assert_eq!(f.snapshot(), before);
    assert_eq!(fs::read(&outside).unwrap(), b"keep bytes");
    assert!(f.applied().is_empty());
    fs::remove_file(f.token()).unwrap();
    f.set_token(b"fixture-token");
    let share = f.home.join(".local/share/comandos");
    fs::create_dir_all(&share).unwrap();
    symlink(f.root.join("tmp"), share.join("backups")).unwrap();
    let before = f.snapshot();
    assert_eq!(f.run(&["on"]).status.code(), Some(1));
    assert_eq!(f.snapshot(), before);
    assert!(f.applied().is_empty());
}

#[test]
fn future_and_sealed_without_database_fail_before_token_or_serve_mutation() {
    let f = Fixture::new(b"{}");
    let dbpath = f.home.join(".local/share/comandos/comandos.sqlite3");
    let db = comandos_store::unified::open_unified(&dbpath).unwrap();
    db.execute_batch("PRAGMA user_version=999999").unwrap();
    let before = f.snapshot();
    assert_eq!(f.run(&["on"]).status.code(), Some(1));
    assert_eq!(f.snapshot(), before);
    assert!(f.applied().is_empty());
    drop(db);
    let f = Fixture::new(b"{}");
    let dbpath = f.home.join(".local/share/comandos/comandos.sqlite3");
    fs::create_dir_all(dbpath.parent().unwrap()).unwrap();
    let guard = comandos_store::unified::seal_guard_path(&dbpath, "session-status").unwrap();
    fs::write(guard, b"comandos-state-protocol-2:sealed\n").unwrap();
    let before = f.snapshot();
    assert_eq!(f.run(&["on"]).status.code(), Some(1));
    assert_eq!(f.snapshot(), before);
    assert!(f.applied().is_empty());
}

#[test]
fn qr_missing_or_failed_is_optional_and_status_and_off_do_not_generate_token() {
    let f = Fixture::new(b"{}");
    fs::remove_file(f.root.join("bin/qrencode")).unwrap();
    ok(&f.run(&["on"]));
    assert_eq!(f.token().metadata().unwrap().mode() & 0o777, 0o600);
    assert!(!fs::read(f.token()).unwrap().is_empty());
    assert!(!f.calls().iter().any(|c| c["name"] == "qrencode"));
    let f = Fixture::new(b"{}");
    fs::write(f.root.join("qr-fail"), b"1").unwrap();
    ok(&f.run(&["on"]));
    let f = Fixture::new(b"{}");
    let before = f.snapshot();
    ok(&f.run(&["status"]));
    ok(&f.run(&["off"]));
    assert_eq!(f.snapshot(), before);
    assert!(f.applied().is_empty());
}

#[test]
fn malformed_argv_and_noncanonical_port_are_readonly_usage_errors() {
    let f = Fixture::new(b"{}");
    let before = f.snapshot();
    for args in [
        vec!["unknown"],
        vec!["on", "off"],
        vec!["on", "--dry-run", "--dry-run"],
        vec!["rollback", "relative.json"],
    ] {
        assert_eq!(f.run(&args).status.code(), Some(2));
    }
    let out = f
        .command(false, &["on"])
        .env("CC_PORT", "4779")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(f.snapshot(), before);
    assert!(f.applied().is_empty());
}

#[test]
fn actual_bash_pairing_keeps_url_qr_and_dashboard_message_with_one_front_difference() {
    let rust = Fixture::new(b"{}");
    let bash = Fixture::new(b"{}");
    for f in [&rust, &bash] {
        f.set_token(b"fixture-token");
    }
    let native = rust.run(&["on"]);
    let legacy = bash.oracle(&["on"]);
    ok(&native);
    ok(&legacy);
    for text in [
        "https://node.fixture.ts.net/?token=fixture-token",
        "Tablero expuesto a TU tailnet (cifrado, privado, con TLS).",
        "Para dejar de exponerlo:  cc-mobile off",
    ] {
        assert!(String::from_utf8_lossy(&native.stdout).contains(text));
        assert!(String::from_utf8_lossy(&legacy.stdout).contains(text));
    }
    let qr = |f: &Fixture| {
        f.calls()
            .into_iter()
            .find(|c| c["name"] == "qrencode")
            .unwrap()["argv"]
            .clone()
    };
    assert_eq!(qr(&rust), qr(&bash));
    assert!(bash.calls().iter().any(|c| c["argv"]
        == json!([
            "serve",
            "--bg",
            "--https=443",
            "--set-path=/term",
            "http://127.0.0.1:4780/term"
        ])));
    assert!(bash.calls().iter().any(|c|c["argv"]==json!(["serve","--bg","--https=8443","http://127.0.0.1:4779"])));
    assert!(rust.calls().iter().filter(|c|c["name"]=="curl").all(|c|c["argv"].as_array().unwrap().last().unwrap()=="http://127.0.0.1:4777/prefs"));
    assert_eq!(
        fs::read(rust.token()).unwrap(),
        fs::read(bash.token()).unwrap()
    );
    evidence("pairing", &rust, &native, &bash, &legacy);
}

#[test]
fn actual_bash_status_common_sections_and_missing_dependency_are_byte_exact() {
    let rust = Fixture::new(b"{}");
    let bash = Fixture::new(b"{}");
    let native = rust.run(&["status"]);
    let legacy = bash.oracle(&["status"]);
    ok(&native);
    ok(&legacy);
    let sections = |o: &Output| {
        String::from_utf8_lossy(&o.stdout)
            .lines()
            .take(5)
            .collect::<Vec<_>>()
            .join("\n")
    };
    assert_eq!(sections(&native), sections(&legacy));
    evidence("status", &rust, &native, &bash, &legacy);
    for f in [&rust, &bash] {
        fs::remove_file(f.root.join("bin/tailscale")).unwrap();
    }
    let native = rust.run(&["on"]);
    let legacy = bash.oracle(&["on"]);
    assert_eq!(native.status.code(), legacy.status.code());
    assert_eq!(native.stdout, legacy.stdout);
    assert_eq!(native.stderr, legacy.stderr);
    evidence("missing-tailscale", &rust, &native, &bash, &legacy);
}

#[test]
fn percent_encoded_token_is_one_qr_argument_and_cannot_inject_a_command() {
    let f = Fixture::new(b"{}");
    f.set_token(" token with &?\"' /雪 ".as_bytes());
    ok(&f.run(&["on"]));
    let qr = f
        .calls()
        .into_iter()
        .find(|c| c["name"] == "qrencode")
        .unwrap();
    let u = url::Url::parse(qr["argv"][2].as_str().unwrap()).unwrap();
    assert_eq!(
        u.query_pairs().collect::<Vec<_>>(),
        vec![("token".into(), "token with &?\"' /雪".into())]
    );
    assert_eq!(
        f.calls().iter().filter(|c| c["name"] == "qrencode").count(),
        1
    );
}

#[test]
fn valid_sealed_database_remains_unchanged_while_configuration_token_is_reused() {
    let f = Fixture::new(b"{}");
    f.set_token(b"existing fixture token");
    let dbpath = f.home.join(".local/share/comandos/comandos.sqlite3");
    let db = comandos_store::unified::open_unified(&dbpath).unwrap();
    comandos_store::unified::set_mode(
        &db,
        "session-status",
        comandos_store::unified::Mode::Sealed,
        "fixture",
        1,
    )
    .unwrap();
    let controls = |f: &Fixture| {
        f.snapshot()
            .into_iter()
            .filter(|(p, _)| p.to_string_lossy().contains("comandos.sqlite3"))
            .collect::<Snapshot>()
    };
    let before = controls(&f);
    let token = fs::read(f.token()).unwrap();
    ok(&f.run(&["on"]));
    assert_eq!(controls(&f), before);
    assert_eq!(fs::read(f.token()).unwrap(), token);
    drop(db);
}

#[test]
fn altered_backup_or_manifest_and_overflow_fail_without_additional_mutation() {
    let f = Fixture::new(b"{}");
    f.set_token(b"fixture-token");
    ok(&f.run(&["on"]));
    let path = f.backup();
    fs::write(&path, b"{\"TCP\":{}}").unwrap();
    let before = f.snapshot();
    let count = f.applied().len();
    assert_eq!(
        f.run(&["rollback", path.to_str().unwrap()]).status.code(),
        Some(1)
    );
    assert_eq!(f.snapshot(), before);
    assert_eq!(f.applied().len(), count);
    let f = Fixture::new(b"{}");
    fs::write(f.root.join("overflow"), b"1").unwrap();
    let before = f.snapshot();
    assert_eq!(f.run(&["on"]).status.code(), Some(1));
    assert_eq!(f.snapshot(), before);
}

#[test]
fn terminal_foreign_mount_direct_tcp_and_unknown_nested_fields_are_preserved_by_refusal() {
    for config in [
        br#"{"TCP":{"8443":{"TCPForward":"127.0.0.1:4779"}}}"#.as_slice(),
        br#"{"Web":{"node.fixture.ts.net:443":{"Handlers":{"/term":{"Proxy":"http://127.0.0.1:6000"}}}}}"#,
        br#"{"Services":{"svc:other":{"Future":true}}}"#,
        br#"{"Web":{"node.fixture.ts.net:443":{"Handlers":{"/other":{"Proxy":"http://127.0.0.1:6000","Future":true}}}}}"#,
    ] {
        let f=Fixture::new(config);let before=f.snapshot();
        assert_eq!(f.run(&["on"]).status.code(),Some(1));
        assert_eq!(f.snapshot(),before);assert!(f.applied().is_empty());
        assert_eq!(fs::read(f.root.join("node.json")).unwrap(),config);
    }
}

#[test]
fn unknown_rollback_manifest_version_and_fields_cannot_authorize_mutation() {
    for change in [json!({"version":2}), json!({"Future":true})] {
        let f = Fixture::new(b"{}");
        f.set_token(b"fixture-token");
        ok(&f.run(&["on"]));
        let path = f.backup();
        let manifest = path.parent().unwrap().join("mobile.json");
        let mut value: Value = serde_json::from_slice(&fs::read(&manifest).unwrap()).unwrap();
        value
            .as_object_mut()
            .unwrap()
            .extend(change.as_object().unwrap().clone());
        fs::write(manifest, serde_json::to_vec(&value).unwrap()).unwrap();
        let before = f.snapshot();
        let count = f.applied().len();
        assert_eq!(
            f.run(&["rollback", path.to_str().unwrap()]).status.code(),
            Some(1)
        );
        assert_eq!(f.snapshot(), before);
        assert_eq!(f.applied().len(), count);
    }
}
