//! Ayudantes de las pruebas: el binario Rust en modo `--headless` y el `Handler`
//! del Python servido con un `gi` falso, ambos en puertos efímeros, con HOME,
//! PATH y XDG_RUNTIME_DIR propios y sin DBUS/DISPLAY. Nada de tmux, GTK ni sonido.
#![allow(dead_code)]

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// Directorio temporal que se borra al soltarse.
pub struct TempDir(PathBuf);

impl TempDir {
    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

pub fn tempdir() -> TempDir {
    use std::os::unix::fs::DirBuilderExt;
    use std::sync::atomic::{AtomicU32, Ordering};
    static NEXT: AtomicU32 = AtomicU32::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    let dir = std::env::temp_dir().join(format!(
        "comandos-notifyd-test-{}-{}-{nanos}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::SeqCst)
    ));
    std::fs::DirBuilder::new().mode(0o700).create(&dir).unwrap();
    TempDir(dir)
}

fn private_env(home: &Path) -> Vec<(&'static str, PathBuf)> {
    use std::os::unix::fs::PermissionsExt;
    let mut env = vec![("HOME", home.to_path_buf())];
    for (key, name) in [
        ("XDG_CONFIG_HOME", "config"),
        ("XDG_DATA_HOME", "data"),
        ("XDG_CACHE_HOME", "cache"),
        ("XDG_STATE_HOME", "state"),
        ("XDG_RUNTIME_DIR", "run"),
        ("TMPDIR", "tmp"),
    ] {
        let dir = home.join(name);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).unwrap();
        env.push((key, dir));
    }
    env.push(("TMP", home.join("tmp")));
    env.push(("TEMP", home.join("tmp")));
    env
}

/// Respuesta tal como llegó por el socket.
#[derive(Debug, Clone)]
pub struct Wire {
    /// 0 si no hubo línea de estado (respuesta HTTP/0.9 o conexión cerrada sin respuesta).
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Wire {
    fn parse(raw: Vec<u8>) -> Wire {
        if !raw.starts_with(b"HTTP/") {
            return Wire {
                status: 0,
                headers: Vec::new(),
                body: raw,
            };
        }
        let split = raw
            .windows(4)
            .position(|w| w == b"\r\n\r\n")
            .unwrap_or(raw.len());
        let head = String::from_utf8_lossy(&raw[..split]).into_owned();
        let body = raw.get(split + 4..).unwrap_or_default().to_vec();
        let mut lines = head.split("\r\n");
        let status = lines
            .next()
            .and_then(|l| l.split(' ').nth(1))
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        let headers = lines
            .filter_map(|l| l.split_once(':'))
            .map(|(k, v)| (k.to_string(), v.trim().to_string()))
            .collect();
        Wire {
            status,
            headers,
            body,
        }
    }

    pub fn header(&self, name: &str) -> Option<String> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.clone())
    }

    pub fn content_type(&self) -> Option<String> {
        self.header("Content-Type")
    }
}

/// Un servidor de prueba (Rust o Python) y los avisos que entregó.
pub struct Daemon {
    child: Child,
    port: u16,
    accepted: Arc<Mutex<Vec<Vec<String>>>>,
    _home: TempDir,
}

