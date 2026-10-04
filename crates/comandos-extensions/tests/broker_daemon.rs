//! Daemon del broker: los clientes finos comparten un upstream; los dedicados no.
use nix::{sys::signal, unistd::Pid};
use serde_json::{Value, json};
use std::{
    fs,
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
    process::{Child, ChildStdin, Command, ExitStatus, Output, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

const BIN: &str = env!("CARGO_BIN_EXE_comandos-extensions");
const FAKE: &str = env!("CARGO_BIN_EXE_fake_mcp_stdio");
const WAIT: Duration = Duration::from_secs(5);
const FALLBACK: &str = "broker no disponible, proxy directo\n";
const INIT: &str = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#;

fn home_with(name: &str, server: &str) -> PathBuf {
    let home = std::env::temp_dir().join(format!("comandos-broker-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&home);
    fs::create_dir_all(home.join(".config/comandos/extensions")).unwrap();
    fs::create_dir_all(home.join("run")).unwrap();
    fs::write(
        home.join(".config/comandos/extensions/catalog.json"),
        format!(
            r#"{{"version":1,"servers":{{"eco":{server},"solo":{{"enabled":true,"shared":false,"command":"sh","args":["-c","echo $$"]}},"muere":{{"command":"{FAKE}","env":{{"FAKE_MCP_DIE":"1"}}}},"cuelga":{{"command":"{FAKE}","env":{{"FAKE_MCP_HANG":"1"}}}}}}}}"#
        ),
    )
    .unwrap();
    home
}

fn fake_home(name: &str) -> PathBuf {
    home_with(name, &format!(r#"{{"enabled":true,"command":"{FAKE}"}}"#))
}

fn until(mut done: impl FnMut() -> bool) -> bool {
    let start = Instant::now();
    while start.elapsed() < WAIT {
        if done() {
            return true;
        }
        thread::sleep(Duration::from_millis(20));
    }
    done()
}

fn alive(pid: u32) -> bool {
    fs::read_to_string(format!("/proc/{pid}/stat")).is_ok_and(|s| {
        s.rsplit(')')
            .next()
            .is_some_and(|r| !r.trim_start().starts_with('Z'))
    })
}

fn is_fake(pid: u32) -> bool {
    alive(pid)
        && fs::read(format!("/proc/{pid}/cmdline")).is_ok_and(|c| c.starts_with(FAKE.as_bytes()))
}

fn pids(home: &Path) -> Vec<u32> {
    let text = fs::read_to_string(home.join("pids")).unwrap_or_default();
    text.lines().map(|l| l.parse().unwrap()).collect()
}

fn command(home: &Path, args: &[&str]) -> Command {
    let mut c = Command::new(BIN);
    c.args(args)
        .env("HOME", home)
        .env("XDG_RUNTIME_DIR", home.join("run"));
    c.env("FAKE_MCP_PIDFILE", home.join("pids"));
    c
}

/// `serve <name>` con `input` en stdin (cerrado después); salida completa.
fn serve_once(home: &Path, name: &str, input: &str) -> Output {
    let mut c = command(home, &["serve", name]);
    let mut child = (c
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped()))
    .spawn()
    .unwrap();
    writeln!(child.stdin.take().unwrap(), "{input}").unwrap();
    child.wait_with_output().unwrap()
}

struct Daemon {
    child: Child,
    home: PathBuf,
}

impl Daemon {
    fn start(home: &Path, idle: &str) -> Self {
        let log = fs::File::create(home.join("daemon.log")).unwrap();
        let child = command(home, &["broker"])
            .env("COMANDOS_BROKER_IDLE_SECS", idle)
            .stdout(Stdio::null())
            .stderr(log)
            .spawn()
            .unwrap();
        let d = Daemon {
            child,
            home: home.into(),
        };
        assert!(
            until(|| d.socket().exists()),
            "el daemon debe crear su socket"
        );
        d
    }
    fn socket(&self) -> PathBuf {
        self.home.join("run/comandos/broker.sock")
    }
    fn log(&self) -> String {
        fs::read_to_string(self.home.join("daemon.log")).unwrap_or_default()
    }
    fn stop(&mut self) -> ExitStatus {
        let pid = Pid::from_raw(self.child.id() as i32);
        let _ = signal::kill(pid, signal::Signal::SIGTERM);
        self.child.wait().unwrap()
    }
}

impl Drop for Daemon {
    fn drop(&mut self) {
        if self.child.try_wait().ok().flatten().is_none() {
            self.stop();
        }
        // Solo fakes vivos: un pid ya muerto puede pertenecer ahora a otro proceso.
        for pid in pids(&self.home).into_iter().filter(|p| is_fake(*p)) {
            let _ = signal::kill(Pid::from_raw(pid as i32), signal::Signal::SIGKILL);
        }
        let _ = fs::remove_dir_all(&self.home);
    }
}

struct Session {
    child: Child,
    stdin: Option<ChildStdin>,
    lines: mpsc::Receiver<String>,
}

impl Session {
    fn open(home: &Path, name: &str) -> Self {
        Self::open_in(home, name, Path::new("."))
    }
    fn open_in(home: &Path, name: &str, cwd: &Path) -> Self {
        let mut child = command(home, &["serve", name])
            .current_dir(cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let out = BufReader::new(child.stdout.take().unwrap());
        let (tx, lines) = mpsc::channel();
        thread::spawn(move || {
            out.lines()
                .map_while(Result::ok)
                .try_for_each(|l| tx.send(l))
        });
        Session {
            stdin: child.stdin.take(),
            child,
            lines,
        }
    }
    fn send(&mut self, msg: Value) {
        writeln!(self.stdin.as_mut().unwrap(), "{msg}").unwrap();
    }
    fn request(&mut self, id: u64, method: &str, params: Value) {
        self.send(json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}));
    }
    fn recv(&mut self) -> Value {
        let line = self.lines.recv_timeout(WAIT).expect("respuesta del broker");
        serde_json::from_str(&line).unwrap()
    }
    fn call(&mut self, id: u64, method: &str) -> Value {
        let params = json!({"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"t","version":"1"}});
        self.request(id, method, params);
        let reply = self.recv();
        assert_eq!(reply["id"], id, "cada cliente recibe su propio id");
        reply
    }
    fn pid(&mut self, id: u64, method: &str) -> String {
        let r = self.call(id, method);
        r["result"]["serverInfo"]["version"]
            .as_str()
            .unwrap()
            .to_string()
    }
    fn closed_by_peer(&mut self) -> bool {
        let eof = matches!(
            self.lines.recv_timeout(WAIT),
            Err(mpsc::RecvTimeoutError::Disconnected)
        );
        eof && until(|| self.child.try_wait().ok().flatten().is_some())
    }
    fn close(mut self) {
        self.stdin.take();
        let _ = self.child.wait();
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[test]
fn two_clients_share_one_upstream_and_dedicated_is_not_shared() {
    let home = fake_home("share");
    let mut daemon = Daemon::start(&home, "600");
    let mut a = Session::open(&home, "eco");
    let p1 = a.pid(1, "initialize");
    a.close();
    let mut b = Session::open(&home, "eco");
    let p2 = b.pid(1, "initialize");
    b.close();
    assert_eq!(
        p1, p2,
        "ambos clientes deben hablar con el mismo proceso upstream"
    );
    let s1 = command(&home, &["serve", "solo"]).output().unwrap();
    let s2 = command(&home, &["serve", "solo"]).output().unwrap();
    assert_ne!(s1.stdout, s2.stdout, "dedicated_server_never_shared");
    assert!(daemon.stop().success());
    assert!(!daemon.socket().exists(), "el socket se borra al salir");
}

#[test]
fn serve_falls_back_to_direct_proxy_without_daemon() {
    let home = home_with(
        "nodaemon",
        r#"{"enabled":true,"command":"sh","args":["-c","echo direct"]}"#,
    );
    let out = command(&home, &["serve", "eco"]).output().unwrap();
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "direct");
    assert!(
        out.stderr.is_empty(),
        "sin broker instalado, el respaldo es silencioso"
    );
    let _ = fs::remove_dir_all(&home);
}

#[test]
fn sessions_multiplex_survive_detach_and_respawn_after_upstream_death() {
    let home = fake_home("mux");
    let mut daemon = Daemon::start(&home, "600");
    let mut a = Session::open(&home, "eco");
    let mut b = Session::open(&home, "eco");
    let pa = a.pid(1, "initialize");
    let pb = b.pid(7, "initialize");
    assert_eq!(pa, pb);
    a.send(json!({"jsonrpc":"2.0","method":"notifications/initialized"}));
    // Mismo id en las dos sesiones: el broker los traduce y devuelve a cada una el suyo.
    assert!(a.call(2, "tools/list")["result"].is_object());
    assert!(b.call(2, "tools/list")["result"].is_object());
    assert_eq!(
        pids(&home).len(),
        1,
        "un solo proceso upstream para dos sesiones"
    );
    a.close();
    assert!(
        b.call(3, "tools/list")["result"].is_object(),
        "la otra sesión sigue viva"
    );
    let old: u32 = pa.parse().unwrap();
    // Pasados los 500 ms de arranque: la muerte ya no cuenta como fallo de arranque.
    thread::sleep(Duration::from_millis(600));
    signal::kill(Pid::from_raw(old as i32), signal::Signal::SIGKILL).unwrap();
    assert!(
        b.closed_by_peer(),
        "si muere el upstream, sus clientes reciben EOF"
    );
    assert!(until(|| !alive(old)));
    let mut c = Session::open(&home, "eco");
    let pc = c.pid(1, "initialize");
    assert_ne!(pc, pa, "el siguiente attach relanza el upstream");
    assert_eq!(pids(&home).len(), 2);
    c.close();
    assert!(daemon.stop().success());
    let new: u32 = pc.parse().unwrap();
    assert!(!alive(new), "SIGTERM al daemon termina sus upstreams");
    let log = daemon.log();
    assert_eq!(log.matches("spawn eco").count(), 2, "{log}");
    assert!(
        !log.contains("tools/list"),
        "el registro nunca incluye cargas: {log}"
    );
}

#[test]
fn idle_upstream_ends_second_daemon_refuses_and_stale_socket_is_replaced() {
    let home = fake_home("idle");
    let mut daemon = Daemon::start(&home, "1");
    let mut a = Session::open(&home, "eco");
    let pid: u32 = a.pid(1, "initialize").parse().unwrap();
    let second = command(&home, &["broker"])
        .stderr(Stdio::null())
        .status()
        .unwrap();
    assert!(
        !second.success(),
        "un segundo daemon no arranca si el primero vive"
    );
    assert!(
        a.call(2, "tools/list")["result"].is_object(),
        "y el primero sigue sirviendo"
    );
    a.close();
    assert!(
        until(|| !alive(pid)),
        "sin clientes, el upstream se cierra tras la inactividad"
    );
    // Daemon muerto sin limpiar: su socket queda huérfano y el siguiente lo reemplaza.
    daemon.child.kill().unwrap();
    daemon.child.wait().unwrap();
    assert!(daemon.socket().exists());
    let mut next = Daemon::start(&home, "600");
    let mut b = Session::open(&home, "eco");
    let pb: u32 = b.pid(1, "initialize").parse().unwrap();
    assert_ne!(pb, pid);
    b.close();
    assert!(next.stop().success());
}

#[test]
fn sharing_key_includes_cwd() {
    let home = fake_home("cwd");
    let (w1, w2) = (home.join("w1"), home.join("w2"));
    fs::create_dir_all(&w1).unwrap();
    fs::create_dir_all(&w2).unwrap();
    let mut daemon = Daemon::start(&home, "600");
    let mut a = Session::open_in(&home, "eco", &w1);
    let mut b = Session::open_in(&home, "eco", &w2);
    let mut c = Session::open_in(&home, "eco", &w1);
    let (pa, pb, pc) = (
        a.pid(1, "initialize"),
        b.pid(1, "initialize"),
        c.pid(1, "initialize"),
    );
    assert_ne!(pa, pb, "distinto cwd ⇒ distinto upstream");
    assert_eq!(pa, pc, "mismo cwd y env ⇒ mismo upstream");
    assert_eq!(pids(&home).len(), 2);
    assert!(daemon.stop().success());
}

#[test]
fn upstream_dying_at_start_falls_back_and_cools_down() {
    let home = fake_home("dies");
    let mut daemon = Daemon::start(&home, "600");
    for round in 1..=2 {
        let out = serve_once(&home, "muere", "");
        assert_eq!(
            String::from_utf8_lossy(&out.stderr),
            FALLBACK,
            "ronda {round}"
        );
        assert_eq!(
            out.status.code(),
            Some(3),
            "el proxy directo ejecutó el fake"
        );
    }
    let log = daemon.log();
    assert_eq!(
        log.matches("spawn muere").count(),
        1,
        "sin relanzar en el enfriamiento: {log}"
    );
    assert!(log.contains("murió al arrancar"), "{log}");
    assert_eq!(pids(&home).len(), 3, "uno del broker y dos directos");
    assert!(daemon.stop().success());
}

#[test]
fn socket_of_dead_daemon_falls_back_with_one_line() {
    let home = fake_home("dead");
    let mut daemon = Daemon::start(&home, "600");
    daemon.child.kill().unwrap();
    daemon.child.wait().unwrap();
    let out = serve_once(&home, "eco", INIT);
    assert_eq!(String::from_utf8_lossy(&out.stderr), FALLBACK);
    let reply: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(reply["id"], 1, "respondió el proxy directo");
}

#[test]
fn hung_upstream_times_out_without_blocking_other_servers() {
    let home = fake_home("hang");
    let mut daemon = Daemon::start(&home, "600");
    let h = home.clone();
    let hung = thread::spawn(move || serve_once(&h, "cuelga", ""));
    thread::sleep(Duration::from_millis(200));
    let start = Instant::now();
    let mut a = Session::open(&home, "eco");
    a.pid(1, "initialize");
    assert!(
        start.elapsed() < Duration::from_secs(3),
        "otro servidor no espera"
    );
    let out = hung.join().unwrap();
    assert_eq!(String::from_utf8_lossy(&out.stderr), FALLBACK);
    a.close();
    assert!(daemon.stop().success());
}

#[test]
fn concurrent_requests_with_the_same_ids_stay_with_their_client() {
    let home = fake_home("conc");
    let mut daemon = Daemon::start(&home, "600");
    let (mut a, mut b) = (Session::open(&home, "eco"), Session::open(&home, "eco"));
    a.pid(1, "initialize");
    b.pid(1, "initialize");
    for id in [5, 6] {
        a.request(id, "tools/list", json!({"who": "a"}));
        b.request(id, "tools/list", json!({"who": "b"}));
    }
    for (s, who) in [(&mut a, "a"), (&mut b, "b")] {
        let ids: Vec<Value> = (0..2)
            .map(|_| {
                let r = s.recv();
                assert_eq!(r["result"]["echo"]["who"], who, "respuesta de otro cliente");
                r["id"].clone()
            })
            .collect();
        assert_eq!(ids, [json!(5), json!(6)]);
    }
    assert!(daemon.stop().success());
}
