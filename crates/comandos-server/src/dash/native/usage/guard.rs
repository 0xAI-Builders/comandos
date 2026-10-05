//! `token_guard_with_forecast` (bin/cc-dash:1429) y la latencia medida de
//! `_suggestion_context` (6953-6973) sin el heredado: el contexto de
//! sugerencias de GET `/state` los calcula el frente (Tarea 5a de la 2e).
//!
//! Solo lo que el Python lanzaría con certeza es su `except Exception` (`{}` o
//! tabla vacía). Lo que el frente no reproduce con certeza declina y GET
//! `/state` se reenvía, como cuando el heredado no respondía en la 2d: un error
//! SQL de la conexión del frente, un BLOB o texto no UTF-8 que el Python quizá
//! aceptaría, un entero fuera de `i64`, o la caché de límites aún vacía (el
//! Python, que lleva tiempo vivo, ya tiene la suya llena).
use super::super::{Fault, Native, states::suggest::latency_from};
use comandos_core::{
    json::truthy,
    usage_state::{self, PyNum, UsageError, float_value, round_float},
};
use comandos_store::usage_read::{self, ReadError};
use serde_json::{Map, Value, json};
use std::time::Duration;

/// Cuánto espera la guardia la primera lectura de límites del frente: el plazo
/// de la subconsulta de la 2d (`SUBREQUEST_TIMEOUT`), para no alargar el peor
/// caso de GET `/state` respecto al que ya está en producción.
pub const LIMITS_WAIT: Duration = Duration::from_secs(2);

/// `7 * 86400` y `10 * 86400` de 1438 y 1441.
const WEEK_S: f64 = 604_800.0;
const NO_PACE_S: i64 = 864_000;

/// La latencia medida por (modelo, esfuerzo): `(p50, intentos)`.
pub type Latency = Vec<((Value, Value), (Value, Value))>;

/// Guardia con pronóstico. `Ok(None)`: el `except Exception` del Python (`{}`);
/// `Err(Decline)`: no reproducible (ver el módulo).
pub async fn token_guard_with_forecast(native: &Native) -> Result<Option<Value>, Fault> {
    let now_ms = (native.options().clock)();
    // `_as_int(time.time())`: truncar los segundos.
    let report = native
        .usage
        .with(move |u| usage_read::token_guard_report(&u.conn, now_ms.div_euclid(1000)))
        .await?;
    let report = match report {
        Ok(Value::Object(report)) => report,
        Err(ReadError::Raises) => return Ok(None),
        Ok(_) | Err(ReadError::Sql(_) | ReadError::Undecodable | ReadError::Unsure) => {
            return Err(Fault::Decline);
        }
    };
    // `usage_provider_limits()` va después del informe: si el informe lanza,
    // el Python no pide los límites (ni lanza su refresco).
    let limits = native
        .limits
        .get_loaded(&native.refresh_deps(), LIMITS_WAIT)
        .await
        .ok_or(Fault::Decline)?;
    match with_forecasts(report, &limits.rows, now_ms as f64 / 1000.0) {
        Ok(guard) => Ok(Some(guard)),
        Err(UsageError::Raises | UsageError::Overflow) => Ok(None),
        Err(UsageError::Unsure) => Err(Fault::Decline),
    }
}

/// `cc_usage.experiment_analytics(USAGE_DB, 7)` y la comprensión de 6966. La
/// excepción del Python (`Raises`, o el `ValueError` interno) es la tabla vacía.
pub async fn latency(native: &Native) -> Result<Latency, Fault> {
    let now = (native.options().clock)() as f64 / 1000.0;
    let stats = native
        .usage
        .with(move |u| usage_read::experiment_analytics(&u.conn, 7, "", now))
        .await?;
    match stats {
        Ok(Ok(stats)) => Ok(latency_from(&stats)),
        Ok(Err(_)) | Err(ReadError::Raises) => Ok(Vec::new()),
        Err(ReadError::Sql(_) | ReadError::Undecodable | ReadError::Unsure) => Err(Fault::Decline),
    }
}

/// 1433-1451 sobre el informe: añade `forecasts` y `forecastLevel` al final.
pub fn with_forecasts(
    mut report: Map<String, Value>,
    rows: &[Map<String, Value>],
    now: f64,
) -> usage_state::Result<Value> {
    let forecasts = forecast_rows(rows, now)?;
    let level = |name: &str| {
        forecasts
            .iter()
            .any(|f| f.get("level").and_then(Value::as_str) == Some(name))
    };
    let level = if level("critical") {
        "critical"
    } else if level("warning") {
        "warning"
    } else {
        "normal"
    };
    report.insert("forecasts".into(), Value::Array(forecasts));
    report.insert("forecastLevel".into(), json!(level));
    Ok(Value::Object(report))
}

/// La forma corta del plan: `None` es la excepción del Python (`float()`
/// imposible, `int()` de un no finito) y también lo no reproducible; quien
/// necesite distinguirlos usa `with_forecasts`.
pub fn forecasts(rows: &[Map<String, Value>], now: f64) -> Option<Vec<Value>> {
    forecast_rows(rows, now).ok()
}