impl Drop for Daemon {
    fn drop(&mut self) {
        // Solo nuestro hijo directo, por su pid.
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Daemon {
    fn start(mut command: Command, home: TempDir, env: &[(&str, &str)]) -> Daemon {
        let fakebin = home.path().join("fakebin");
        let runtime = home.path().join("run");
        std::fs::create_dir_all(&fakebin).unwrap();
        std::fs::create_dir_all(&runtime).unwrap();
        command
            .env_clear()
            .envs(private_env(home.path()))
            .env("PATH", &fakebin)
            .env("XDG_RUNTIME_DIR", &runtime)
            .env("LC_ALL", "C.UTF-8")
            .env("PYTHONDONTWRITEBYTECODE", "1")
            .envs(env.iter().copied())
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        let mut child = command.spawn().unwrap();
        let stdout = child.stdout.take().unwrap();
        let accepted = Arc::new(Mutex::new(Vec::new()));
        let (port_tx, port_rx) = mpsc::channel();
        let sink = Arc::clone(&accepted);
        std::thread::spawn(move || {
            let mut port_tx = Some(port_tx);
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                if let Some(tx) = port_tx.take() {
                    // «… listo en 127.0.0.1:<puerto> …»
                    let port = line
                        .split("127.0.0.1:")
                        .nth(1)
                        .and_then(|rest| rest.split(' ').next())
                        .and_then(|p| p.parse::<u16>().ok());
                    let _ = tx.send(port);
                    continue;
                }
                if let Ok(fields) = serde_json::from_str::<Vec<String>>(&line) {
                    sink.lock().unwrap().push(fields);
                }
            }
        });
        let port = port_rx
            .recv_timeout(Duration::from_secs(30))
            .ok()
            .flatten()
            .expect("el servidor de prueba no anunció su puerto");
        Daemon {
            child,
            port,
            accepted,
            _home: home,
        }
    }

    /// Envía una petición HTTP/1.0; añade `Content-Length` si el cuerpo no está vacío
    /// y la prueba no la fija.
    pub async fn request(
        &self,
        method: &str,
        path: &str,
        headers: &[(&str, &str)],
        body: &str,
    ) -> Wire {
        let mut raw = format!("{method} {path} HTTP/1.0\r\n").into_bytes();
        for (k, v) in headers {
            raw.extend_from_slice(format!("{k}: {v}\r\n").as_bytes());
        }
        if !body.is_empty()
            && !headers
                .iter()
                .any(|(k, _)| k.eq_ignore_ascii_case("Content-Length"))
        {
            raw.extend_from_slice(format!("Content-Length: {}\r\n", body.len()).as_bytes());
        }
        raw.extend_from_slice(b"\r\n");
        raw.extend_from_slice(body.as_bytes());
        self.raw(&raw).await
    }

    /// Envía bytes tal cual, cierra la escritura y lee hasta EOF (o reinicio).
    pub async fn raw(&self, request: &[u8]) -> Wire {
        let mut stream = tokio::net::TcpStream::connect(("127.0.0.1", self.port))
            .await
            .unwrap();
        let _ = stream.write_all(request).await;
        let _ = stream.shutdown().await;
        let mut out = Vec::new();
        let mut buf = [0u8; 8192];
        loop {
            match tokio::time::timeout(Duration::from_secs(10), stream.read(&mut buf)).await {
                Ok(Ok(0)) | Ok(Err(_)) | Err(_) => break,
                Ok(Ok(n)) => out.extend_from_slice(&buf[..n]),
            }
        }
        Wire::parse(out)
    }

    /// Avisos entregados, `[title, body, session, kind, project, options, full, pane]`;
    /// espera a que la lista deje de crecer.
    pub fn accepted(&self) -> Vec<Vec<String>> {
        let mut last = usize::MAX;
        let mut stable = 0;
        for _ in 0..50 {
            std::thread::sleep(Duration::from_millis(100));
            let now = self.accepted.lock().unwrap().len();
            if now == last {
                stable += 1;
                if stable >= 3 {
                    break;
                }
            } else {
                stable = 0;
                last = now;
            }
        }
        self.accepted.lock().unwrap().clone()
    }
}

fn hooks_home(hooks: &Path) -> TempDir {
    let home = tempdir();
    std::fs::create_dir_all(home.path().join(".claude")).unwrap();
    std::os::unix::fs::symlink(hooks, home.path().join(".claude/hooks")).unwrap();
    home
}

/// `comandos-notifyd --headless --port 0 --hooks-dir <hooks>`.
pub fn rust_notifyd(hooks: &Path) -> Daemon {
    rust_notifyd_with_env(hooks, &[])
}

pub fn rust_notifyd_with_env(hooks: &Path, env: &[(&str, &str)]) -> Daemon {
    let mut command = Command::new(env!("CARGO_BIN_EXE_comandos-notifyd"));
    command
        .args(["--headless", "--port", "0", "--hooks-dir"])
        .arg(hooks);
    Daemon::start(command, hooks_home(hooks), env)
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn which_python() -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join("python3"))
        .find(|p| p.is_file())
}

