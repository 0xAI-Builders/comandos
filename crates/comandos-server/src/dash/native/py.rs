//! Semántica de CPython que las rutas nativas reproducen. Todo lo que no se
//! puede reproducir con certeza devuelve `Exotic` y la ruta declina.
use serde_json::Value;

/// `str.isspace()` de CPython: `char::is_whitespace` más U+001C–U+001F.
pub fn is_space(c: char) -> bool {
    c.is_whitespace() || ('\u{1c}'..='\u{1f}').contains(&c)
}

pub fn strip(s: &str) -> &str {
    s.trim_matches(is_space)
}

pub use comandos_core::text::{NumError, float, splitlines, strip_numeric};

/// Dígitos ASCII con `_` solo entre dígitos (PEP 515).
fn digits_with_underscores(s: &str) -> Option<String> {
    let bytes = s.as_bytes();
    if bytes.is_empty() {
        return None;
    }
    let mut out = String::with_capacity(s.len());
    for (i, &b) in bytes.iter().enumerate() {
        if b == b'_' {
            let left = i > 0 && bytes[i - 1].is_ascii_digit();
            let right = bytes.get(i + 1).is_some_and(u8::is_ascii_digit);
            if !(left && right) {
                return None;
            }
        } else if b.is_ascii_digit() {
            out.push(b as char);
        } else {
            return None;
        }
    }
    Some(out)
}

/// `int(text)` en base 10.
pub fn int(text: &str) -> Result<i64, NumError> {
    if !text.is_ascii() {
        return Err(NumError::Exotic);
    }
    let t = strip_numeric(text);
    let (negative, body) = match t.as_bytes().first() {
        Some(b'-') => (true, &t[1..]),
        Some(b'+') => (false, &t[1..]),
        _ => (false, t),
    };
    let digits = digits_with_underscores(body).ok_or(NumError::Invalid)?;
    let magnitude: i128 = digits.parse().map_err(|_| NumError::Exotic)?;
    let value = if negative { -magnitude } else { magnitude };
    i64::try_from(value).map_err(|_| NumError::Exotic)
}

/// `str(ValueError)` de `int(text)`; `None` si el texto no es ASCII. CPython
/// lo arma con `%.200R`: el `repr` se corta a 200 caracteres (comilla final incluida).
pub fn int_error_message(text: &str) -> Option<String> {
    repr_ascii(text).map(|r| {
        format!(
            "invalid literal for int() with base 10: {}",
            take_chars(&r, 200)
        )
    })
}

/// `max(lo, min(hi, x))` de Python: con NaN, `min(hi, nan)` devuelve `hi`.
pub fn clamp_py_float(x: f64, lo: f64, hi: f64) -> f64 {
    let upper = if x < hi { x } else { hi };
    if lo < upper { upper } else { lo }
}

/// `repr(str)` para texto ASCII; `None` si hay no-ASCII (se declina).
pub fn repr_ascii(s: &str) -> Option<String> {
    if !s.is_ascii() {
        return None;
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
            c if (c as u32) < 0x20 || c == '\u{7f}' => {
                out.push_str(&format!("\\x{:02x}", c as u32))
            }
            c => out.push(c),
        }
    }
    out.push(quote);
    Some(out)
}

fn integer_text(raw: &str) -> bool {
    !raw.contains(['.', 'e', 'E']) && !matches!(raw, "NaN" | "Infinity" | "-Infinity")
}

/// `str(value)` para escalares cuya forma es segura; `None` → declinar
/// (flotantes: el `repr` de Python; listas y objetos: su `repr`).
pub fn str_scalar(value: &Value) -> Option<String> {
    match value {
        Value::String(s) => Some(s.clone()),
        Value::Bool(true) => Some("True".into()),
        Value::Bool(false) => Some("False".into()),
        Value::Null => Some("None".into()),
        Value::Number(n) if integer_text(n.as_str()) => Some(if n.as_str() == "-0" {
            "0".into()
        } else {
            n.as_str().to_owned()
        }),
        _ => None,
    }
}

/// Excepción que lanzaría `int(value)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Conversion {
    /// `ValueError(str)`.
    Value(String),
    /// `OverflowError` (infinito).
    Overflow,
    /// `TypeError`.
    Type,
    /// No reproducible con certeza: declinar.
    Exotic,
}

/// `int(value)` sobre un valor JSON.
pub fn int_of(value: &Value) -> Result<i64, Conversion> {
    match value {
        Value::Bool(b) => Ok(i64::from(*b)),
        Value::Number(n) => {
            let raw = n.as_str();
            match raw {
                "NaN" => Err(Conversion::Value(
                    "cannot convert float NaN to integer".into(),
                )),
                "Infinity" | "-Infinity" => Err(Conversion::Overflow),
                _ if integer_text(raw) => raw.parse::<i64>().map_err(|_| Conversion::Exotic),
                _ => {
                    let x: f64 = raw.parse().map_err(|_| Conversion::Exotic)?;
                    if !x.is_finite() {
                        return Err(Conversion::Overflow);
                    }
                    let t = x.trunc();
                    // i64 en flotante: [-2^63, 2^63); fuera, Python da un entero grande.
                    let limit = 2f64.powi(63);
                    if (-limit..limit).contains(&t) {
                        Ok(t as i64)
                    } else {
                        Err(Conversion::Exotic)
                    }
                }
            }
        }
        Value::String(s) => match int(s) {
            Ok(v) => Ok(v),
            Err(NumError::Exotic) => Err(Conversion::Exotic),
            Err(NumError::Invalid) => {
                Err(int_error_message(s).map_or(Conversion::Exotic, Conversion::Value))
            }
        },
        Value::Null | Value::Array(_) | Value::Object(_) => Err(Conversion::Type),
    }
}

/// `SESSION_RE = ^[A-Za-z0-9._-]{1,80}\Z` (`bin/cc-dash:5590`). La clase es
/// ASCII explícita (sin `\d` ni `\w`): un sí/no exacto también con no-ASCII.
pub fn is_session(s: &str) -> bool {
    (1..=80).contains(&s.len())
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
}

/// `PANE_RE = ^%\d{1,7}\Z` (5591). `Some(sí/no)` para texto ASCII; `None` si hay
/// no-ASCII: el `\d` de Python acepta dígitos Unicode (`%١٢`), así que se declina.
pub fn is_pane(s: &str) -> Option<bool> {
    if !s.is_ascii() {
        return None;
    }
    Some(
        s.strip_prefix('%')
            .is_some_and(|d| (1..=7).contains(&d.len()) && d.bytes().all(|b| b.is_ascii_digit())),
    )
}

/// `s[:n]` de Python (por caracteres).
pub fn take_chars(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}
