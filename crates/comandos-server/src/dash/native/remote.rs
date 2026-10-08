//! Corte `services` (plan 2f-3, Tarea 2): acceso remoto por Tailscale y
//! terminal web de `bin/cc-dash` (`remote_urls` 4843, `http_healthy` 4853,
//! `webterm_health` 4863, `remote_status_from_text` 4871, `run_quiet` 4896,
//! `tailscale_host` 4903, `remote_state_cached` 4938, `remote_state` 4957,
//! `serve_path` 4975, `remote_dashboard_on` 4989, `webterm_on` 5004,
//! `webterm_off` 5016, `restore_requested_webterm` 5027).
//!
//! - GET `/remote-state` (prefijo): la foto en caché (15 s; vieja → se
//!   responde y se refresca en una tarea registrada).
//! - GET `/remote-qr.png` (prefijo): `qrencode` sobre la URL del tablero.
//! - POST `/remote-on`, `/remote-off`, `/remote-webterm-on`,
//!   `/remote-webterm-off`: órdenes de `tailscale serve` y `cc-webterm` con los
//!   plazos del Python; responden la foto recién calculada.
//!
//! D9: `/remote-off` NUNCA ejecuta `tailscale serve reset` (la máquina tiene
//! servicios ajenos en 8444–8447); apaga solo lo suyo: `--https=443 off` y
//! `--https=8443 off`, y después la terminal web.
//!
//! Todo programa se lanza por la ruta absoluta de `which_in` sobre el `PATH`
//! del frente, con el entorno de `NativeOptions::program`; los textos de error
//! que el Python saca de `str(exc)` (`FileNotFoundError`, `TimeoutExpired`,
//! `UnicodeDecodeError`) se reproducen porque pueden llegar a un `error`.
//!
//! Ligereza: dos saltos al pool de bloqueo por POST (resolver programas;
//! programas y token de la foto final), uno por foto calculada y dos por QR
//! (resolver `qrencode` y crear el temporal; leerlo y borrarlo). Procesos y
//! sondas HTTP en el runtime, una sola foto en caché y como mucho un refresco
//! en vuelo.
//!
//! Desviaciones mínimas del Python: los hijos tienen stdin `/dev/null` (el
//! Python les deja el del servidor, que ya es `/dev/null` bajo systemd); la
//! sonda de salud no sigue redirecciones (ttyd nunca redirige) ni usa
//! `http_proxy`; las dos sondas van a la vez; la caché mide con reloj
//! monotónico; y el `json.loads` de `status --json` no acepta `NaN`.
use super::{
    Answer, Entry, Fault, Key, Native, NativeOptions, NativeRoute, Verb, light::error,
    procs::which_in, py::repr_ascii, reply, settings::access_token,
};
use crate::{HandlerError, Reply, Request};
use comandos_core::text::strip;
use http::StatusCode;
use regex::Regex;
use serde_json::{Map, Value, json};
use std::{
    io,
    net::Ipv4Addr,
    os::unix::fs::OpenOptionsExt,
    path::{Path, PathBuf},
    process::Stdio,
    sync::{Arc, Mutex, OnceLock},
    time::{Duration, Instant},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemoteRoute {
    State,
    Qr,
    On,
    Off,
    WebtermOn,
    WebtermOff,
}

pub const ROUTES: &[Entry] = &[
    Entry {
        verb: Verb::Get,
        key: Key::Prefix("/remote-state"),
        route: NativeRoute::Remote(RemoteRoute::State),
    },
    Entry {
        verb: Verb::Get,
        key: Key::Prefix("/remote-qr.png"),
        route: NativeRoute::Remote(RemoteRoute::Qr),
    },
    Entry {
        verb: Verb::Post,
        key: Key::Raw("/remote-on"),
        route: NativeRoute::Remote(RemoteRoute::On),
    },
    Entry {
        verb: Verb::Post,
        key: Key::Raw("/remote-off"),
        route: NativeRoute::Remote(RemoteRoute::Off),
    },
    Entry {
        verb: Verb::Post,
        key: Key::Raw("/remote-webterm-on"),
        route: NativeRoute::Remote(RemoteRoute::WebtermOn),
    },
    Entry {
        verb: Verb::Post,
        key: Key::Raw("/remote-webterm-off"),
        route: NativeRoute::Remote(RemoteRoute::WebtermOff),
    },
];

/// `remote_state_cached(max_age=15)`.
const MAX_AGE: Duration = Duration::from_secs(15);
/// Plazos de `run_quiet` del Python (segundos enteros: entran en el texto de
/// `TimeoutExpired`).
const STATUS_SECONDS: u64 = 8;
const SERVE_SECONDS: u64 = 12;
const QRENCODE_SECONDS: u64 = 5;
/// `http_healthy(url, timeout=0.4)`: plazo de cada operación del socket.
const HEALTH_TIMEOUT: Duration = Duration::from_millis(400);
/// `_MAXLINE` de `http.client`: una línea más larga es un error (no sano).
const MAX_LINE: usize = 65_536;

/// `WEBTERM_ENABLED_FILE` (164): `HOOKS/webterm-enabled`.
pub const WEBTERM_ENABLED_FILE: &str = "webterm-enabled";
/// Destinos literales de `tailscale serve` (no son los puertos de salud de
/// `opts`: esos solo cambian las sondas, R5).
const DASHBOARD_TARGET: &str = "http://127.0.0.1:4777";
const TERM_TARGET: &str = "http://127.0.0.1:4780/term";
const FALLBACK_TARGET: &str = "http://127.0.0.1:4779";

const NOT_INSTALLED: &str = "Tailscale no esta instalado";
const NOT_LOGGED_IN: &str = "Tailscale no ha iniciado sesion";
const NO_HOST: &str = "No pude leer el host de Tailscale";
const NO_QRENCODE: &str = "qrencode no esta instalado";

pub async fn answer(native: &Arc<Native>, route: RemoteRoute, _request: &Request) -> Answer {
    match route {
        RemoteRoute::State => {
            let state = cached(native).await?;
            reply(StatusCode::OK, &state)
        }
        RemoteRoute::Qr => qr(native).await,
        RemoteRoute::On => remote_on(native).await,
        RemoteRoute::Off => remote_off(native).await,
        RemoteRoute::WebtermOn => remote_webterm_on(native).await,
        RemoteRoute::WebtermOff => remote_webterm_off(native).await,
    }
}

fn failure() -> Fault {
    Fault::Error(HandlerError::Failure)
}

// ---------------------------------------------------------------------------
// Caché de la foto (`_remote_state_cache`)
// ---------------------------------------------------------------------------

/// `{"at", "data", "refreshing"}` del Python, por `Native`.
#[derive(Default)]
pub struct RemoteCache {
    slot: Mutex<Slot>,
}

#[derive(Default)]
struct Slot {
    at: Option<Instant>,
    data: Option<Arc<Value>>,
    refreshing: bool,
}

impl RemoteCache {
    fn lock(&self) -> std::sync::MutexGuard<'_, Slot> {
        self.slot.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// `_remote_state_cache.update({"at": time.time(), "data": data})`.
    fn store(&self, data: Arc<Value>) {
        let mut slot = self.lock();
        slot.at = Some(Instant::now());
        slot.data = Some(data);
    }
}

/// Baja `refreshing` al soltarse (el `finally` de `_remote_state_refresh`),
/// también si la tarea revienta o el runtime la abandona.
struct Refreshing(Arc<Native>);

impl Drop for Refreshing {
    fn drop(&mut self) {
        self.0.remote.lock().refreshing = false;
    }
}

/// `remote_state_cached()`: la última foto; vieja (≥ 15 s) y sin refresco en
/// curso → se lanza uno en una tarea registrada y se responde la vieja. Sin
/// foto, se calcula en línea.
async fn cached(native: &Arc<Native>) -> Result<Arc<Value>, Fault> {
    let (data, refresh) = {
        let mut slot = native.remote.lock();
        let stale = slot.at.is_none_or(|at| at.elapsed() >= MAX_AGE);
        let refresh = slot.data.is_some() && stale && !slot.refreshing;
        if refresh {
            slot.refreshing = true;
        }
        (slot.data.clone(), refresh)
    };
    if refresh {
        let guard = Refreshing(Arc::clone(native));
        let spawned = native.tasks().spawn(async move {
            let native = Arc::clone(&guard.0);
            // `except Exception: pass`: un fallo deja la foto vieja.
            let _ = remote_state(&native, false).await;
            drop(guard);
        });
        // Sin runtime (no pasa en el frente) el guardián ya se soltó dentro
        // de la tarea no lanzada: `refreshing` vuelve a falso.
        drop(spawned);
    }
    match data {
        Some(data) => Ok(data),
        None => remote_state(native, true).await,
    }
}

// ---------------------------------------------------------------------------
// Programas (`shutil.which`, `run_quiet`)
// ---------------------------------------------------------------------------

/// Los programas de una petición, resueltos de una vez en el pool de bloqueo.
struct Tools {
    tailscale: Option<PathBuf>,
    qrencode: Option<PathBuf>,
}

impl Tools {
    fn resolve(search: Option<&std::ffi::OsStr>) -> Tools {
        Tools {
            tailscale: which_in(search, "tailscale"),
            qrencode: which_in(search, "qrencode"),
        }
    }
}

/// Programas y, si se pide, el token (`access_token`) en un solo salto.
async fn prepare(
    opts: &NativeOptions,
    token: bool,
) -> Result<(Tools, Option<io::Result<String>>), Fault> {
    let search = opts.search_path.clone();
    let hooks = opts.hooks.clone();
    tokio::task::spawn_blocking(move || {
        let tools = Tools::resolve(search.as_deref());
        let token = token.then(|| access_token(&hooks));
        (tools, token)
    })
    .await
    .map_err(|_| failure())
}

/// `subprocess.CompletedProcess` de `run_quiet`: solo `returncode == 0` importa.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Quiet {
    ok: bool,
    stdout: String,
    stderr: String,
}

