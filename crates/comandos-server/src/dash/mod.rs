//! `comandos dash`: frente del tablero en 127.0.0.1:4777.
//!
//! Rust aplica la puerta de seguridad portada (transporte + `dashboard_access`)
//! y sirve los estáticos; lo demás se reenvía al `cc-dash` Python heredado en
//! `legacy_port`. Fase 2a: los estáticos los sirve `statics` y lo demás lo
//! reenvía `forward` con cabeceras y cuerpo intactos. Fase 2b: las rutas de
//! `native` se responden en Rust (salvo `Decline`); `--no-native` vuelve a la 2a.
pub mod forward;
pub mod native;
pub mod router;
pub mod statics;
pub mod term;
pub mod token;

pub use token::{load_token, token_path};

use crate::{AssetExists, Config, Handler, HandlerError, Limits, Reply, Request};
use http::{Method, StatusCode};
use router::RouteClass;
use std::{
    fmt, io,
    net::{Ipv4Addr, SocketAddr},
    path::{Component, Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use tokio::{net::TcpListener, sync::watch};

pub const DEFAULT_PORT: u16 = 4777;
pub const DEFAULT_LEGACY_PORT: u16 = 4781;
pub const LEGACY_PORT_ENV: &str = "COMANDOS_DASH_LEGACY_PORT";
pub const DASH_DIR_ENV: &str = "COMANDOS_DASH_DIR";
pub const NATIVE_ENV: &str = "COMANDOS_DASH_NATIVE";
pub const TRACE_ENV: &str = "COMANDOS_DASH_TRACE_FORWARD";
/// Raíz del checkout del Python heredado (`REPO_ROOT`, `bin/cc-dash:1370`).
pub const REPO_ENV: &str = comandos_core::repo::REPO_ENV;

#[derive(Clone)]
pub struct DashConfig {
    pub port: u16,
    /// Puerto del `cc-dash` Python al que se reenvía lo no nativo.
    pub legacy_port: u16,
    pub home: PathBuf,
    /// Raíz de estáticos; sus entradas suelen ser symlinks al repositorio.
    pub dash_dir: PathBuf,
    pub token: Vec<u8>,
    /// Falso con `--no-native` o `COMANDOS_DASH_NATIVE=0`: todo se reenvía (2a).
    pub native: bool,
    /// `app-state.sqlite3`, resuelto como `lib/app_state.py`.
    pub state_db: PathBuf,
    /// `COMANDOS_DASH_TRACE_FORWARD=1`: una línea en stderr por reenvío.
    pub trace_forward: bool,
    /// Checkout del heredado (`config/model-tiers.json`); `None` si no se sabe.
    pub repo_root: Option<PathBuf>,
}

impl fmt::Debug for DashConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // El token nunca aparece en trazas.
        f.debug_struct("DashConfig")
            .field("port", &self.port)
            .field("legacy_port", &self.legacy_port)
            .field("home", &self.home)
            .field("dash_dir", &self.dash_dir)
            .field("token", &"<oculto>")
            .field("native", &self.native)
            .field("state_db", &self.state_db)
            .field("trace_forward", &self.trace_forward)
            .field("repo_root", &self.repo_root)
            .finish()
    }
}

