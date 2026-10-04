//! Daemon del broker: los clientes finos comparten un upstream; los dedicados no.
#[allow(dead_code)] // cada prueba usa solo parte de las utilidades compartidas
mod support;
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
    support::assert_isolated(&c);
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
    let log = daemon.log();
    assert!(
        !log.contains("fake_mcp_stdio-stderr-marker"),
        "el stderr del upstream (tokens, URLs de auth) no va al journal: {log}"
    );
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

/// Daemon con entorno mínimo (`PATH=/usr/bin:/bin`), como bajo `systemd --user`.
fn start_clean_daemon(home: &Path) -> Daemon {
    let log = fs::File::create(home.join("daemon.log")).unwrap();
    let mut cmd = Command::new(BIN);
    (cmd.arg("broker").env_clear())
        .env("HOME", home)
        .env("XDG_RUNTIME_DIR", home.join("run"))
        .env("PATH", "/usr/bin:/bin")
        .env("COMANDOS_BROKER_IDLE_SECS", "600")
        .stdout(Stdio::null())
        .stderr(log);
    support::assert_isolated(&cmd);
    let child = cmd.spawn().unwrap();
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

/// Catálogo con `eco` = `fake_mcp_stdio` sin `/`, cuyo binario vive en `bin_dir`.
fn bare_command_home(name: &str, env: &str) -> (PathBuf, PathBuf, PathBuf) {
    let bin_dir = std::env::temp_dir().join(format!("comandos-bin-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&bin_dir);
    fs::create_dir_all(&bin_dir).unwrap();
    std::os::unix::fs::symlink(FAKE, bin_dir.join("fake_mcp_stdio")).unwrap();
    let home = home_with(name, r#"{"enabled":true,"command":"fake_mcp_stdio"}"#);
    let dump = home.join("dump-path");
    let server = format!(
        r#"{{"enabled":true,"command":"fake_mcp_stdio","env":{{"FAKE_MCP_DUMP_PATH":"{}"{env}}}}}"#,
        dump.display()
    );
    let catalog = home.join(".config/comandos/extensions/catalog.json");
    let text = format!(r#"{{"version":1,"servers":{{"eco":{server}}}}}"#);
    fs::write(catalog, text).unwrap();
    (home, bin_dir, dump)
}

/// Una línea `attach` cruda (con o sin `path`); devuelve la respuesta del daemon.
fn raw_attach(home: &Path, path: Option<&str>) -> String {
    raw_attach_stream(home, path).1
}

/// Como [`raw_attach`], pero conserva la conexión para hablar MCP tras `{"ok":true}`.
fn raw_attach_stream(home: &Path, path: Option<&str>) -> (std::os::unix::net::UnixStream, String) {
    let catalog = home.join(".config/comandos/extensions/catalog.json");
    let mut attach = json!({"attach":"eco","cwd":home,"env":{},"catalog":catalog,"home":home});
    attach["env"] = serde_json::from_str(&fs::read_to_string(&catalog).unwrap_or_default())
        .ok()
        .and_then(|v: Value| v["servers"]["eco"]["env"].as_object().cloned())
        .map_or(json!({}), Value::Object);
    if let Some(p) = path {
        attach["path"] = json!(p);
    }
    let mut stream =
        std::os::unix::net::UnixStream::connect(home.join("run/comandos/broker.sock")).unwrap();
    stream.set_read_timeout(Some(WAIT)).unwrap();
    writeln!(stream, "{attach}").unwrap();
    let reply = read_one(&stream);
    (stream, reply)
}

/// Lee exactamente una línea sin adelantar bytes de la siguiente (lectura de a uno).
fn read_one(stream: &std::os::unix::net::UnixStream) -> String {
    let mut line = Vec::new();
    let mut byte = [0u8; 1];
    while line.last() != Some(&b'\n') {
        match std::io::Read::read(&mut &*stream, &mut byte) {
            Ok(1) => line.push(byte[0]),
            other => panic!("conexión cerrada o error: {other:?}"),
        }
    }
    String::from_utf8(line).unwrap()
}

/// `initialize` crudo pidiendo `version`; devuelve la `protocolVersion` contestada.
fn negotiated(home: &Path, version: &str) -> Value {
    let (mut stream, ok) = raw_attach_stream(home, None);
    assert_eq!(ok.trim(), r#"{"ok":true}"#);
    let init = json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":version,"capabilities":{},"clientInfo":{"name":"t","version":"1"}}});
    writeln!(stream, "{init}").unwrap();
    let reply: Value = serde_json::from_str(&read_one(&stream)).unwrap();
    assert_eq!(reply["id"], 1);
    reply["result"]["protocolVersion"].clone()
}

#[test]
fn each_client_gets_the_protocol_version_a_direct_connection_would_negotiate() {
    let home = home_with(
        "proto",
        &format!(r#"{{"enabled":true,"command":"{FAKE}"}}"#),
    );
    let mut daemon = Daemon::start(&home, "600");
    assert_eq!(negotiated(&home, "2025-11-25"), "2025-11-25");
    assert_eq!(negotiated(&home, "2025-06-18"), "2025-06-18");
    assert_eq!(pids(&home).len(), 1, "los dos clientes comparten upstream");
    assert!(daemon.stop().success());
    drop(daemon);

    let env = r#""env":{"FAKE_MCP_MAX_PROTOCOL":"2025-06-18"}"#;
    let old = home_with(
        "proto-old",
        &format!(r#"{{"enabled":true,"command":"{FAKE}",{env}}}"#),
    );
    let mut daemon = Daemon::start(&old, "600");
    assert_eq!(negotiated(&old, "2025-11-25"), "2025-06-18");
    assert_eq!(negotiated(&old, "2024-11-05"), "2024-11-05");
    assert!(daemon.stop().success());
}

#[test]
fn attach_path_resolves_a_bare_command_and_becomes_upstream_path() {
    let (home, bin_dir, dump) = bare_command_home("pathok", "");
    let _daemon = start_clean_daemon(&home);
    let path = format!("{}:/usr/bin:/bin", bin_dir.display());
    let reply = raw_attach(&home, Some(&path));
    assert_eq!(reply.trim(), r#"{"ok":true}"#, "daemon: {}", _daemon.log());
    assert_eq!(fs::read_to_string(&dump).unwrap(), path);
    let _ = fs::remove_dir_all(&bin_dir);
}

#[test]
fn attach_without_path_cannot_resolve_a_bare_command() {
    let (home, bin_dir, _dump) = bare_command_home("pathno", "");
    let _daemon = start_clean_daemon(&home);
    let reply = raw_attach(&home, None);
    assert!(reply.contains("error"), "sin path no se resuelve: {reply}");
    let _ = fs::remove_dir_all(&bin_dir);
}

#[test]
fn spec_own_path_wins_over_attach_path() {
    let (home, bin_dir, dump) = bare_command_home("pathspec", "");
    let own = format!("{}:/usr/bin:/bin", bin_dir.display());
    let catalog = home.join(".config/comandos/extensions/catalog.json");
    let text = fs::read_to_string(&catalog).unwrap();
    let text = text.replace(
        r#""FAKE_MCP_DUMP_PATH""#,
        &format!(r#""PATH":"{own}","FAKE_MCP_DUMP_PATH""#),
    );
    fs::write(&catalog, text).unwrap();
    let _daemon = start_clean_daemon(&home);
    let reply = raw_attach(&home, Some("/nonexistent"));
    assert_eq!(reply.trim(), r#"{"ok":true}"#, "daemon: {}", _daemon.log());
    assert_eq!(fs::read_to_string(&dump).unwrap(), own);
    let _ = fs::remove_dir_all(&bin_dir);
}

/// Catálogo con dos fakes compartibles que contestan `tools/call` con `bytes` de texto.
fn huge_home(name: &str, bytes: usize) -> PathBuf {
    let server =
        format!(r#"{{"enabled":true,"command":"{FAKE}","env":{{"FAKE_MCP_HUGE":"{bytes}"}}}}"#);
    home_with(name, &server)
}

/// Respuesta con un plazo amplio: mover cientos de MiB en un binario de depuración tarda.
fn recv_slow(s: &mut Session) -> Value {
    let line = (s.lines.recv_timeout(Duration::from_secs(120))).expect("respuesta del broker");
    serde_json::from_str(&line).unwrap()
}

#[test]
fn a_twenty_mib_response_passes_intact() {
    const SIZE: usize = 20 * 1024 * 1024;
    let home = huge_home("big", SIZE);
    let mut daemon = Daemon::start(&home, "600");
    let mut a = Session::open(&home, "eco");
    a.pid(1, "initialize");
    a.request(2, "tools/call", json!({"name": "x", "arguments": {}}));
    let reply = recv_slow(&mut a);
    assert_eq!(reply["id"], 2);
    let text = reply["result"]["content"][0]["text"].as_str().unwrap();
    assert_eq!(text.len(), SIZE, "la respuesta llega entera");
    assert!(a.call(3, "ping")["result"].is_object());
    assert!(daemon.stop().success());
}

#[test]
fn an_oversized_response_errors_only_its_client_and_the_upstream_survives() {
    let home = huge_home("huge", 300 * 1024 * 1024);
    let mut daemon = Daemon::start(&home, "600");
    let (mut a, mut b) = (Session::open(&home, "eco"), Session::open(&home, "eco"));
    let pa = a.pid(1, "initialize");
    assert_eq!(b.pid(1, "initialize"), pa, "comparten upstream");
    a.request(7, "tools/call", json!({"name": "x", "arguments": {}}));
    let reply = recv_slow(&mut a);
    assert_eq!(reply["id"], 7, "el error va al dueño con su id: {reply}");
    assert_eq!(reply["error"]["code"], -32603);
    assert_eq!(reply["error"]["message"], "respuesta demasiado grande");
    assert_eq!(
        b.pid(2, "tools/list"),
        pa,
        "el otro cliente sigue con el mismo upstream"
    );
    assert!(
        a.call(8, "ping")["result"].is_object(),
        "y el dueño también"
    );
    assert!(b.lines.try_recv().is_err(), "nada llegó al otro cliente");
    let log = daemon.log();
    assert!(daemon.stop().success());
    assert!(log.contains("demasiado"), "{log}");
}

#[test]
fn sharing_key_includes_command_and_args() {
    let home = fake_home("args");
    let mut daemon = Daemon::start(&home, "600");
    let mut a = Session::open(&home, "eco");
    let pa = a.pid(1, "initialize");
    // Mismo nombre, otros argumentos: el proceso ya no sería idéntico.
    let catalog = home.join(".config/comandos/extensions/catalog.json");
    let text = fs::read_to_string(&catalog).unwrap();
    let spec = format!(r#"{{"enabled":true,"command":"{FAKE}"}}"#);
    let with_args = format!(r#"{{"enabled":true,"command":"{FAKE}","args":["otro"]}}"#);
    fs::write(&catalog, text.replace(&spec, &with_args)).unwrap();
    let mut b = Session::open(&home, "eco");
    let pb = b.pid(1, "initialize");
    assert_ne!(pa, pb, "distintos args ⇒ distinto upstream");
    // `shared` no cambia el proceso: no entra en la clave.
    let text = fs::read_to_string(&catalog).unwrap();
    let marked = with_args.replace(r#""enabled":true"#, r#""enabled":true,"shared":true"#);
    fs::write(&catalog, text.replace(&with_args, &marked)).unwrap();
    let mut c = Session::open(&home, "eco");
    assert_eq!(
        c.pid(1, "initialize"),
        pb,
        "`shared` no forma parte de la clave"
    );
    assert_eq!(pids(&home).len(), 2);
    assert!(daemon.stop().success());
}

/// Límite de descriptores (blando, duro) de un proceso, leído de `/proc/<pid>/limits`.
fn nofile(pid: u32) -> (String, String) {
    let limits = fs::read_to_string(format!("/proc/{pid}/limits")).unwrap();
    let line = limits
        .lines()
        .find(|l| l.starts_with("Max open files"))
        .unwrap();
    let mut cols = line["Max open files".len()..].split_whitespace();
    (cols.next().unwrap().into(), cols.next().unwrap().into())
}

#[test]
fn daemon_raises_its_soft_fd_limit_to_the_hard_one() {
    let home = fake_home("nofile");
    let log = fs::File::create(home.join("daemon.log")).unwrap();
    // Lanzado con el límite blando de 1024 (o menos) que da systemd por omisión.
    let mut cmd = Command::new("sh");
    cmd.args(["-c", r#"ulimit -Sn 256 && exec "$0" broker"#, BIN])
        .env("HOME", &home)
        .env("XDG_RUNTIME_DIR", home.join("run"))
        .stdout(Stdio::null())
        .stderr(log);
    support::assert_isolated(&cmd);
    let d = Daemon {
        child: cmd.spawn().unwrap(),
        home: home.clone(),
    };
    assert!(until(|| d.socket().exists()), "{}", d.log());
    let (soft, hard) = nofile(d.child.id());
    assert_eq!(
        soft,
        hard,
        "el daemon sube el límite blando al duro: {}",
        d.log()
    );
}

/// Una línea `attach` cruda con `environ` (el entorno completo de la sesión) y sin `path`.
fn raw_attach_environ(home: &Path, environ: Value) -> String {
    let catalog = home.join(".config/comandos/extensions/catalog.json");
    let spec: Value = serde_json::from_str(&fs::read_to_string(&catalog).unwrap()).unwrap();
    let env = spec["servers"]["eco"]["env"].clone();
    let attach = json!({"attach":"eco","cwd":home,"env":env,"catalog":catalog,"home":home,"environ":environ});
    let mut stream =
        std::os::unix::net::UnixStream::connect(home.join("run/comandos/broker.sock")).unwrap();
    stream.set_read_timeout(Some(WAIT)).unwrap();
    writeln!(stream, "{attach}").unwrap();
    read_one(&stream)
}

#[test]
fn attach_environ_is_the_whole_upstream_environment() {
    let (home, bin_dir, dump) = bare_command_home("environ", "");
    let env_dump = home.join("dump-env");
    let catalog = home.join(".config/comandos/extensions/catalog.json");
    let text = fs::read_to_string(&catalog).unwrap().replace(
        r#""FAKE_MCP_DUMP_PATH""#,
        &format!(
            r#""FAKE_MCP_DUMP_ENV":"{}","FAKE_MCP_DUMP_PATH""#,
            env_dump.display()
        ),
    );
    fs::write(&catalog, text).unwrap();
    let daemon = start_clean_daemon(&home);
    let path = format!("{}:/usr/bin:/bin", bin_dir.display());
    let reply = raw_attach_environ(&home, json!({"PATH": path, "SHADOW_ONLY": "1"}));
    assert_eq!(reply.trim(), r#"{"ok":true}"#, "daemon: {}", daemon.log());
    assert_eq!(
        fs::read_to_string(&dump).unwrap(),
        path,
        "el PATH del environ resuelve el comando"
    );
    let seen = fs::read_to_string(&env_dump).unwrap();
    assert!(seen.lines().any(|l| l == "SHADOW_ONLY=1"), "{seen}");
    assert!(
        !seen.contains("COMANDOS_BROKER_IDLE_SECS"),
        "el upstream no hereda el entorno del daemon: {seen}"
    );
    let _ = fs::remove_dir_all(&bin_dir);
}

#[test]
fn spec_own_path_wins_over_attach_environ() {
    let (home, bin_dir, dump) = bare_command_home("environspec", "");
    let own = format!("{}:/usr/bin:/bin", bin_dir.display());
    let catalog = home.join(".config/comandos/extensions/catalog.json");
    let text = fs::read_to_string(&catalog).unwrap().replace(
        r#""FAKE_MCP_DUMP_PATH""#,
        &format!(r#""PATH":"{own}","FAKE_MCP_DUMP_PATH""#),
    );
    fs::write(&catalog, text).unwrap();
    let daemon = start_clean_daemon(&home);
    let reply = raw_attach_environ(&home, json!({"PATH": "/nonexistent"}));
    assert_eq!(reply.trim(), r#"{"ok":true}"#, "daemon: {}", daemon.log());
    assert_eq!(fs::read_to_string(&dump).unwrap(), own);
    let _ = fs::remove_dir_all(&bin_dir);
}

#[test]
fn a_session_variable_reaches_the_shared_upstream() {
    let home = fake_home("shadow");
    let env_dump = home.join("dump-env");
    let catalog = home.join(".config/comandos/extensions/catalog.json");
    let spec = format!(r#"{{"enabled":true,"command":"{FAKE}"}}"#);
    let with_dump = format!(
        r#"{{"enabled":true,"command":"{FAKE}","env":{{"FAKE_MCP_DUMP_ENV":"{}"}}}}"#,
        env_dump.display()
    );
    let text = fs::read_to_string(&catalog)
        .unwrap()
        .replace(&spec, &with_dump);
    fs::write(&catalog, text).unwrap();
    let mut daemon = Daemon::start(&home, "600");
    let mut child = command(&home, &["serve", "eco"])
        .env("SHADOW_ONLY", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    writeln!(child.stdin.as_mut().unwrap(), "{INIT}").unwrap();
    let mut line = String::new();
    BufReader::new(child.stdout.as_mut().unwrap())
        .read_line(&mut line)
        .unwrap();
    assert!(line.contains(r#""id":1"#), "{line}");
    let seen = fs::read_to_string(&env_dump).unwrap();
    assert!(seen.lines().any(|l| l == "SHADOW_ONLY=1"), "{seen}");
    let _ = child.kill();
    let _ = child.wait();
    assert!(daemon.stop().success());
    assert!(
        daemon.log().contains("spawn eco"),
        "pasó por el broker: {}",
        daemon.log()
    );
}

/// Sesión con una petición en vuelo (el fake ignora `tools/call`) cuando el daemon muere.
/// `restart` mata el daemon (como lo haría systemd o un fallo) y arranca otro en el mismo socket.
fn survives_a_daemon_restart(tag: &str, restart: impl Fn(&mut Daemon) -> Daemon) {
    let server = format!(
        r#"{{"enabled":true,"command":"{FAKE}","env":{{"FAKE_MCP_IGNORE":"tools/call"}}}}"#
    );
    let home = home_with(tag, &server);
    let mut old = Daemon::start(&home, "600");
    let mut a = Session::open(&home, "eco");
    let first = a.pid(1, "initialize");
    a.send(json!({"jsonrpc":"2.0","method":"notifications/initialized"}));
    assert!(a.call(2, "ping")["result"].is_object());
    a.request(3, "tools/call", json!({"name": "lento", "arguments": {}}));
    thread::sleep(Duration::from_millis(100));
    let mut new = restart(&mut old);
    let lost = a.recv();
    assert_eq!(
        lost["id"], 3,
        "la petición en vuelo recibe un error: {lost}"
    );
    assert_eq!(lost["error"]["code"], -32603);
    assert_eq!(lost["error"]["message"], "broker reiniciado");
    let pong = a.call(4, "ping");
    assert!(
        pong["result"].is_object(),
        "la sesión sigue tras el reinicio: {pong}"
    );
    assert!(
        a.lines.try_recv().is_err(),
        "el cliente no ve la respuesta del initialize repetido"
    );
    assert_ne!(
        a.pid(5, "tools/list"),
        first,
        "habla con el upstream del daemon nuevo"
    );
    assert!(new.log().contains("attach eco"), "{}", new.log());
    a.close();
    assert!(new.stop().success());
}

#[test]
fn the_thin_client_reconnects_when_the_daemon_is_restarted() {
    survives_a_daemon_restart("restart-term", |old| {
        assert!(old.stop().success());
        Daemon::start(&old.home.clone(), "600")
    });
}

#[test]
fn the_thin_client_reconnects_when_the_daemon_crashes() {
    survives_a_daemon_restart("restart-kill", |old| {
        old.child.kill().unwrap();
        old.child.wait().unwrap();
        Daemon::start(&old.home.clone(), "600")
    });
}