impl Quiet {
    /// `CompletedProcess(args, 1, "", str(e))`.
    fn failed(message: String) -> Quiet {
        Quiet {
            ok: false,
            stdout: String::new(),
            stderr: message,
        }
    }
}

/// `run_quiet([argv0, *args], timeout)`: `program` es la ruta que el Python
/// encontraría para `argv0` (`None` → `FileNotFoundError`). Toda excepción
/// (arranque, plazo, decodificación) da `rc 1`, `stdout ""` y `stderr = str(e)`.
///
/// Como `subprocess.run`, al vencer el plazo se mata solo al hijo directo y la
/// salida se lee hasta que se cierran las tuberías.
async fn run_quiet(
    opts: &NativeOptions,
    argv0: &str,
    program: Option<&Path>,
    args: &[&str],
    seconds: u64,
) -> Quiet {
    run_quiet_input(opts, argv0, program, args, None, seconds).await
}

/// `run_quiet` con `input` por stdin (como `communicate`: se escribe mientras
/// se lee la salida; un hijo que cierra stdin antes no es un error). Lo que va
/// por stdin nunca aparece en `argv` ni en el texto de un error.
async fn run_quiet_input(
    opts: &NativeOptions,
    argv0: &str,
    program: Option<&Path>,
    args: &[&str],
    input: Option<&[u8]>,
    seconds: u64,
) -> Quiet {
    let Some(path) = program else {
        return Quiet::failed(os_error_message(2, argv0));
    };
    let program = opts.program(path);
    let mut cmd = program.command();
    cmd.args(args);
    cmd.stdin(if input.is_some() {
        Stdio::piped()
    } else {
        Stdio::null()
    })
    .stdout(Stdio::piped())
    .stderr(Stdio::piped())
    .kill_on_drop(true);
    let mut child = match cmd.spawn() {
        Ok(child) => child,
        Err(e) => return Quiet::failed(io_message(&e, argv0)),
    };
    let stdin = child.stdin.take();
    let feed = async move {
        if let (Some(mut pipe), Some(input)) = (stdin, input) {
            let _ = pipe.write_all(input).await;
            // Al soltar `pipe` el hijo ve el fin de la entrada.
        }
    };
    let run = async move {
        let ((), output) = tokio::join!(feed, child.wait_with_output());
        output
    };
    let waited = tokio::time::timeout(Duration::from_secs(seconds), run).await;
    let output = match waited {
        Err(_) => return Quiet::failed(timeout_message(argv0, args, seconds)),
        Ok(Err(e)) => return Quiet::failed(io_message(&e, argv0)),
        Ok(Ok(output)) => output,
    };
    // `_translate_newlines`: primero stdout, después stderr.
    let stdout = match python_text(output.stdout) {
        Ok(text) => text,
        Err(message) => return Quiet::failed(message),
    };
    let stderr = match python_text(output.stderr) {
        Ok(text) => text,
        Err(message) => return Quiet::failed(message),
    };
    Quiet {
        ok: output.status.success(),
        stdout,
        stderr,
    }
}