/// `[puerto] [--no-open] [--legacy-port N] [--no-native]`. Como el Python, el
/// primer argumento numérico es el puerto y los demás argumentos se ignoran;
/// `--no-open` se acepta y no hace nada (nunca se abre navegador).
/// `--no-native` reenvía todo lo no estático, como en la Fase 2a.
/// Devuelve `dash_dir`, `state_db` por defecto y token vacío: `from_env` los resuelve.
pub fn parse_args(
    args: &[String],
    home: &Path,
    legacy_env: Option<&str>,
) -> Result<DashConfig, String> {
    let mut port = None;
    let mut legacy_flag = None;
    let mut native = true;
    let mut words = args.iter();
    while let Some(word) = words.next() {
        if word == "--no-native" {
            native = false;
        } else if word == "--legacy-port" {
            let value = words
                .next()
                .ok_or_else(|| "--legacy-port requiere un puerto".to_string())?;
            legacy_flag = Some(parse_port("--legacy-port", value)?);
        } else if let Some(value) = word.strip_prefix("--legacy-port=") {
            legacy_flag = Some(parse_port("--legacy-port", value)?);
        } else if port.is_none() && !word.is_empty() && word.bytes().all(|b| b.is_ascii_digit()) {
            port = Some(parse_port("puerto", word)?);
        }
    }
    let legacy_port = match (legacy_flag, legacy_env.filter(|v| !v.is_empty())) {
        (Some(port), _) => port,
        (None, Some(value)) => parse_port(LEGACY_PORT_ENV, value)?,
        (None, None) => DEFAULT_LEGACY_PORT,
    };
    let port = port.unwrap_or(DEFAULT_PORT);
    if port == legacy_port {
        return Err(format!(
            "el puerto heredado ({legacy_port}) no puede ser el del tablero"
        ));
    }
    Ok(DashConfig {
        port,
        legacy_port,
        home: home.to_path_buf(),
        dash_dir: default_dash_dir(home),
        token: Vec::new(),
        native,
        state_db: home.join(".local/state/comandos/app-state.sqlite3"),
        trace_forward: false,
        repo_root: None,
    })
}

fn parse_port(name: &str, value: &str) -> Result<u16, String> {
    value
        .parse::<u16>()
        .map_err(|_| format!("{name}: puerto no válido: {value}"))
}

fn default_dash_dir(home: &Path) -> PathBuf {
    home.join(".claude/hooks/dash")
}

/// `_dashboard_directory()` del Python: sin override, `~/.claude/hooks/dash`;
/// con override, `abspath(expanduser(..))` que debe ser un directorio legible.
pub fn dash_dir(home: &Path, override_dir: Option<&str>) -> Result<PathBuf, String> {
    let Some(raw) = override_dir.filter(|v| !v.is_empty()) else {
        return Ok(default_dash_dir(home));
    };
    let expanded = if raw == "~" {
        home.to_path_buf()
    } else if let Some(rest) = raw.strip_prefix("~/") {
        home.join(rest)
    } else {
        PathBuf::from(raw)
    };
    let absolute = if expanded.is_absolute() {
        expanded
    } else {
        std::env::current_dir()
            .map_err(|e| format!("{DASH_DIR_ENV}: sin directorio actual: {e}"))?
            .join(expanded)
    };
    let candidate = normalize(&absolute);
    // Directorio (tras symlinks) que se puede listar: equivale a R_OK | X_OK.
    if candidate.is_dir() && std::fs::read_dir(&candidate).is_ok() {
        Ok(candidate)
    } else {
        Err(format!(
            "{DASH_DIR_ENV} no es un directorio legible: {}",
            candidate.display()
        ))
    }
}

/// `COMANDOS_DASH_REPO` si está; si no, el destino canónico de
/// `<dash_dir>/index.html` dos niveles arriba. La regla es la de
/// `comandos-core` (compartida con `comandos-notifyd`); aquí se resuelve el
/// enlace, que es I/O.
pub fn repo_root(dash_dir: &Path, override_dir: Option<&str>) -> Option<PathBuf> {
    let index = std::fs::canonicalize(dash_dir.join("index.html")).ok();
    comandos_core::repo::repo_root_from(index.as_deref(), override_dir)
}

/// `os.path.normpath` léxico: quita `.` y resuelve `..` sin tocar el disco.
fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out
}

