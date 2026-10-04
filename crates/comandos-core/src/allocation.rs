//! Portable quota pace and projection; time and language belong to callers.
use serde_json::{Value, json};
use std::cmp::Ordering;
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    pub kind: &'static str,
    pub message: String,
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}
impl std::error::Error for Error {}
pub type Result<T> = std::result::Result<T, Error>;
pub(crate) fn error(kind: &'static str, message: impl Into<String>) -> Error {
    Error {
        kind,
        message: message.into(),
    }
}
pub(crate) fn required<'a>(row: &'a Value, key: &str) -> Result<&'a Value> {
    row.get(key).ok_or_else(|| error("KeyError", key))
}
pub(crate) fn string(value: &Value) -> Result<&str> {
    value
        .as_str()
        .ok_or_else(|| error("AttributeError", "se requiere texto"))
}
pub(crate) fn default<'a>(value: &'a Value, fallback: &'a Value) -> &'a Value {
    if crate::json::truthy(value) {
        value
    } else {
        fallback
    }
}
pub(crate) fn pyfloat(value: &Value) -> Result<f64> {
    crate::focus::float(value).map_err(|message| {
        error(
            match value {
                Value::Number(n) if crate::pomodoro::integer_token(n.as_str()) => "OverflowError",
                Value::String(_) | Value::Number(_) => "ValueError",
                _ => "TypeError",
            },
            message,
        )
    })
}
pub(crate) fn integer(value: &Value) -> Result<String> {
    crate::focus::integer_string(value)
        .map(|s| canonical(&s))
        .map_err(|e| {
            error(
                match e {
                    crate::focus::IntegerError::NonFiniteOverflow => "OverflowError",
                    _ if matches!(value, Value::Null | Value::Array(_) | Value::Object(_)) => {
                        "TypeError"
                    }
                    _ => "ValueError",
                },
                e.to_string(),
            )
        })
}
pub(crate) fn numeric(value: &Value) -> Result<f64> {
    if !matches!(value, Value::Number(_) | Value::Bool(_)) {
        return Err(error("TypeError", "se requiere número"));
    }
    pyfloat(value)
}
pub(crate) fn number(n: f64) -> Value {
    if n.is_finite() {
        return Value::from(n);
    }
    let raw = if n.is_nan() {
        "NaN".into()
    } else if n == f64::INFINITY {
        "Infinity".into()
    } else if n == f64::NEG_INFINITY {
        "-Infinity".into()
    } else {
        n.to_string()
    };
    crate::json::workspace_loads(&raw).expect("numeric spelling")
}
pub(crate) fn integer_value(raw: &str) -> Value {
    crate::json::workspace_loads(raw).expect("integer spelling")
}
pub(crate) fn round_int(n: f64) -> Result<Value> {
    if !n.is_finite() {
        return Err(error(
            if n.is_nan() {
                "ValueError"
            } else {
                "OverflowError"
            },
            "redondeo no finito",
        ));
    }
    Ok(integer_value(&canonical(&format!(
        "{:.0}",
        n.round_ties_even()
    ))))
}
pub(crate) fn round_value(value: &Value) -> Result<Value> {
    match value {
        Value::Bool(_) => Ok(integer_value(&integer(value)?)),
        Value::Number(n) if crate::pomodoro::integer_token(n.as_str()) => {
            Ok(integer_value(&canonical(n.as_str())))
        }
        Value::Number(_) => round_int(numeric(value)?),
        _ => Err(error("TypeError", "valor sin redondeo numérico")),
    }
}
pub(crate) fn round_digits(n: f64, digits: usize) -> f64 {
    if !n.is_finite() {
        n
    } else {
        format!("{n:.digits$}").parse().expect("formatted float")
    }
}
pub(crate) fn pymin(a: f64, b: f64) -> f64 {
    if b < a { b } else { a }
}
pub(crate) fn pymax(a: f64, b: f64) -> f64 {
    if b > a { b } else { a }
}
pub(crate) fn numeric_cmp(a: &Value, b: &Value) -> Result<Option<Ordering>> {
    for value in [a, b] {
        if !matches!(value, Value::Number(_) | Value::Bool(_)) {
            return Err(error("TypeError", "comparación no numérica"));
        }
    }
    let special = |v: &Value| -> Option<f64> {
        match v {
            Value::Number(n) if !crate::pomodoro::integer_token(n.as_str()) => {
                n.as_str().parse().ok()
            }
            _ => None,
        }
    };
    let (x, y) = (special(a), special(b));
    if x.is_some_and(f64::is_nan) || y.is_some_and(f64::is_nan) {
        return Ok(None);
    }
    if let Some(x) = x.filter(|x| x.is_infinite()) {
        return Ok(Some(if let Some(y) = y.filter(|y| y.is_infinite()) {
            x.partial_cmp(&y).expect("not NaN")
        } else if x.is_sign_positive() {
            Ordering::Greater
        } else {
            Ordering::Less
        }));
    }
    if let Some(y) = y.filter(|y| y.is_infinite()) {
        return Ok(Some(if y.is_sign_positive() {
            Ordering::Less
        } else {
            Ordering::Greater
        }));
    }
    Ok(Some(crate::json::number_cmp(a, b)))
}
fn canonical(raw: &str) -> String {
    let negative = raw.starts_with('-');
    let digits = raw.trim_start_matches('-').trim_start_matches('0');
    if digits.is_empty() {
        "0".into()
    } else {
        format!("{}{digits}", if negative { "-" } else { "" })
    }
}
fn magnitude_cmp(a: &str, b: &str) -> Ordering {
    a.len().cmp(&b.len()).then_with(|| a.cmp(b))
}
fn magnitude_add(a: &str, b: &str) -> String {
    let (mut a, mut b) = (a.bytes().rev(), b.bytes().rev());
    let mut out = vec![];
    let mut carry = 0;
    loop {
        let (x, y) = (a.next(), b.next());
        if x.is_none() && y.is_none() && carry == 0 {
            break;
        }
        let sum = x.map_or(0, |d| d - b'0') + y.map_or(0, |d| d - b'0') + carry;
        out.push(b'0' + sum % 10);
        carry = sum / 10;
    }
    out.reverse();
    String::from_utf8(out).expect("digits")
}
fn magnitude_sub(a: &str, b: &str) -> String {
    let mut out = vec![];
    let mut b = b.bytes().rev();
    let mut borrow = 0i16;
    for d in a.bytes().rev() {
        let mut n = (d - b'0') as i16 - b.next().map_or(0, |d| (d - b'0') as i16) - borrow;
        borrow = if n < 0 {
            n += 10;
            1
        } else {
            0
        };
        out.push(b'0' + n as u8);
    }
    out.reverse();
    canonical(std::str::from_utf8(&out).expect("digits"))
}
pub(crate) fn add_integers(a: &str, b: &str) -> String {
    let (a, b) = (canonical(a), canonical(b));
    let (an, bn) = (a.starts_with('-'), b.starts_with('-'));
    let (x, y) = (a.trim_start_matches('-'), b.trim_start_matches('-'));
    if an == bn {
        return canonical(&format!(
            "{}{}",
            if an { "-" } else { "" },
            magnitude_add(x, y)
        ));
    }
    let cmp = magnitude_cmp(x, y);
    if cmp.is_eq() {
        return "0".into();
    }
    let (large, small, negative) = if cmp.is_gt() { (x, y, an) } else { (y, x, bn) };
    canonical(&format!(
        "{}{}",
        if negative { "-" } else { "" },
        magnitude_sub(large, small)
    ))
}
pub(crate) fn divide_positive(raw: &str, divisor: u32) -> (String, u32) {
    let mut rem = 0;
    let mut out = String::new();
    for d in raw.bytes() {
        let n = rem * 10 + (d - b'0') as u32;
        out.push((b'0' + (n / divisor) as u8) as char);
        rem = n % divisor;
    }
    (canonical(&out), rem)
}
pub(crate) fn integer_float(raw: &str) -> Result<f64> {
    let n = raw
        .parse::<f64>()
        .map_err(|_| error("OverflowError", "entero fuera de rango float"))?;
    if n.is_finite() {
        Ok(n)
    } else {
        Err(error("OverflowError", "entero fuera de rango float"))
    }
}
fn integer_number(value: &Value) -> bool {
    matches!(value, Value::Bool(_))
        || value
            .as_number()
            .is_some_and(|n| crate::pomodoro::integer_token(n.as_str()))
}
pub(crate) fn numeric_add(a: &Value, b: &Value) -> Result<Value> {
    if integer_number(a) && integer_number(b) {
        Ok(integer_value(&add_integers(&integer(a)?, &integer(b)?)))
    } else {
        Ok(number(numeric(a)? + numeric(b)?))
    }
}
pub(crate) fn numeric_sub(a: &Value, b: &Value) -> Result<Value> {
    if integer_number(a) && integer_number(b) {
        let raw = integer(b)?;
        let negative = if let Some(stripped) = raw.strip_prefix('-') {
            stripped.to_string()
        } else {
            format!("-{raw}")
        };
        Ok(integer_value(&add_integers(&integer(a)?, &negative)))
    } else {
        Ok(number(numeric(a)? - numeric(b)?))
    }
}
pub(crate) fn object<const N: usize>(fields: [(&str, Value); N]) -> Value {
    Value::Object(
        fields
            .into_iter()
            .map(|(k, v)| (k.to_string(), v))
            .collect(),
    )
}
// Preserve int/int division before float rounding, without first rounding a large
// integer to f64. Decimal long division retains the fraction's rounding direction.
pub(crate) fn divide_number(value: &Value, divisor: u32) -> Result<f64> {
    if matches!(value, Value::Bool(_))
        || value
            .as_number()
            .is_some_and(|n| crate::pomodoro::integer_token(n.as_str()))
    {
        let raw = integer(value)?;
        let negative = raw.starts_with('-');
        let (q, mut remainder) = divide_positive(raw.trim_start_matches('-'), divisor);
        let mut decimal = format!("{}{q}.", if negative { "-" } else { "" });
        for _ in 0..80 {
            let current = remainder * 10;
            decimal.push((b'0' + (current / divisor) as u8) as char);
            remainder = current % divisor;
        }
        integer_float(&decimal)
    } else {
        Ok(numeric(value)? / divisor as f64)
    }
}
pub fn fmt_duration(seconds: &Value) -> Result<String> {
    let seconds = pymax(0., pyfloat(seconds)?);
    let hours = seconds / 3600.;
    if seconds < 3600. {
        return Ok(format!("{} min", round_int(seconds / 60.)?));
    }
    if seconds < 86400. {
        return Ok(format!("{} h", round_int(hours)?));
    }
    let raw = round_int(hours)?.to_string();
    let (days, rest) = divide_positive(&raw, 24);
    Ok(format!("{days}d {rest}h"))
}
pub fn enrich_limits(limits: &[Value], now: f64, lang: &str) -> Result<Vec<Value>> {
    let mut out = vec![];
    for raw in limits {
        let mut row = raw
            .as_object()
            .cloned()
            .ok_or_else(|| error("TypeError", "se requiere fila de cuota"))?;
        let win = match raw["window"].as_str() {
            Some("5h") => Some(18000.),
            Some("7d") => Some(604800.),
            _ => None,
        };
        let reset = pyfloat(default(&raw["resets_at"], &json!(0)))?;
        row.insert(
            "windowSeconds".into(),
            win.map_or(Value::Null, |n| json!(n as i64)),
        );
        if win.is_none() || reset <= now || raw["percent"].is_null() {
            for key in ["pace", "burn", "runsOutIn", "reachesReset"] {
                row.insert(key.into(), Value::Null);
            }
            row.insert("verdict".into(), json!(""));
            out.push(Value::Object(row));
            continue;
        }
        let win = win.expect("checked window");
        let remaining = reset - now;
        let elapsed = pymax(1., win - remaining);
        let pace = pymin(100., elapsed / win * 100.);
        let pct = pyfloat(&raw["percent"])?;
        let burn = pct / pymax(1., pace);
        let rate = pct / elapsed;
        let run = if rate > 0. {
            Some((100. - pct) / rate)
        } else {
            None
        };
        let reaches = run.is_none_or(|run| run >= remaining);
        row.insert("pace".into(), number(pace));
        row.insert("burn".into(), number(burn));
        row.insert(
            "runsOutIn".into(),
            if reaches {
                Value::Null
            } else {
                number(run.expect("positive rate"))
            },
        );
        row.insert("reachesReset".into(), json!(reaches));
        let verdict = if reaches {
            if lang == "en" {
                "lasts to reset"
            } else {
                "llega al reset"
            }
            .into()
        } else {
            let duration = fmt_duration(&number(run.expect("positive rate")))?;
            format!(
                "{} {duration}",
                if lang == "en" {
                    "runs out in"
                } else {
                    "se acaba en"
                }
            )
        };
        row.insert("verdict".into(), json!(verdict));
        out.push(Value::Object(row));
    }
    Ok(out)
}
