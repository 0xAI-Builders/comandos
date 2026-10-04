//! Compara byte a byte las respuestas del proxy Rust y del proxy Python ante la misma
//! secuencia JSON-RPC por stdio, con un upstream HTTP falso.
#[allow(dead_code)] // cada prueba usa solo parte de las utilidades compartidas
mod support;
use std::{
    fs,
    io::{BufRead, BufReader, Read, Write},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{Arc, Mutex, mpsc},
    thread,
    time::{Duration, Instant},
};

const NOTIFY_INITIALIZED: &str = r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#;

/// Cliente que pide `2025-06-18`. El upstream falso contesta la versión que le piden y ambos
/// proxies le piden `2025-11-25`, así que el `initialize` prueba la negociación hacia el
/// cliente. El resto recorre cada modelo que el Python reescribe con pydantic.
const SEQUENCE: &[&str] = &[
    r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"t","version":"1"}}}"#,
    NOTIFY_INITIALIZED,
    // `title`/`annotations` null en la herramienta, `default: null` y `1.50` en el esquema.
    r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#,
    // Bloque de contenido con `annotations: null`; UTF-8 sin escapar.
    r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"echo","arguments":{"text":"hola ñ 日本"}}}"#,
    // `isError: true`, `priority` entera, `structuredContent` con `null` y `1e3`.
    r#"{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"fail","arguments":{}}}"#,
    // Error del upstream con claves desordenadas y `data: null`.
    r#"{"jsonrpc":"2.0","id":5,"method":"prompts/list"}"#,
    // Resultado sin `content`: `-32603` por validación.
    r#"{"jsonrpc":"2.0","id":6,"method":"tools/call","params":{"name":"bare","arguments":{}}}"#,
    // `AnyUrl` normalizado.
    r#"{"jsonrpc":"2.0","id":7,"method":"resources/list"}"#,
    r#"{"jsonrpc":"2.0","id":8,"method":"prompts/get","params":{"name":"p"}}"#,
];

/// Cliente que pide una versión antigua soportada: debe recibir esa misma.
const OLD_CLIENT: &[&str] = &[
    r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"t","version":"1"}}}"#,
    NOTIFY_INITIALIZED,
    r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#,
];

