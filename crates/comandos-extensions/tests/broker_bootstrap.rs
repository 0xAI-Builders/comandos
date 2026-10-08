//! Autostart checks with private homes/sockets and fake upstreams; never use the live broker.
use serde_json::{Value, json};
use std::{
    fs,
    io::{BufRead, BufReader, Write},
    os::unix::fs::PermissionsExt,
    path::PathBuf,
    process::{Child, ChildStdin, Command, Stdio},
    sync::{
        atomic::{AtomicU64, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};

const BIN: &str = env!("CARGO_BIN_EXE_comandos-extensions");
const FAKE: &str = env!("CARGO_BIN_EXE_fake_mcp_stdio");
const INIT: &str = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#;
static NEXT: AtomicU64 = AtomicU64::new(0);

struct Fixture {
    home: PathBuf,
}

impl Fixture {
    fn new(hang: bool, configured_account: bool) -> Self {
        let home = std::env::temp_dir().join(format!(
            "cc-boot-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(home.join(".config/comandos/extensions")).unwrap();
        fs::create_dir_all(home.join("run")).unwrap();
        fs::set_permissions(&home, fs::Permissions::from_mode(0o700)).unwrap();
        fs::set_permissions(home.join("run"), fs::Permissions::from_mode(0o700)).unwrap();
        let mut env =
            json!({"FAKE_MCP_PIDFILE":home.join("pids"),"FAKE_MCP_DUMP_ENV":home.join("env")});
        if hang {
            env["FAKE_MCP_HANG"] = json!("1");
        }
        if configured_account {
            env["ACCOUNT_ID"] = json!("fake-catalog-account");
        }
        fs::write(
            home.join(".config/comandos/extensions/catalog.json"),
            json!({
                "version":1,"servers":{"fixture":{"command":FAKE,"env":env}}
            })
            .to_string(),
        )
        .unwrap();
        Self { home }
    }

    fn client(&self, account: &str) -> Client {
        let mut child = Command::new(BIN)
            .args(["serve", "fixture"])
            .env_clear()
            .env("HOME", &self.home)
            .env("PATH", "/usr/bin:/bin")
            .env("XDG_RUNTIME_DIR", self.home.join("run"))
            .env("ACCOUNT_ID", account)
            .env("FAKE_AMBIENT_SECRET", "fake-terminal-only")
            .current_dir(&self.home)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let stdin = child.stdin.take();
        let stdout = child.stdout.take().unwrap();
        let (tx, lines) = mpsc::channel();
        let reader = thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if tx.send(line).is_err() {
                    break;
                }
            }
        });
        Client {
            child,
            stdin,
            lines,
            reader: Some(reader),
        }
    }

    fn socket(&self) -> PathBuf {
        self.home.join("run/comandos/broker-v2.sock")
    }

    fn daemon_pid(&self) -> Option<i32> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .ok()?;
        runtime.block_on(async {
            let stream = tokio::time::timeout(
                Duration::from_millis(200),
                tokio::net::UnixStream::connect(self.socket()),
            )
            .await
            .ok()?
            .ok()?;
            stream.peer_cred().ok()?.pid()
        })
    }

    fn fake_pids(&self) -> Vec<i32> {
        fs::read_to_string(self.home.join("pids"))
            .unwrap_or_default()
            .lines()
            .filter_map(|line| line.parse().ok())
            .collect()
    }

    fn owns(&self, pid: i32, executable: &str) -> bool {
        if pid <= 1 {
            return false;
        }
        let path = PathBuf::from(format!("/proc/{pid}"));
        let same_exe = fs::read_link(path.join("exe")).ok() == fs::canonicalize(executable).ok();
        let home = format!("HOME={}", self.home.display());
        same_exe
            && fs::read(path.join("environ")).is_ok_and(|env| {
                env.split(|byte| *byte == 0)
                    .any(|entry| entry == home.as_bytes())
            })
    }

    fn owned_daemons(&self) -> Vec<i32> {
        fs::read_dir("/proc")
            .unwrap()
            .flatten()
            .filter_map(|entry| {
                let pid = entry.file_name().to_str()?.parse::<i32>().ok()?;
                let args = fs::read(entry.path().join("cmdline")).ok()?;
                (args.split(|byte| *byte == 0).any(|arg| arg == b"broker") && self.owns(pid, BIN))
                    .then_some(pid)
            })
            .collect()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        // SO_PEERCRED identifies the owner of this fixture's private socket. Verify its
        // executable and HOME before signalling; never use a shared/global process list.
        if let Some(pid) = self.daemon_pid().filter(|pid| self.owns(*pid, BIN)) {
            signal(pid, nix::sys::signal::Signal::SIGTERM);
            let until = Instant::now() + Duration::from_secs(8);
            while self.owns(pid, BIN) && Instant::now() < until {
                thread::sleep(Duration::from_millis(10));
            }
            if self.owns(pid, BIN) {
                signal(pid, nix::sys::signal::Signal::SIGKILL);
            }
        }
        for pid in self.fake_pids() {
            if self.owns(pid, FAKE) {
                signal(pid, nix::sys::signal::Signal::SIGKILL);
            }
        }
        let _ = fs::remove_dir_all(&self.home);
    }
}

