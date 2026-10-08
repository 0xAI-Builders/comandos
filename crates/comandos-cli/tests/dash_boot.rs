//! Proceso real de `comandos dash` sobre un HOME temporal y un puerto libre.
use std::{
    fs,
    io::{BufRead, BufReader, Read, Write},
    net::{TcpListener, TcpStream},
    path::PathBuf,
    process::{Command, Stdio},
    sync::mpsc,
    time::{Duration, Instant},
};

fn temp_home(tag: &str) -> PathBuf {
    let h = std::env::temp_dir().join(format!("cmd-dash-cli-{tag}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&h);
    fs::create_dir_all(h.join(".claude/hooks/dash")).unwrap();
    h
}

fn free_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.local_addr().unwrap().port()
}

fn base(home: &PathBuf) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_comandos"));
    command
        .env("HOME", home)
        // El frente abre app-state al arrancar: nunca la base real del usuario.
        .env_remove("COMANDOS_STATE_DB")
        .env_remove("COMANDOS_USAGE_DB")
        .env_remove("COMANDOS_DASH_NATIVE")
        .env("XDG_STATE_HOME", home.join(".local/state"))
        .env_remove("COMANDOS_DASH_DIR")
        // Ni cuentas ni tmux del usuario: el refresco de límites del arranque
        // lee el HOME temporal.
        .env_remove("CLAUDE_CONFIG_DIR")
        .env_remove("CODEX_HOME")
        .env_remove("GROK_HOME")
        .env_remove("TMUX")
        .env("COMANDOS_DASH_LEGACY_PORT", "1")
        // El censo de declinaciones (D7) va a `$XDG_RUNTIME_DIR`: nunca el real.
        .env("XDG_RUNTIME_DIR", home.join("run"))
        .env_remove("COMANDOS_DASH_BACKGROUND")
        .env_remove("COMANDOS_DASH_CUTS_OFF")
        .stdin(Stdio::null());
    command
}

#[test]
fn dash_help_needs_no_home_and_creates_no_runtime_files() {
    let home = temp_home("help-no-home");
    let output = base(&home)
        .env_remove("HOME")
        .args(["dash", "--help"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&output.stdout).starts_with("uso: comandos dash"));
    assert!(output.stderr.is_empty());
    assert!(!home.join("run").exists());
    assert!(!home.join(".claude/hooks/dash-token").exists());
}

#[test]
fn cc_dash_short_help_skips_invalid_configuration_without_writes() {
    let home = temp_home("help-alias");
    let link = home.join("cc-dash");
    std::os::unix::fs::symlink(env!("CARGO_BIN_EXE_comandos"), &link).unwrap();
    let mut command = base(&home);
    // argv[0] must exercise the installed alias, including its dash dispatch.
    use std::os::unix::process::CommandExt;
    let output = command
        .arg0(&link)
        .env("COMANDOS_DASH_TERM", "invalid-help-fixture")
        .arg("-h")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&output.stdout).starts_with("uso: comandos dash"));
    assert!(output.stderr.is_empty());
    assert!(!home.join("run").exists());
    assert!(!home.join(".claude/hooks/dash-token").exists());
}

// Every owned frontend is reaped even when an assertion fails. Taking the
// child transfers that responsibility to wait_with_output after SIGTERM.
struct Front(Option<std::process::Child>);
impl Drop for Front {
    fn drop(&mut self) {
        if let Some(child) = self.0.as_mut() {
            if matches!(child.try_wait(), Ok(None)) {
                let _ = child.kill();
            }
            let _ = child.wait();
        }
    }
}

