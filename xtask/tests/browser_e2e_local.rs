use comandos_browser::{broker::Broker, config::BrokerConfig};
use serde_json::json;
use std::{
    net::{TcpListener, TcpStream},
    os::unix::fs::PermissionsExt,
    path::PathBuf,
    process::Command,
    sync::mpsc,
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

struct Fixture {
    home: PathBuf,
    port: u16,
    stop: Option<tokio::sync::oneshot::Sender<()>>,
    server: Option<thread::JoinHandle<()>>,
}
impl Fixture {
    fn new() -> Self {
        Self::with_env(json!({}))
    }
    fn with_env(env: serde_json::Value) -> Self {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let home = std::env::temp_dir().join(format!(
            "comandos-browser-e2e-{}-{stamp}",
            std::process::id()
        ));
        std::fs::create_dir(&home).unwrap();
        std::fs::set_permissions(&home, std::fs::Permissions::from_mode(0o700)).unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        let tools=(0..29).map(|i|json!({"name":match i {0=>"new_page".into(),1=>"list_pages".into(),2=>"take_snapshot".into(),3=>"take_screenshot".into(),_=>format!("fixture_{i}")},"inputSchema":{"type":"object"}})).collect::<Vec<_>>();
        let cfg=BrokerConfig::from_value(json!({"command":[env!("CARGO_BIN_EXE_browser-e2e-worker-fixture")],"env":env,"catalog":{"tools":tools},"state_dir":home.join("state"),"port":port,"idle_seconds":60,"tool_timeout":3,"stop_grace":0.2}),&home).unwrap();
        let (stop, rx) = tokio::sync::oneshot::channel();
        let (done, started) = mpsc::channel();
        let server = thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            let result = rt.block_on(Broker::serve(cfg, async {
                let _ = rx.await;
            }));
            let _ = done.send(());
            result.unwrap();
        });
        let deadline = Instant::now() + Duration::from_secs(5);
        while TcpStream::connect(("127.0.0.1", port)).is_err() {
            assert!(started.try_recv().is_err(), "broker exited before bind");
            assert!(Instant::now() < deadline, "private broker startup timeout");
            thread::sleep(Duration::from_millis(20));
        }
        Self {
            home,
            port,
            stop: Some(stop),
            server: Some(server),
        }
    }
    fn run(&self, args: &[&str]) -> std::process::Output {
        Command::new(env!("CARGO_BIN_EXE_xtask"))
            .args([
                "browser-e2e",
                "--host",
                "",
                "--port",
                &self.port.to_string(),
                "--status-path",
                self.home.join("state/status.json").to_str().unwrap(),
            ])
            .args(args)
            .output()
            .unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(server) = self.server.take() {
            server.join().unwrap();
        }
        std::fs::remove_dir_all(&self.home).unwrap();
    }
}

#[test]
fn eight_catalog_clients_never_create_workers() {
    let f = Fixture::new();
    let out = f.run(&["--catalog-only"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(
        text.contains("8 clients") && text.contains("29 tools") && text.contains("workers = 0"),
        "{text}"
    );
}
#[test]
fn disposable_pages_are_isolated_and_capacity_is_released() {
    let f = Fixture::new();
    let out = f.run(&[]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let text = String::from_utf8(out.stdout).unwrap();
    for expected in [
        "isolated pages",
        "PNG screenshot",
        "ERR_BUSY",
        "released capacity",
    ] {
        assert!(text.contains(expected), "{text}");
    }
}
#[test]
fn bad_usage_fails_before_connecting_or_reading_status() {
    for args in [
        vec!["--port", "0"],
        vec!["--host", "-oProxyCommand=bad"],
        vec!["--host", "", "--status-path", "relative"],
        vec!["--unknown"],
    ] {
        let out = Command::new(env!("CARGO_BIN_EXE_xtask"))
            .arg("browser-e2e")
            .args(args)
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(2));
        assert!(String::from_utf8_lossy(&out.stderr).contains("browser-e2e"));
    }
}

#[test]
fn ssh_transport_has_noninteractive_options_before_destination_and_reads_fresh_status() {
    let f = Fixture::new();
    let bin = f.home.join("bin");
    std::fs::create_dir(&bin).unwrap();
    std::os::unix::fs::symlink(
        env!("CARGO_BIN_EXE_browser-e2e-worker-fixture"),
        bin.join("ssh"),
    )
    .unwrap();
    let trace = f.home.join("ssh.jsonl");
    let out = Command::new(env!("CARGO_BIN_EXE_xtask"))
        .args([
            "browser-e2e",
            "--host",
            "fixture-host",
            "--port",
            &f.port.to_string(),
            "--catalog-only",
            "--status-path",
            f.home.join("state/status.json").to_str().unwrap(),
        ])
        .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
        .env("FIXTURE_SSH_TRACE", &trace)
        .env("FIXTURE_SSH_STATUS", f.home.join("state/status.json"))
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let calls = std::fs::read_to_string(trace)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str::<Vec<String>>(line).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        calls
            .iter()
            .filter(|args| args.iter().any(|arg| arg == "-W"))
            .count(),
        8
    );
    assert!(
        calls
            .iter()
            .any(|args| args.last().is_some_and(|arg| arg.starts_with("cat -- ")))
    );
}

#[test]
fn leaked_pages_fail_the_smoke_instead_of_reporting_success() {
    let f = Fixture::with_env(json!({"FIXTURE_LEAK_PAGES":"1"}));
    let out = f.run(&[]);
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("crosses client ownership"));
}

#[test]
fn image_content_that_is_not_png_fails_the_smoke() {
    let f = Fixture::with_env(json!({"FIXTURE_BAD_PNG":"1"}));
    let out = f.run(&[]);
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("lacks a PNG image"));
}

#[test]
fn png_signature_without_image_chunks_fails_the_smoke() {
    let f = Fixture::with_env(json!({"FIXTURE_BAD_PNG":"signature"}));
    let out = f.run(&[]);
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("lacks a PNG image"));
}