/// Configuración completa desde el proceso: `HOME`, `COMANDOS_DASH_LEGACY_PORT`,
/// `COMANDOS_DASH_DIR`, `COMANDOS_DASH_NATIVE`, `COMANDOS_DASH_TRACE_FORWARD`,
/// la ruta de `app-state.sqlite3` y el token (creado si falta).
pub fn from_env(args: &[String]) -> Result<DashConfig, StartError> {
    let home = std::env::var_os("HOME")
        .filter(|h| !h.is_empty())
        .map(PathBuf::from)
        .ok_or_else(|| StartError::Usage("HOME no está definido".into()))?;
    let legacy = std::env::var(LEGACY_PORT_ENV).ok();
    let mut cfg = parse_args(args, &home, legacy.as_deref()).map_err(StartError::Usage)?;
    let override_dir = std::env::var(DASH_DIR_ENV).ok();
    cfg.dash_dir = dash_dir(&home, override_dir.as_deref()).map_err(StartError::Config)?;
    cfg.repo_root = repo_root(&cfg.dash_dir, std::env::var(REPO_ENV).ok().as_deref());
    if std::env::var(NATIVE_ENV).is_ok_and(|v| v == "0") {
        cfg.native = false;
    }
    cfg.trace_forward = std::env::var(TRACE_ENV).is_ok_and(|v| v == "1");
    let state_override = std::env::var_os("COMANDOS_STATE_DB").map(PathBuf::from);
    let xdg = std::env::var_os("XDG_STATE_HOME").map(PathBuf::from);
    cfg.state_db =
        comandos_runtime::state_path(None, state_override.as_deref(), xdg.as_deref(), Some(&home))
            .map_err(|e| StartError::Config(format!("app-state: {e}")))?;
    cfg.token = load_token(&home).map_err(|e| {
        StartError::Config(format!("dash-token ({}): {e}", token_path(&home).display()))
    })?;
    Ok(cfg)
}

#[derive(Debug)]
pub enum StartError {
    /// Argumentos o entorno mal formados: salida 2.
    Usage(String),
    /// Configuración inutilizable (como el `SystemExit` del Python): salida 1.
    Config(String),
}

/// Límites del frente. Las clases de ruta de la Fase 2b los afinarán por ruta.
pub fn limits() -> Limits {
    Limits {
        connections: 256,
        websockets: 64,
        header_bytes: 64 * 1024,
        buffered_wire_bytes: 20_000_000 + 64 * 1024,
        header_timeout: Duration::from_secs(30),
        body_timeout: Duration::from_secs(30),
        handler_timeout: Duration::from_secs(120),
        write_timeout: Duration::from_secs(30),
        shutdown_grace: Duration::from_secs(2),
    }
}

/// `isfile(join(DASH, path.lstrip("/")))`: los enlaces de `dash_dir` apuntan
/// al repositorio, así que basta con que el destino resuelto sea un archivo.
pub fn asset_exists(dash_dir: &Path) -> AssetExists {
    let root = dash_dir.to_path_buf();
    Arc::new(move |path: &str| {
        std::fs::metadata(root.join(path.trim_start_matches('/'))).is_ok_and(|m| m.is_file())
    })
}

/// Estado compartido por las ramas del enrutador.
pub struct DashState {
    pub config: DashConfig,
    pub asset_exists: AssetExists,
    pub native: Option<Arc<native::Native>>,
}

pub fn handler(state: Arc<DashState>) -> Handler {
    Arc::new(move |request| {
        let state = state.clone();
        Box::pin(async move { handle(&state, request).await })
    })
}

async fn handle(state: &DashState, request: Request) -> Result<Reply, HandlerError> {
    let live = state.native.as_ref().filter(|n| n.enabled());
    if live.is_none()
        && let Some(reply) = state.native.as_ref().and_then(|n| n.typing_retry(&request))
    {
        return Ok(reply);
    }
    let class = router::classify_with(
        &request.method,
        &request.target,
        &*state.asset_exists,
        live.is_some(),
    );
    match class {
        RouteClass::Native(route) => {
            let Some(native) = live else {
                return forward_to_legacy(state, request).await;
            };
            match native.dispatch(route, &request).await? {
                native::Outcome::Reply(reply) => Ok(reply),
                native::Outcome::Decline => forward_to_legacy(state, request).await,
            }
        }
        RouteClass::Static => statics::serve(&state.config.dash_dir, &request).await,
        // HEAD solo existe para estáticos; a una ruta API es 404 (el Python
        // devolvía 404 HTML vía SimpleHTTPRequestHandler.do_HEAD).
        RouteClass::Forward if request.method == Method::HEAD => not_found(),
        RouteClass::Forward => forward_to_legacy(state, request).await,
    }
}