/// HOME temporal que se borra también si la prueba falla a medias.
struct TempHome(PathBuf);
impl Drop for TempHome {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn temp_home(label: &str, port: u16, path: &str) -> TempHome {
    let home = std::env::temp_dir().join(format!(
        "comandos-parity-{label}-{}-{port}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&home);
    prepare_home(&home, port, path);
    TempHome(home)
}

/// HOME temporal con un catálogo de un solo servidor (`fake`) apuntando al upstream falso.
/// Exige un directorio nuevo: nunca escribe sobre un HOME existente.
fn prepare_home(home: &Path, port: u16, path: &str) {
    fs::create_dir(home).expect("el HOME de la prueba debe ser un directorio nuevo");
    fs::create_dir_all(home.join(".config/comandos/extensions")).unwrap();
    fs::write(
        home.join(".config/comandos/extensions/catalog.json"),
        format!(
            r#"{{"version":1,"servers":{{"fake":{{"enabled":true,"url":"http://127.0.0.1:{port}{path}","transport":"http"}}}}}}"#
        ),
    )
    .unwrap();
    support::link_oracle_venv(home);
}

fn rust_cmd() -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_comandos-extensions"));
    cmd.args(["serve", "fake"]);
    cmd
}

fn python_cmd() -> Command {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut cmd = Command::new("python3.11");
    cmd.arg(root.join("bin/cc-extensions"))
        .args(["serve", "fake"]);
    cmd
}

fn spawn(mut cmd: Command, home: &Path) -> Child {
    cmd.env("HOME", home)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap()
}

/// Lanza el proxy, le da la secuencia mensaje a mensaje (esperando cada respuesta) y
/// devuelve las líneas de stdout. Un plazo por línea evita colgar la suite.
fn run_proxy(cmd: Command, home: &Path, label: &str, sequence: &[&str]) -> Vec<String> {
    let mut child = spawn(cmd, home);
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
    for msg in sequence {
        writeln!(stdin, "{msg}").unwrap();
        if !msg.contains("notifications/") {
            let line = rx
                .recv_timeout(Duration::from_secs(30))
                .unwrap_or_else(|_| {
                    let _ = child.kill();
                    let mut err = String::new();
                    let _ = child.stderr.take().unwrap().read_to_string(&mut err);
                    panic!("{label}: sin respuesta a {msg}\nstderr: {err}")
                });
            lines.push(line);
        }
    }
    drop(stdin);
    let _ = child.wait();
    lines
}

/// Proxy que debe terminar solo (con stdin abierto): código de salida, stdout y stderr.
fn run_until_exit(cmd: Command, home: &Path, label: &str) -> (Option<i32>, String, String) {
    let mut child = spawn(cmd, home);
    let deadline = Instant::now() + Duration::from_secs(30);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if Instant::now() > deadline {
            let _ = child.kill();
            panic!("{label}: el proxy no terminó");
        }
        thread::sleep(Duration::from_millis(20));
    };
    let (mut out, mut err) = (String::new(), String::new());
    child
        .stdout
        .take()
        .unwrap()
        .read_to_string(&mut out)
        .unwrap();
    child
        .stderr
        .take()
        .unwrap()
        .read_to_string(&mut err)
        .unwrap();
    (status.code(), out, err)
}

fn assert_same(rust: &[String], python: &[String], expected: usize) {
    for (r, p) in rust.iter().zip(python) {
        if r != p {
            eprintln!("rust:   {r}\npython: {p}");
        }
    }
    assert_eq!(rust, python, "las respuestas del proxy difieren");
    assert_eq!(rust.len(), expected);
}

#[test]
fn rust_proxy_matches_python_proxy() {
    let log = Arc::new(Mutex::new(Vec::new()));
    let (port, _fake) = support::fake_mcp_http::spawn(log.clone());
    let home = temp_home("main", port, "/mcp");

    let rust = run_proxy(rust_cmd(), &home.0, "rust", SEQUENCE);
    let rust_log = std::mem::take(&mut *log.lock().unwrap());
    let python = run_proxy(python_cmd(), &home.0, "python", SEQUENCE);
    let python_log = std::mem::take(&mut *log.lock().unwrap());

    assert_same(&rust, &python, 8);
    assert!(
        rust[0].contains(r#""protocolVersion":"2025-06-18""#),
        "{}",
        rust[0]
    );
    // Ambos piden la última versión al upstream y recorren la misma sesión.
    for log in [&rust_log, &python_log] {
        assert_eq!(
            log.first().map(String::as_str),
            Some(r#"POST initialize session=- version="2025-11-25""#),
            "{log:?}"
        );
        assert!(
            log.iter()
                .any(|l| l == "POST tools/call session=fake-session"),
            "{log:?}"
        );
    }
}

#[test]
fn old_client_version_is_negotiated_like_python() {
    let (port, _fake) = support::fake_mcp_http::spawn(Arc::new(Mutex::new(Vec::new())));
    let home = temp_home("old", port, "/mcp");
    let rust = run_proxy(rust_cmd(), &home.0, "rust", OLD_CLIENT);
    let python = run_proxy(python_cmd(), &home.0, "python", OLD_CLIENT);
    assert_same(&rust, &python, 2);
    assert!(
        rust[0].contains(r#""protocolVersion":"2024-11-05""#),
        "{}",
        rust[0]
    );
}

#[test]
fn unsupported_upstream_version_fails_like_python() {
    let (port, _fake) = support::fake_mcp_http::spawn(Arc::new(Mutex::new(Vec::new())));
    let home = temp_home("badversion", port, "/mcp-badversion");
    let rust = run_until_exit(rust_cmd(), &home.0, "rust");
    let python = run_until_exit(python_cmd(), &home.0, "python");
    assert_eq!(rust, python);
    assert_eq!(rust.0, Some(1));
    assert!(rust.1.is_empty());
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
    prepare_home(&home, port, "/mcp");
    eprintln!(
        "upstream falso en 127.0.0.1:{port}, HOME={}",
        home.display()
    );
    thread::sleep(Duration::from_secs(secs));
}
