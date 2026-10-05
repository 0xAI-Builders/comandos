//! G. Terminal: POST /terminal-history (8762, `lib/terminal_history.py`) y
//! POST /terminal-panes (8771, `lib/terminal_panes.py`) sobre las librerías
//! de `comandos-runtime`, que llaman a tmux de forma síncrona: corren en un
//! hilo de bloqueo y cada llamada la conduce el runtime (`run_blocking`).
use super::{
    Answer, Entry, Fault, Key, Native, NativeRoute, Verb,
    light::{data, error, read_reply},
    py, reply,
    tmux::{Tmux, TmuxError},
};
use crate::Request;
use comandos_runtime::{
    pane_typing::TmuxResult,
    terminal_history::{self, HistoryError},
    terminal_panes::{self, PaneError},
};
use http::StatusCode;
use serde_json::{Map, Value};
use std::{cell::RefCell, path::Path};
use tokio::runtime::Handle;

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
    /// `TimeoutExpired`, `FileNotFoundError`, `IndexError` de `/proc`: 503.
    pub unavailable: bool,
    /// Salida no UTF-8: `UnicodeDecodeError` con texto del códec.
    pub undecodable: bool,
    /// Ya corrió un comando que muta (`resize-pane`, `if-shell`, `set-option`).
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
    // `close` guarda una copia con `tmux_snapshot` + `PaneInspector`
    // (inspección de procesos, aún en Python): se declina antes de nada.
    if data.get("action").and_then(Value::as_str) == Some("close") {
        return Err(Fault::Decline);
    }
    let home = home_text(native)?;
    let tmux = native.options().tmux.clone();
    let handle = Handle::current();
    let joined = tokio::task::spawn_blocking(move || {
        let bridge = RefCell::new(Bridge::default());
        let result = terminal_panes::execute(
            |args| {
                call(&handle, &tmux, &bridge, args).ok_or_else(|| PaneError(BRIDGE_FAILED.into()))
            },
            |session, pane| identify(&handle, &tmux, &bridge, session, pane),
            |_, _| Err(PaneError(BRIDGE_FAILED.into())),
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
    if bridge.undecodable {
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