async fn forward_to_legacy(state: &DashState, request: Request) -> Result<Reply, HandlerError> {
    if state.config.trace_forward {
        eprintln!("{}", trace_line(&request.method, &request.target));
    }
    let legacy = SocketAddr::from((Ipv4Addr::LOCALHOST, state.config.legacy_port));
    forward::relay(legacy, request).await
}

/// Sin consulta: los `deviceId` y tokens nunca llegan a los registros.
pub fn trace_line(method: &Method, target: &str) -> String {
    format!(
        "comandos dash: reenvío {method} {}",
        router::path_of(target)
    )
}

pub fn not_found() -> Result<Reply, HandlerError> {
    Reply::json(
        StatusCode::NOT_FOUND,
        &serde_json::json!({"error": "No encontrado"}),
    )
}

/// Transporte + conjunto nativo. `opts` sustituye a las opciones derivadas
/// de `cfg` (las pruebas inyectan reloj, tmux y `fc-list`).
pub fn build(
    cfg: DashConfig,
    opts: Option<native::NativeOptions>,
) -> (Config, Option<Arc<native::Native>>) {
    let asset_exists = asset_exists(&cfg.dash_dir);
    let token = cfg.token.clone();
    let native = cfg.native.then(|| {
        let mut o = opts.unwrap_or_else(|| {
            let mut o = native::NativeOptions::for_home(&cfg.home, cfg.state_db.clone());
            o.repo_root = cfg.repo_root.clone();
            o
        });
        // El contexto de sugerencias habla con el MISMO heredado al que se reenvía.
        o.legacy = SocketAddr::from((Ipv4Addr::LOCALHOST, cfg.legacy_port));
        o.legacy_token = cfg.token.clone();
        Arc::new(native::Native::new(o))
    });
    let state = Arc::new(DashState {
        config: cfg,
        asset_exists: asset_exists.clone(),
        native: native.clone(),
    });
    let config = Config {
        websocket: None,
        token,
        asset_exists,
        handler: handler(state),
        limits: limits(),
    };
    (config, native)
}

/// Configuración del transporte para un `DashConfig` ya resuelto (API de la 2a).
pub fn transport_config(cfg: DashConfig) -> Config {
    build(cfg, None).0
}

/// Atiende con opciones nativas explícitas hasta que `shutdown` pase a `true`;
/// al terminar para el worker de la base.
pub async fn serve_with(
    listener: TcpListener,
    cfg: DashConfig,
    opts: Option<native::NativeOptions>,
    shutdown: watch::Receiver<bool>,
) -> io::Result<()> {
    let (config, native) = build(cfg, opts);
    if let Some(native) = &native {
        // Abre la base antes de atender: la primera petición no paga la migración.
        native.ready().await;
    }
    let served = crate::serve(listener, config, shutdown).await;
    if let Some(native) = native {
        native.shutdown().await;
    }
    served
}

/// Atiende en un listener ya abierto hasta que `shutdown` pase a `true`.
pub async fn serve_listener(
    listener: TcpListener,
    cfg: DashConfig,
    shutdown: watch::Receiver<bool>,
) -> io::Result<()> {
    serve_with(listener, cfg, None, shutdown).await
}

/// Solo IPv4 de bucle local, como el `ThreadingTCPServer(("127.0.0.1", port))`.
pub async fn bind(port: u16) -> io::Result<TcpListener> {
    TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, port))).await
}

pub async fn run(cfg: DashConfig, shutdown: watch::Receiver<bool>) -> io::Result<()> {
    let listener = bind(cfg.port).await?;
    serve_listener(listener, cfg, shutdown).await
}