/// `os.strerror(code)`: el texto de glibc (el `Display` de Rust le añade
/// `" (os error N)"`).
fn strerror(code: i32) -> String {
    let text = io::Error::from_raw_os_error(code).to_string();
    let suffix = format!(" (os error {code})");
    text.strip_suffix(&suffix).unwrap_or(&text).to_owned()
}

/// `str(OSError(code, strerror, filename))`.
fn os_error_message(code: i32, filename: &str) -> String {
    format!("[Errno {code}] {}: {}", strerror(code), py_repr(filename))
}

/// El error de arrancar o esperar al hijo como lo escribiría el Python: el
/// `filename` de `_execute_child` es el `args[0]` tal cual se pasó.
fn io_message(e: &io::Error, argv0: &str) -> String {
    match e.raw_os_error() {
        Some(code) => os_error_message(code, argv0),
        None => e.to_string(),
    }
}

/// `str(TimeoutExpired(cmd, timeout))` con `cmd` la lista del Python.
fn timeout_message(argv0: &str, args: &[&str], seconds: u64) -> String {
    let parts: Vec<String> = std::iter::once(argv0)
        .chain(args.iter().copied())
        .map(py_repr)
        .collect();
    format!(
        "Command '[{}]' timed out after {seconds} seconds",
        parts.join(", ")
    )
}

/// `repr(str)`. Fuera de ASCII (rutas con tildes) los caracteres de control
/// se escapan y el resto va tal cual, como `str.isprintable` para el texto
/// habitual (los separadores Unicode raros no se escapan: desviación anotada).
fn py_repr(s: &str) -> String {
    if let Some(repr) = repr_ascii(s) {
        return repr;
    }
    let quote = if s.contains('\'') && !s.contains('"') {
        '"'
    } else {
        '\''
    };
    let mut out = String::with_capacity(s.len() + 2);
    out.push(quote);
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            c if c == quote => {
                out.push('\\');
                out.push(c);
            }
            c if c.is_control() => {
                let code = u32::from(c);
                if code < 0x100 {
                    out.push_str(&format!("\\x{code:02x}"));
                } else {
                    out.push_str(&format!("\\u{code:04x}"));
                }
            }
            c => out.push(c),
        }
    }
    out.push(quote);
    out
}

/// `text=True`: UTF-8 estricto (`str(UnicodeDecodeError)` del Python si no) y
/// saltos universales.
fn python_text(bytes: Vec<u8>) -> Result<String, String> {
    match String::from_utf8(bytes) {
        Ok(text) => Ok(text.replace("\r\n", "\n").replace('\r', "\n")),
        Err(e) => Err(decode_message(e.as_bytes(), &e.utf8_error())),
    }
}

