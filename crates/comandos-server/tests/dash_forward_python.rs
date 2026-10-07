//! Immutable source replies behind the real Rust forwarding transport.
//! Pedir al Python directo y pedir al frente debe dar el mismo status,
//! `Content-Type`, `Content-Length` y cuerpo, con y sin token remoto.
//! Replay serves frozen original replies over a real private Rust listener; no Python.
mod support;
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
/// fijo en 0600, `TMUX_TMPDIR` privado y sin `$TMUX`. Con el fakebin del
/// oráculo delante del `PATH` (tmux, `systemctl` y `systemd-run` son `true`,
/// ssh es `false`), un `XDG_RUNTIME_DIR` propio y sin DBus ni pantalla: el
/// Python nunca alcanza el tmux, el systemd ni la sesión reales.
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
    let fakebin = home.join("fakebin");
    support::oracle::fake_effects(&fakebin);
    support::oracle::fake_tmux_true(&fakebin);
    let runtime = home.join("xdg-runtime");
    fs::create_dir_all(&runtime).unwrap();
    let path = format!(
        "{}:{}",
        fakebin.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let stderr = fs::File::create(home.join("cc-dash.err")).unwrap();
    assert!(matches!(
        std::env::var("COMANDOS_ORACLE").as_deref(),
        Ok("record" | "check")
    ));
    let repo = support::frozen::reference(&home).unwrap();
    let interpreter = std::env::var("COMANDOS_SERVER_ORACLE_PYTHON")
        .unwrap_or_else(|_| "/usr/bin/python3".into());
    let child = Command::new(interpreter)
        .arg(repo.join("bin/cc-dash"))
        .arg(port.to_string())
        .arg("--no-open")
        .env_remove("TMUX")
        .env_remove("COMANDOS_STATE_DB")
        .env_remove("COMANDOS_USAGE_DB")
        .env_remove("COMANDOS_QUICK_TERMINAL_BASE")
        .env_remove("CLAUDE_CONFIG_DIR")
        .env_remove("CODEX_HOME")
        .env_remove("GROK_HOME")
        .env_remove("DBUS_SESSION_BUS_ADDRESS")
        .env_remove("DISPLAY")
        .env_remove("WAYLAND_DISPLAY")
        .env("PATH", &path)
        .env("XDG_RUNTIME_DIR", &runtime)
        .env("XDG_STATE_HOME", home.join(".local/state"))
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

#[derive(Debug, PartialEq, Clone)]
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
    let explicit = matches!(
        std::env::var("COMANDOS_ORACLE").as_deref(),
        Ok("record" | "check")
    );
    let source_port = free_port();
    let mut legacy = explicit.then(|| launch(source_port));
    if let Some(source) = legacy.as_mut() {
        wait_listening(source, source_port).await;
    }
    let fixture = support::TestHome::new("forward-frozen");
    let source_home = legacy
        .as_ref()
        .map_or(fixture.root.as_path(), |source| source.home.as_path());
    let roots = [("<HOME>", source_home)];
    let remote = "Host: test.ts.net\r\nX-Forwarded-For: 100.64.0.9\r\n";
    let token = format!("X-Comandos-Token: {TOKEN}\r\n");
    let mut cases = Vec::new();
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
            let input = serde_json::json!({"source_commit":support::frozen::SOURCE_COMMIT,
                "source_sha256":"4e4e26305485b4926bd2c77618a4a68eb8da9ea425825c57a9a0fea6847a6f24",
                "python":"CPython 3.10.12", "request":raw,"fixture":{"app_tabs":{},"token":TOKEN}});
            let actual = if explicit {
                Some(ask(source_port, &raw).await)
            } else {
                None
            };
            let expected = comandos_oracle::oracle_at(
                &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden"),
                "server-forward-original",
                &input,
                || {
                    let actual = actual.unwrap();
                    Ok(comandos_oracle::normalize(
                        &serde_json::to_vec(&serde_json::json!({"status":actual.status,"content_type":actual.content_type,
                            "content_length":actual.content_length,"body":actual.body})).unwrap(),
                        &roots,
                    ))
                },
            );
            let value: serde_json::Value =
                serde_json::from_slice(&comandos_oracle::restore(&expected, &roots)).unwrap();
            let expected = Answer {
                status: value["status"].as_u64().unwrap().try_into().unwrap(),
                content_type: value["content_type"].as_str().map(str::to_owned),
                content_length: value["content_length"].as_str().map(str::to_owned),
                body: value["body"].as_str().unwrap().to_owned(),
            };
            cases.push((
                raw,
                format!("{method} {path} token={}", !auth.is_empty()),
                expected,
            ));
        }
    }
    // Replay has an actual HTTP actor that serves immutable wire expectations,
    // with no invented domain effects. Record/check forwards to CPython itself.
    let replay_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let replay_port = replay_listener.local_addr().unwrap().port();
    let replies = cases.clone();
    let observed = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let requests = observed.clone();
    let (replay_stop, mut replay_shutdown) = watch::channel(false);
    let replay = tokio::spawn(async move {
        loop {
            let mut stream = tokio::select! {
                _=replay_shutdown.changed()=>break,
                result=replay_listener.accept()=>result.unwrap().0,
            };
            let mut buffer = Vec::new();
            loop {
                let mut chunk = [0; 4096];
                let n = stream.read(&mut chunk).await.unwrap();
                if n == 0 {
                    break;
                }
                buffer.extend_from_slice(&chunk[..n]);
                if let Some(end) = buffer.windows(4).position(|window| window == b"\r\n\r\n") {
                    let head = String::from_utf8_lossy(&buffer[..end]);
                    let length = head
                        .lines()
                        .filter_map(|line| line.split_once(':'))
                        .find(|(name, _)| name.eq_ignore_ascii_case("content-length"))
                        .map_or(0, |(_, value)| value.trim().parse::<usize>().unwrap());
                    if buffer.len() >= end + 4 + length {
                        break;
                    }
                }
            }
            let raw = String::from_utf8(buffer).unwrap();
            requests.lock().unwrap().push(raw.clone());
            let first = raw.lines().next().unwrap();
            let auth = raw.lines().any(|line| {
                line.split_once(':')
                    .is_some_and(|(name, _)| name.eq_ignore_ascii_case("X-Comandos-Token"))
            });
            let (_, _, answer) = replies
                .iter()
                .find(|(case, _, _)| {
                    case.lines().next() == Some(first)
                        && case.lines().any(|line| {
                            line.split_once(':').is_some_and(|(name, _)| {
                                name.eq_ignore_ascii_case("X-Comandos-Token")
                            })
                        }) == auth
                })
                .expect("recorded forwarding case");
            let response = format!(
                "HTTP/1.1 {} frozen\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                answer.status,
                answer.content_type.as_deref().unwrap_or("text/plain"),
                answer.content_length.as_deref().unwrap(),
                answer.body
            );
            stream.write_all(response.as_bytes()).await.unwrap();
        }
    });
    let py_port = if explicit { source_port } else { replay_port };
    let home = fixture.root.join("front");
    fs::create_dir_all(&home).unwrap();
    let mut cfg: DashConfig = parse_args(&[], &home, Some(&py_port.to_string())).unwrap();
    cfg.dash_dir = repo().join("dash");
    cfg.token = TOKEN.as_bytes().to_vec();
    // Esta prueba compara el reenvío puro de la 2a.
    cfg.native = false;
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let front_port = listener.local_addr().unwrap().port();
    let (stop, shutdown) = watch::channel(false);
    let front = tokio::spawn(serve(listener, transport_config(cfg), shutdown));

    let mut seen = Vec::new();
    for (raw, label, direct) in cases {
        let relayed = ask(front_port, &raw).await;
        assert_eq!(relayed, direct, "{label}");
        seen.push((label, direct.status));
    }
    if !explicit {
        assert!(
            !observed.lock().unwrap().is_empty(),
            "real native forwarding reaches the private Rust actor"
        );
    }
    // Los casos cubren puerta cerrada, rutas reales y errores del Python.
    let statuses: Vec<u16> = seen.iter().map(|(_, s)| *s).collect();
    assert!(statuses.contains(&401), "{seen:?}");
    assert!(statuses.contains(&200), "{seen:?}");
    assert!(statuses.contains(&400), "{seen:?}");
    assert!(statuses.contains(&404), "{seen:?}");

    stop.send(true).unwrap();
    timeout(WAIT, front).await.unwrap().unwrap().unwrap();
    replay_stop.send(true).unwrap();
    timeout(WAIT, replay).await.unwrap().unwrap();
    drop(legacy);
}
