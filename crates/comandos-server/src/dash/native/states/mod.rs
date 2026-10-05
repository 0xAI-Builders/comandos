//! GET `/state` (`read_states` 7035, `read_states_cached` 7191 en el `bin/cc-dash`
//! confirmado). Tarea 4: ensamblado puro sobre `CardEffects`; Tarea 5: ruta,
//! caché y recolección.
pub mod cache;
pub mod cards;
pub mod context;
pub mod gather;
pub mod observe;
pub mod records;
pub mod suggest;
pub mod tab_models;

use super::{Answer, Entry, Fault, Key, Native, NativeRoute, Verb, tmux::TmuxError};
use crate::{HandlerError, Reply};
use comandos_core::json::truthy;
use comandos_runtime::{
    hooks::py::{float_repr, int_text},
    tui_state::StateTracker,
};
use serde_json::{Number, Value};
use std::sync::{Arc, Mutex};

/// Estado del frente para `/state`: cachés de los lectores (D1, D5), el
/// rastreador de configuración, el contexto de sugerencias y el vuelo único.
pub struct Engine {
    pub(crate) cache: cache::StatesCache,
    /// La línea limitada con la causa de un `Decline` (revisión final, I1).
    pub(crate) declines: cache::DeclineLog,
    pub(crate) context: context::Context,
    pub(crate) tracker: Mutex<StateTracker>,
    pub(crate) shared: Arc<Mutex<gather::Blocking>>,
}

impl Default for Engine {
    fn default() -> Self {
        Self {
            cache: cache::StatesCache::default(),
            declines: cache::DeclineLog::default(),
            context: context::Context::default(),
            tracker: gather::new_tracker(),
            shared: Arc::default(),
        }
    }
}

/// Un cómputo terminado: las tarjetas (para `/workspace/sort` `by`) y el
/// cuerpo ya serializado que se responde tal cual.
pub struct States {
    pub items: Arc<Vec<Value>>,
    pub body: bytes::Bytes,
}

/// `self.path.startswith("/state")` reclamando solo la ruta exacta (con
/// consulta opcional): `/states` y `/state/…` siguen en el Python.
pub const ROUTES: &[Entry] = &[Entry {
    verb: Verb::Get,
    key: Key::Path("/state"),
    route: NativeRoute::State,
}];

/// `self._json(200, read_states_cached())`.
pub async fn answer(native: &Native) -> Answer {
    let states = native.states_cached().await?;
    Ok(Reply::bytes(
        http::StatusCode::OK,
        "application/json",
        states.body.clone(),
    ))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StateFault {
    /// Incierto (D2): el frente reenvía; nada se escribió.
    Decline,
    /// Excepción no capturada del Python: 500.
    Failure,
    /// `subprocess.TimeoutExpired` no capturado: 504.
    Timeout,
}

impl StateFault {
    /// `tmux(...)` fuera de un `try`: el plazo vencido es 504; no arrancar o
    /// una salida que no es UTF-8, 500.
    pub fn from_tmux(error: &TmuxError) -> Self {
        match error.uncaught() {
            HandlerError::Timeout => StateFault::Timeout,
            HandlerError::Failure => StateFault::Failure,
        }
    }
}

impl From<comandos_runtime::Unsure> for StateFault {
    fn from(_: comandos_runtime::Unsure) -> Self {
        StateFault::Decline
    }
}

impl From<StateFault> for Fault {
    fn from(fault: StateFault) -> Self {
        match fault {
            StateFault::Decline => Fault::Decline,
            StateFault::Failure => Fault::Error(HandlerError::Failure),
            StateFault::Timeout => Fault::Error(HandlerError::Timeout),
        }
    }
}

impl From<Fault> for StateFault {
    fn from(fault: Fault) -> Self {
        match fault {
            Fault::Decline => StateFault::Decline,
            Fault::Error(HandlerError::Failure) => StateFault::Failure,
            Fault::Error(HandlerError::Timeout) => StateFault::Timeout,
        }
    }
}

/// `float(x)` de Python sobre un valor de JSON.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum PyFloat {
    Value(f64),
    /// `ValueError` o `TypeError`.
    Raises,
    /// No se sabe con certeza (dígitos Unicode, entero enorme): declinar.
    Unsure,
}

/// El número de JSON es `int` en Python si no lleva fracción ni exponente
/// (`NaN`/`Infinity` son `float`).
fn integer_number(n: &Number) -> bool {
    let raw = n.as_str();
    !raw.contains(['.', 'e', 'E']) && !matches!(raw, "NaN" | "Infinity" | "-Infinity")
}

