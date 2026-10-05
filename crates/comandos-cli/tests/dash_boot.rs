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
        .env("COMANDOS_DASH_LEGACY_PORT", "1")
        // El censo de declinaciones (D7) va a `$XDG_RUNTIME_DIR`: nunca el real.
        .env("XDG_RUNTIME_DIR", home.join("run"))
        .env_remove("COMANDOS_DASH_BACKGROUND")
        .env_remove("COMANDOS_DASH_CUTS_OFF")
        .stdin(Stdio::null());
    command
}

#[test]
fn dash_prints_the_python_banner_answers_502_without_legacy_and_stops_on_sigterm() {
    let home = temp_home("boot");
    let port = free_port();
    let mut child = base(&home)
        .args(["dash", &port.to_string(), "--no-open"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
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
    // Una ruta que no es archivo se reenvía al Python heredado; sin él, 502.
    assert!(head.starts_with("HTTP/1.1 502"), "{head}");
    assert!(
        head.to_ascii_lowercase()
            .contains("cache-control: no-store")
    );
    assert_eq!(body, r#"{"error": "Servidor heredado no disponible"}"#);

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
    let mut child = base(&home)
        .env("COMANDOS_DASH_NATIVE", "0")
        .env("COMANDOS_DASH_TRACE_FORWARD", "1")
        .args(["dash", &port.to_string()])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
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
    let output = child.wait_with_output().unwrap();
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
