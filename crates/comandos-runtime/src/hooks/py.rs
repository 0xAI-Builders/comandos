//! Semántica de Python que necesitan los adaptadores que antes eran Python
//! (`grok-hooks.py`, `agy-statusline.py`, el bloque `python3 -c` de
//! `agy-hooks.sh`): `json.load` estricto, `repr(float)`, `str()`, verdad,
//! igualdad numérica y `json.dump` con o sin `ensure_ascii`.
use serde_json::{Number, Value};

/// `json.load` de un texto UTF-8 estricto (lo que no decodifica es `ValueError`).
pub fn json_load(raw: &[u8]) -> Option<Value> {
    serde_json::from_str(std::str::from_utf8(raw).ok()?).ok()
}

/// El número de JSON es `float` en Python si lleva fracción o exponente.
pub fn is_float(n: &Number) -> bool {
    n.to_string().contains(['.', 'e', 'E'])
}

/// Valor numérico de un número de JSON (`int` grande → `f64` con pérdida).
pub fn as_f64(n: &Number) -> f64 {
    n.as_f64()
        .unwrap_or_else(|| n.to_string().parse().unwrap_or(f64::NAN))
}

/// `repr(float)`: dígitos más cortos que reconstruyen el valor; notación fija si el
/// exponente decimal está en [-4, 16) y científica fuera.
pub fn float_repr(f: f64) -> String {
    if f.is_nan() {
        return "nan".into();
    }
    if f.is_infinite() {
        return if f > 0.0 { "inf" } else { "-inf" }.into();
    }
    let sci = format!("{:e}", f.abs());
    let (mantissa, exp) = sci.split_once('e').unwrap_or((&sci, "0"));
    let digits: String = mantissa.chars().filter(char::is_ascii_digit).collect();
    let exp: i32 = exp.parse().unwrap_or(0);
    let sign = if f.is_sign_negative() { "-" } else { "" };
    let nd = digits.len() as i32;
    if (-4..16).contains(&exp) {
        let point = exp + 1;
        let body = if point <= 0 {
            format!("0.{}{digits}", "0".repeat((-point) as usize))
        } else if point >= nd {
            format!("{digits}{}.0", "0".repeat((point - nd) as usize))
        } else {
            format!(
                "{}.{}",
                &digits[..point as usize],
                &digits[point as usize..]
            )
        };
        format!("{sign}{body}")
    } else {
        let frac = if nd > 1 {
            format!(".{}", &digits[1..])
        } else {
            String::new()
        };
        let e_sign = if exp < 0 { '-' } else { '+' };
        format!("{sign}{}{frac}e{e_sign}{:02}", &digits[..1], exp.abs())
    }
}

/// `str(int)` de un entero de JSON (`-0` es `0`).
pub fn int_text(n: &Number) -> String {
    let text = n.to_string();
    if text.trim_start_matches('-').bytes().all(|b| b == b'0') {
        "0".into()
    } else {
        text
    }
}

/// `str()` de un escalar numérico de JSON.
pub fn number_str(n: &Number) -> String {
    if is_float(n) {
        float_repr(as_f64(n))
    } else {
        int_text(n)
    }
}

/// Verdad de Python.
pub fn truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => as_f64(n) != 0.0,
        Value::String(s) => !s.is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => !o.is_empty(),
    }
}

/// `==` de Python entre valores de JSON (`True == 1 == 1.0`).
pub fn eq(a: &Value, b: &Value) -> bool {
    let num = |v: &Value| match v {
        Value::Bool(x) => Some(f64::from(u8::from(*x))),
        Value::Number(n) => Some(as_f64(n)),
        _ => None,
    };
    match (a, b) {
        (Value::Array(x), Value::Array(y)) => {
            x.len() == y.len() && x.iter().zip(y).all(|(p, q)| eq(p, q))
        }
        (Value::Object(x), Value::Object(y)) => {
            x.len() == y.len() && x.iter().all(|(k, v)| y.get(k).is_some_and(|w| eq(v, w)))
        }
        _ => match (num(a), num(b)) {
            (Some(x), Some(y)) => x == y,
            (None, None) => a == b,
            _ => false,
        },
    }
}

/// `str()` de un valor de JSON (contenedores con su `repr`).
pub fn str_of(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        other => repr(other),
    }
}

