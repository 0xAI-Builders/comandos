//! Cliente de cable para los tests del frente `comandos dash`: escribe la
//! petición tal cual y lee la respuesta entera (`Connection: close`), así las
//! cabeceras que se comprueban son las que viajan, también en HEAD y 304.
#![allow(dead_code)]
pub mod frozen;
pub mod http_golden;
pub mod news;
pub mod ops;
pub mod oracle;
pub mod services;
pub mod tabs;
pub mod twin;
use comandos_server::dash::{
    DashConfig,
    native::{
        NativeOptions,
        tmux::{Program, Tmux, private_socket},
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

/// El checkout de las pruebas: el oráculo corre `bin/cc-dash` desde aquí.
pub fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// HOME temporal con `~/.claude/hooks/state`, token y `TMUX_TMPDIR` propios.
pub struct TestHome {
    pub root: PathBuf,
}

impl TestHome {
    pub fn new(tag: &str) -> Self {
        Self::new_in(&std::env::temp_dir(), tag)
    }

    /// Como `new`, bajo un directorio temporal corto: el socket
    /// `<root>/tmux/tmux-<uid>/default` debe caber en los 107 bytes de
    /// `sun_path` aunque el `TMPDIR` del desarrollador sea largo.
    pub fn new_short(tag: &str) -> Self {
        let tmp = std::env::temp_dir();
        let base = if tmp.as_os_str().len() <= 24 {
            tmp
        } else {
            PathBuf::from("/tmp")
        };
        Self::new_in(&base, tag)
    }

    fn new_in(base: &Path, tag: &str) -> Self {
        let root = base.join(format!("cmd-native-{tag}-{}", std::process::id()));
        // Un resto de una corrida anterior: primero se para SU servidor (por
        // `-S`, nunca el del usuario) y solo después se borra el directorio.
        let stale = private_socket(&root.join("tmux"));
        if stale.exists() {
            let _ = private_tmux_command(&root.join("tmux"))
                .arg("kill-server")
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
        }
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join(".claude/hooks/state")).unwrap();
        std::fs::create_dir_all(root.join("tmux")).unwrap();
        std::fs::create_dir_all(root.join("bin")).unwrap();
        std::fs::write(root.join(".claude/hooks/dash-token"), TOKEN).unwrap();
        std::fs::write(root.join(".claude/hooks/app-tabs.json"), "{}").unwrap();
        // El directorio del socket privado (0700) existe desde el principio:
        // `-S` nunca apunta a un directorio ausente.
        drop(Tmux::private(&root.join("tmux")));
        for dir in ["tmp", "xdg-runtime"] {
            use std::os::unix::fs::DirBuilderExt;
            std::fs::DirBuilder::new()
                .recursive(true)
                .mode(0o700)
                .create(root.join(dir))
                .unwrap();
        }
        Self { root }
    }
    pub fn hooks(&self) -> PathBuf {
        self.root.join(".claude/hooks")
    }
    pub fn tmux_dir(&self) -> PathBuf {
        self.root.join("tmux")
    }
    /// `tmux -f /dev/null -S <tmux_dir>/tmux-<uid>/default` del servidor
    /// privado de esta prueba. Es la ÚNICA forma de llamar a tmux a mano en las
    /// pruebas: con `-S` explícito un directorio ausente da «no server running»
    /// y nunca alcanza el `/tmp/tmux-<uid>/default` del usuario.
    ///
    /// El cliente corre con `env_clear` + `confined_env`: si esta llamada
    /// arranca el servidor, el servidor y sus paneles nacen con el HOME
    /// temporal, sin `DISPLAY`/DBus y sin el `PATH` del desarrollador.
    pub fn tmux_command(&self) -> Command {
        let socket = private_socket(&self.tmux_dir());
        assert!(
            socket.parent().is_some_and(Path::is_dir),
            "socket tmux privado sin directorio: se negaría a caer en el servidor real"
        );
        let real = oracle::real_program("tmux").unwrap_or_else(|| PathBuf::from("tmux"));
        let mut cmd = Command::new(real);
        cmd.args(["-f", "/dev/null", "-S"])
            .arg(socket)
            .env_clear()
            .envs(self.confined_env());
        cmd
    }

    /// Entorno de todo lo que este HOME lanza (tmux, paneles, oráculo
    /// confinado): HOME temporal, `SHELL=/bin/bash`, `TMPDIR` corto, XDG bajo el
    /// HOME, `LANG=C.UTF-8`, sin escritorio ni DBus. `PATH` = solo el `fakebin`
    /// si está confinado (`oracle::confined_fakebin`); si no, `/usr/bin:/bin`.
    pub fn confined_env(&self) -> Vec<(String, String)> {
        let at = |rel: &str| self.root.join(rel).display().to_string();
        let home = self.root.display().to_string();
        let fakebin = self.root.join("fakebin");
        let path = if fakebin.join(".confined").exists() {
            fakebin.display().to_string()
        } else {
            "/usr/bin:/bin".to_owned()
        };
        let mut env = vec![
            ("HOME".to_owned(), home),
            ("PATH".to_owned(), path),
            ("SHELL".to_owned(), "/bin/bash".to_owned()),
            ("TMPDIR".to_owned(), at("tmp")),
            ("LANG".to_owned(), "C.UTF-8".to_owned()),
            ("XDG_RUNTIME_DIR".to_owned(), at("xdg-runtime")),
            ("XDG_STATE_HOME".to_owned(), at(".local/state")),
            ("XDG_CONFIG_HOME".to_owned(), at(".config")),
            ("XDG_DATA_HOME".to_owned(), at(".local/share")),
            ("XDG_CACHE_HOME".to_owned(), at(".cache")),
            ("TMUX_TMPDIR".to_owned(), at("tmux")),
        ];
        // El `sitecustomize.py` del confinamiento (`oracle::SITECUSTOMIZE`).
        let site = self.root.join(oracle::SITE_DIR);
        if site.join("sitecustomize.py").is_file() {
            env.push(("PYTHONPATH".to_owned(), site.display().to_string()));
        }
        if let Ok(user) = std::env::var("USER") {
            env.push(("USER".to_owned(), user.clone()));
            env.push(("LOGNAME".to_owned(), user));
        }
        env
    }
    pub fn state_db(&self) -> PathBuf {
        self.root.join(".local/state/comandos/app-state.sqlite3")
    }
    pub fn usage_db(&self) -> PathBuf {
        self.hooks().join("comandos-usage.sqlite")
    }
    pub fn journal_db(&self) -> PathBuf {
        self.hooks().join("session-operations.sqlite3")
    }
    pub fn write(&self, name: &str, text: &str) {
        std::fs::write(self.hooks().join(name), text).unwrap();
    }
    pub fn options(&self) -> NativeOptions {
        let mut opts = NativeOptions::for_home(&self.root, self.state_db());
        opts.clock = Arc::new(|| NOW_MS);
        // GET `/usage/state` nativo, como producción (`USAGE_STATE_NATIVE`),
        // aunque la constante se apague.
        opts.usage_state_native = true;
        // Nunca la red real: OAuth falso sin guion (toda petición es un error).
        opts.oauth = Arc::new(FakeOauth::default());
        // Nunca el cc-notifyd real (127.0.0.1:4778): ningún popup de verdad.
        opts.notifyd = Arc::new(FakeNotify::default());
        // Nunca los feeds reales de noticias: el puerto 1 no escucha.
        opts.news_feeds = Arc::new(
            comandos_server::dash::native::background::models::Feeds::new(
                comandos_runtime::news_watch::Endpoints::local("http://127.0.0.1:1"),
            ),
        );
        opts.zone = Arc::new(chrono_tz::America::Mexico_City);
        opts.usage_env = Arc::default();
        opts.extension_env = Arc::default();
        opts.tmux = Tmux::private(&self.tmux_dir());
        // Sin fc-list en las pruebas salvo que una prueba lo fije.
        opts.fc_list = Program::named("/no-existe/fc-list");
        opts.repo_root = std::fs::canonicalize(repo()).ok();
        opts.home = self.root.clone();
        // `which` solo ve el `bin` vacío de la prueba; nunca el ssh ni el
        // heredado reales (el puerto 1 no escucha; `front` lo sustituye).
        opts.search_path = Some(self.root.join("bin").into_os_string());
        opts.ssh = Program::named("/no-existe/ssh");
        opts.codex_home = None;
        opts.grok_home = None;
        opts.cwd = self.root.clone();
        opts.legacy = std::net::SocketAddr::from(([127, 0, 0, 1], 1));
        // Nunca el `systemd-run` real (lo resuelve `for_home` con el PATH del
        // desarrollador): sin scope, POST /terminal/quick declina. Las pruebas
        // de la terminal rápida fijan un `systemd-run` falso (`fake_scope`).
        opts.scope = None;
        opts.quick_base = self.root.join("Terminal");
        // Nunca el `~/.local/state` real ni la terminal web real (4779/4780):
        // el puerto 1 no escucha. El censo no se escribe salvo que la prueba
        // fije su archivo dentro del HOME temporal.
        opts.xdg_state_home = Some(self.root.join(".local/state"));
        opts.webterm_health_ports = [1, 1];
        opts.census_path = None;
        // Lo que el frente lance fuera de tmux (`NativeOptions::program`) ve el
        // entorno confinado de este HOME, nunca el del proceso de pruebas.
        opts.child_env = Some(
            self.confined_env()
                .into_iter()
                .map(|(k, v)| (k.into(), v.into()))
                .collect(),
        );
        opts.display = Some(None);
        opts
    }
}

/// `tmux <args>` en el servidor privado de `home` (`tmux_command`); la prueba
/// falla si tmux falla. Devuelve la salida estándar.
pub fn run_tmux(home: &TestHome, args: &[&str]) -> String {
    let out = home.tmux_command().args(args).output().unwrap();
    assert!(
        out.status.success(),
        "tmux {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap()
}

/// Opciones del frente que solo pueden tocar un tmux privado: si su programa
/// es un tmux que existe (el real, el guardián o un envoltorio llamado
/// `tmux`), el prefijo lleva `-S <socket>`. Los dobles que no son tmux
/// (`sh -c …`, `tail`, `/no-existe/tmux`) no alcanzan ningún servidor.
pub fn assert_private_tmux(opts: &NativeOptions) {
    let program = &opts.tmux.program;
    let socket = program
        .prefix
        .windows(2)
        .any(|w| w.first().is_some_and(|a| a == "-S") && w.get(1).is_some_and(|s| !s.is_empty()));
    let named_tmux = program.path.file_name().is_some_and(|n| n == "tmux");
    let reaches_tmux =
        named_tmux && (program.path.is_file() || program.path.components().count() == 1);
    assert!(
        socket || !reaches_tmux,
        "opts.tmux ({}) sin -S: alcanzaría el servidor tmux del usuario",
        program.path.display()
    );
}

/// `systemd-run` falso en `<home>/fakescope/systemd-run`: anota su argv (una
/// línea por argumento y `--` al final) en `<home>/fakescope/argv`, exige las
/// banderas de `scope_cmd` (`--user --scope --collect --quiet`) y ejecuta el
/// resto, que es el tmux de la prueba con su `-S` privado. Nunca toca systemd.
pub fn fake_scope(home: &TestHome) -> Program {
    use std::os::unix::fs::PermissionsExt;
    let dir = home.root.join("fakescope");
    std::fs::create_dir_all(&dir).unwrap();
    let log = dir.join("argv");
    let path = dir.join("systemd-run");
    std::fs::write(
        &path,
        format!(
            "#!/bin/sh\n\
             printf '%s\\0' systemd-run \"$@\" \"$(printf '\\036')\" >> '{log}'\n\
             [ \"$1\" = --user ] && [ \"$2\" = --scope ] && [ \"$3\" = --collect ] \
             && [ \"$4\" = --quiet ] || exit 97\n\
             shift 4\n\
             exec \"$@\"\n",
            log = log.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    comandos_server::dash::native::quick::scope_program(path)
}

/// Llamadas al `systemd-run` falso, cada una como su lista de argumentos. El
/// registro separa con NUL (`oracle::calls_in`): un argumento con saltos de
/// línea no parte la llamada.
pub fn fake_scope_calls(home: &TestHome) -> Vec<Vec<String>> {
    oracle::calls_in(&home.root.join("fakescope/argv"))
        .into_iter()
        .map(|call| call.args)
        .collect()
}

/// Copia `/bin/sleep` como `<home>/bin/<name>`: un proceso con ese argv[0]
/// para `agent_procs` (nunca un agente real).
pub fn fake_agent(home: &TestHome, name: &str) -> PathBuf {
    let path = home.root.join("bin").join(name);
    std::fs::copy("/bin/sleep", &path).unwrap();
    path
}

/// Sesión del tmux privado con `cmd` como proceso del pane, sin el entorno
/// del desarrollador (`env -i`), con el HOME de la prueba.
pub fn start_session(home: &TestHome, name: &str, cmd: &str) {
    let status = home
        .tmux_command()
        .args(["new-session", "-d", "-s", name, "-c"])
        .arg(&home.root)
        .arg(format!(
            "env -i HOME={} PATH=/usr/bin:/bin {cmd}",
            home.root.display()
        ))
        .status()
        .unwrap();
    assert!(status.success());
}

/// El socket privado de `socket_dir` (`<socket_dir>/tmux-<uid>/default`).
pub fn private_socket_of(socket_dir: &Path) -> PathBuf {
    private_socket(socket_dir)
}

/// `tmux` con `-f /dev/null -S <socket_dir>/tmux-<uid>/default`; crea el
/// directorio del socket (0700) para que tmux lo acepte.
pub fn private_tmux_command(socket_dir: &Path) -> Command {
    let socket = private_socket(socket_dir);
    if let Some(parent) = socket.parent() {
        use std::os::unix::fs::DirBuilderExt;
        let _ = std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(parent);
    }
    let mut cmd = Command::new("tmux");
    cmd.arg("-f")
        .arg("/dev/null")
        .arg("-S")
        .arg(socket)
        .env_remove("TMUX")
        .env("TMUX_TMPDIR", socket_dir);
    cmd
}

/// `kill-server` del servidor privado de `tmux_dir`, solo por su `-S`.
fn kill_private_server(tmux_dir: &Path) {
    let tmux = oracle::real_program("tmux").unwrap_or_else(|| PathBuf::from("tmux"));
    let _ = Command::new(tmux)
        .arg("-S")
        .arg(private_socket(tmux_dir))
        .arg("kill-server")
        .env_remove("TMUX")
        .env("TMUX_TMPDIR", tmux_dir)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

impl Drop for TestHome {
    fn drop(&mut self) {
        // Nunca el servidor tmux del usuario: socket explícito con `-S`. Solo
        // `TMUX_TMPDIR` no basta: si el directorio ya no existe, tmux cae en
        // silencio al `/tmp/tmux-<uid>/default` real y el kill-server es suyo.
        kill_private_server(&self.tmux_dir());
        // Algo que siguiera vivo (una tarea del frente, un hijo suelto) podría
        // volver a arrancar el servidor entre el `kill-server` y el borrado.
        // Tras renombrar, el socket viejo ya no tiene directorio (tmux con `-S`
        // daría «no server running»), y un servidor que se colara se mata en
        // la ruta nueva antes de borrar.
        let mut dying = self.root.clone().into_os_string();
        dying.push(".dying");
        let dying = PathBuf::from(dying);
        // Un `.dying` viejo (corrida anterior abortada) puede tener aún su
        // servidor vivo: se mata por su `-S` antes de borrar el directorio.
        if dying.exists() {
            kill_private_server(&dying.join("tmux"));
        }
        let _ = std::fs::remove_dir_all(&dying);
        let root = if std::fs::rename(&self.root, &dying).is_ok() {
            kill_private_server(&dying.join("tmux"));
            dying
        } else {
            self.root.clone()
        };
        let _ = std::fs::remove_dir_all(&root);
    }
}

pub struct Front {
    pub port: u16,
    stop: watch::Sender<bool>,
    task: Option<JoinHandle<std::io::Result<()>>>,
}

impl Front {
    pub async fn stop(mut self) {
        let _ = self.stop.send(true);
        if let Some(task) = self.task.take() {
            let _ = task.await;
        }
    }
}

/// Un `Front` que no se paró (la prueba falló antes del `stop`) no deja el
/// servidor vivo: se pide la parada y se aborta su tarea.
impl Drop for Front {
    fn drop(&mut self) {
        let _ = self.stop.send(true);
        if let Some(task) = self.task.take() {
            task.abort();
        }
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
    assert_private_tmux(&opts);
    let (stop, shutdown) = watch::channel(false);
    let cfg = config(home, legacy_port);
    let task = tokio::spawn(serve_with(listener, cfg, Some(opts), shutdown));
    Front {
        port,
        stop,
        task: Some(task),
    }
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

/// Heredado con status y cuerpo fijos para cualquier ruta; anota la línea de
/// petición. Al soltarse deja de escuchar.
pub struct FixedLegacy {
    pub port: u16,
    pub seen: Arc<Mutex<Vec<String>>>,
    task: JoinHandle<()>,
}

impl FixedLegacy {
    pub async fn start(status: u16, body: &'static str) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let log = seen.clone();
        let task = tokio::spawn(async move {
            while let Ok((mut stream, _)) = listener.accept().await {
                let log = log.clone();
                tokio::spawn(async move {
                    let mut buf = Vec::new();
                    let mut chunk = [0u8; 4096];
                    while !buf.windows(4).any(|w| w == b"\r\n\r\n") {
                        match stream.read(&mut chunk).await {
                            Ok(0) | Err(_) => return,
                            Ok(n) => buf.extend_from_slice(&chunk[..n]),
                        }
                    }
                    let head = String::from_utf8_lossy(&buf).to_string();
                    log.lock()
                        .unwrap()
                        .push(head.lines().next().unwrap_or("").into());
                    let reply = format!(
                        "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    let _ = stream.write_all(reply.as_bytes()).await;
                });
            }
        });
        Self { port, seen, task }
    }
    pub fn requests(&self) -> Vec<String> {
        self.seen.lock().unwrap().clone()
    }
}

impl Drop for FixedLegacy {
    fn drop(&mut self) {
        self.task.abort();
    }
}

/// OAuth falso: respuestas programadas por token; cuenta las llamadas.
#[derive(Default)]
pub struct FakeOauth {
    pub script: Mutex<std::collections::HashMap<String, FakeAnswer>>,
    pub calls: std::sync::atomic::AtomicUsize,
}

#[derive(Clone)]
pub enum FakeAnswer {
    Json(serde_json::Value),
    Error(String),
    /// Nunca responde (la tarea de refresco queda colgada).
    Hang,
}

impl FakeOauth {
    pub fn calls(&self) -> usize {
        self.calls.load(std::sync::atomic::Ordering::SeqCst)
    }
    pub fn set(&self, token: &str, answer: FakeAnswer) {
        self.script.lock().unwrap().insert(token.to_owned(), answer);
    }
}

impl comandos_server::dash::native::usage::limits::OauthHttp for FakeOauth {
    fn get_json(
        &self,
        _url: &'static str,
        token: String,
        _timeout: Duration,
    ) -> comandos_server::dash::native::usage::limits::HttpFuture {
        self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let answer = self
            .script
            .lock()
            .unwrap()
            .get(&token)
            .cloned()
            .unwrap_or(FakeAnswer::Error("sin guion".into()));
        Box::pin(async move {
            match answer {
                FakeAnswer::Json(v) => Ok(v),
                FakeAnswer::Error(e) => Err(e),
                FakeAnswer::Hang => std::future::pending().await,
            }
        })
    }
}

/// cc-notifyd falso: guarda cada cuerpo que se le manda, sin red.
#[derive(Default)]
pub struct FakeNotify {
    pub sent: Mutex<Vec<String>>,
}

impl FakeNotify {
    pub fn bodies(&self) -> Vec<String> {
        self.sent.lock().unwrap().clone()
    }
}

impl comandos_server::dash::native::usage::pane_models::NotifyPost for FakeNotify {
    fn post(
        &self,
        body: String,
    ) -> comandos_server::dash::native::usage::pane_models::NotifyFuture {
        self.sent.lock().unwrap().push(body);
        Box::pin(async {})
    }
}

/// Crea la base de uso del HOME con su esquema y ejecuta `sql`.
pub fn seed_usage(home: &TestHome, sql: &str) {
    let conn = comandos_store::usage::open_usage_db_at(&home.usage_db()).unwrap();
    comandos_store::usage::ensure_schema(&conn).unwrap();
    conn.execute_batch(sql).unwrap();
}
