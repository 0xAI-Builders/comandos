//! Real pane ancestry and live per-pane gates against one private shared upstream.
//! The only tmux server this fixture starts or stops uses its own explicit -S socket.
use serde_json::{Value, json};
use std::{
    fs,
    io::{BufRead, BufReader, Write},
    os::unix::{
        fs::PermissionsExt,
        net::{UnixListener, UnixStream},
    },
    path::PathBuf,
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};

const BIN: &str = env!("CARGO_BIN_EXE_comandos-extensions");
const FAKE: &str = env!("CARGO_BIN_EXE_fake_mcp_stdio");
const WAIT: Duration = Duration::from_secs(10);

fn quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

struct Fixture {
    home: PathBuf,
    daemon: Child,
}

impl Fixture {
    fn start() -> Self {
        let home = std::env::temp_dir().join(format!("cx-live-gate-{}", std::process::id()));
        fs::create_dir_all(home.join(".config/comandos/extensions")).unwrap();
        fs::create_dir_all(home.join("run")).unwrap();
        fs::set_permissions(&home, fs::Permissions::from_mode(0o700)).unwrap();
        fs::set_permissions(home.join("run"), fs::Permissions::from_mode(0o700)).unwrap();
        fs::write(
            home.join(".config/comandos/extensions/catalog.json"),
            json!({"version":1,"servers":{"fixture":{"command":FAKE}}}).to_string(),
        )
        .unwrap();
        let log = fs::File::create(home.join("broker.log")).unwrap();
        log.set_permissions(fs::Permissions::from_mode(0o600))
            .unwrap();
        let daemon = Command::new(BIN)
            .arg("broker")
            .env_clear()
            .env("HOME", &home)
            .env("PATH", "/usr/bin:/bin")
            .env("XDG_RUNTIME_DIR", home.join("run"))
            .env("COMANDOS_BROKER_IDLE_SECS", "30")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(log)
            .spawn()
            .unwrap();
        let fixture = Self { home, daemon };
        let deadline = Instant::now() + WAIT;
        while !fixture.home.join("run/comandos/broker-v2.sock").exists() {
            assert!(Instant::now() < deadline, "private broker did not start");
            thread::sleep(Duration::from_millis(10));
        }
        fixture
    }

    fn tmux(&self) -> Command {
        let mut cmd = Command::new("tmux");
        cmd.arg("-S")
            .arg(self.home.join("tmux.sock"))
            .env_remove("TMUX")
            .env_remove("TMUX_PANE")
            .env("HOME", &self.home);
        cmd
    }

