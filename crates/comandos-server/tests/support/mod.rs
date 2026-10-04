//! Cliente de cable para los tests del frente `comandos dash`: escribe la
//! petición tal cual y lee la respuesta entera (`Connection: close`), así las
//! cabeceras que se comprueban son las que viajan, también en HEAD y 304.
#![allow(dead_code)]
use comandos_server::dash::{
    DashConfig,
    native::{
        NativeOptions,
        tmux::{Program, Tmux},
    },
    parse_args, serve_with,
};
use std::{
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::watch,
    task::JoinHandle,
    time::timeout,
};

pub const WAIT: Duration = Duration::from_secs(3);
pub const TOKEN: &str = "dash-native-test-token";
/// 2026-10-04T12:00:00Z en ms: el reloj fijo de las pruebas nativas.
pub const NOW_MS: i64 = 1_791_115_200_000;

pub struct Wire {
    pub status: u16,
    /// Nombres en minúsculas, en el orden del cable.
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Wire {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }
    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }
}

/// `METHOD target` con `Host` local, cabeceras extra (`Nombre: valor\r\n`) y cierre.
pub async fn request(port: u16, method: &str, target: &str, extra: &str) -> Wire {
    let wire = format!(
        "{method} {target} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n{extra}Connection: close\r\n\r\n"
    );
    let mut stream = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    stream.write_all(wire.as_bytes()).await.unwrap();
    let mut out = Vec::new();
    timeout(WAIT, stream.read_to_end(&mut out))
        .await
        .expect("la respuesta debe cerrar la conexión")
        .unwrap();
    parse(&out)
}

fn parse(raw: &[u8]) -> Wire {
    let split = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .expect("respuesta sin fin de cabeceras");
    let head = String::from_utf8_lossy(&raw[..split]).into_owned();
    let mut lines = head.split("\r\n");
    let status = lines
        .next()
        .and_then(|l| l.split(' ').nth(1))
        .and_then(|s| s.parse().ok())
        .expect("línea de estado");
    let headers = lines
        .filter_map(|l| l.split_once(':'))
        .map(|(k, v)| (k.trim().to_ascii_lowercase(), v.trim().to_owned()))
        .collect();
    Wire {
        status,
        headers,
        body: raw[split + 4..].to_vec(),
    }
}

/// Como `request`, con cuerpo JSON y su `Content-Length`.
pub async fn request_body(port: u16, method: &str, target: &str, extra: &str, body: &str) -> Wire {
    let extra = format!(
        "{extra}X-Comandos-Token: {TOKEN}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n",
        body.len()
    );
    let wire = format!(
        "{method} {target} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n{extra}Connection: close\r\n\r\n{body}"
    );
    let mut stream = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    stream.write_all(wire.as_bytes()).await.unwrap();
    let mut out = Vec::new();
    timeout(Duration::from_secs(40), stream.read_to_end(&mut out))
        .await
        .expect("la respuesta debe cerrar la conexión")
        .unwrap();
    parse(&out)
}

/// GET con el token local (las rutas nativas son API: pasan la puerta).
pub async fn get(port: u16, target: &str) -> Wire {
    request_body(port, "GET", target, "", "").await
}

