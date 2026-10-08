//! Shared-by-default routing applies to stdio, filtered stdio and HTTP connectors.
#[allow(dead_code)]
mod support;

use comandos_extensions::{broker, config};
use serde_json::{Value, json};

#[test]
fn global_default_includes_previously_dedicated_and_http_connectors() {
    for name in config::DEDICATED {
        assert!(
            config::is_shared(&json!({"command":"example"}), name),
            "{name}"
        );
    }
    assert!(broker::brokerable(
        &json!({"transport":"http","url":"http://127.0.0.1/mcp"}),
        "http"
    ));
    assert!(broker::brokerable(
        &json!({"command":"example","enabled_tools":["echo"]}),
        "filtered"
    ));
}

#[test]
fn explicit_dedicated_override_remains_available() {
    for spec in [
        json!({"command":"example","shared":false}),
        json!({"transport":"http","url":"http://127.0.0.1/mcp","shared":false}),
    ] {
        assert!(!config::is_shared(&spec, "example"));
        assert!(!broker::brokerable(&spec, "example"));
    }
}

mod routing {
    use super::*;
    use nix::{
        sys::signal::{Signal, kill},
        unistd::Pid,
    };
    use std::{
        fs,
        io::{BufRead, BufReader, Write},
        path::{Path, PathBuf},
        process::{Child, ChildStdin, Command, Stdio},
        sync::{Arc, Mutex, mpsc},
        thread,
        time::{Duration, Instant},
    };

    const BIN: &str = env!("CARGO_BIN_EXE_comandos-extensions");
    const WAIT: Duration = Duration::from_secs(8);

    struct Fixture {
        home: PathBuf,
        daemon: Child,
    }

    impl Fixture {
        fn command(home: &Path) -> Command {
            let mut c = Command::new(BIN);
            c.env("HOME", home)
                .env("XDG_RUNTIME_DIR", home.join("run"))
                .env("COMANDOS_BROKER_IDLE_SECS", "30");
            support::assert_isolated(&c);
            c
        }

        fn start(tag: &str, servers: Value) -> Self {
            let home = std::env::temp_dir().join(format!("cx-all-{}-{tag}", std::process::id()));
            fs::create_dir_all(home.join(".config/comandos/extensions")).unwrap();
            fs::create_dir_all(home.join("run")).unwrap();
            fs::create_dir_all(home.join("a")).unwrap();
            fs::create_dir_all(home.join("b")).unwrap();
            fs::write(
                home.join(".config/comandos/extensions/catalog.json"),
                json!({"version":1,"servers":servers}).to_string(),
            )
            .unwrap();
            let log = fs::File::create(home.join("broker.log")).unwrap();
            let daemon = Self::command(&home)
                .arg("broker")
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(log)
                .spawn()
                .unwrap();
            let fixture = Self { home, daemon };
            let deadline = Instant::now() + WAIT;
            while !fs::read_to_string(fixture.home.join("broker.log"))
                .unwrap_or_default()
                .contains("escuchando")
            {
                assert!(Instant::now() < deadline, "private broker did not start");
                thread::sleep(Duration::from_millis(20));
            }
            fixture
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = kill(Pid::from_raw(self.daemon.id() as i32), Signal::SIGTERM);
            let _ = self.daemon.wait();
            let _ = fs::remove_dir_all(&self.home);
        }
    }

    struct Session {
        child: Child,
        input: Option<ChildStdin>,
        replies: mpsc::Receiver<Value>,
    }

    impl Session {
        fn open(fixture: &Fixture, name: &str, cwd: &str) -> Self {
            // Explicit private home/catalog must route through the same private broker.
            let mut child = Fixture::command(&fixture.home)
                .args(["--home", fixture.home.to_str().unwrap(), "serve", name])
                .current_dir(fixture.home.join(cwd))
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()
                .unwrap();
            let reader = BufReader::new(child.stdout.take().unwrap());
            let (tx, replies) = mpsc::channel();
            thread::spawn(move || {
                for line in reader.lines().map_while(Result::ok) {
                    let Ok(message) = serde_json::from_str(&line) else {
                        break;
                    };
                    if tx.send(message).is_err() {
                        break;
                    }
                }
            });
            Self {
                input: child.stdin.take(),
                child,
                replies,
            }
        }

