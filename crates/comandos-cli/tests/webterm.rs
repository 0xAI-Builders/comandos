use comandos_cli::{
    webterm::{self, Options, Probe, Response, Runner},
    webterm_attach,
};
use std::{
    cell::RefCell,
    io,
    os::unix::{fs::PermissionsExt, process::ExitStatusExt},
    path::PathBuf,
    process::{ExitStatus, Output},
};
struct Home(PathBuf);
impl Home {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!(
            "a4-cli-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(p.join(".claude/hooks/dash")).unwrap();
        std::fs::write(p.join(".claude/hooks/dash-token"), b" t0k\n").unwrap();
        std::fs::write(p.join(".claude/hooks/dash/term.html"), b"html").unwrap();
        Self(p)
    }
}
impl Drop for Home {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
#[derive(Default)]
struct Fake {
    calls: RefCell<Vec<(String, Vec<String>)>>,
    detached: bool,
}
impl Runner for Fake {
    fn run(&self, p: &str, a: &[String]) -> io::Result<Output> {
        self.calls.borrow_mut().push((p.into(), a.to_vec()));
        Ok(Output {
            status: ExitStatus::from_raw(0),
            stdout: match (p, a.first().map(String::as_str)) {
                ("ttyd", _) => b"--writable --index".to_vec(),
                ("tmux", Some("-V")) => b"tmux 3.3a".to_vec(),
                _ => vec![],
            },
            stderr: vec![],
        })
    }
    fn exec(&self, p: &str, a: &[String]) -> io::Error {
        self.calls
            .borrow_mut()
            .push((format!("exec {p}"), a.to_vec()));
        io::Error::other("fake exec")
    }
    fn available(&self, p: &str) -> bool {
        p != "systemd-run" || !self.detached
    }
    fn detach(&self, p: &str, a: &[String]) -> io::Result<u32> {
        self.calls.borrow_mut().push((p.into(), a.to_vec()));
        Ok(12345)
    }
}
struct Front {
    mode: Option<&'static str>,
    ports: Vec<u16>,
}
impl Probe for Front {
    fn get(&self, p: u16, _: &str) -> Option<Response> {
        if p != 4777 {
            return None;
        }
        self.mode.map(|mode| Response {
            body: r#"{"token":""}"#.into(),
            mode: Some(mode.into()),
            ports: self.ports.clone(),
        })
    }
    fn wait(&self, _: u16, _: &str) -> bool {
        true
    }
}
#[test]
fn ttyd_claims_only_path_native_claims_both_and_unclaimed_path_falls_back() {
    for (mode, ports, want) in [
        (Some("ttyd"), vec![4780], vec!["--unit=cc-webterm"]),
        (Some("native"), vec![4780, 4779], vec![]),
        (
            None,
            vec![],
            vec!["--unit=cc-webterm", "--unit=cc-webterm-path"],
        ),
        (
            Some("ttyd"),
            vec![],
            vec!["--unit=cc-webterm", "--unit=cc-webterm-path"],
        ),
    ] {
        let h = Home::new();
        let opts = Options::for_home(&h.0);
        let f = Fake::default();
        let mut out = vec![];
        assert_eq!(
            webterm::run_with(&[], &opts, &f, &Front { mode, ports }, &mut out).unwrap(),
            0
        );
        let calls = f.calls.borrow();
        let commands: Vec<_> = calls.iter().filter(|(p, _)| p == "systemd-run").collect();
        let units: Vec<_> = commands
            .iter()
            .map(|(_, a)| {
                a.iter()
                    .find(|a| a.starts_with("--unit="))
                    .unwrap()
                    .as_str()
            })
            .collect();
        assert_eq!(units, want);
        for (_, args) in commands {
            assert!(args.contains(&"fontSize=11".into()));
            assert!(args.contains(&"rendererType=canvas".into()));
            assert!(args.contains(&"-W".into()));
            assert!(args.contains(&"--url-arg".into()));
            if args.contains(&"--unit=cc-webterm-path".into()) {
                assert!(args.contains(&"-I".into()));
            }
        }
        assert_eq!(
            std::fs::metadata(h.0.join(".claude/hooks/webterm-enabled"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }
}
#[test]
fn off_only_stops_named_units_and_removes_enabled() {
    let h = Home::new();
    let opts = Options::for_home(&h.0);
    std::fs::write(h.0.join(".claude/hooks/webterm-enabled"), b"").unwrap();
    let f = Fake::default();
    assert_eq!(
        webterm::run_with(
            &["off".into()],
            &opts,
            &f,
            &Front {
                mode: None,
                ports: vec![]
            },
            &mut vec![]
        )
        .unwrap(),
        0
    );
    assert!(!h.0.join(".claude/hooks/webterm-enabled").exists());
    assert_eq!(
        f.calls.borrow()[0],
        (
            "systemctl".into(),
            vec![
                "--user".into(),
                "stop".into(),
                "cc-webterm.service".into(),
                "cc-webterm-path.service".into()
            ]
        )
    );
}
#[test]
fn attach_denies_before_commands_and_execs_exact_target() {
    let h = Home::new();
    let f = Fake::default();
    let mut out = vec![];
    assert_eq!(
        webterm_attach::run_with(
            &h.0,
            &["bad".into()],
            &f,
            &mut &b""[..],
            &mut out,
            "/bin/sh"
        )
        .unwrap(),
        1
    );
    assert_eq!(out, b"Acceso denegado al terminal de ComandOS.\n");
    assert!(f.calls.borrow().is_empty());
    assert!(
        webterm_attach::run_with(
            &h.0,
            &["t0k".into(), "t1".into()],
            &f,
            &mut &b""[..],
            &mut vec![],
            "/bin/sh"
        )
        .is_err()
    );
    assert_eq!(
        f.calls.borrow().last().unwrap(),
        &(
            "exec tmux".into(),
            vec![
                "attach".into(),
                "-f".into(),
                "active-pane".into(),
                "-t".into(),
                "=t1".into()
            ]
        )
    );
}
#[test]
fn aliases_resolve_and_detach_records_only_owned_children() {
    let h = Home::new();
    let f = Fake {
        detached: true,
        ..Fake::default()
    };
    assert_eq!(
        webterm::run_with(
            &[],
            &Options::for_home(&h.0),
            &f,
            &Front {
                mode: None,
                ports: vec![]
            },
            &mut vec![]
        )
        .unwrap(),
        0
    );
    assert_eq!(
        f.calls
            .borrow()
            .iter()
            .filter(|(p, _)| p == "setsid")
            .count(),
        2
    );
    assert!(
        h.0.join(".claude/hooks/cc-webterm-rust-pids.json")
            .is_file()
    );
    assert!(matches!(
        comandos_cli::dispatch::resolve("cc-webterm", &[]),
        comandos_cli::dispatch::Command::Webterm(_)
    ));
    assert!(matches!(
        comandos_cli::dispatch::resolve("cc-webterm-attach", &[]),
        comandos_cli::dispatch::Command::WebtermAttach(_)
    ));
}
#[test]
fn real_http_probe_reads_exact_listener_claims() {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let task = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut b = Vec::new();
        loop {
            let mut chunk = [0; 1024];
            let n = stream.read(&mut chunk).unwrap();
            assert!(n > 0);
            b.extend_from_slice(&chunk[..n]);
            if b.windows(4).any(|w| w == b"\r\n\r\n") {
                break;
            }
        }
        assert!(String::from_utf8_lossy(&b).starts_with("GET /term/token HTTP/1.1"));
        stream.write_all(b"HTTP/1.1 200 OK\r\nX-Comandos-Term: native\r\nX-Comandos-Term-Compat: 4780,4779\r\nContent-Length: 12\r\nConnection: close\r\n\r\n{\"token\":\"\"}").unwrap();
    });
    let response = webterm::HttpProbe.get(port, "/term/token").unwrap();
    assert_eq!(response.mode.as_deref(), Some("native"));
    assert_eq!(response.ports, [4780, 4779]);
    assert_eq!(response.body, r#"{"token":""}"#);
    task.join().unwrap();
}

#[test]
fn persisted_native_mode_avoids_restart_race_when_front_probe_is_down() {
    let home = Home::new();
    std::fs::write(
        home.0.join(".claude/hooks/webterm-mode.json"),
        br#"{"mode":"native","ports":[4780,4779]}"#,
    )
    .unwrap();
    let runner = Fake::default();
    assert_eq!(
        webterm::run_with(
            &[],
            &Options::for_home(&home.0),
            &runner,
            &Front {
                mode: None,
                ports: vec![]
            },
            &mut vec![]
        )
        .unwrap(),
        0
    );
    assert!(
        !runner
            .calls
            .borrow()
            .iter()
            .any(|(p, _)| p == "systemd-run")
    );
    assert!(home.0.join(".claude/hooks/webterm-enabled").is_file());
}