#[test]
fn dash_prints_the_python_banner_answers_native_404_and_stops_on_sigterm() {
    let home = temp_home("boot");
    let port = free_port();
    let mut front = Front(Some(
        base(&home)
            .args(["dash", &port.to_string(), "--no-open"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap(),
    ));
    let child = front.0.as_mut().unwrap();
    let stdout = child.stdout.take().unwrap();
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut line = String::new();
        let _ = BufReader::new(stdout).read_line(&mut line);
        let _ = tx.send(line);
    });
    let line = rx
        .recv_timeout(Duration::from_secs(10))
        .expect("banner de arranque");
    assert_eq!(
        line,
        format!("Centro Claude corriendo en http://127.0.0.1:{port}  (Ctrl+C para salir)\n")
    );
    assert!(
        home.join(".claude/hooks/dash-token").is_file(),
        "token creado"
    );

    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    write!(
        stream,
        "GET /no-existe HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n"
    )
    .unwrap();
    let mut wire = String::new();
    stream.read_to_string(&mut wire).unwrap();
    let (head, body) = wire.split_once("\r\n\r\n").unwrap();
    // The migrated default serves the original missing-file response locally.
    // The separate native-off test retains the legacy forwarding/502 check.
    assert!(head.starts_with("HTTP/1.1 404"), "{head}");
    assert!(
        head.to_ascii_lowercase()
            .contains("cache-control: no-store")
    );
    assert!(body.contains("Error response"), "{body}");
    assert!(body.contains("File not found"), "{body}");

    let started = Instant::now();
    let killed = Command::new("kill")
        .args(["-TERM", &child.id().to_string()])
        .status()
        .unwrap();
    assert!(killed.success());
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        assert!(
            started.elapsed() < Duration::from_secs(3),
            "SIGTERM debe parar en < 3 s"
        );
        std::thread::sleep(Duration::from_millis(20));
    };
    assert_eq!(status.code(), Some(0));
    // El apagado ordenado deja el censo (vacío: nada declinó) en el
    // `XDG_RUNTIME_DIR` del proceso, con el puerto en el nombre.
    let census = fs::read_to_string(home.join(format!("run/comandos-dash-declines-{port}.json")))
        .expect("censo escrito al apagar");
    assert!(census.contains(r#""counts":{}"#), "{census}");
}

#[test]
fn dash_rejects_an_unreadable_dash_dir_with_the_python_message() {
    let home = temp_home("baddir");
    let output = base(&home)
        .env("COMANDOS_DASH_DIR", "/definitivamente/no")
        .args(["dash", &free_port().to_string()])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(
        String::from_utf8_lossy(&output.stderr),
        "COMANDOS_DASH_DIR no es un directorio legible: /definitivamente/no\n"
    );
    assert!(output.stdout.is_empty());
}

#[test]
fn cc_dash_alias_reaches_the_same_parser() {
    let home = temp_home("alias");
    let link = home.join("cc-dash");
    std::os::unix::fs::symlink(env!("CARGO_BIN_EXE_comandos"), &link).unwrap();
    let output = Command::new(&link)
        .env("HOME", &home)
        .env_remove("COMANDOS_DASH_LEGACY_PORT")
        .args(["--legacy-port"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2), "error de argumentos");
    assert!(String::from_utf8_lossy(&output.stderr).contains("--legacy-port"));
}

#[test]
fn dash_native_off_never_opens_the_state_db_and_traces_forwards() {
    let home = temp_home("native-off");
    let port = free_port();
    let mut front = Front(Some(
        base(&home)
            .env("COMANDOS_DASH_NATIVE", "0")
            .env("COMANDOS_DASH_TRACE_FORWARD", "1")
            .args(["dash", &port.to_string()])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap(),
    ));
    let child = front.0.as_mut().unwrap();
    let stdout = child.stdout.take().unwrap();
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut line = String::new();
        let _ = BufReader::new(stdout).read_line(&mut line);
        let _ = tx.send(line);
    });
    rx.recv_timeout(Duration::from_secs(10))
        .expect("banner de arranque");

    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    write!(
        stream,
        "GET /prefs?deviceId=secreto HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n"
    )
    .unwrap();
    let mut wire = String::new();
    stream.read_to_string(&mut wire).unwrap();
    assert!(wire.starts_with("HTTP/1.1 502"), "{wire}");

    let killed = Command::new("kill")
        .args(["-TERM", &child.id().to_string()])
        .status()
        .unwrap();
    assert!(killed.success());
    let output = front.0.take().unwrap().wait_with_output().unwrap();
    assert_eq!(output.status.code(), Some(0));
    let stderr = String::from_utf8_lossy(&output.stderr);
    // La traza es la línea exacta, sin la consulta (`deviceId` no aparece en ella).
    assert_eq!(
        stderr
            .lines()
            .filter(|l| *l == "comandos dash: reenvío GET /prefs")
            .count(),
        1,
        "{stderr}"
    );
    assert!(
        !home
            .join(".local/state/comandos/app-state.sqlite3")
            .exists(),
        "con COMANDOS_DASH_NATIVE=0 la base no se abre"
    );
    let _ = fs::remove_dir_all(&home);
}