/// `repr()` de un valor de JSON tal como lo deja `json.load`.
pub fn repr(value: &Value) -> String {
    match value {
        Value::Null => "None".into(),
        Value::Bool(b) => if *b { "True" } else { "False" }.into(),
        Value::Number(n) => number_str(n),
        Value::String(s) => str_repr(s),
        Value::Array(items) => {
            let inner: Vec<String> = items.iter().map(repr).collect();
            format!("[{}]", inner.join(", "))
        }
        Value::Object(map) => {
            let inner: Vec<String> = map
                .iter()
                .map(|(k, v)| format!("{}: {}", str_repr(k), repr(v)))
                .collect();
            format!("{{{}}}", inner.join(", "))
        }
    }
}

fn str_repr(s: &str) -> String {
    let quote = if s.contains('\'') && !s.contains('"') {
        '"'
    } else {
        '\''
    };
    let mut out = String::from(quote);
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c == quote => {
                out.push('\\');
                out.push(c);
            }
            c if c.is_control() || (c.is_whitespace() && c != ' ') => {
                let n = c as u32;
                if n < 0x100 {
                    out.push_str(&format!("\\x{n:02x}"));
                } else if n < 0x10000 {
                    out.push_str(&format!("\\u{n:04x}"));
                } else {
                    out.push_str(&format!("\\U{n:08x}"));
                }
            }
            c => out.push(c),
        }
    }
    out.push(quote);
    out
}

/// Los primeros `n` puntos de código (`texto[:n]`).
pub fn take(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

/// Cadena de `json.dump` (`ensure_ascii` escapa todo lo que no es ASCII imprimible).
pub fn dump_str(s: &str, ensure_ascii: bool, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c if ensure_ascii && (c as u32) > 0x7e => {
                let mut units = [0u16; 2];
                for unit in c.encode_utf16(&mut units) {
                    out.push_str(&format!("\\u{unit:04x}"));
                }
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

/// `json.dump(valor)` con separadores `(item, clave)`.
pub fn dump(value: &Value, ensure_ascii: bool, seps: (&str, &str), out: &mut String) {
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Number(n) if is_float(n) => {
            let f = as_f64(n);
            out.push_str(&if f.is_nan() {
                "NaN".into()
            } else if f.is_infinite() {
                if f > 0.0 { "Infinity" } else { "-Infinity" }.into()
            } else {
                float_repr(f)
            });
        }
        Value::Number(n) => out.push_str(&int_text(n)),
        Value::String(s) => dump_str(s, ensure_ascii, out),
        Value::Array(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push_str(seps.0);
                }
                dump(item, ensure_ascii, seps, out);
            }
            out.push(']');
        }
        Value::Object(map) => {
            out.push('{');
            for (i, (k, v)) in map.iter().enumerate() {
                if i > 0 {
                    out.push_str(seps.0);
                }
                dump_str(k, ensure_ascii, out);
                out.push_str(seps.1);
                dump(v, ensure_ascii, seps, out);
            }
            out.push('}');
        }
    }
}

/// Número `float` de JSON que `json.dump` imprime con `repr`.
pub fn float_value(f: f64) -> Value {
    Number::from_f64(f).map_or(Value::Null, Value::Number)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn float_repr_matches_python() {
        // Casos medidos con `repr()` de CPython 3.
        for (f, want) in [
            (1.0, "1.0"),
            (0.9725484, "0.9725484"),
            (1e-5, "1e-05"),
            (0.0001, "0.0001"),
            (1e16, "1e+16"),
            (123456789012345678.0, "1.2345678901234568e+17"),
            (1234567890123456.0, "1234567890123456.0"),
            (-0.0, "-0.0"),
            (2.5e-7, "2.5e-07"),
            (100000.0, "100000.0"),
        ] {
            assert_eq!(float_repr(f), want, "{f}");
        }
    }

    #[test]
    fn dump_and_str_like_python() {
        let mut out = String::new();
        dump(
            &json!({"a": [1, "é\u{7f}", null, true], "b": float_value(2.0)}),
            true,
            (", ", ": "),
            &mut out,
        );
        assert_eq!(out, r#"{"a": [1, "\u00e9\u007f", null, true], "b": 2.0}"#);
        let mut out = String::new();
        dump_str("é😀\u{7f}", false, &mut out);
        assert_eq!(out, "\"é😀\u{7f}\"");
        let mut out = String::new();
        dump_str("😀", true, &mut out);
        assert_eq!(out, r#""\ud83d\ude00""#);
        assert_eq!(
            str_of(&json!([1, "it's", {"k": 2.5}])),
            r#"[1, "it's", {'k': 2.5}]"#
        );
        assert!(eq(&json!({"x": [true]}), &json!({"x": [1.0]})));
        assert_eq!(
            int_text(&serde_json::from_str::<Number>("-0").unwrap()),
            "0"
        );
    }
}
