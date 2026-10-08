//! G. Terminal: POST /terminal-history (8762, `lib/terminal_history.py`) y
//! POST /terminal-panes (8771, `lib/terminal_panes.py`) sobre las librerías
//! de `comandos-runtime`, que llaman a tmux de forma síncrona: corren en un
//! hilo de bloqueo y cada llamada la conduce el runtime (`run_blocking`).
use super::{
    Answer, Cut, Entry, Fault, Key, Native, NativeRoute, SecondsClock, Verb, cut_is_off,
    light::{data, error, read_reply},
    py, reply,
    tmux::{Tmux, TmuxError},
};
use crate::Request;
use comandos_runtime::{
    closed_panes,
    pane_snapshot::PaneInspector,
    pane_typing::TmuxResult,
    terminal_history::{self, HistoryError},
    terminal_panes::{self, PaneError},
    tmux_snapshot::SnapshotError,
};
use http::StatusCode;
use serde_json::{Map, Value};
use std::{
    cell::RefCell,
    path::{Path, PathBuf},
};
use tokio::{runtime::Handle, sync::Semaphore};

/// Un solo `/terminal-panes` en un hilo de bloqueo a la vez. La librería ya
/// serializa todo el proceso con su `SERIAL` (el `RLock` del Python), pero lo
/// toma dentro del hilo: sin esta puerta, una ola de iframes de `term.html`
/// ocuparía todo el pool con hilos aparcados en ese candado, y los estáticos y
/// el resto de saltos esperarían detrás (con tmux lento, 5 s × 17 seguidos).
/// Aquí la espera es asíncrona y la ola ocupa como mucho un hilo.
///
/// Un `close` no tiene plazo total, igual que el Python (que hace la copia
/// bajo su `RLock` sin límite propio): cada `tmux` tiene sus 5 s, pero la
/// copia hace ≈ 5 llamadas por ventana de la sesión, así que con tmux colgado
/// retiene la puerta (2 + 5·ventanas)·5 s y las demás acciones esperan detrás.
static PANES_GATE: Semaphore = Semaphore::const_new(1);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerminalRoute {
    History,
    Panes,
}

pub const ROUTES: &[Entry] = &[
    Entry {
        verb: Verb::Post,
        key: Key::Raw("/terminal-history"),
        route: NativeRoute::Terminal(TerminalRoute::History),
    },
    Entry {
        verb: Verb::Post,
        key: Key::Raw("/terminal-panes"),
        route: NativeRoute::Terminal(TerminalRoute::Panes),
    },
];

const PANES_UNAVAILABLE: &str =
    "No se pudo completar la acción del panel. Comprueba Paneles antes de reintentar";
const HISTORY_UNAVAILABLE: &str = "Historial temporalmente no disponible";
/// Errores de callback: nunca llegan a la respuesta (se mira `Bridge` antes).
const BRIDGE_FAILED: &str = "comandos: tmux no disponible";
/// Comandos que mueven tmux: tras uno de ellos ya no se puede declinar.
const MUTATING: [&str; 3] = ["resize-pane", "if-shell", "set-option"];

/// `os.path.expanduser("~").rstrip("/")` de `friendly_path`: el HOME es el
/// padre de `~/.claude/hooks`.
pub fn home_text(native: &Native) -> Result<String, Fault> {
    let home = native
        .options()
        .hooks
        .parent()
        .and_then(Path::parent)
        .ok_or(Fault::Decline)?;
    Ok(home
        .to_str()
        .ok_or(Fault::Decline)?
        .trim_end_matches('/')
        .to_owned())
}

/// Lo que pasó con tmux durante la llamada a la librería.
#[derive(Debug, Default)]
pub struct Bridge {
    /// `TimeoutExpired`, `FileNotFoundError`, `IndexError` de `/proc`, y en
    /// la copia de `close` el `RuntimeError` de tmux o el `OSError` de la
    /// carpeta: el `except Exception` del Python, 503.
    pub unavailable: bool,
    /// Salida no UTF-8: `UnicodeDecodeError` con texto del códec.
    pub undecodable: bool,
    /// La copia de `close` no sabe reproducir lo que haría el Python
    /// (`SnapshotError::Unsure`): se trata como `undecodable`.
    pub uncertain: bool,
    /// Ya corrió un comando que muta (`resize-pane`, `if-shell`, `set-option`)
    /// o ya se escribió la copia de `close`.
    pub effected: bool,
}

