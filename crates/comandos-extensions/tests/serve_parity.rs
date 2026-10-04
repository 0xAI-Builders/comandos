//! Compara byte a byte las respuestas del proxy Rust y del proxy Python ante la misma
//! secuencia JSON-RPC por stdio, con un upstream HTTP falso.
mod support;
use std::{
    fs,
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{Arc, Mutex, mpsc},
    thread,
    time::Duration,
};

const SEQUENCE: &[&str] = &[
    r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"t","version":"1"}}}"#,
    r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
    r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#,
    r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"echo","arguments":{"text":"hola ñ 日本"}}}"#,
    r#"{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"fail","arguments":{}}}"#,
    r#"{"jsonrpc":"2.0","id":5,"method":"prompts/list"}"#,
];

/// Lanza el proxy, le da la secuencia mensaje a mensaje (esperando cada respuesta) y
/// devuelve las líneas de stdout. Un plazo por línea evita colgar la suite.
fn run_proxy(cmd: &mut Command, home: &Path, label: &str) -> Vec<String> {
    let stderr = fs::File::create(home.join(format!("{label}.stderr"))).unwrap();
    let mut child = cmd
        .env("HOME", home)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(stderr)
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let stdout = BufReader::new(child.stdout.take().unwrap());
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        for line in stdout.lines() {
            if tx.send(line.unwrap()).is_err() {
                break;
            }
        }
    });
    let mut lines = Vec::new();
    for msg in SEQUENCE {
        writeln!(stdin, "{msg}").unwrap();
        if !msg.contains("notifications/") {
            let line = rx
                .recv_timeout(Duration::from_secs(30))
                .unwrap_or_else(|_| {
                    let _ = child.kill();
                    let err = fs::read_to_string(home.join(format!("{label}.stderr")));
                    panic!("{label}: sin respuesta a {msg}\nstderr: {err:?}")
                });
            lines.push(line);
        }
    }
    drop(stdin);
    let _ = child.wait();
    lines
}

/// HOME temporal con un catálogo de un solo servidor (`fake`) apuntando al upstream falso.
/// Exige un directorio nuevo: nunca escribe sobre un HOME existente.
fn prepare_home(home: &Path, port: u16) {
    fs::create_dir(home).expect("el HOME de la prueba debe ser un directorio nuevo");
    fs::create_dir_all(home.join(".config/comandos/extensions")).unwrap();
    fs::write(
        home.join(".config/comandos/extensions/catalog.json"),
        format!(
            r#"{{"version":1,"servers":{{"fake":{{"enabled":true,"url":"http://127.0.0.1:{port}/mcp","transport":"http"}}}}}}"#
        ),
    )
    .unwrap();
    support::link_oracle_venv(home);
}

#[test]
fn rust_proxy_matches_python_proxy() {
    let log = Arc::new(Mutex::new(Vec::new()));
    let (port, _fake) = support::fake_mcp_http::spawn(log.clone());
    let home = std::env::temp_dir().join(format!("comandos-parity-{}-{port}", std::process::id()));
    let _ = fs::remove_dir_all(&home);
    prepare_home(&home, port);
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");

    let rust = run_proxy(
        Command::new(env!("CARGO_BIN_EXE_comandos-extensions")).args(["serve", "fake"]),
        &home,
        "rust",
    );
    let rust_log = std::mem::take(&mut *log.lock().unwrap());
    let python = run_proxy(
        Command::new("python3.11")
            .arg(root.join("bin/cc-extensions"))
            .args(["serve", "fake"]),
        &home,
        "python",
    );
    let python_log = std::mem::take(&mut *log.lock().unwrap());
    fs::remove_dir_all(&home).unwrap();

    for (r, p) in rust.iter().zip(&python) {
        if r != p {
            eprintln!("rust:   {r}\npython: {p}");
        }
    }
    assert_eq!(rust, python, "las respuestas del proxy difieren");
    assert_eq!(rust.len(), 5);
    // Ambos clientes deben haber recorrido la misma sesión upstream.
    for log in [&rust_log, &python_log] {
        assert!(
            log.iter()
                .any(|l| l == "POST tools/call session=fake-session"),
            "{log:?}"
        );
    }
}

/// Herramienta manual para la medición de memoria (docs/verification/serve-parity.md):
/// prepara `COMANDOS_PARITY_HOME` y mantiene el upstream falso vivo
/// `COMANDOS_PARITY_SECS` segundos (180 por omisión).
#[test]
#[ignore = "herramienta manual: mantiene el upstream falso para medir RSS"]
fn hold_fake_upstream_for_rss() {
    let home =
        PathBuf::from(std::env::var_os("COMANDOS_PARITY_HOME").expect("COMANDOS_PARITY_HOME"));
    let secs = std::env::var("COMANDOS_PARITY_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(180);
    let (port, _fake) = support::fake_mcp_http::spawn(Arc::new(Mutex::new(Vec::new())));
    prepare_home(&home, port);
    eprintln!(
        "upstream falso en 127.0.0.1:{port}, HOME={}",
        home.display()
    );
    thread::sleep(Duration::from_secs(secs));
}