    fn pane_client(&self, name: &str) -> (Client, String) {
        let control = self.home.join(format!("{name}.sock"));
        let listener = UnixListener::bind(&control).unwrap();
        listener.set_nonblocking(true).unwrap();
        let exe = std::env::current_exe().unwrap();
        // A final shell command keeps the real pane shell as an ancestor. The helper's
        // extension child deliberately omits TMUX, as Codex MCP environments do.
        let script = format!(
            "umask 077; env COMANDOS_GATE_FIXTURE_HOME={} COMANDOS_GATE_CONTROL={} {} --exact gate_pane_helper --ignored --nocapture >{} 2>&1; exit $?",
            quote(self.home.to_str().unwrap()),
            quote(control.to_str().unwrap()),
            quote(exe.to_str().unwrap()),
            quote(self.home.join(format!("{name}.log")).to_str().unwrap())
        );
        let output = self
            .tmux()
            .args([
                "-f",
                "/dev/null",
                "new-session",
                "-d",
                "-P",
                "-F",
                "#{pane_id}",
                "-s",
                name,
            ])
            .arg(script)
            .output()
            .unwrap();
        assert!(output.status.success(), "private tmux pane did not start");
        let pane = String::from_utf8(output.stdout).unwrap().trim().to_owned();
        let deadline = Instant::now() + WAIT;
        let stream = loop {
            match listener.accept() {
                Ok((stream, _)) => break stream,
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    assert!(
                        Instant::now() < deadline,
                        "private pane client did not connect"
                    );
                    thread::sleep(Duration::from_millis(10));
                }
                Err(_) => panic!("private control socket failed"),
            }
        };
        stream.set_read_timeout(Some(WAIT)).unwrap();
        let output = self
            .tmux()
            .args([
                "display-message",
                "-p",
                "-t",
                &pane,
                "#{socket_path}|#{pid}|#{session_id}|#{pane_id}|#{pane_pid}",
            ])
            .output()
            .unwrap();
        assert!(output.status.success());
        let line = String::from_utf8(output.stdout).unwrap();
        let fields: Vec<_> = line.trim().split('|').collect();
        assert_eq!(fields.len(), 5);
        let stat = fs::read_to_string(format!("/proc/{}/stat", fields[1])).unwrap();
        let start = stat
            .rsplit_once(')')
            .unwrap()
            .1
            .split_whitespace()
            .nth(19)
            .unwrap();
        let identity = format!(
            "{}|{}|{}|{}|{}|{}",
            fields[0], fields[1], start, fields[2], fields[3], fields[4]
        );
        (
            Client {
                reader: BufReader::new(stream),
                child: None,
            },
            identity,
        )
    }

    // This proxy is a child of the test process, outside both private pane trees.
    fn bound_client(&self, binding: &str) -> Client {
        let mut child = Command::new(BIN)
            .args(["serve", "fixture"])
            .env_clear()
            .env("HOME", &self.home)
            .env("PATH", "/usr/bin:/bin")
            .env("XDG_RUNTIME_DIR", self.home.join("run"))
            .env("COMANDOS_MCP_SESSION_BINDING", binding)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let mut input = child.stdin.take().unwrap();
        let output = child.stdout.take().unwrap();
        let (stream, bridge) = UnixStream::pair().unwrap();
        stream.set_read_timeout(Some(WAIT)).unwrap();
        let mut reverse = bridge.try_clone().unwrap();
        thread::spawn(move || {
            for line in BufReader::new(bridge).lines().map_while(Result::ok) {
                if writeln!(input, "{line}")
                    .and_then(|()| input.flush())
                    .is_err()
                {
                    break;
                }
            }
        });
        thread::spawn(move || {
            for line in BufReader::new(output).lines().map_while(Result::ok) {
                if writeln!(reverse, "{line}")
                    .and_then(|()| reverse.flush())
                    .is_err()
                {
                    break;
                }
            }
        });
        Client {
            reader: BufReader::new(stream),
            child: Some(child),
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        // Never addresses the user's default tmux socket or any production process.
        let _ = self
            .tmux()
            .arg("kill-server")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        if self.daemon.try_wait().ok().flatten().is_none() {
            let _ = nix::sys::signal::kill(
                nix::unistd::Pid::from_raw(self.daemon.id() as i32),
                nix::sys::signal::Signal::SIGTERM,
            );
            let deadline = Instant::now() + WAIT;
            while self.daemon.try_wait().ok().flatten().is_none() && Instant::now() < deadline {
                thread::sleep(Duration::from_millis(20));
            }
            if self.daemon.try_wait().ok().flatten().is_none() {
                let _ = self.daemon.kill();
            }
            let _ = self.daemon.wait();
        }
        if thread::panicking() {
            eprintln!("private gate fixture retained at {}", self.home.display());
        } else {
            let _ = fs::remove_dir_all(&self.home);
        }
    }
}

struct Client {
    reader: BufReader<UnixStream>,
    child: Option<Child>,
}
impl Drop for Client {
    fn drop(&mut self) {
        let _ = self.reader.get_mut().shutdown(std::net::Shutdown::Both);
        if let Some(child) = &mut self.child {
            let deadline = Instant::now() + Duration::from_secs(2);
            while child.try_wait().ok().flatten().is_none() && Instant::now() < deadline {
                thread::sleep(Duration::from_millis(10));
            }
            if child.try_wait().ok().flatten().is_none() {
                let _ = child.kill();
            }
            let _ = child.wait();
        }
    }
}
impl Client {
    fn send(&mut self, id: u64, method: &str, params: Value) {
        writeln!(
            self.reader.get_mut(),
            "{}",
            json!({"jsonrpc":"2.0","id":id,"method":method,"params":params})
        )
        .unwrap();
    }
    fn receive(&mut self) -> Value {
        self.receive_for("interleaved call")
    }
    fn receive_for(&mut self, context: &str) -> Value {
        let mut line = String::new();
        self.reader
            .read_line(&mut line)
            .unwrap_or_else(|error| panic!("private client response ({context}): {error}"));
        serde_json::from_str(&line).unwrap_or_else(|_| panic!("MCP JSON response ({context})"))
    }
    fn request(&mut self, id: u64, method: &str, params: Value) -> Value {
        self.send(id, method, params);
        let response = self.receive_for(&format!("id={id}, method={method}"));
        assert_eq!(response["id"], id);
        response
    }
}

