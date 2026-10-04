//! Serialización de jq 1.6: objetos de `jq -n`/`jq -cn`, cadenas con sus escapes
//! (`\u007f` incluido, `/` sin escapar), números como dobles (`jvp_dtoa_fmt`) y la
//! salida de `jq -r`, `tostring` y `join` sobre valores arbitrarios.
use super::text::strip_nl;
use serde_json::Value;

/// Valor de un objeto construido con `jq -n --arg/--argjson`.
pub enum J<'a> {
    S(&'a str),
    I(i64),
}

/// `jq -c` de un objeto plano.
pub fn jq_compact(fields: &[(&str, J)]) -> String {
    let mut out = String::from("{");
    for (n, (key, value)) in fields.iter().enumerate() {
        if n > 0 {
            out.push(',');
        }
        escape(key, &mut out);
        out.push(':');
        push_j(value, &mut out);
    }
    out.push('}');
    out
}

/// Salida por defecto (indentada) de `jq -n` de un objeto plano, con su `\n` final.
pub fn jq_pretty(fields: &[(&str, J)]) -> String {
    if fields.is_empty() {
        return "{}\n".into();
    }
    let mut out = String::from("{\n");
    for (n, (key, value)) in fields.iter().enumerate() {
        if n > 0 {
            out.push_str(",\n");
        }
        out.push_str("  ");
        escape(key, &mut out);
        out.push_str(": ");
        push_j(value, &mut out);
    }
    out.push_str("\n}\n");
    out
}

fn push_j(value: &J, out: &mut String) {
    match value {
        J::S(s) => escape(s, out),
        J::I(i) => out.push_str(&i.to_string()),
    }
}

/// Cadena JSON como la imprime jq 1.6 (`\u007f` incluido, `/` sin escapar).
pub fn escape(s: &str, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 || c == '\u{7f}' => {
                out.push_str(&format!("\\u{:04x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

/// Número como lo imprime jq 1.6 (doble, `jvp_dtoa_fmt`).
pub fn jq_number(n: &serde_json::Number) -> String {
    let f = n.as_f64().unwrap_or(0.0);
    if f == 0.0 {
        return if f.is_sign_negative() { "-0" } else { "0" }.into();
    }
    let sci = format!("{:e}", f.abs());
    let (mantissa, exp) = sci.split_once('e').unwrap_or((&sci, "0"));
    let digits: String = mantissa.chars().filter(|c| c.is_ascii_digit()).collect();
    let decpt = exp.parse::<i32>().unwrap_or(0) + 1;
    let nd = digits.len() as i32;
    let mut out = String::from(if f < 0.0 { "-" } else { "" });
    if decpt <= -4 || decpt > nd + 15 {
        out.push_str(&digits[..1]);
        if nd > 1 {
            out.push('.');
            out.push_str(&digits[1..]);
        }
        let e = decpt - 1;
        out.push_str(&format!("e{}{:02}", if e < 0 { '-' } else { '+' }, e.abs()));
    } else if decpt <= 0 {
        out.push_str("0.");
        out.push_str(&"0".repeat((-decpt) as usize));
        out.push_str(&digits);
    } else if decpt >= nd {
        out.push_str(&digits);
        out.push_str(&"0".repeat((decpt - nd) as usize));
    } else {
        out.push_str(&digits[..decpt as usize]);
        out.push('.');
        out.push_str(&digits[decpt as usize..]);
    }
    out
}

/// JSON de jq (`indent` 2 = salida por defecto, `None` = `-c`).
pub fn jq_dump(value: &Value, indent: Option<usize>, out: &mut String) {
    let pad = |out: &mut String, level: usize| {
        if let Some(width) = indent {
            out.push('\n');
            out.push_str(&" ".repeat(width * level));
        }
    };
    fn walk(
        value: &Value,
        level: usize,
        indent: Option<usize>,
        out: &mut String,
        pad: &dyn Fn(&mut String, usize),
    ) {
        match value {
            Value::Null => out.push_str("null"),
            Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
            Value::Number(n) => out.push_str(&jq_number(n)),
            Value::String(s) => escape(s, out),
            Value::Array(items) if items.is_empty() => out.push_str("[]"),
            Value::Object(map) if map.is_empty() => out.push_str("{}"),
            Value::Array(items) => {
                out.push('[');
                for (n, item) in items.iter().enumerate() {
                    if n > 0 {
                        out.push(',');
                    }
                    pad(out, level + 1);
                    walk(item, level + 1, indent, out, pad);
                }
                pad(out, level);
                out.push(']');
            }
            Value::Object(map) => {
                out.push('{');
                for (n, (key, item)) in map.iter().enumerate() {
                    if n > 0 {
                        out.push(',');
                    }
                    pad(out, level + 1);
                    escape(key, out);
                    out.push_str(if indent.is_some() { ": " } else { ":" });
                    walk(item, level + 1, indent, out, pad);
                }
                pad(out, level);
                out.push('}');
            }
        }
    }
    walk(value, 0, indent, out, &pad);
}

/// Lo que imprime `jq -r` de un valor, ya pasado por `$(...)`.
pub fn jq_r(value: &Value) -> Vec<u8> {
    let text = match value {
        Value::String(s) => s.clone(),
        other => {
            let mut out = String::new();
            jq_dump(other, Some(2), &mut out);
            out
        }
    };
    strip_nl(text.as_bytes()).to_vec()
}

/// `tostring` / interpolación `\(...)` de jq.
pub fn jq_tostring(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        other => {
            let mut out = String::new();
            jq_dump(other, None, &mut out);
            out
        }
    }
}

/// `join(sep)` de jq 1.6: `null` cuenta como vacío y el resto va con `tojson`.
pub fn jq_join<'a>(items: impl IntoIterator<Item = &'a Value>, sep: &str) -> String {
    let mut out = String::new();
    for (n, item) in items.into_iter().enumerate() {
        if n > 0 {
            out.push_str(sep);
        }
        if !item.is_null() {
            out.push_str(&jq_tostring(item));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_and_escapes_like_jq() {
        let n = |s: &str| jq_number(&serde_json::from_str(s).unwrap());
        for (input, expected) in [
            ("1e6", "1000000"),
            ("1e17", "1e+17"),
            ("1e-5", "1e-05"),
            ("0.0001", "0.0001"),
            ("123e-7", "1.23e-05"),
            ("-2.5", "-2.5"),
            ("3.0e10", "30000000000"),
            ("12345678901234567890", "12345678901234567000"),
            ("7.0", "7"),
        ] {
            assert_eq!(n(input), expected, "{input}");
        }
        let mut out = String::new();
        escape("a\u{7f}b\u{1f}\t/é", &mut out);
        assert_eq!(out, r#""a\u007fb\u001f\t/é""#);
    }
}