pub fn tmux_available() -> bool {
    Command::new("tmux")
        .arg("-V")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// Un puerto sin nadie escuchando: si el frente reenviara, respondería 502.
pub fn dead_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

/// HOME temporal con `~/.claude/hooks/state`, token y `TMUX_TMPDIR` propios.
pub struct TestHome {
    pub root: PathBuf,
}

impl TestHome {
    pub fn new(tag: &str) -> Self {
        let root = std::env::temp_dir().join(format!("cmd-native-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join(".claude/hooks/state")).unwrap();
        std::fs::create_dir_all(root.join("tmux")).unwrap();
        std::fs::write(root.join(".claude/hooks/dash-token"), TOKEN).unwrap();
        std::fs::write(root.join(".claude/hooks/app-tabs.json"), "{}").unwrap();
        Self { root }
    }
    pub fn hooks(&self) -> PathBuf {
        self.root.join(".claude/hooks")
    }
    pub fn tmux_dir(&self) -> PathBuf {
        self.root.join("tmux")
    }
    pub fn state_db(&self) -> PathBuf {
        self.root.join(".local/state/comandos/app-state.sqlite3")
    }
    pub fn write(&self, name: &str, text: &str) {
        std::fs::write(self.hooks().join(name), text).unwrap();
    }
    pub fn options(&self) -> NativeOptions {
        let mut opts = NativeOptions::for_home(&self.root, self.state_db());
        opts.clock = Arc::new(|| NOW_MS);
        opts.tmux = Tmux::private(&self.tmux_dir());
        // Sin fc-list en las pruebas salvo que una prueba lo fije.
        opts.fc_list = Program::named("/no-existe/fc-list");
        opts
    }
}

impl Drop for TestHome {
    fn drop(&mut self) {
        // Nunca el servidor tmux del usuario: el socket vive en tmux_dir.
        let _ = Command::new("tmux")
            .arg("kill-server")
            .env_remove("TMUX")
            .env("TMUX_TMPDIR", self.tmux_dir())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

pub struct Front {
    pub port: u16,
    stop: watch::Sender<bool>,
    task: JoinHandle<std::io::Result<()>>,
}

impl Front {
    pub async fn stop(self) {
        let _ = self.stop.send(true);
        let _ = self.task.await;
    }
}

pub fn config(home: &TestHome, legacy_port: u16) -> DashConfig {
    let mut cfg = parse_args(&[], &home.root, Some(&legacy_port.to_string())).unwrap();
    cfg.token = TOKEN.as_bytes().to_vec();
    cfg.dash_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../dash");
    cfg.state_db = home.state_db();
    cfg
}

pub async fn front(home: &TestHome, legacy_port: u16, opts: NativeOptions) -> Front {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let (stop, shutdown) = watch::channel(false);
    let cfg = config(home, legacy_port);
    let task = tokio::spawn(serve_with(listener, cfg, Some(opts), shutdown));
    Front { port, stop, task }
}

/// Heredado falso: responde `{"legacy": true}` y anota la línea de petición.
pub struct FakeLegacy {
    pub port: u16,
    pub seen: Arc<Mutex<Vec<String>>>,
}

impl FakeLegacy {
    pub async fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let log = seen.clone();
        tokio::spawn(async move {
            while let Ok((mut stream, _)) = listener.accept().await {
                let log = log.clone();
                tokio::spawn(async move {
                    let mut buf = Vec::new();
                    let mut chunk = [0u8; 4096];
                    loop {
                        let Ok(n) = stream.read(&mut chunk).await else {
                            return;
                        };
                        if n == 0 {
                            break;
                        }
                        buf.extend_from_slice(&chunk[..n]);
                        if let Some(end) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                            let head = String::from_utf8_lossy(&buf[..end]).to_string();
                            let length = head
                                .lines()
                                .find_map(|l| {
                                    l.to_ascii_lowercase()
                                        .strip_prefix("content-length:")
                                        .map(|v| v.trim().parse::<usize>().unwrap_or(0))
                                })
                                .unwrap_or(0);
                            if buf.len() >= end + 4 + length {
                                log.lock()
                                    .unwrap()
                                    .push(head.lines().next().unwrap_or("").into());
                                break;
                            }
                        }
                    }
                    let body = br#"{"legacy": true}"#;
                    let head = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len()
                    );
                    let _ = stream.write_all(head.as_bytes()).await;
                    let _ = stream.write_all(body).await;
                });
            }
        });
        Self { port, seen }
    }
    pub fn requests(&self) -> Vec<String> {
        self.seen.lock().unwrap().clone()
    }
}