/// El `Handler` de `bin/cc-notifyd` con `gi` sustituido, en un puerto efímero.
/// `GLib.idle_add` imprime sus argumentos (el aviso) como una línea JSON.
const ORACLE: &str = r#"
import sys, types, json, threading, socketserver
import importlib.util
from importlib.machinery import SourceFileLoader
sys.dont_write_bytecode = True

class _Stub:
    def __getattr__(self, name):
        return _Stub()
    def __call__(self, *a, **k):
        return _Stub()

_lock = threading.Lock()
def _idle_add(fn, *args):
    with _lock:
        print(json.dumps(list(args)), flush=True)
    return 0

gi = types.ModuleType("gi")
def _require_version(name, version):
    if name != "Gtk":
        raise ValueError(name)
gi.require_version = _require_version
repository = types.ModuleType("gi.repository")
repository.Gtk = _Stub()
repository.Gdk = _Stub()
repository.GdkPixbuf = _Stub()
repository.GLib = types.SimpleNamespace(idle_add=_idle_add)
gi.repository = repository
sys.modules["gi"] = gi
sys.modules["gi.repository"] = repository

loader = SourceFileLoader("cc_notifyd_oracle", sys.argv[1])
spec = importlib.util.spec_from_loader("cc_notifyd_oracle", loader)
mod = importlib.util.module_from_spec(spec)
loader.exec_module(mod)

if len(sys.argv) > 2 and sys.argv[2] == "--eval":
    print(json.dumps(eval(sys.argv[3], {"mod": mod, "json": json, "sys": sys})), flush=True)
    sys.exit(0)

socketserver.ThreadingTCPServer.daemon_threads = True
srv = socketserver.ThreadingTCPServer(("127.0.0.1", 0), mod.Handler)
print("oracle listo en 127.0.0.1:%d" % srv.server_address[1], flush=True)
srv.serve_forever()
"#;

/// `None` si no hay `python3` o `bin/cc-notifyd`.
pub fn python_notifyd(hooks: &Path) -> Option<Daemon> {
    python_notifyd_with_env(hooks, &[])
}

pub fn python_notifyd_with_env(hooks: &Path, env: &[(&str, &str)]) -> Option<Daemon> {
    let python = which_python()?;
    let script = repo_root().join("bin/cc-notifyd");
    if !script.is_file() {
        return None;
    }
    let mut command = Command::new(python);
    command.args(["-c", ORACLE]).arg(script);
    Some(Daemon::start(command, hooks_home(hooks), env))
}

/// Evalúa una expresión Python con el módulo `mod` (= `bin/cc-notifyd` con `gi` falso)
/// cargado bajo un HOME cuyo `.claude/hooks` apunta a `hooks`; devuelve su `json.dumps`.
pub fn python_eval(hooks: &Path, env: &[(&str, &str)], expr: &str) -> Option<serde_json::Value> {
    let python = which_python()?;
    let script = repo_root().join("bin/cc-notifyd");
    if !script.is_file() {
        return None;
    }
    let home = hooks_home(hooks);
    let runtime = home.path().join("run");
    let fakebin = home.path().join("fakebin");
    std::fs::create_dir_all(&runtime).unwrap();
    std::fs::create_dir_all(&fakebin).unwrap();
    let out = Command::new(python)
        .args(["-c", ORACLE])
        .arg(script)
        .args(["--eval", expr])
        .env_clear()
        .envs(private_env(home.path()))
        .env("PATH", &fakebin)
        .env("XDG_RUNTIME_DIR", &runtime)
        .env("LC_ALL", "C.UTF-8")
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .envs(env.iter().copied())
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .unwrap();
    // Con python3 y el script presentes, un fallo del oráculo es un fallo de la prueba.
    Some(serde_json::from_slice(&out.stdout).expect("salida del oráculo Python"))
}
