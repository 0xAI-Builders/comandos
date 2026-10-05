//! Acciones de los popups (`send_key`, `choose`, `send_text`, `open_session`
//! de `bin/cc-notifyd`): primero el tablero (`POST /key`, `/send`, `/focus`,
//! que sabe en qué pestaña vive cada agente) y, si no responde, tmux directo
//! sobre la sesión o el pane exacto del aviso.
//!
//! Todo bloquea (red y procesos): se llama desde un hilo propio
//! ([`in_background`]), nunca desde el de GTK (ruling 3 del sub-plan).
//!
//! tmux: en producción `tmux <args>` sin socket, como el Python (mismo argv
//! byte a byte). Con `--tmux-socket <ruta>` (solo pruebas) todo va con
//! `tmux -S <ruta> <args>`: nunca el servidor del usuario.
use crate::dash::{DashClient, POST_TIMEOUT};
use crate::markup::py_strip;
use crate::stack::valid_pane;
use serde_json::{Map, Value};
use std::ffi::OsString;
use std::io::Read;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

/// `ALLOWED_KEYS`: lo único que se teclea con `send_key`.
pub const ALLOWED_KEYS: [&str; 5] = ["Enter", "Escape", "1", "2", "3"];
/// Plazo de `tmux()` (5 s) y de `wmctrl -a` (2 s).
pub const TMUX_TIMEOUT: Duration = Duration::from_secs(5);
pub const WMCTRL_TIMEOUT: Duration = Duration::from_secs(2);
/// `choose`: el `Enter` de confirmación sale 250 ms después del dígito.
pub const CHOOSE_DELAY: Duration = Duration::from_millis(250);
/// Paso del sondeo de un proceso hijo.
const POLL: Duration = Duration::from_millis(5);

/// `SESSION_RE = ^[A-Za-z0-9._-]{1,80}$` con `match` (el `$` admite un `\n` final).
pub fn valid_session(session: &str) -> bool {
    let core = session.strip_suffix('\n').unwrap_or(session);
    (1..=80).contains(&core.len())
        && core
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
}

/// A quién va la acción: la sesión del aviso y, si lo trae, su pane.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Target {
    pub session: String,
    pub pane: String,
}

impl Target {
    /// `_target_payload(sess, pane, **extra)`: `session`, los extra y `pane`
    /// solo si cumple `PANE_RE` (en ese orden, como el `dict` del Python).
    pub fn payload(&self, extra: &[(&str, &str)]) -> Value {
        let mut map = Map::new();
        map.insert("session".into(), Value::String(self.session.clone()));
        for (key, value) in extra {
            map.insert((*key).into(), Value::String((*value).into()));
        }
        if valid_pane(&self.pane) {
            map.insert("pane".into(), Value::String(self.pane.clone()));
        }
        Value::Object(map)
    }
}

/// Resultado de un proceso: `returncode == 0` y su salida estándar.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Ran {
    pub ok: bool,
    pub stdout: String,
}

/// Ejecuta `command` con salidas capturadas y un plazo. Como el `except` de
/// `tmux()`: si no arranca, vence el plazo (se mata) o la salida no es UTF-8
/// (`text=True`), devuelve `Ran { ok: false, stdout: "" }`.
fn run_captured(mut command: Command, timeout: Duration) -> Ran {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let Ok(mut child) = command.spawn() else {
        return Ran::default();
    };
    // Lectores en hilos propios: un hijo que llena la tubería no se bloquea, y
    // si un nieto se quedara con ella el plazo sigue mandando.
    let (out_tx, out_rx) = mpsc::channel();
    let (err_tx, err_rx) = mpsc::channel();
    for (pipe, tx) in [
        (
            child
                .stdout
                .take()
                .map(|p| Box::new(p) as Box<dyn Read + Send>),
            out_tx,
        ),
        (
            child
                .stderr
                .take()
                .map(|p| Box::new(p) as Box<dyn Read + Send>),
            err_tx,
        ),
    ] {
        let _ = std::thread::Builder::new()
            .name("notifyd-pipe".into())
            .spawn(move || {
                let mut buf = Vec::new();
                if let Some(mut pipe) = pipe {
                    let _ = pipe.read_to_end(&mut buf);
                }
                let _ = tx.send(buf);
            });
    }
    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if Instant::now() < deadline => std::thread::sleep(POLL),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
        }
    };
    let Some(status) = status else {
        return Ran::default();
    };
    let remaining = deadline.saturating_duration_since(Instant::now());
    let (Ok(out), Ok(err)) = (
        out_rx.recv_timeout(remaining),
        err_rx.recv_timeout(remaining),
    ) else {
        return Ran::default();
    };
    match (String::from_utf8(out), std::str::from_utf8(&err)) {
        (Ok(stdout), Ok(_)) => Ran {
            ok: status.success(),
            stdout,
        },
        _ => Ran::default(),
    }
}

/// Cómo se llama a tmux en la caída (`tmux(*args)` del Python).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TmuxRunner {
    program: OsString,
    socket: Option<PathBuf>,
}

impl TmuxRunner {
    /// Producción: `tmux` del `PATH` y el entorno del proceso, como el Python.
    pub fn system() -> TmuxRunner {
        TmuxRunner {
            program: "tmux".into(),
            socket: None,
        }
    }

    /// `--tmux-socket <ruta>`: `tmux -S <ruta> …` (servidor privado de pruebas).
    pub fn with_socket(socket: PathBuf) -> TmuxRunner {
        TmuxRunner {
            program: "tmux".into(),
            socket: Some(socket),
        }
    }