/// Una llamada a tmux desde la librería. `None` si falló (y queda anotado).
fn call(
    handle: &Handle,
    tmux: &Tmux,
    bridge: &RefCell<Bridge>,
    args: &[&str],
) -> Option<TmuxResult> {
    if args.first().is_some_and(|name| MUTATING.contains(name)) {
        bridge.borrow_mut().effected = true;
    }
    match tmux.run_blocking(handle, args) {
        Ok(out) => Some(TmuxResult {
            returncode: if out.ok { 0 } else { 1 },
            stdout: out.stdout,
            stderr: out.stderr,
        }),
        Err(TmuxError::Decode) => {
            bridge.borrow_mut().undecodable = true;
            None
        }
        Err(TmuxError::Timeout { .. } | TmuxError::Spawn(..)) => {
            bridge.borrow_mut().unavailable = true;
            None
        }
    }
}

pub async fn answer(native: &Native, route: TerminalRoute, request: &Request) -> Answer {
    let data = Value::Object(data(request)?.clone());
    match route {
        TerminalRoute::History => history(native, data).await,
        TerminalRoute::Panes => panes(native, data).await,
    }
}

async fn history(native: &Native, data: Value) -> Answer {
    let home = home_text(native)?;
    let tmux = native.options().tmux.clone();
    let handle = Handle::current();
    let joined = tokio::task::spawn_blocking(move || {
        let bridge = RefCell::new(Bridge::default());
        let result = terminal_history::capture(
            |args| {
                // Tras un fallo la librería no vuelve a llamar: el código 1
                // solo la detiene, la respuesta sale de `bridge`.
                call(&handle, &tmux, &bridge, args).unwrap_or(TmuxResult {
                    returncode: 1,
                    ..TmuxResult::default()
                })
            },
            &data,
            &home,
        );
        (result, bridge.into_inner())
    })
    .await;
    // Un pánico de la librería es el `except Exception` del Python: 503.
    let Ok((result, bridge)) = joined else {
        return error(StatusCode::SERVICE_UNAVAILABLE, HISTORY_UNAVAILABLE);
    };
    if bridge.unavailable {
        return error(StatusCode::SERVICE_UNAVAILABLE, HISTORY_UNAVAILABLE);
    }
    // Ni la salida no UTF-8 ni las filas que el Python convierte con `int()`
    // tienen respuesta segura; la ruta no escribe: se reenvía.
    if bridge.undecodable {
        return Err(Fault::Decline);
    }
    match result {
        Ok(response) => read_reply(&response.to_json()),
        Err(HistoryError::MalformedPanes) => Err(Fault::Decline),
        Err(e) => error(StatusCode::BAD_REQUEST, &e.to_string()),
    }
}