        fn request(&mut self, id: u64, method: &str, params: Value) -> Value {
            writeln!(
                self.input.as_mut().unwrap(),
                "{}",
                json!({"jsonrpc":"2.0","id":id,"method":method,"params":params})
            )
            .unwrap();
            let answer = self.replies.recv_timeout(WAIT).expect("MCP response");
            assert_eq!(answer["id"], id);
            assert!(answer.get("error").is_none(), "{answer}");
            answer
        }

        fn initialize(&mut self) -> Value {
            self.request(1, "initialize", json!({"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"test","version":"1"}}))
        }
    }

    #[test]
    fn formerly_dedicated_stdio_clients_share_one_global_process() {
        let fixture = Fixture::start(
            "stdio",
            json!({"x-suite":{"command":env!("CARGO_BIN_EXE_fake_mcp_stdio")}}),
        );
        let mut a = Session::open(&fixture, "x-suite", "a");
        let mut b = Session::open(&fixture, "x-suite", "b");
        let first = a.initialize();
        let second = b.initialize();
        assert_eq!(
            first["result"]["serverInfo"]["version"], second["result"]["serverInfo"]["version"],
            "both clients must reach the same upstream PID"
        );
    }

    impl Drop for Session {
        fn drop(&mut self) {
            self.input.take();
            let deadline = Instant::now() + WAIT;
            while self.child.try_wait().ok().flatten().is_none() && Instant::now() < deadline {
                thread::sleep(Duration::from_millis(20));
            }
            if self.child.try_wait().ok().flatten().is_none() {
                let _ = self.child.kill();
            }
            let _ = self.child.wait();
        }
    }

    #[test]
    fn http_clients_in_different_directories_share_one_http_session_and_filters() {
        let log = Arc::new(Mutex::new(Vec::new()));
        let (port, _) = support::fake_mcp_http::spawn(log.clone());
        let fixture = Fixture::start(
            "http",
            json!({"http":{"transport":"http","url":format!("http://127.0.0.1:{port}/mcp"),"enabled_tools":["echo"]}}),
        );
        let mut a = Session::open(&fixture, "http", "a");
        let mut b = Session::open(&fixture, "http", "b");
        a.initialize();
        b.initialize();
        let tools = a.request(2, "tools/list", json!({}));
        assert_eq!(tools["result"]["tools"].as_array().unwrap().len(), 1);
        assert_eq!(tools["result"]["tools"][0]["name"], "echo");
        for (client, text) in [(&mut a, "one"), (&mut b, "two")] {
            let result = client.request(
                3,
                "tools/call",
                json!({"name":"echo","arguments":{"text":text}}),
            );
            assert_eq!(result["result"]["content"][0]["text"], text);
        }
        assert_eq!(
            log.lock()
                .unwrap()
                .iter()
                .filter(|line| line.starts_with("POST initialize "))
                .count(),
            1,
            "one shared HTTP backend, independent client IDs"
        );
        drop(a);
        let response = b.request(
            4,
            "tools/call",
            json!({"name":"echo","arguments":{"text":"still alive"}}),
        );
        assert_eq!(response["result"]["content"][0]["text"], "still alive");
    }

    #[test]
    fn separate_http_catalog_accounts_keep_separate_upstream_sessions() {
        let log = Arc::new(Mutex::new(Vec::new()));
        let (port, _) = support::fake_mcp_http::spawn(log.clone());
        let url = format!("http://127.0.0.1:{port}/mcp");
        let fixture = Fixture::start(
            "accounts",
            json!({"account_a":{"transport":"http","url":url,"headers":{"X-Account":"a"}},"account_b":{"transport":"http","url":url,"headers":{"X-Account":"b"}}}),
        );
        let mut a = Session::open(&fixture, "account_a", "a");
        let mut b = Session::open(&fixture, "account_b", "b");
        a.initialize();
        b.initialize();
        assert_eq!(
            log.lock()
                .unwrap()
                .iter()
                .filter(|line| line.starts_with("POST initialize "))
                .count(),
            2,
            "distinct catalog accounts keep independent HTTP sessions"
        );
    }
}
