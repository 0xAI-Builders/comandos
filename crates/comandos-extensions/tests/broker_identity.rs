//! Real broker/fake upstream checks. All homes, sockets, environments and accounts are private.
use serde_json::{Value, json};
use std::{
    fs,
    io::{BufRead, BufReader, Write},
    os::unix::{fs::PermissionsExt, net::UnixStream},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::atomic::{AtomicU64, Ordering},
    thread,
    time::{Duration, Instant},
};

const BIN: &str = env!("CARGO_BIN_EXE_comandos-extensions");
const FAKE: &str = env!("CARGO_BIN_EXE_fake_mcp_stdio");
static NEXT: AtomicU64 = AtomicU64::new(0);

struct Fixture {
    home: PathBuf,
    child: Child,
}

impl Fixture {
    fn start(extra_spec: Value) -> Self {
        let home = std::env::temp_dir().join(format!(
            "cc-mcp-id-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(home.join(".config/comandos/extensions")).unwrap();
        fs::create_dir_all(home.join("run")).unwrap();
        fs::set_permissions(&home, fs::Permissions::from_mode(0o700)).unwrap();
        fs::set_permissions(home.join("run"), fs::Permissions::from_mode(0o700)).unwrap();
        let mut spec = json!({"command":FAKE,"env":{"FAKE_MCP_DUMP_ENV":home.join("env")}});
        for (key, value) in extra_spec.as_object().unwrap() {
            spec[key] = value.clone();
        }
        spec["env"]["FAKE_MCP_DUMP_ENV"] = json!(home.join("env"));
        fs::write(
            home.join(".config/comandos/extensions/catalog.json"),
            json!({"version":1,"servers":{"fixture":spec}}).to_string(),
        )
        .unwrap();
        let log = fs::File::create(home.join("daemon.log")).unwrap();
        let child = Command::new(BIN)
            .arg("broker")
            .env_clear()
            .env("HOME", &home)
            .env("PATH", "/usr/bin:/bin")
            .env("XDG_RUNTIME_DIR", home.join("run"))
            .env("ACCOUNT_ID", "fake-global-account")
            .env("COMANDOS_BROKER_IDLE_SECS", "1")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(log)
            .spawn()
            .unwrap();
        let fixture = Self { home, child };
        let until = Instant::now() + Duration::from_secs(5);
        while !fixture.socket().exists() && Instant::now() < until {
            thread::sleep(Duration::from_millis(10));
        }
        assert!(fixture.socket().exists(), "private broker did not start");
        fixture
    }

    fn socket(&self) -> PathBuf {
        self.home.join("run/comandos/broker-v2.sock")
    }

    fn connect(&self, cwd: &Path, account: &str, global: bool) -> (BufReader<UnixStream>, u32) {
        let mut stream = UnixStream::connect(self.socket()).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let catalog = self.home.join(".config/comandos/extensions/catalog.json");
        let spec: Value = serde_json::from_slice(&fs::read(&catalog).unwrap()).unwrap();
        let attach = json!({
            "attach":"fixture", "cwd":cwd, "home":self.home, "catalog":catalog,
            "global":global, "env":spec["servers"]["fixture"]["env"],
            "environ":{"HOME":self.home,"PATH":"/usr/bin:/bin","ACCOUNT_ID":account}
        });
        writeln!(stream, "{attach}").unwrap();
        let mut reader = BufReader::new(stream);
        assert_eq!(read_json(&mut reader)["ok"], true);
        writeln!(
            reader.get_mut(),
            "{}",
            json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}})
        )
        .unwrap();
        let result = read_json(&mut reader);
        let pid = result["result"]["serverInfo"]["version"]
            .as_str()
            .unwrap()
            .parse()
            .unwrap();
        (reader, pid)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = nix::sys::signal::kill(
                nix::unistd::Pid::from_raw(self.child.id() as i32),
                nix::sys::signal::Signal::SIGTERM,
            );
            let until = Instant::now() + Duration::from_secs(8);
            while self.child.try_wait().ok().flatten().is_none() && Instant::now() < until {
                thread::sleep(Duration::from_millis(10));
            }
            if self.child.try_wait().ok().flatten().is_none() {
                let _ = self.child.kill();
            }
            let _ = self.child.wait();
        }
        let _ = fs::remove_dir_all(&self.home);
    }
}

fn read_json(reader: &mut BufReader<UnixStream>) -> Value {
    let mut line = String::new();
    reader.read_line(&mut line).unwrap();
    serde_json::from_str(&line).unwrap()
}

#[test]
fn legacy_attach_separates_inherited_accounts() {
    let fixture = Fixture::start(json!({}));
    let (_a, a) = fixture.connect(&fixture.home, "fake-account-a", false);
    let (_b, b) = fixture.connect(&fixture.home, "fake-account-b", false);
    let (_again, again) = fixture.connect(&fixture.home, "fake-account-a", false);
    assert_ne!(
        a, b,
        "different inherited accounts must never reuse an upstream"
    );
    assert_eq!(a, again, "identical effective accounts may share");
}

#[test]
fn global_attach_uses_daemon_account_and_home_across_client_projects() {
    let fixture = Fixture::start(json!({}));
    let left = fixture.home.join("left");
    let right = fixture.home.join("right");
    fs::create_dir_all(&left).unwrap();
    fs::create_dir_all(&right).unwrap();
    let (_a, a) = fixture.connect(&left, "fake-account-a", true);
    let (_b, b) = fixture.connect(&right, "fake-account-b", true);
    assert_eq!(
        a, b,
        "global clients share the configured service across projects"
    );
    let seen = fs::read_to_string(fixture.home.join("env")).unwrap();
    assert!(
        seen.lines()
            .any(|line| line == "ACCOUNT_ID=fake-global-account")
    );
    assert!(!seen.contains("fake-account-a") && !seen.contains("fake-account-b"));
    assert_eq!(
        fs::read_link(format!("/proc/{a}/cwd")).unwrap(),
        fixture.home
    );
}

#[test]
fn effective_catalog_override_can_share_legacy_accounts() {
    let fixture = Fixture::start(json!({"env":{"ACCOUNT_ID":"fake-configured-account"}}));
    let (_a, a) = fixture.connect(&fixture.home, "fake-account-a", false);
    let (_b, b) = fixture.connect(&fixture.home, "fake-account-b", false);
    assert_eq!(
        a, b,
        "the catalog override is the effective account for both clients"
    );
    let seen = fs::read_to_string(fixture.home.join("env")).unwrap();
    assert!(
        seen.lines()
            .any(|line| line == "ACCOUNT_ID=fake-configured-account")
    );
}

#[test]
fn global_catalog_env_is_expanded_in_daemon_and_relative_cwd_uses_home() {
    let fixture = Fixture::start(json!({
        "cwd":"configured",
        "env":{"ACCOUNT_ID":"${ACCOUNT_ID}:configured"}
    }));
    let configured = fixture.home.join("configured");
    fs::create_dir_all(&configured).unwrap();
    let (_client, pid) = fixture.connect(&fixture.home, "fake-session-account", true);
    let seen = fs::read_to_string(fixture.home.join("env")).unwrap();
    assert!(
        seen.lines()
            .any(|line| line == "ACCOUNT_ID=fake-global-account:configured")
    );
    assert_eq!(
        fs::read_link(format!("/proc/{pid}/cwd")).unwrap(),
        configured
    );
}