/// `str(UnicodeDecodeError)` del decodificador UTF-8 de CPython: la posición
/// es donde empieza la secuencia mala y el final, el de su parte inválida.
fn decode_message(bytes: &[u8], err: &std::str::Utf8Error) -> String {
    let start = err.valid_up_to();
    let byte = bytes.get(start).copied().unwrap_or(0);
    let (end, reason) = match err.error_len() {
        None => (bytes.len(), "unexpected end of data"),
        Some(len) if (0xC2..=0xF4).contains(&byte) => (start + len, "invalid continuation byte"),
        Some(len) => (start + len, "invalid start byte"),
    };
    if end <= start + 1 {
        format!("'utf-8' codec can't decode byte 0x{byte:02x} in position {start}: {reason}")
    } else {
        format!(
            "'utf-8' codec can't decode bytes in position {start}-{}: {reason}",
            end - 1
        )
    }
}

/// `(a or b or default).strip()`.
fn first_text(a: &str, b: &str, default: &str) -> String {
    let text = if !a.is_empty() {
        a
    } else if !b.is_empty() {
        b
    } else {
        default
    };
    strip(text).to_owned()
}

/// `if err:` del Python sobre un `str | None` (`""` es falso): el texto si hay
/// error.
fn error_text(err: Option<String>) -> Option<String> {
    err.filter(|e| !e.is_empty())
}

// ---------------------------------------------------------------------------
// Estado (`remote_state`)
// ---------------------------------------------------------------------------

/// `tailscale_host()`: `Self.DNSName` de `status --json` sin puntos finales;
/// si no, la primera `*.ts.net` de `status --self --json`.
async fn tailscale_host(opts: &NativeOptions, tools: &Tools) -> String {
    let program = tools.tailscale.as_deref();
    let r = run_quiet(
        opts,
        "tailscale",
        program,
        &["status", "--json"],
        STATUS_SECONDS,
    )
    .await;
    if r.ok
        && let Some(host) = dns_name(&r.stdout)
        && !host.is_empty()
    {
        return host;
    }
    let r = run_quiet(
        opts,
        "tailscale",
        program,
        &["status", "--self", "--json"],
        STATUS_SECONDS,
    )
    .await;
    ts_net(&r.stdout)
}

/// `json.loads(text).get("Self", {}).get("DNSName", "").rstrip(".")`; `None`
/// donde el Python lanza (y cae al segundo intento).
fn dns_name(text: &str) -> Option<String> {
    let doc: Value = serde_json::from_str(text).ok()?;
    let me = match doc.as_object()?.get("Self") {
        None => return Some(String::new()),
        Some(value) => value.as_object()?,
    };
    match me.get("DNSName") {
        None => Some(String::new()),
        Some(name) => Some(name.as_str()?.trim_end_matches('.').to_owned()),
    }
}

/// `re.search(r"[a-z0-9-]+(?:\.[a-z0-9-]+)*\.ts\.net", text)`.
fn ts_net(text: &str) -> String {
    static RE: OnceLock<Option<Regex>> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"[a-z0-9-]+(?:\.[a-z0-9-]+)*\.ts\.net").ok())
        .as_ref()
        .and_then(|re| re.find(text))
        .map(|m| m.as_str().to_owned())
        .unwrap_or_default()
}

/// `tailscale_serve_status_text()`.
async fn serve_status(opts: &NativeOptions, tools: &Tools) -> String {
    let r = run_quiet(
        opts,
        "tailscale",
        tools.tailscale.as_deref(),
        &["serve", "status"],
        STATUS_SECONDS,
    )
    .await;
    if r.ok { r.stdout } else { String::new() }
}

/// `webterm_health()`: `(primario, respaldo)`. `opts.webterm_health_ports` es
/// `[respaldo, primario]` (`[4779, 4780]` en producción).
async fn webterm_health(ports: [u16; 2]) -> (bool, bool) {
    let [fallback, primary] = ports;
    tokio::join!(
        http_healthy(primary, "/term/token"),
        http_healthy(fallback, "/token")
    )
}

/// `http_healthy(url, timeout=0.4)`: `urlopen` sin seguir nada raro; sano si
/// la respuesta es `2xx` (urllib levanta `HTTPError` en lo demás). Las
/// redirecciones no se siguen (ttyd nunca redirige: desviación anotada).
async fn http_healthy(port: u16, path: &str) -> bool {
    // Plazo total de la sonda: un servidor que gotea un byte cada 0,39 s no
    // la alarga sin fin (cada lectura sola sí respetaría los 0,4 s).
    tokio::time::timeout(PROBE_DEADLINE, probe(port, path))
        .await
        .ok()
        .flatten()
        .unwrap_or(false)
}

/// Tope de una sonda entera (conexión, petición y cabeceras).
const PROBE_DEADLINE: Duration = Duration::from_secs(1);

/// ¿Terminan las cabeceras en `head`? Solo mira lo recién leído (desde
/// `from`, con 3 bytes de solape por si el `\r\n\r\n` quedó partido).
fn headers_end(head: &[u8], from: usize) -> bool {
    let tail = head.get(from.saturating_sub(3)..).unwrap_or_default();
    tail.windows(4).any(|w| w == b"\r\n\r\n") || tail.windows(2).any(|w| w == b"\n\n")
}