/// El `GLIBC_TUNABLES` del drop-in de `cc-dash` es del frente: un hijo (aquí
/// un `fc-list` falso, el mismo camino que `tmux`, `systemd-run` y `ssh`) no
/// lo hereda.
#[test]
fn dash_children_do_not_inherit_the_front_malloc_tuning() {
    use std::os::unix::fs::PermissionsExt;
    let home = temp_home("tunables");
    let fakebin = home.join("fakebin");
    fs::create_dir_all(&fakebin).unwrap();
    let seen = home.join("fc-list-env");
    let script = fakebin.join("fc-list");
    fs::write(
        &script,
        format!("#!/bin/sh\nenv > '{}'\necho 'Hack'\n", seen.display()),
    )
    .unwrap();
    fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
    let path = format!(
        "{}:{}",
        fakebin.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let port = free_port();
    let tunables = comandos_core::malloc_tuning::PRODUCTION_GLIBC_TUNABLES;
    let mut front = Front(Some(
        base(&home)
            .env("GLIBC_TUNABLES", tunables)
            .env("PATH", path)
            .args(["dash", &port.to_string(), "--no-open"])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    ));
    let child = front.0.as_mut().unwrap();
    let stdout = child.stdout.take().unwrap();
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut line = String::new();
        let _ = BufReader::new(stdout).read_line(&mut line);
        let _ = tx.send(line);
    });
    rx.recv_timeout(Duration::from_secs(10))
        .expect("banner de arranque");
    // El frente sí lo tiene. glibc 2.35 parte la cadena en su sitio (cada
    // `:` pasa a NUL), así que solo se mira el prefijo.
    let environ = fs::read(format!("/proc/{}/environ", child.id())).unwrap();
    let front_has = environ
        .split(|b| *b == 0)
        .any(|kv| kv.starts_with(b"GLIBC_TUNABLES="));
    assert!(front_has, "el frente arranca con el ajuste");

    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    write!(
        stream,
        "GET /prefs HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n"
    )
    .unwrap();
    let mut wire = String::new();
    stream.read_to_string(&mut wire).unwrap();
    assert!(wire.starts_with("HTTP/1.1 200"), "{wire}");
    assert!(
        wire.contains(r#""family": "Hack""#),
        "el fc-list falso corrió: {wire}"
    );

    let env = fs::read_to_string(&seen).expect("el fc-list falso guardó su entorno");
    assert!(
        env.lines().any(|l| l.starts_with("HOME=")),
        "entorno heredado: {env}"
    );
    assert!(
        !env.lines().any(|l| l.starts_with("GLIBC_TUNABLES=")),
        "el hijo no hereda GLIBC_TUNABLES: {env}"
    );

    let killed = Command::new("kill")
        .args(["-TERM", &child.id().to_string()])
        .status()
        .unwrap();
    assert!(killed.success());
    assert_eq!(child.wait().unwrap().code(), Some(0));
    let _ = fs::remove_dir_all(&home);
}