    /// Otro ejecutable (pruebas: un `tmux` falso que anota su argv).
    pub fn with_program(program: OsString, socket: Option<PathBuf>) -> TmuxRunner {
        TmuxRunner { program, socket }
    }

    /// Argumentos completos tras el ejecutable: `-S <ruta>` delante si hay socket.
    pub fn argv(&self, args: &[&str]) -> Vec<OsString> {
        let mut argv = Vec::with_capacity(args.len() + 2);
        if let Some(socket) = &self.socket {
            argv.push("-S".into());
            argv.push(socket.clone().into_os_string());
        }
        argv.extend(args.iter().map(OsString::from));
        argv
    }

    /// `tmux(*args)`: plazo de 5 s; cualquier fallo es `returncode 1` sin salida.
    pub fn run(&self, args: &[&str]) -> Ran {
        let mut command = Command::new(&self.program);
        command.args(self.argv(args));
        if self.socket.is_some() {
            // Con socket propio, nada del entorno apunta al servidor del usuario.
            command.env_remove("TMUX");
        }
        run_captured(command, TMUX_TIMEOUT)
    }
}

/// Lo que tocan las acciones: el tablero, tmux y `wmctrl`.
#[derive(Clone, Debug)]
pub struct Effects {
    pub dash: DashClient,
    pub tmux: TmuxRunner,
    /// `wmctrl` (producción: el del `PATH`).
    pub wmctrl: OsString,
}

impl Effects {
    /// `dash(path, payload)`: `true` si el tablero respondió sin error. El
    /// cuerpo es el `json.dumps(payload)` del Python, byte a byte.
    fn dash(&self, path: &str, payload: &Value) -> bool {
        let Ok(body) = comandos_core::json::response_dumps(payload) else {
            return false;
        };
        self.dash.post_body(path, &body, POST_TIMEOUT)
    }

    /// `_pane_alive(pane)`: tmux 3.2a devuelve `rc=0` con destinos muertos,
    /// así que se compara la salida.
    fn pane_alive(&self, pane: &str) -> bool {
        valid_pane(pane)
            && py_strip(
                &self
                    .tmux
                    .run(&["display-message", "-p", "-t", pane, "#{pane_id}"])
                    .stdout,
            ) == pane
    }

    /// `alive(sess)`: `has-session -t =<sesión>`.
    fn session_alive(&self, session: &str) -> bool {
        valid_session(session)
            && self
                .tmux
                .run(&["has-session", "-t", &format!("={session}")])
                .ok
    }

    /// `_fallback_target(sess, pane)`: con pane, SOLO ese pane (varios agentes
    /// por sesión: el activo puede ser otro); sin pane, `=<sesión>:`.
    fn fallback_target(&self, target: &Target) -> Option<String> {
        if !target.pane.is_empty() {
            return self.pane_alive(&target.pane).then(|| target.pane.clone());
        }
        self.session_alive(&target.session)
            .then(|| format!("={}:", target.session))
    }
}

/// `send_key(sess, key, pane)`.
pub fn send_key(fx: &Effects, target: &Target, key: &str) {
    if !ALLOWED_KEYS.contains(&key) || !valid_session(&target.session) {
        return;
    }
    if !target.pane.is_empty() && !valid_pane(&target.pane) {
        return;
    }
    if fx.dash("/key", &target.payload(&[("key", key)])) {
        return;
    }
    if let Some(dest) = fx.fallback_target(target) {
        fx.tmux.run(&["send-keys", "-t", &dest, key]);
    }
}

/// `choose(sess, digit, pane)`: el dígito y, 250 ms después, `Enter`. El
/// Python programa el `Enter` con `GLib.timeout_add`; aquí la acción ya corre
/// en su propio hilo, así que espera ahí (mismo orden y plazo, sin GTK).
pub fn choose(fx: &Effects, target: &Target, digit: &str) {
    send_key(fx, target, digit);
    std::thread::sleep(CHOOSE_DELAY);
    send_key(fx, target, "Enter");
}

/// `send_text(sess, text, pane)`: el texto literal (`-l --`) y `Enter`.
pub fn send_text(fx: &Effects, target: &Target, text: &str) {
    let text = py_strip(text);
    if text.is_empty() || !valid_session(&target.session) {
        return;
    }
    if !target.pane.is_empty() && !valid_pane(&target.pane) {
        return;
    }
    if fx.dash("/send", &target.payload(&[("text", text)])) {
        return;
    }
    if let Some(dest) = fx.fallback_target(target) {
        fx.tmux.run(&["send-keys", "-t", &dest, "-l", "--", text]);
        fx.tmux.run(&["send-keys", "-t", &dest, "Enter"]);
    }
}

/// `open_session(sess, pane)`: el tablero enfoca la pestaña correcta; si no
/// responde, `switch-client -t =<sesión>` y `wmctrl -a <sesión>` (2 s).
pub fn open_session(fx: &Effects, target: &Target) {
    if !valid_session(&target.session) {
        return;
    }
    if fx.dash("/focus", &target.payload(&[])) {
        return;
    }
    fx.tmux
        .run(&["switch-client", "-t", &format!("={}", target.session)]);
    let mut wmctrl = Command::new(&fx.wmctrl);
    wmctrl.args(["-a", &target.session]);
    run_captured(wmctrl, WMCTRL_TIMEOUT);
}

/// Ejecuta una acción fuera del hilo de GTK (el Python bloqueaba el bucle).
pub fn in_background(name: &str, action: impl FnOnce() + Send + 'static) {
    if let Err(err) = std::thread::Builder::new()
        .name(name.to_string())
        .spawn(action)
    {
        eprintln!("comandos-notifyd: no se pudo lanzar {name}: {err}");
    }
}