pub(crate) fn py_float(value: &Value) -> PyFloat {
    match value {
        Value::Bool(b) => PyFloat::Value(f64::from(u8::from(*b))),
        Value::Number(n) => match n.as_str().parse::<f64>() {
            // `float(int)` enorme: `OverflowError`; no se reproduce.
            Ok(f) if f.is_infinite() && integer_number(n) => PyFloat::Unsure,
            Ok(f) => PyFloat::Value(f),
            Err(_) => PyFloat::Unsure,
        },
        Value::String(s) => match super::py::float(s) {
            Ok(f) => PyFloat::Value(f),
            Err(comandos_core::text::NumError::Invalid) => PyFloat::Raises,
            // No ASCII: `float()` admite dígitos decimales y blancos Unicode.
            // Con cualquier otro carácter no ASCII el `ValueError` es seguro.
            Err(comandos_core::text::NumError::Exotic) => {
                if s.chars()
                    .any(|c| !c.is_ascii() && !c.is_numeric() && !c.is_whitespace())
                {
                    PyFloat::Raises
                } else {
                    PyFloat::Unsure
                }
            }
        },
        Value::Null | Value::Array(_) | Value::Object(_) => PyFloat::Raises,
    }
}

/// `str(x)` de un escalar de JSON; contenedores (su `repr`) → declinar.
pub(crate) fn py_str(value: &Value) -> Result<String, StateFault> {
    match value {
        Value::String(s) => Ok(s.clone()),
        Value::Null => Ok("None".into()),
        Value::Bool(true) => Ok("True".into()),
        Value::Bool(false) => Ok("False".into()),
        Value::Number(n) => match n.as_str() {
            "NaN" => Ok("nan".into()),
            "Infinity" => Ok("inf".into()),
            "-Infinity" => Ok("-inf".into()),
            // `str(int)` con más de 4300 dígitos: `ValueError` en CPython.
            raw if integer_number(n) && raw.len() > 4300 => Err(StateFault::Decline),
            _ if integer_number(n) => Ok(int_text(n)),
            raw => raw
                .parse::<f64>()
                .map(float_repr)
                .map_err(|_| StateFault::Decline),
        },
        Value::Array(_) | Value::Object(_) => Err(StateFault::Decline),
    }
}

/// `str(x or "")`.
pub(crate) fn str_or_empty(value: Option<&Value>) -> Result<String, StateFault> {
    match value.filter(|v| truthy(v)) {
        Some(v) => py_str(v),
        None => Ok(String::new()),
    }
}

/// `x or default` sobre una clave de un `dict`.
pub(crate) fn or_default(value: Option<&Value>, default: &str) -> Value {
    value
        .filter(|v| truthy(v))
        .cloned()
        .unwrap_or_else(|| Value::from(default))
}

/// `x == "texto"` de Python.
pub(crate) fn is_text(value: Option<&Value>, text: &str) -> bool {
    value.and_then(Value::as_str) == Some(text)
}

/// `round(x)` de Python sobre un `float` y su `str`: mitad al par; `None` es
/// la excepción (`ValueError` con NaN, `OverflowError` con infinito).
pub(crate) fn py_round_text(x: f64) -> Option<String> {
    if !x.is_finite() {
        return None;
    }
    let r = x.round_ties_even();
    // `{:.0}` escribe el entero exacto del `double`; `-0` es `0` en Python.
    Some(if r == 0.0 {
        "0".into()
    } else {
        format!("{r:.0}")
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn round_matches_python() {
        // `round()` de CPython 3.
        for (x, want) in [
            (2.5, "2"),
            (3.5, "4"),
            (-0.4, "0"),
            (-2.5, "-2"),
            (30.5, "30"),
            (1e20, "100000000000000000000"),
        ] {
            assert_eq!(py_round_text(x).as_deref(), Some(want), "{x}");
        }
        assert_eq!(py_round_text(f64::NAN), None);
    }

    #[test]
    fn py_str_and_float() {
        assert_eq!(py_str(&json!(1.5)).unwrap(), "1.5");
        assert_eq!(py_str(&json!(1e16)).unwrap(), "1e+16");
        assert_eq!(py_str(&json!(-0)).unwrap(), "0");
        assert_eq!(py_str(&json!(true)).unwrap(), "True");
        assert_eq!(py_str(&json!([1])), Err(StateFault::Decline));
        assert_eq!(py_float(&json!("  7.5 ")), PyFloat::Value(7.5));
        assert_eq!(py_float(&json!("x")), PyFloat::Raises);
        assert_eq!(py_float(&json!("mañana")), PyFloat::Raises);
        assert_eq!(py_float(&json!("١٢")), PyFloat::Unsure);
        assert_eq!(py_float(&json!([])), PyFloat::Raises);
        assert_eq!(py_float(&json!(false)), PyFloat::Value(0.0));
    }
}
