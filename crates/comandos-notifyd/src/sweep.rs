//! Barrido de los popups «te espera» ya atendidos (`stale_waiting` y
//! `waiting_sweep_loop` de `bin/cc-notifyd`). `stale_waiting` es puro; `run`
//! es el bucle del hilo del barrido, con la espera, la consulta a la pila y la
//! entrega al hilo de GTK inyectadas (las pruebas usan un reloj falso).
use crate::dash::DashClient;
use crate::stack::PopupMeta;
use crate::theme::py_truthy;
use serde_json::Value;
use std::time::Duration;

/// Gracia de un popup recién nacido: `/state` tarda en reflejar la espera.
pub const WAITING_GRACE: f64 = 4.0;
/// Cada cuánto mira el barrido y plazo de `GET /state`.
pub const SWEEP_EVERY: Duration = Duration::from_secs(3);
pub const STATE_TIMEOUT: Duration = Duration::from_secs(2);

/// Una entrada «te espera» de `/state`: `(session, pane or "")`. Solo los
/// valores de texto pueden coincidir con los de un popup; el resto se guarda
/// como `None` (en Python es un valor hashable que nunca es igual a un `str`).
type Waiting = (Option<String>, Option<String>);

/// El conjunto `waiting` del Python, o `None` si el Python lanzaría (una
/// entrada que no es objeto, o una sesión/pane no hashable): entonces no se
/// cierra nada.
fn waiting_set(state: &Value) -> Option<Vec<Waiting>> {
    let items: &[Value] = match state {
        Value::Array(items) => items,
        // `state or []`: el barrido solo entrega listas.
        _ => &[],
    };
    let mut out = Vec::new();
    for item in items {
        let object = item.as_object()?;
        if object.get("status").and_then(Value::as_str) != Some("waiting") {
            continue;
        }
        // `i.get("alive", True) is not False`
        if matches!(object.get("alive"), Some(Value::Bool(false))) {
            continue;
        }
        let session = match object.get("session") {
            Some(Value::Array(_) | Value::Object(_)) => return None,
            Some(Value::String(s)) => Some(s.clone()),
            _ => None,
        };
        // `i.get("pane") or ""`
        let pane = match object.get("pane") {
            Some(v) if !py_truthy(v) => Some(String::new()),
            None => Some(String::new()),
            Some(Value::Array(_) | Value::Object(_)) => return None,
            Some(Value::String(s)) => Some(s.clone()),
            Some(_) => None,
        };
        out.push((session, pane));
    }
    Some(out)
}

/// `stale_waiting(wins, state, now)`: índices de los popups «te espera» cuya
/// sesión (y su pane, si lo trae) ya no espera en `/state` o ya no existe.
/// Los nacidos hace menos de `WAITING_GRACE` segundos se respetan.
pub fn stale_waiting(wins: &[PopupMeta], state: &Value, now: f64) -> Vec<usize> {
    let Some(waiting) = waiting_set(state) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for (index, win) in wins.iter().enumerate() {
        if win.kind != "waiting" || now - win.born < WAITING_GRACE || win.session.is_empty() {
            continue;
        }
        let still = waiting.iter().any(|(session, pane)| {
            session.as_deref() == Some(win.session.as_str())
                && (win.pane.is_empty() || pane.as_deref() == Some(win.pane.as_str()))
        });
        if !still {
            out.push(index);
        }
    }
    out
}

/// `waiting_sweep_loop()`: espera 3 s (`wait` devuelve `false` para parar);
/// si hay algún popup «te espera» (`has_waiting`, que pregunta al hilo de
/// GTK), pide `GET /state` (2 s) y, si es una lista, la entrega (`deliver`,
/// que en el hilo de GTK cierra los que sobran).
pub fn run(
    dash: &DashClient,
    mut wait: impl FnMut(Duration) -> bool,
    mut has_waiting: impl FnMut() -> bool,
    mut deliver: impl FnMut(Value),
) {
    while wait(SWEEP_EVERY) {
        if !has_waiting() {
            continue;
        }
        let Some(state) = dash.get_json("/state", STATE_TIMEOUT) else {
            continue;
        };
        if state.is_array() {
            deliver(state);
        }
    }
}