/// Invoked only inside the private tmux pane by the parent test.
#[test]
#[ignore = "private tmux child helper"]
fn gate_pane_helper() {
    let home = PathBuf::from(
        std::env::var_os("COMANDOS_GATE_FIXTURE_HOME").expect("private fixture home"),
    );
    let control =
        PathBuf::from(std::env::var_os("COMANDOS_GATE_CONTROL").expect("private fixture control"));
    assert!(
        home.file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("cx-live-gate-")
    );
    assert!(control.starts_with(&home));
    assert!(
        std::env::var_os("TMUX").is_some(),
        "helper must have real private pane ancestry"
    );
    let child_log = fs::File::create(control.with_extension("client.log")).unwrap();
    child_log
        .set_permissions(fs::Permissions::from_mode(0o600))
        .unwrap();
    let mut child = Command::new(BIN)
        .args(["serve", "fixture"])
        .env_remove("TMUX")
        .env_remove("TMUX_PANE")
        .env("HOME", &home)
        .env("XDG_RUNTIME_DIR", home.join("run"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(child_log)
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    let output = child.stdout.take().unwrap();
    let control = UnixStream::connect(control).unwrap();
    let mut reverse = control.try_clone().unwrap();
    let writer = thread::spawn(move || {
        for line in BufReader::new(control).lines().map_while(Result::ok) {
            eprintln!("private helper forwards one request");
            if writeln!(input, "{line}")
                .and_then(|()| input.flush())
                .is_err()
            {
                break;
            }
        }
    });
    for line in BufReader::new(output).lines().map_while(Result::ok) {
        eprintln!("private helper forwards one response");
        if writeln!(reverse, "{line}")
            .and_then(|()| reverse.flush())
            .is_err()
        {
            break;
        }
    }
    let _ = reverse.shutdown(std::net::Shutdown::Both);
    let _ = writer.join();
    let _ = child.wait();
}

#[test]
fn live_toggle_uses_real_pane_ancestry_and_keeps_other_client_and_upstream_alive() {
    let fixture = Fixture::start();
    let (mut a, identity_a) = fixture.pane_client("gate-a");
    let (mut b, identity_b) = fixture.pane_client("gate-b");
    assert_ne!(identity_a, identity_b);
    let init = json!({"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"gate-fixture","version":"1"}});
    let first = a.request(1, "initialize", init.clone());
    let second = b.request(1, "initialize", init);
    let pid = first["result"]["serverInfo"]["version"].clone();
    assert!(pid.is_string());
    assert_eq!(
        pid, second["result"]["serverInfo"]["version"],
        "one global process for both real panes"
    );
    // Same request IDs in two concurrent streams remain bound to their own callers.
    for id in 10..20 {
        a.send(
            id,
            "tools/call",
            json!({"name":"echo","arguments":{"owner":"a"}}),
        );
        b.send(
            id,
            "tools/call",
            json!({"name":"echo","arguments":{"owner":"b"}}),
        );
    }
    for id in 10..20 {
        let left = a.receive();
        let right = b.receive();
        assert_eq!(left["id"], id);
        assert_eq!(right["id"], id);
        assert_eq!(left["result"]["echo"]["arguments"]["owner"], "a");
        assert_eq!(right["result"]["echo"]["arguments"]["owner"], "b");
    }
    comandos_store::extension_gate::save(&fixture.home, &identity_a, &json!({"fixture":false}))
        .unwrap();
    a.send(30, "tools/call", json!({"name":"echo"}));
    b.send(30, "tools/call", json!({"name":"echo"}));
    assert_eq!(a.receive()["error"]["code"], -32003);
    assert_eq!(b.receive()["result"]["serverInfo"]["version"], pid);
    assert_eq!(
        a.request(31, "tools/list", json!({}))["result"]["tools"],
        json!([])
    );
    comandos_store::extension_gate::save(&fixture.home, &identity_a, &json!({"fixture":true}))
        .unwrap();
    assert_eq!(
        a.request(32, "tools/call", json!({"name":"echo"}))["result"]["serverInfo"]["version"],
        pid
    );
    assert_eq!(
        b.request(32, "tools/call", json!({"name":"echo"}))["result"]["serverInfo"]["version"],
        pid
    );

    let mut outside_a = fixture.bound_client(&identity_a);
    let mut outside_b = fixture.bound_client(&identity_b);
    for client in [&mut outside_a, &mut outside_b] {
        assert_eq!(
            client.request(40, "initialize", json!({}))["result"]["serverInfo"]["version"],
            pid
        );
    }
    comandos_store::extension_gate::save(&fixture.home, &identity_a, &json!({"fixture":false}))
        .unwrap();
    assert_eq!(
        outside_a.request(41, "tools/call", json!({"name":"echo"}))["error"]["code"],
        -32003,
        "explicit pane binding applies even outside the pane ancestry"
    );
    assert_eq!(
        outside_b.request(41, "tools/call", json!({"name":"echo"}))["result"]["serverInfo"]["version"],
        pid
    );
    comandos_store::extension_gate::save(&fixture.home, &identity_a, &json!({"fixture":true}))
        .unwrap();
    assert_eq!(
        outside_a.request(42, "tools/call", json!({"name":"echo"}))["result"]["serverInfo"]["version"],
        pid
    );

    for field in [1, 2, 3] {
        let mut stale: Vec<String> = identity_a.split('|').map(str::to_owned).collect();
        stale[field] = if field == 3 {
            "$999999999".into()
        } else {
            "999999999".into()
        };
        let mut denied = fixture.bound_client(&stale.join("|"));
        denied.request(50, "initialize", json!({}));
        assert_eq!(
            denied.request(51, "tools/call", json!({"name":"echo"}))["error"]["code"],
            -32003,
            "stale binding field {field} must not degrade to global access"
        );
    }
}