fn forecast_rows(rows: &[Map<String, Value>], now: f64) -> usage_state::Result<Vec<Value>> {
    let null = Value::Null;
    let field = |row: &'_ Map<String, Value>, key: &str| row.get(key).unwrap_or(&null).clone();
    let mut out = Vec::new();
    for limit in rows {
        let account = field(limit, "account");
        if field(limit, "provider") != json!("claude")
            || (truthy(&account) && account != json!("main"))
            || field(limit, "window") != json!("7d")
        {
            continue;
        }
        let percent = py_float(&field(limit, "percent"))?;
        let reset = py_float(&field(limit, "resets_at"))?;
        let start = reset - WEEK_S;
        // `max(1.0, x)` de Python: con `x` NaN se queda el primero.
        let elapsed = py_max(1.0, now - start);
        let rate = if percent > 0.0 {
            percent / elapsed
        } else {
            0.0
        };
        let exhaustion = if rate > 0.0 {
            now + (100.0 - percent) / rate
        } else {
            now + NO_PACE_S as f64
        };
        let downtime = py_max(0.0, reset - exhaustion);
        let scope = field(limit, "scope");
        let scope = if truthy(&scope) {
            usage_state::text(&scope)?
        } else {
            "General".to_owned()
        };
        // Las claves en el orden del literal; `int()` antes de redondear, como
        // lo evalúa el Python al armar el diccionario.
        let reset_at = py_int(reset)?;
        let exhausted_at = py_int(exhaustion)?;
        let hours = downtime / 3600.0;
        let level = if downtime >= 24.0 {
            "critical"
        } else if downtime > 0.0 {
            "warning"
        } else {
            "normal"
        };
        let message = if downtime > 0.0 {
            format!(
                "Al ritmo promedio actual, {scope} se agotaría {} h antes del reset.",
                round_int(hours)?
            )
        } else {
            format!("{scope} llegaría al reset con margen.")
        };
        let mut item = Map::new();
        item.insert("scope".into(), scope.into());
        item.insert("percent".into(), float_value(percent));
        item.insert("resetAt".into(), reset_at.into());
        item.insert("projectedExhaustionAt".into(), exhausted_at.into());
        item.insert("downtimeHours".into(), float_value(round_float(hours, 1)));
        item.insert("level".into(), level.into());
        item.insert("message".into(), message.into());
        out.push(Value::Object(item));
    }
    Ok(out)
}

/// `max(a, b)` de Python con floats: `b` solo si `b > a`.
fn py_max(a: f64, b: f64) -> f64 {
    if b > a { b } else { a }
}

/// `float(x or 0)`: los falsos son `0.0`; texto con la gramática de `float()`;
/// lo demás (contenedores no vacíos) es `TypeError`.
fn py_float(value: &Value) -> usage_state::Result<f64> {
    if !truthy(value) {
        return Ok(0.0);
    }
    match value {
        Value::String(s) => match comandos_core::text::float(s) {
            Ok(x) => Ok(x),
            Err(comandos_core::text::NumError::Invalid) => Err(UsageError::Raises),
            Err(comandos_core::text::NumError::Exotic) => Err(UsageError::Unsure),
        },
        Value::Array(_) | Value::Object(_) => Err(UsageError::Raises),
        // `float(int)` de un entero fuera de `i64` no se reproduce (`Unsure`).
        _ => match PyNum::of(value)? {
            Some(n) => Ok(n.as_f64()),
            None => Err(UsageError::Unsure),
        },
    }
}

/// `int(x)` de un float: `inf` → `OverflowError`, `nan` → `ValueError`; un
/// entero de Python fuera de `i64` no se reproduce.
fn py_int(x: f64) -> usage_state::Result<i64> {
    if x.is_nan() {
        return Err(UsageError::Raises);
    }
    if x.is_infinite() {
        return Err(UsageError::Overflow);
    }
    let t = x.trunc();
    if (-(2f64.powi(63))..2f64.powi(63)).contains(&t) {
        Ok(t as i64)
    } else {
        Err(UsageError::Unsure)
    }
}

/// `round(x)` de Python sobre un float finito: entero, mitad al par.
fn round_int(x: f64) -> usage_state::Result<i64> {
    py_int(x.round_ties_even())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn python_rounding_and_int() {
        assert_eq!(round_int(2.5), Ok(2));
        assert_eq!(round_int(3.5), Ok(4));
        assert_eq!(round_int(-0.5), Ok(0));
        assert_eq!(py_int(f64::INFINITY), Err(UsageError::Overflow));
        assert_eq!(py_int(f64::NAN), Err(UsageError::Raises));
        assert_eq!(py_int(1e300), Err(UsageError::Unsure));
        assert_eq!(py_max(1.0, f64::NAN), 1.0);
        assert_eq!(py_max(0.0, -0.0).to_bits(), 0f64.to_bits());
    }

    #[test]
    fn python_float_of_values() {
        assert_eq!(py_float(&json!(null)), Ok(0.0));
        assert_eq!(py_float(&json!("")), Ok(0.0));
        assert_eq!(py_float(&json!([])), Ok(0.0));
        assert_eq!(py_float(&json!(true)), Ok(1.0));
        assert_eq!(py_float(&json!(" 1_0 ")), Ok(10.0));
        assert_eq!(py_float(&json!("abc")), Err(UsageError::Raises));
        assert_eq!(py_float(&json!([1])), Err(UsageError::Raises));
        assert_eq!(py_float(&json!("١")), Err(UsageError::Unsure));
    }
}
