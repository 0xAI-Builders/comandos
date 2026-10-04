//! Oráculo real: el `cc-dash` Python del repositorio detrás del frente Rust.
//! Pedir al Python directo y pedir al frente debe dar el mismo status,
//! `Content-Type`, `Content-Length` y cuerpo, con y sin token remoto.
//! Sin `python3` la prueba se salta con un aviso (no `#[ignore]`).
use comandos_server::{
    dash::{DashConfig, parse_args, transport_config},
    serve,
};
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::watch,
    time::{sleep, timeout},
};

const TOKEN: &str = "dash-harness-test-token";
const WAIT: Duration = Duration::from_secs(20);

/// Mata el Python y borra el HOME temporal también si la prueba entra en pánico.
struct Legacy {
    child: Child,
    home: PathBuf,
}

impl Drop for Legacy {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = fs::remove_dir_all(&self.home);
    }
}

fn python_available() -> bool {
    Command::new("python3")
        .args(["-c", "import sys"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

/// HOME aislado como `tests/dash_harness.py`: `app-tabs.json` vacío, token
/// fijo en 0600, `TMUX_TMPDIR` privado y sin `$TMUX`.
fn launch(port: u16) -> Legacy {
    let home = std::env::temp_dir().join(format!("cmd-dash-py-{}", std::process::id()));
    let _ = fs::remove_dir_all(&home);
    let hooks = home.join(".claude/hooks");
    fs::create_dir_all(hooks.join("state")).unwrap();
    fs::write(hooks.join("app-tabs.json"), "{}").unwrap();
    fs::write(hooks.join("dash-token"), TOKEN).unwrap();
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(hooks.join("dash-token"), fs::Permissions::from_mode(0o600)).unwrap();
    }
    let tmux = home.join("tmux-tmp");
    fs::create_dir_all(&tmux).unwrap();
    let stderr = fs::File::create(home.join("cc-dash.err")).unwrap();
    let repo = repo();
    let child = Command::new("python3")
        .arg(repo.join("bin/cc-dash"))
        .arg(port.to_string())
        .arg("--no-open")
        .env_remove("TMUX")
        .env("HOME", &home)
        .env("TMUX_TMPDIR", &tmux)
        .env("COMANDOS_DASH_DIR", repo.join("dash"))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(stderr)
        .spawn()
        .unwrap();
    Legacy { child, home }
}

async fn wait_listening(legacy: &mut Legacy, port: u16) {
    let started = Instant::now();
    while TcpStream::connect(("127.0.0.1", port)).await.is_err() {
        if let Ok(Some(status)) = legacy.child.try_wait() {
            let err = fs::read_to_string(legacy.home.join("cc-dash.err")).unwrap_or_default();
            panic!("cc-dash salió con {status}: {err}");
        }
        assert!(started.elapsed() < WAIT, "cc-dash no arrancó");
        sleep(Duration::from_millis(100)).await;
    }
}

#[derive(Debug, PartialEq)]
struct Answer {
    status: u16,
    content_type: Option<String>,
    content_length: Option<String>,
    body: String,
}

async fn ask(port: u16, raw: &str) -> Answer {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    stream.write_all(raw.as_bytes()).await.unwrap();
    let mut out = Vec::new();
    timeout(WAIT, stream.read_to_end(&mut out))
        .await
        .expect("la respuesta debe cerrar la conexión")
        .unwrap();
    let split = out
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .expect("respuesta sin fin de cabeceras");
    let head = String::from_utf8_lossy(&out[..split]).into_owned();
    let mut lines = head.split("\r\n");
    let status = lines
        .next()
        .and_then(|l| l.split(' ').nth(1))
        .and_then(|s| s.parse().ok())
        .expect("línea de estado");
    let headers: Vec<(String, String)> = lines
        .filter_map(|l| l.split_once(':'))
        .map(|(k, v)| (k.trim().to_ascii_lowercase(), v.trim().to_owned()))
        .collect();
    let header = |name: &str| {
        headers
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.clone())
    };
    assert_eq!(header("transfer-encoding"), None, "{head}");
    Answer {
        status,
        content_type: header("content-type"),
        content_length: header("content-length"),
        body: String::from_utf8_lossy(&out[split + 4..]).into_owned(),
    }
}

#[tokio::test]
async fn front_answers_exactly_like_the_python() {
    if !python_available() {
        println!("sin python3: prueba saltada");
        return;
    }
    let py_port = free_port();
    let mut legacy = launch(py_port);
    wait_listening(&mut legacy, py_port).await;

    let home = legacy.home.join("front");
    fs::create_dir_all(&home).unwrap();
    let mut cfg: DashConfig = parse_args(&[], &home, Some(&py_port.to_string())).unwrap();
    cfg.dash_dir = repo().join("dash");
    cfg.token = TOKEN.as_bytes().to_vec();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let front_port = listener.local_addr().unwrap().port();
    let (stop, shutdown) = watch::channel(false);
    let front = tokio::spawn(serve(listener, transport_config(cfg), shutdown));

    let remote = "Host: test.ts.net\r\nX-Forwarded-For: 100.64.0.9\r\n";
    let token = format!("X-Comandos-Token: {TOKEN}\r\n");
    let mut seen = Vec::new();
    for auth in ["", token.as_str()] {
        for (method, path, body) in [
            ("GET", "/prefs", ""),
            ("GET", "/notices", ""),
            ("POST", "/pane/type", "{}"),
            ("GET", "/no-existe-api", ""),
        ] {
            let framing = if method == "POST" {
                format!(
                    "Content-Type: application/json\r\nContent-Length: {}\r\n",
                    body.len()
                )
            } else {
                String::new()
            };
            let raw = format!(
                "{method} {path} HTTP/1.1\r\n{remote}{auth}{framing}Connection: close\r\n\r\n{body}"
            );
            let direct = ask(py_port, &raw).await;
            let relayed = ask(front_port, &raw).await;
            let label = format!("{method} {path} token={}", !auth.is_empty());
            assert_eq!(relayed, direct, "{label}");
            seen.push((label, direct.status));
        }
    }
    // Los casos cubren puerta cerrada, rutas reales y errores del Python.
    let statuses: Vec<u16> = seen.iter().map(|(_, s)| *s).collect();
    assert!(statuses.contains(&401), "{seen:?}");
    assert!(statuses.contains(&200), "{seen:?}");
    assert!(statuses.contains(&400), "{seen:?}");
    assert!(statuses.contains(&404), "{seen:?}");

    stop.send(true).unwrap();
    timeout(WAIT, front).await.unwrap().unwrap().unwrap();
    drop(legacy);
}