fn signal(pid: i32, signal: nix::sys::signal::Signal) {
    let _ = nix::sys::signal::kill(nix::unistd::Pid::from_raw(pid), signal);
}

struct Client {
    child: Child,
    stdin: Option<ChildStdin>,
    lines: mpsc::Receiver<String>,
    reader: Option<thread::JoinHandle<()>>,
}

impl Client {
    fn send(&mut self, line: &str) {
        writeln!(self.stdin.as_mut().unwrap(), "{line}").unwrap();
        self.stdin.as_mut().unwrap().flush().unwrap();
    }

    fn reply(&self) -> Value {
        let line = self
            .lines
            .recv_timeout(Duration::from_secs(10))
            .expect("private MCP reply");
        let reply: Value = serde_json::from_str(&line).expect("stdout must contain MCP JSON only");
        assert_eq!(reply["jsonrpc"], "2.0");
        reply
    }

    fn close(&mut self, timeout: Duration) {
        self.stdin.take();
        let until = Instant::now() + timeout;
        while self.child.try_wait().unwrap().is_none() && Instant::now() < until {
            thread::sleep(Duration::from_millis(10));
        }
        assert!(
            self.child
                .try_wait()
                .unwrap()
                .is_some_and(|status| status.success()),
            "client did not exit cleanly on EOF"
        );
        if let Some(reader) = self.reader.take() {
            reader.join().unwrap();
        }
        for line in self.lines.try_iter() {
            let reply: Value =
                serde_json::from_str(&line).expect("bootstrap must not print logs to stdout");
            assert_eq!(reply["jsonrpc"], "2.0");
        }
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        self.stdin.take();
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = self.child.kill();
        }
        let _ = self.child.wait();
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
    }
}

#[test]
fn autostart_two_clients_share_one_daemon_and_one_upstream() {
    let fixture = Fixture::new(false, true);
    let mut a = fixture.client("fake-session-a");
    let mut b = fixture.client("fake-session-b");
    a.send(INIT);
    b.send(INIT);
    let first = a.reply();
    let second = b.reply();
    assert_eq!(
        first["result"]["serverInfo"]["version"],
        second["result"]["serverInfo"]["version"]
    );
    assert_eq!(fixture.fake_pids().len(), 1);
    let daemon = fixture.daemon_pid().expect("private daemon peer");
    assert_eq!(fixture.owned_daemons(), vec![daemon]);
    let seen = fs::read_to_string(fixture.home.join("env")).unwrap();
    assert!(
        seen.lines()
            .any(|line| line == "ACCOUNT_ID=fake-catalog-account")
    );
    assert!(
        !seen.contains("fake-session-a")
            && !seen.contains("fake-session-b")
            && !seen.contains("FAKE_AMBIENT_SECRET")
    );
    a.close(Duration::from_secs(3));
    b.close(Duration::from_secs(3));
}

#[test]
fn autostart_does_not_inherit_an_ambient_client_account() {
    let fixture = Fixture::new(false, false);
    let mut client = fixture.client("fake-must-not-leak");
    client.send(INIT);
    client.reply();
    let seen = fs::read_to_string(fixture.home.join("env")).unwrap();
    assert!(!seen.lines().any(|line| line.starts_with("ACCOUNT_ID=") || line.starts_with("FAKE_AMBIENT_SECRET=")));
    client.close(Duration::from_secs(3));
}

fn eof_while_starting(lines: usize) {
    let fixture = Fixture::new(true, false);
    let mut client = fixture.client("fake-session-account");
    for _ in 0..lines {
        client.send(INIT);
    }
    let until = Instant::now() + Duration::from_secs(5);
    while fixture.fake_pids().is_empty() && Instant::now() < until {
        thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(
        fixture.fake_pids().len(),
        1,
        "fake must be hanging in initialize"
    );
    let before = Instant::now();
    client.close(Duration::from_secs(3));
    assert!(
        before.elapsed() < Duration::from_secs(3),
        "EOF must cancel startup promptly"
    );
}

#[test]
fn eof_during_hung_initialize_exits_within_three_seconds() {
    eof_while_starting(1);
}

#[test]
fn eof_with_full_startup_queue_exits_within_three_seconds() {
    eof_while_starting(64);
}
