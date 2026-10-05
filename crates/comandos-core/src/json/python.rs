//! Python-compatible JSON encoding for persisted values and responses.
use super::workspace::validate_workspace_depth;
use serde_json::Value;
use std::fmt::Write;

#[derive(Clone, Copy)]
enum Policy {
    Metadata,
    Workspace,
}

/// Sorted keys; compact selects (",", ":") or Python's default separators.
/// Metadata retains its 128-level limit and rejects overflowing decimals.
pub fn dumps(value: &Value, ensure_ascii: bool, compact: bool) -> Result<String, String> {
    encode(value, ensure_ascii, compact, true, Policy::Metadata)
}

/// Python workspace canonical JSON: sorted keys, compact UTF-8, and default
/// non-finite float spelling. Retained Values allow up to 1000 containers,
/// separately from the metadata nesting limit.
/// Traversal uses heap frames, so directly supplied deep Values do not recurse.
pub fn workspace_dumps(value: &Value) -> Result<String, String> {
    workspace_dumps_with_options(value, false, true)
}

/// Python JSON with the workspace retained-number and depth policy.
/// ASCII escaping and default separators support native request digests.
pub fn workspace_dumps_with_options(
    value: &Value,
    ensure_ascii: bool,
    compact: bool,
) -> Result<String, String> {
    validate_workspace_depth(value, 0)?;
    encode(value, ensure_ascii, compact, true, Policy::Workspace)
}

/// Python json.dumps defaults for HTTP responses: insertion order, ASCII
/// escapes and spaced separators. Canonical persistence APIs remain sorted.
/// The retained-number/depth policy is shared with workspace values.
pub fn response_dumps(value: &Value) -> Result<String, String> {
    validate_workspace_depth(value, 0)?;
    encode(value, true, false, false, Policy::Workspace)
}

/// Una entrada `"clave": valor` de un objeto de respuesta, como la escribe
/// `response_dumps` dentro de un objeto (el valor está un nivel por dentro).
pub fn response_dumps_entry(key: &str, value: &Value) -> Result<String, String> {
    validate_workspace_depth(value, 1)?;
    let mut out = encode(
        &Value::String(key.to_owned()),
        true,
        false,
        false,
        Policy::Workspace,
    )?;
    out.push_str(": ");
    out.push_str(&encode(value, true, false, false, Policy::Workspace)?);
    Ok(out)
}

/// Une entradas ya escritas con `response_dumps_entry` en un objeto: los mismos
/// bytes que `response_dumps` del objeto entero.
pub fn join_response_entries<'a>(entries: impl IntoIterator<Item = &'a str> + Clone) -> String {
    let len: usize = entries.clone().into_iter().map(|e| e.len() + 2).sum();
    let mut out = String::with_capacity(len + 2);
    out.push('{');
    for (index, entry) in entries.into_iter().enumerate() {
        if index > 0 {
            out.push_str(", ");
        }
        out.push_str(entry);
    }
    out.push('}');
    out
}

/// `response_dumps` de un objeto dado por sus entradas prestadas, en orden:
/// igual que `response_dumps(&Value::Object(..))` sin construir el mapa (una
/// copia superficial de un `dict` grande del Python sale gratis; en Rust no).
pub fn response_dumps_entries<'a>(
    entries: impl IntoIterator<Item = (&'a str, &'a Value)>,
) -> Result<String, String> {
    let parts = entries
        .into_iter()
        .map(|(key, value)| response_dumps_entry(key, value))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(join_response_entries(parts.iter().map(String::as_str)))
}

/// `json.dumps(value, ensure_ascii=False)` del Python: orden de inserción,
/// separadores `", "`/`": "` y UTF-8 sin escapar (líneas de registros JSONL).
pub fn response_dumps_unicode(value: &Value) -> Result<String, String> {
    validate_workspace_depth(value, 0)?;
    encode(value, false, false, false, Policy::Workspace)
}

/// `str(float)`/`repr(float)` de Python, con `inf`, `-inf` y `nan` para los no finitos.
pub fn float_repr(value: f64) -> String {
    if value.is_nan() {
        "nan".into()
    } else if value.is_infinite() {
        if value > 0.0 { "inf" } else { "-inf" }.into()
    } else {
        python_float(value, Policy::Workspace).unwrap_or_else(|_| value.to_string())
    }
}