/// Tope de hilos de bloqueo del frente. Con el de tokio (512) una ola de
/// peticiones simultáneas (los iframes de `term.html` que despiertan a la vez
/// con `visibilitychange`, cada `/terminal-panes` en su hilo) creaba 17 hilos
/// de golpe, y ninguno volvía a quedar 10 s ocioso: el pool despierta a sus
/// hilos por turno y GET `/state` daba cientos de saltos por cómputo, así que
/// la recolección se repartía entre todos y cada uno engordaba su propia arena
/// de glibc (en vivo tras la 2d: 4 → 24 hilos y 45 → 198 MiB de Pss). Ahora
/// `/state` tiene su hilo (`states::serial`) y una ola de `/terminal-panes`
/// ocupa como mucho un hilo (`terminal::PANES_GATE`), así que el pool queda
/// para el trabajo esporádico: estáticos, historial, tecleo, escrituras.
///
/// Ningún trabajo del pool espera a otro trabajo del pool (solo a procesos de
/// tmux que mueve el hilo del runtime; los candados que se toman dentro son
/// `try_lock` o, el de `/terminal-panes`, ya serializado por su puerta
/// asíncrona), así que el tope no puede interbloquear: lo que no cabe espera
/// en la cola. Ocho deja sitio a varios tecleos largos de `/pane/type`
/// sin frenar los estáticos.
pub const MAX_BLOCKING_THREADS: usize = 8;

/// Ocio tras el que un hilo de bloqueo se retira (tokio: 10 s).
pub const BLOCKING_KEEP_ALIVE: Duration = Duration::from_secs(2);

/// El runtime del frente: monohilo para la red y un pool de bloqueo acotado.
pub fn runtime() -> io::Result<tokio::runtime::Runtime> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .max_blocking_threads(MAX_BLOCKING_THREADS)
        .thread_keep_alive(BLOCKING_KEEP_ALIVE)
        .build()
}

/// Punto de entrada del binario: runtime monohilo, señales y código de salida.
pub fn main(args: &[String]) -> i32 {
    let cfg = match from_env(args) {
        Ok(cfg) => cfg,
        Err(StartError::Usage(message)) => {
            eprintln!("comandos dash: {message}");
            return 2;
        }
        Err(StartError::Config(message)) => {
            eprintln!("{message}");
            return 1;
        }
    };
    let runtime = match runtime() {
        Ok(runtime) => runtime,
        Err(error) => {
            eprintln!("comandos dash: runtime: {error}");
            return 1;
        }
    };
    runtime.block_on(serve_until_signal(cfg))
}

async fn serve_until_signal(cfg: DashConfig) -> i32 {
    use tokio::signal::unix::{SignalKind, signal};
    let port = cfg.port;
    let listener = match bind(port).await {
        Ok(listener) => listener,
        Err(error) => {
            eprintln!("comandos dash: no se pudo escuchar en 127.0.0.1:{port}: {error}");
            return 1;
        }
    };
    // Las señales se instalan antes del aviso: quien lo lee ya puede parar.
    let (Ok(mut term), Ok(mut int)) = (
        signal(SignalKind::terminate()),
        signal(SignalKind::interrupt()),
    ) else {
        eprintln!("comandos dash: no se pudieron instalar las señales");
        return 1;
    };
    let (stop, shutdown) = watch::channel(false);
    tokio::spawn(async move {
        tokio::select! {
            _ = term.recv() => {}
            _ = int.recv() => {}
        }
        let _ = stop.send(true);
    });
    {
        use std::io::Write;
        let mut out = std::io::stdout().lock();
        let _ = writeln!(
            out,
            "Centro Claude corriendo en http://127.0.0.1:{port}  (Ctrl+C para salir)"
        );
        let _ = out.flush();
    }
    match serve_listener(listener, cfg, shutdown).await {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("comandos dash: {error}");
            1
        }
    }
}