async fn panes(native: &Native, data: Value) -> Answer {
    // La ruta es de la 2c (`Cut::Base`), pero `close` es del corte `tabs`
    // (2f-1): con ese corte apagado se declina antes de cualquier efecto.
    if data.get("action").and_then(Value::as_str) == Some("close")
        && cut_is_off(&native.options().cuts_off, Cut::Tabs)
    {
        return Err(Fault::Decline);
    }
    let home = home_text(native)?;
    let tmux = native.options().tmux.clone();
    let copy = CloseCopy {
        home: native.options().home.clone(),
        proc_root: native.options().proc_root.clone(),
        clock_seconds: native.options().clock_seconds.clone(),
    };
    let handle = Handle::current();
    // El permiso viaja con el trabajo: si el cliente se va a mitad, la puerta
    // sigue cerrada hasta que el hilo termine de verdad. La copia de `close`
    // corre dentro de `execute`, en el mismo hilo, bajo el mismo permiso y el
    // `SERIAL` de la librería, como el resto de acciones (el Python la hace
    // bajo su `RLock`); su duración la acota cada `tmux` (5 s) y el
    // `capture-pane -S -2000`, igual que en el Python.
    let Ok(permit) = PANES_GATE.acquire().await else {
        return error(StatusCode::SERVICE_UNAVAILABLE, PANES_UNAVAILABLE);
    };
    let joined = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        let bridge = RefCell::new(Bridge::default());
        let result = terminal_panes::execute(
            |args| {
                call(&handle, &tmux, &bridge, args).ok_or_else(|| PaneError(BRIDGE_FAILED.into()))
            },
            |session, pane| identify(&handle, &tmux, &bridge, session, pane),
            |session, pane| copy.save(&handle, &tmux, &bridge, session, pane),
            &data,
            &home,
        );
        (result, bridge.into_inner())
    })
    .await;
    // Un pánico de la librería es el `except Exception` del Python: 503.
    let Ok((result, bridge)) = joined else {
        return error(StatusCode::SERVICE_UNAVAILABLE, PANES_UNAVAILABLE);
    };
    if bridge.unavailable {
        return error(StatusCode::SERVICE_UNAVAILABLE, PANES_UNAVAILABLE);
    }
    if bridge.undecodable || bridge.uncertain {
        // Antes de mutar se reenvía; después, el Python respondería 400 con
        // el texto del códec: diferencia aceptada, se responde el 503.
        return if bridge.effected {
            error(StatusCode::SERVICE_UNAVAILABLE, PANES_UNAVAILABLE)
        } else {
            Err(Fault::Decline)
        };
    }
    match result {
        Ok(value) => reply(StatusCode::OK, &value),
        Err(PaneError(message)) => error(StatusCode::BAD_REQUEST, &message),
    }
}

const IDENTITY_FIELDS: [&str; 8] = [
    "socket_path",
    "pid",
    "session_id",
    "session_name",
    "pane_id",
    "pane_pid",
    "pane_current_command",
    "pane_current_path",
];

/// `_pane_identity` (6763).
fn identify(
    handle: &Handle,
    tmux: &Tmux,
    bridge: &RefCell<Bridge>,
    session: &str,
    pane: &str,
) -> Result<Value, PaneError> {
    if py::is_pane(pane) != Some(true) {
        return Err(PaneError("se necesita el panel exacto".into()));
    }
    let format = IDENTITY_FIELDS
        .iter()
        .map(|f| format!("#{{{f}}}"))
        .collect::<Vec<_>>()
        .join("\t");
    let out = call(
        handle,
        tmux,
        bridge,
        &["display-message", "-p", "-t", pane, &format],
    )
    .ok_or_else(|| PaneError(BRIDGE_FAILED.into()))?;
    let parts: Vec<&str> = py::strip(&out.stdout).split('\t').collect();
    if out.returncode != 0 || parts.len() != IDENTITY_FIELDS.len() {
        return Err(PaneError("el panel ya no existe".into()));
    }
    let mut identity = Map::new();
    for (key, value) in IDENTITY_FIELDS.iter().zip(&parts) {
        identity.insert((*key).into(), Value::String((*value).to_owned()));
    }
    if parts.get(4) != Some(&pane) || parts.get(3) != Some(&session) {
        return Err(PaneError("el panel no pertenece a esa sesión".into()));
    }
    let pid = parts.get(1).copied().unwrap_or("");
    #[cfg(target_os = "macos")]
    let start = pid
        .parse::<i64>()
        .ok()
        .map(|p| comandos_runtime::agent_procs::process_start(std::path::Path::new("/proc"), p))
        .unwrap_or_default();
    #[cfg(not(target_os = "macos"))]
    let start = match std::fs::read(format!("/proc/{pid}/stat")) {
        // `except OSError`: sin inicio del servidor.
        Err(_) => String::new(),
        Ok(bytes) => {
            let Ok(text) = String::from_utf8(bytes) else {
                bridge.borrow_mut().undecodable = true;
                return Err(PaneError(BRIDGE_FAILED.into()));
            };
            // `fh.read().rsplit(')', 1)[1].split()[19]`; sin campo, `IndexError`.
            match text
                .rsplit_once(')')
                .and_then(|(_, rest)| rest.split_whitespace().nth(19))
            {
                Some(field) => field.to_owned(),
                None => {
                    bridge.borrow_mut().unavailable = true;
                    return Err(PaneError(BRIDGE_FAILED.into()));
                }
            }
        }
    };
    identity.insert("server_start".into(), Value::String(start));
    Ok(Value::Object(identity))
}