async fn probe(port: u16, path: &str) -> Option<bool> {
    let connect = TcpStream::connect((Ipv4Addr::LOCALHOST, port));
    let mut stream = tokio::time::timeout(HEALTH_TIMEOUT, connect)
        .await
        .ok()?
        .ok()?;
    let request = format!(
        "GET {path} HTTP/1.1\r\nAccept-Encoding: identity\r\nHost: 127.0.0.1:{port}\r\n\
         User-Agent: ComandOS-health\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n"
    );
    tokio::time::timeout(HEALTH_TIMEOUT, stream.write_all(request.as_bytes()))
        .await
        .ok()?
        .ok()?;
    // `begin()` de http.client: línea de estado y cabeceras, cada lectura con
    // el plazo del socket; el cuerpo no se lee.
    let mut head: Vec<u8> = Vec::new();
    let mut buf = [0u8; 2048];
    let mut scanned = 0;
    loop {
        if headers_end(&head, scanned) {
            break;
        }
        scanned = head.len();
        if head.len() > MAX_LINE {
            return None;
        }
        let n = tokio::time::timeout(HEALTH_TIMEOUT, stream.read(&mut buf))
            .await
            .ok()?
            .ok()?;
        if n == 0 {
            break;
        }
        head.extend_from_slice(buf.get(..n)?);
    }
    let line = head.split(|&b| b == b'\n').next()?;
    if line.len() > MAX_LINE {
        return None;
    }
    // `line.split(None, 2)`: versión `HTTP/…` y estado de tres dígitos.
    let line = String::from_utf8_lossy(line);
    let mut parts = line.split_ascii_whitespace();
    let version = parts.next()?;
    let status = parts.next()?;
    if !version.starts_with("HTTP/") || status.len() != 3 {
        return None;
    }
    let status: u16 = status.parse().ok()?;
    Some((200..300).contains(&status))
}

/// `remote_status_from_text(serve_text, health)`.
fn status_from_text(serve: &str, (primary, fallback): (bool, bool)) -> Map<String, Value> {
    let remote_on = serve.contains("proxy http://127.0.0.1:4777");
    let term_route = serve.contains("proxy http://127.0.0.1:4780/term");
    let fallback_route = serve.contains("proxy http://127.0.0.1:4779");
    let primary_on = primary && term_route;
    let fallback_on = fallback && fallback_route;
    let state = if remote_on && primary_on {
        "active"
    } else if remote_on && fallback_on {
        "degraded"
    } else {
        "off"
    };
    let value = json!({
        "remoteOn": remote_on,
        "primaryHealthy": primary,
        "fallbackHealthy": fallback,
        "termRouteOn": term_route,
        "fallbackRouteOn": fallback_route,
        "primaryTerminalOn": primary_on,
        "fallbackTerminalOn": fallback_on,
        "webtermOn": primary_on,
        "webtermReachable": primary_on || fallback_on,
        "webtermDegraded": state == "degraded",
        "terminalState": state,
    });
    match value {
        Value::Object(map) => map,
        _ => Map::new(),
    }
}

