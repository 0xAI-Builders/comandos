//! Python-compatible sorted JSON bytes for persisted metadata identities.
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::fmt::Write;

/// Sorted keys; compact selects (",", ":") or Python's default separators.
pub fn dumps(value: &Value, ensure_ascii: bool, compact: bool) -> Result<String, String> {
    let mut out = String::new();
    encode(value, ensure_ascii, compact, &mut out, 0)?;
    Ok(out)
}

pub fn digest(value: &Value) -> Result<String, String> {
    Ok(format!(
        "{:x}",
        Sha256::digest(dumps(value, true, true)?.as_bytes())
    ))
}

/// Object ordering cannot change length; ASCII escaping makes bytes == chars.
pub fn default_len(value: &Value) -> Result<usize, String> {
    Ok(dumps(value, true, false)?.len())
}

fn encode(
    value: &Value,
    ascii: bool,
    compact: bool,
    out: &mut String,
    depth: usize,
) -> Result<(), String> {
    if depth >= 128 {
        return Err("JSON nesting limit reached".into());
    }
    let comma = if compact { "," } else { ", " };
    let colon = if compact { ":" } else { ": " };
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(value) => out.push_str(if *value { "true" } else { "false" }),
        Value::String(value) => quoted(value, ascii, out),
        Value::Number(number) => {
            let raw = number.as_str();
            if raw.contains(['.', 'e', 'E']) {
                let value = raw.parse::<f64>().map_err(|_| "Invalid JSON number")?;
                out.push_str(&python_float(value)?);
            } else {
                out.push_str(if raw == "-0" { "0" } else { raw });
            }
        }
        Value::Array(values) => {
            out.push('[');
            for (index, value) in values.iter().enumerate() {
                if index != 0 {
                    out.push_str(comma);
                }
                encode(value, ascii, compact, out, depth + 1)?;
            }
            out.push(']');
        }
        Value::Object(values) => {
            // Explicit sorting also works if another workspace consumer later
            // enables serde_json's preserve_order feature.
            let mut keys: Vec<_> = values.keys().collect();
            keys.sort_unstable();
            out.push('{');
            for (index, key) in keys.iter().enumerate() {
                if index != 0 {
                    out.push_str(comma);
                }
                quoted(key, ascii, out);
                out.push_str(colon);
                encode(&values[*key], ascii, compact, out, depth + 1)?;
            }
            out.push('}');
        }
    }
    Ok(())
}

fn quoted(value: &str, ascii: bool, out: &mut String) {
    out.push('"');
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            ch if ch < '\u{20}' || (ascii && ch >= '\u{7f}') => {
                for unit in ch.encode_utf16(&mut [0; 2]) {
                    let _ = write!(out, "\\u{unit:04x}");
                }
            }
            ch => out.push(ch),
        }
    }
    out.push('"');
}

// serde_json's maintained shortest-roundtrip formatter supplies nearest/even
// digits, including exact decimal ties where Rust Debug differs from CPython.
// CPython's repr uses scientific notation outside [-4,16), with signed,
// minimum-two-digit exponents. Preserve float identity in fixed integral forms.
fn python_float(value: f64) -> Result<String, String> {
    if !value.is_finite() {
        return Err("Non-finite JSON number".into());
    }
    if value == 0.0 {
        return Ok(if value.is_sign_negative() {
            "-0.0"
        } else {
            "0.0"
        }
        .into());
    }
    let raw = serde_json::to_string(&value).map_err(|_| "Invalid JSON float")?;
    let negative = raw.starts_with('-');
    let raw = raw.trim_start_matches('-');
    let (mantissa, exponent) = match raw.split_once('e') {
        Some((digits, exponent)) => (
            digits,
            exponent
                .parse::<i32>()
                .map_err(|_| "Invalid float exponent")?,
        ),
        None => (raw, 0),
    };
    let mut decimal = mantissa.find('.').unwrap_or(mantissa.len()) as i32 + exponent;
    let digits = mantissa.replace('.', "");
    let leading = digits.len() - digits.trim_start_matches('0').len();
    decimal -= leading as i32;
    let digits = digits[leading..].trim_end_matches('0');
    let exponent = decimal - 1;
    let mut out = String::new();
    if negative {
        out.push('-');
    }
    if !(-4..16).contains(&exponent) {
        out.push_str(&digits[..1]);
        if digits.len() > 1 {
            out.push('.');
            out.push_str(&digits[1..]);
        }
        let _ = write!(out, "e{exponent:+03}");
    } else if decimal <= 0 {
        out.push_str("0.");
        out.extend(std::iter::repeat_n('0', (-decimal) as usize));
        out.push_str(digits);
    } else if decimal as usize >= digits.len() {
        out.push_str(digits);
        out.extend(std::iter::repeat_n('0', decimal as usize - digits.len()));
        out.push_str(".0");
    } else {
        out.push_str(&digits[..decimal as usize]);
        out.push('.');
        out.push_str(&digits[decimal as usize..]);
    }
    Ok(out)
}