/// Lo que la copia de `close` toma del frente: `Path.home()`, `/proc` y
/// `time.time()`.
struct CloseCopy {
    home: PathBuf,
    proc_root: PathBuf,
    clock_seconds: SecondsClock,
}

impl CloseCopy {
    /// `save_closed_pane_snapshot(sess, pane)` (5684) como `save_snapshot` de
    /// `execute`: devuelve el nombre del archivo. Un `ValueError` sale con su
    /// texto (400); cualquier otra excepción es el 503 de la ruta (queda en
    /// `bridge.unavailable`); lo incierto, en `bridge.uncertain` (se declina
    /// si aún no hubo efectos).
    ///
    /// Diferencia de orden con el Python: `time.time_ns()`, `uuid4().hex` y
    /// `time.time()` (nombre del archivo y `closedAt`) se toman aquí, ANTES de
    /// capturar la sesión; el Python los toma después de `capture_session` y
    /// de `capture-pane`. Las marcas quedan unos milisegundos antes (lo que
    /// tarde la captura); el contenido y el orden de las copias no cambian.
    fn save(
        &self,
        handle: &Handle,
        tmux: &Tmux,
        bridge: &RefCell<Bridge>,
        session: &str,
        pane: &str,
    ) -> Result<Value, PaneError> {
        let failed = || PaneError(BRIDGE_FAILED.into());
        // `pane_snapshot.PaneInspector()` se construye dentro de la copia.
        let Ok(inspector) = PaneInspector::new(&self.home, &self.proc_root) else {
            bridge.borrow_mut().uncertain = true;
            return Err(failed());
        };
        // `uuid.uuid4().hex` (`os.urandom`; si falla, excepción: 503).
        let Some(uuid_hex) = uuid4_hex() else {
            bridge.borrow_mut().unavailable = true;
            return Err(failed());
        };
        let saved = closed_panes::save(
            &self.home,
            &mut |args: &[&str]| {
                // Un fallo de tmux ya quedó anotado en `bridge` por `call`.
                call(handle, tmux, bridge, args).ok_or(SnapshotError::Unsure)
            },
            &inspector,
            session,
            pane,
            time_ns(),
            &uuid_hex,
            (self.clock_seconds)(),
        );
        match saved {
            Ok(name) => {
                // Desde aquí hay una copia escrita: ya no se puede declinar.
                bridge.borrow_mut().effected = true;
                Ok(Value::String(name))
            }
            Err(SnapshotError::Value(text)) => Err(PaneError(text)),
            Err(SnapshotError::Runtime(_) | SnapshotError::Io(_)) => {
                bridge.borrow_mut().unavailable = true;
                Err(failed())
            }
            Err(SnapshotError::Unsure) => {
                let mut bridge = bridge.borrow_mut();
                if !bridge.unavailable && !bridge.undecodable {
                    bridge.uncertain = true;
                }
                Err(failed())
            }
        }
    }
}

/// `time.time_ns()`: los nanosegundos de `CLOCK_REALTIME`.
fn time_ns() -> i128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i128::try_from(d.as_nanos()).unwrap_or(i128::MAX))
}

/// `uuid.uuid4().hex`: 16 bytes aleatorios con la versión 4 y la variante
/// RFC 4122, en hexadecimal minúsculo (32 dígitos).
fn uuid4_hex() -> Option<String> {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).ok()?;
    if let Some(byte) = bytes.get_mut(6) {
        *byte = (*byte & 0x0f) | 0x40;
    }
    if let Some(byte) = bytes.get_mut(8) {
        *byte = (*byte & 0x3f) | 0x80;
    }
    Some(bytes.iter().map(|b| format!("{b:02x}")).collect())
}