fn encode(
    value: &Value,
    ascii: bool,
    compact: bool,
    sorted: bool,
    policy: Policy,
) -> Result<String, String> {
    enum Frame<'a> {
        Value(&'a Value, usize),
        Array(std::iter::Enumerate<std::slice::Iter<'a, Value>>, usize),
        Object(
            std::iter::Enumerate<std::vec::IntoIter<(&'a String, &'a Value)>>,
            usize,
        ),
    }
    let comma = if compact { "," } else { ", " };
    let colon = if compact { ":" } else { ": " };
    let mut out = String::new();
    let mut frames = vec![Frame::Value(value, 0)];
    while let Some(frame) = frames.pop() {
        match frame {
            Frame::Value(value, depth) => {
                if matches!(policy, Policy::Metadata) && depth >= 128 {
                    return Err("JSON nesting limit reached".into());
                }
                match value {
                    Value::Null => out.push_str("null"),
                    Value::Bool(value) => out.push_str(if *value { "true" } else { "false" }),
                    Value::String(value) => quoted(value, ascii, &mut out),
                    Value::Number(number) => {
                        let raw = number.as_str();
                        if matches!(raw, "NaN" | "Infinity" | "-Infinity") {
                            out.push_str(raw);
                        } else if raw.contains(['.', 'e', 'E']) {
                            let value = raw.parse::<f64>().map_err(|_| "Invalid JSON number")?;
                            out.push_str(&python_float(value, policy)?);
                        } else {
                            out.push_str(if raw == "-0" { "0" } else { raw });
                        }
                    }
                    Value::Array(values) => {
                        out.push('[');
                        frames.push(Frame::Array(values.iter().enumerate(), depth + 1));
                    }
                    Value::Object(values) => {
                        let mut entries: Vec<_> = values.iter().collect();
                        if sorted {
                            entries.sort_unstable_by_key(|(key, _)| *key);
                        }
                        out.push('{');
                        frames.push(Frame::Object(entries.into_iter().enumerate(), depth + 1));
                    }
                }
            }
            Frame::Array(mut values, depth) => {
                if let Some((index, value)) = values.next() {
                    if index != 0 {
                        out.push_str(comma);
                    }
                    frames.push(Frame::Array(values, depth));
                    frames.push(Frame::Value(value, depth));
                } else {
                    out.push(']');
                }
            }
            Frame::Object(mut entries, depth) => {
                if let Some((index, (key, value))) = entries.next() {
                    if index != 0 {
                        out.push_str(comma);
                    }
                    quoted(key, ascii, &mut out);
                    out.push_str(colon);
                    frames.push(Frame::Object(entries, depth));
                    frames.push(Frame::Value(value, depth));
                } else {
                    out.push('}');
                }
            }
        }
    }
    Ok(out)
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
fn python_float(value: f64, policy: Policy) -> Result<String, String> {
    if !value.is_finite() {
        return match policy {
            Policy::Metadata => Err("Non-finite JSON number".into()),
            Policy::Workspace => Ok(if value.is_nan() {
                "NaN"
            } else if value.is_sign_negative() {
                "-Infinity"
            } else {
                "Infinity"
            }
            .into()),
        };
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

#[cfg(test)]
mod tests {
    use super::{dumps, response_dumps, response_dumps_entries};
    use serde_json::{Value, json};

    #[test]
    fn borrowed_entries_encode_like_the_whole_object() {
        let value = json!({"b": {"x": [1.5, "é"]}, "a": null, "c": [], "d": {}});
        let Value::Object(map) = &value else {
            panic!("objeto");
        };
        assert_eq!(
            response_dumps_entries(map.iter().map(|(k, v)| (k.as_str(), v))).unwrap(),
            response_dumps(&value).unwrap()
        );
        assert_eq!(response_dumps_entries([]).unwrap(), "{}");
        let mut deep = Value::Null;
        for _ in 0..999 {
            deep = Value::Array(vec![deep]);
        }
        let whole = json!({"k": deep.clone()});
        assert_eq!(
            response_dumps_entries([("k", &deep)]).is_err(),
            response_dumps(&whole).is_err()
        );
    }

    #[test]
    fn persisted_float_spelling_preserves_integer_precision() {
        let value: Value = serde_json::from_str(
            "[1.00,1e3,1e-5,1.000e+16,-0.0,9007199254740993,340282366920938463463374607431768211457]",
        ).unwrap();
        assert_eq!(
            dumps(&value, false, true).unwrap(),
            "[1.0,1000.0,1e-05,1e+16,-0.0,9007199254740993,340282366920938463463374607431768211457]"
        );
    }

    #[test]
    fn sorted_metadata_bytes_keep_ascii_and_separator_options() {
        let value = json!({"z":"é😊\n\t", "a":[true, null]});
        assert_eq!(
            dumps(&value, false, true).unwrap(),
            "{\"a\":[true,null],\"z\":\"é😊\\n\\t\"}"
        );
        assert_eq!(
            dumps(&value, true, true).unwrap(),
            "{\"a\":[true,null],\"z\":\"\\u00e9\\ud83d\\ude0a\\n\\t\"}"
        );
        assert_eq!(
            dumps(&value, true, false).unwrap(),
            "{\"a\": [true, null], \"z\": \"\\u00e9\\ud83d\\ude0a\\n\\t\"}"
        );
    }

    #[test]
    fn encoder_preserves_depth_and_overflow_errors() {
        let mut value = Value::Null;
        for _ in 0..127 {
            value = json!([value]);
        }
        assert!(dumps(&value, false, true).is_ok());
        value = json!([value]);
        assert_eq!(
            dumps(&value, false, true).unwrap_err(),
            "JSON nesting limit reached"
        );
        let overflow: Value = serde_json::from_str("1e9999").unwrap();
        assert_eq!(
            dumps(&overflow, false, true).unwrap_err(),
            "Non-finite JSON number"
        );
    }
}