/// `urllib.parse.quote(text, safe="")`.
fn quote_all(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for &b in text.as_bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'-' | b'~') {
            out.push(char::from(b));
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// `remote_urls(host, token)`.
fn remote_urls(host: &str, token: &str) -> Value {
    let base = format!("https://{host}");
    let auth = quote_all(token);
    json!({
        "dashboard": format!("{base}/?token={token}"),
        "terminal": format!("{base}/term/?auth={auth}"),
        "terminalFallback": format!("{base}:8443/?arg={auth}"),
    })
}

/// `remote_state()`: calcula la foto y la deja en la caché. `before_effects`:
/// la ruta aún no hizo nada (un token ilegible declina en vez de 500).
async fn remote_state(native: &Native, before_effects: bool) -> Result<Arc<Value>, Fault> {
    let opts = native.options();
    let (tools, token) = prepare(opts, true).await?;
    let token = match token {
        Some(Ok(token)) => token,
        // El Python lee el token con la codificación del proceso: un archivo
        // que no es UTF-8 no se puede reproducir con certeza.
        Some(Err(e)) if e.kind() == io::ErrorKind::InvalidData && before_effects => {
            return Err(Fault::Decline);
        }
        _ => return Err(failure()),
    };
    let host = tailscale_host(opts, &tools).await;
    let serve = serve_status(opts, &tools).await;
    let health = webterm_health(opts.webterm_health_ports).await;
    let mut status = status_from_text(&serve, health);
    let urls = if host.is_empty() {
        json!({"dashboard": "", "terminal": "", "terminalFallback": ""})
    } else {
        remote_urls(&host, &token)
    };
    status.insert("host".into(), Value::String(host));
    status.insert("urls".into(), urls);
    status.insert("qrAvailable".into(), Value::Bool(tools.qrencode.is_some()));
    status.insert("serveStatus".into(), Value::String(serve));
    let data = Arc::new(Value::Object(status));
    native.remote.store(Arc::clone(&data));
    Ok(data)
}

/// 200 con la foto recién calculada (el final de cada POST).
async fn fresh(native: &Native) -> Answer {
    let state = remote_state(native, false).await?;
    reply(StatusCode::OK, &state)
}

// ---------------------------------------------------------------------------
// `tailscale serve` y `cc-webterm`
// ---------------------------------------------------------------------------

/// `serve_path(path, target, https_port)`: tres formas de la orden (12 s cada
/// una); `None` con la primera que funcione; si no, el texto de la última.
async fn serve_path(
    opts: &NativeOptions,
    tools: &Tools,
    path: &str,
    target: &str,
    https_port: &str,
) -> Option<String> {
    let https = format!("--https={https_port}");
    let set_path = format!("--set-path={path}");
    let attempts: [&[&str]; 3] = [
        &["serve", "--bg", &https, &set_path, target],
        &["serve", "--bg", &set_path, target],
        &["serve", &https, path, target],
    ];
    let mut last = None;
    for args in attempts {
        let r = run_quiet(
            opts,
            "tailscale",
            tools.tailscale.as_deref(),
            args,
            SERVE_SECONDS,
        )
        .await;
        if r.ok {
            return None;
        }
        last = Some(r);
    }
    let last = last.unwrap_or_else(|| Quiet::failed(String::new()));
    Some(first_text(
        &last.stderr,
        &last.stdout,
        "tailscale serve fallo",
    ))
}

/// `remote_dashboard_on()`.
async fn dashboard_on(opts: &NativeOptions, tools: &Tools) -> Option<String> {
    let Some(tailscale) = tools.tailscale.as_deref() else {
        return Some(NOT_INSTALLED.to_owned());
    };
    let status = run_quiet(
        opts,
        "tailscale",
        Some(tailscale),
        &["status"],
        STATUS_SECONDS,
    )
    .await;
    if !status.ok {
        return Some(NOT_LOGGED_IN.to_owned());
    }
    serve_path(opts, tools, "/", DASHBOARD_TARGET, "443").await
}

/// Rutas de la terminal web en Tailscale (`/term` y el respaldo en 8443).
async fn serve_webterm(opts: &NativeOptions, tools: &Tools) {
    serve_path(opts, tools, "/term", TERM_TARGET, "443").await;
    serve_path(opts, tools, "/", FALLBACK_TARGET, "8443").await;
}

/// Enable the Rust compatibility leases and publish Tailscale routes.
async fn webterm_on(opts: &NativeOptions, tools: &Tools) -> Option<String> {
    let home = opts.home.clone();
    let hooks = opts.hooks.clone();
    let result = tokio::task::spawn_blocking(move || {
        super::super::term::lifecycle::set_enabled(&home, &hooks, true)
    })
    .await;
    match result {
        Ok(Ok(())) => {
            serve_webterm(opts, tools).await;
            None
        }
        Ok(Err(error)) => Some(error.to_string()),
        Err(_) => Some("No se pudo habilitar el terminal web".into()),
    }
}

/// The existing lifecycle closes listeners and active bridges when disabled.
async fn webterm_off(opts: &NativeOptions, _tools: &Tools) {
    let home = opts.home.clone();
    let hooks = opts.hooks.clone();
    let _ = tokio::task::spawn_blocking(move || {
        super::super::term::lifecycle::set_enabled(&home, &hooks, false)
    })
    .await;
}

async fn tools_of(native: &Native) -> Result<Tools, Fault> {
    Ok(prepare(native.options(), false).await?.0)
}

/// POST `/remote-on`.
async fn remote_on(native: &Native) -> Answer {
    let opts = native.options();
    let tools = tools_of(native).await?;
    if let Some(message) = error_text(dashboard_on(opts, &tools).await) {
        return error(StatusCode::BAD_REQUEST, &message);
    }
    let (primary, fallback) = webterm_health(opts.webterm_health_ports).await;
    if primary || fallback {
        serve_webterm(opts, &tools).await;
    }
    fresh(native).await
}

/// POST `/remote-off` (D9): sin `tailscale serve reset`.
async fn remote_off(native: &Native) -> Answer {
    let opts = native.options();
    let tools = tools_of(native).await?;
    let tailscale = tools.tailscale.as_deref();
    for port in ["--https=443", "--https=8443"] {
        run_quiet(
            opts,
            "tailscale",
            tailscale,
            &["serve", port, "off"],
            SERVE_SECONDS,
        )
        .await;
    }
    webterm_off(opts, &tools).await;
    fresh(native).await
}

/// POST `/remote-webterm-on`.
async fn remote_webterm_on(native: &Native) -> Answer {
    let opts = native.options();
    let tools = tools_of(native).await?;
    if let Some(message) = error_text(dashboard_on(opts, &tools).await) {
        return error(StatusCode::BAD_REQUEST, &message);
    }
    if let Some(message) = error_text(webterm_on(opts, &tools).await) {
        return error(StatusCode::BAD_REQUEST, &message);
    }
    fresh(native).await
}

/// POST `/remote-webterm-off`.
async fn remote_webterm_off(native: &Native) -> Answer {
    let tools = tools_of(native).await?;
    webterm_off(native.options(), &tools).await;
    fresh(native).await
}

// ---------------------------------------------------------------------------
// QR
// ---------------------------------------------------------------------------

/// GET `/remote-qr.png`.
async fn qr(native: &Arc<Native>) -> Answer {
    let state = cached(native).await?;
    let url = state
        .get("urls")
        .and_then(|urls| urls.get("dashboard"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    if url.is_empty() {
        return error(StatusCode::BAD_REQUEST, NO_HOST);
    }
    let search = native.options().search_path.clone();
    // `shutil.which("qrencode")` y, si está, `NamedTemporaryFile(suffix=".png")`.
    let prepared = tokio::task::spawn_blocking(move || {
        which_in(search.as_deref(), "qrencode").map(|qrencode| (qrencode, temp_png()))
    })
    .await
    .map_err(|_| failure())?;
    let Some((qrencode, temp)) = prepared else {
        return error(StatusCode::NOT_FOUND, NO_QRENCODE);
    };
    let temp = match temp {
        Ok(temp) => temp,
        Err(message) => return error(StatusCode::INTERNAL_SERVER_ERROR, &message),
    };
    let name = temp.0.to_string_lossy().into_owned();
    // La URL lleva el token de acceso: va por stdin (`qrencode` sin texto lo
    // lee de ahí), no en `argv` (visible en `/proc` para todo usuario) ni en el
    // texto de un plazo vencido. El Python la pasa como argumento; el PNG es
    // el mismo byte a byte.
    let r = run_quiet_input(
        native.options(),
        "qrencode",
        Some(&qrencode),
        &["-o", &name, "-s", "8", "-m", "2"],
        Some(url.as_bytes()),
        QRENCODE_SECONDS,
    )
    .await;
    if !r.ok {
        let message = strip(&r.stderr);
        let message = if message.is_empty() {
            "qrencode fallo"
        } else {
            message
        };
        return error(StatusCode::INTERNAL_SERVER_ERROR, message);
    }
    let read = tokio::task::spawn_blocking(move || {
        let path = temp.0.clone();
        std::fs::read(&path).map_err(|e| io_message(&e, &path.to_string_lossy()))
    })
    .await
    .map_err(|_| failure())?;
    match read {
        Ok(bytes) => Ok(Reply::bytes(StatusCode::OK, "image/png", bytes)),
        Err(message) => error(StatusCode::INTERNAL_SERVER_ERROR, &message),
    }
}

/// El temporal del QR: se borra al soltarse (el `with NamedTemporaryFile`),
/// en el hilo que lo suelte (un `unlink`).
struct TempPng(PathBuf);

impl Drop for TempPng {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// `mkstemp` en el temporal del proceso: `tmp<8 caracteres>.png`, 0600,
/// exclusivo. Error → `str(e)` del Python.
fn temp_png() -> Result<TempPng, String> {
    const ALPHABET: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789_";
    let dir = std::env::temp_dir();
    let mut last = String::from("No usable temporary file name found");
    for _ in 0..100 {
        let mut random = [0u8; 8];
        getrandom::fill(&mut random).map_err(|e| e.to_string())?;
        let name: String = random
            .iter()
            .map(|b| {
                char::from(
                    ALPHABET
                        .get(usize::from(*b) % ALPHABET.len())
                        .copied()
                        .unwrap_or(b'x'),
                )
            })
            .collect();
        let path = dir.join(format!("tmp{name}.png"));
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&path)
        {
            Ok(_) => return Ok(TempPng(path)),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
                last = io_message(&e, &path.to_string_lossy());
            }
            Err(e) => return Err(io_message(&e, &path.to_string_lossy())),
        }
    }
    Err(last)
}

// ---------------------------------------------------------------------------
// Arranque (`restore_requested_webterm`)
// ---------------------------------------------------------------------------

/// `restore_requested_webterm()`: con `Background::front` y la marca
/// `HOOKS/webterm-enabled` (archivo regular), `webterm_on` en una tarea
/// registrada. Con `legacy` (el valor de toda la 2f) no hace nada: lo hace el
/// Python. Devuelve si lanzó la tarea.
pub fn start(native: &Arc<Native>) -> bool {
    let opts = native.options();
    if !native.enabled()
        || super::cut_is_off(&opts.cuts_off, super::Cut::Services)
        || !opts.background.webterm_restore
        || super::files::DomainDocument::new(&opts.home, &opts.hooks, WEBTERM_ENABLED_FILE)
            .ok()
            .and_then(|d| d.read_bytes().ok().flatten())
            .is_none()
    {
        return false;
    }
    let task_native = Arc::clone(native);
    native
        .tasks()
        .spawn(async move {
            if let Ok(tools) = tools_of(&task_native).await {
                let _ = webterm_on(task_native.options(), &tools).await;
            }
        })
        .is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn headers_end_scans_only_the_new_bytes_with_overlap() {
        assert!(headers_end(b"HTTP/1.0 200 OK\r\n\r\n", 0));
        // El `\r\n\r\n` partido entre dos lecturas se ve con el solape.
        assert!(headers_end(b"HTTP/1.0 200 OK\r\n\r\n", 17));
        assert!(headers_end(b"HTTP/1.0 200 OK\n\n", 16));
        assert!(!headers_end(b"HTTP/1.0 200 OK\r\n", 0));
        // Lo ya revisado (más de 3 bytes atrás) no se vuelve a mirar.
        assert!(!headers_end(b"\n\nHTTP/1.0 200 OK", 10));
    }

    /// Un servidor local que gotea un byte cada 0,3 s (cada lectura cabe en
    /// los 0,4 s del socket) no alarga la sonda más allá de su plazo total.
    #[tokio::test]
    async fn probe_has_an_overall_deadline() {
        let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .await
            .unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = [0u8; 512];
            let _ = socket.read(&mut request).await;
            for byte in b"HTTP/1.1 200 OK\r\nX-Lento: 1".iter().cycle().take(200) {
                if socket.write_all(&[*byte]).await.is_err() {
                    return;
                }
                tokio::time::sleep(Duration::from_millis(300)).await;
            }
        });
        let started = Instant::now();
        assert!(!http_healthy(port, "/token").await);
        assert!(started.elapsed() < Duration::from_millis(1500));
        server.abort();
    }

    #[test]
    fn status_text_states() {
        let serve =
            "|-- / proxy http://127.0.0.1:4777\n|-- /term proxy http://127.0.0.1:4780/term\n";
        let s = status_from_text(serve, (true, false));
        assert_eq!(s.get("terminalState"), Some(&json!("active")));
        let s = status_from_text(
            "proxy http://127.0.0.1:4777 proxy http://127.0.0.1:4779",
            (false, true),
        );
        assert_eq!(s.get("terminalState"), Some(&json!("degraded")));
        assert_eq!(s.get("webtermDegraded"), Some(&json!(true)));
        let s = status_from_text("", (true, true));
        assert_eq!(s.get("terminalState"), Some(&json!("off")));
        let keys: Vec<&str> = s.keys().map(String::as_str).collect();
        assert_eq!(keys.first(), Some(&"remoteOn"));
        assert_eq!(keys.last(), Some(&"terminalState"));
        assert_eq!(keys.len(), 11);
    }

    #[test]
    fn host_parsing_like_python() {
        assert_eq!(
            dns_name(r#"{"Self":{"DNSName":"a.b.ts.net.."}}"#),
            Some("a.b.ts.net".into())
        );
        assert_eq!(dns_name(r#"{"Peer":1}"#), Some(String::new()));
        assert_eq!(dns_name(r#"{"Self":null}"#), None);
        assert_eq!(dns_name(r#"{"Self":{"DNSName":5}}"#), None);
        assert_eq!(dns_name("[1]"), None);
        assert_eq!(dns_name("no json"), None);
        assert_eq!(
            ts_net("100.1.2.3 x-y.tail9.ts.net. linux"),
            "x-y.tail9.ts.net"
        );
        assert_eq!(ts_net("nada"), "");
    }

    #[test]
    fn urls_quote_like_urllib() {
        assert_eq!(
            quote_all("a-b_c.d~e/f+g=h é"),
            "a-b_c.d~e%2Ff%2Bg%3Dh%20%C3%A9"
        );
        let urls = remote_urls("h.ts.net", "t/k");
        assert_eq!(urls["dashboard"], "https://h.ts.net/?token=t/k");
        assert_eq!(urls["terminal"], "https://h.ts.net/term/?auth=t%2Fk");
        assert_eq!(urls["terminalFallback"], "https://h.ts.net:8443/?arg=t%2Fk");
    }

    #[test]
    fn python_error_texts() {
        assert_eq!(
            os_error_message(2, "tailscale"),
            "[Errno 2] No such file or directory: 'tailscale'"
        );
        assert_eq!(
            os_error_message(13, "/x/cc-webterm"),
            "[Errno 13] Permission denied: '/x/cc-webterm'"
        );
        assert_eq!(
            timeout_message("tailscale", &["serve", "--https=443", "off"], 12),
            "Command '['tailscale', 'serve', '--https=443', 'off']' timed out after 12 seconds"
        );
        assert_eq!(py_repr("/home/é/x"), "'/home/é/x'");
        assert_eq!(py_repr("it's"), "\"it's\"");
    }

    #[test]
    fn decode_errors_like_cpython() {
        let msg = |b: &[u8]| python_text(b.to_vec()).unwrap_err();
        assert_eq!(
            msg(b"ab\xff"),
            "'utf-8' codec can't decode byte 0xff in position 2: invalid start byte"
        );
        assert_eq!(
            msg(b"\xe2\x28\xa1"),
            "'utf-8' codec can't decode byte 0xe2 in position 0: invalid continuation byte"
        );
        assert_eq!(
            msg(b"\xf0\x90\x28"),
            "'utf-8' codec can't decode bytes in position 0-1: invalid continuation byte"
        );
        assert_eq!(
            msg(b"x\xe2\x82"),
            "'utf-8' codec can't decode bytes in position 1-2: unexpected end of data"
        );
        assert_eq!(
            msg(b"\xe2"),
            "'utf-8' codec can't decode byte 0xe2 in position 0: unexpected end of data"
        );
        assert_eq!(python_text(b"a\r\nb\rc".to_vec()).unwrap(), "a\nb\nc");
    }

    #[test]
    fn first_text_is_python_or_then_strip() {
        assert_eq!(first_text("", "", "def"), "def");
        assert_eq!(first_text(" \n", "out", "def"), "");
        assert_eq!(first_text("", " out\n", "def"), "out");
        assert_eq!(error_text(Some(String::new())), None);
        assert_eq!(error_text(Some("x".into())), Some("x".into()));
        assert_eq!(error_text(None), None);
    }

    #[tokio::test]
    async fn closed_ports_are_unhealthy() {
        // Puerto 1: nadie escucha (nunca 4779/4780 reales).
        assert_eq!(webterm_health([1, 1]).await, (false, false));
    }
}
