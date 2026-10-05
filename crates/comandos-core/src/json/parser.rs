//! Lossless JSON numbers without interpreting user object keys as serde tags.
use serde::Deserializer;
use serde::de::{self, MapAccess, Visitor};
use serde_json::{Map, Value, value::RawValue};

fn error(message: &str) -> serde_json::Error {
    <serde_json::Error as de::Error>::custom(message)
}

pub fn parse_value(raw: &str) -> Result<Value, serde_json::Error> {
    // serde_json's arbitrary_precision/raw_value visitors reserve object keys.
    // Ordinary input retains its single-pass parser; escaped keys take the safe
    // path too, so spelling a reserved name with Unicode escapes cannot bypass it.
    if raw.contains("serde_json::private::") || raw.contains("\\u") {
        parse_raw(serde_json::from_str::<&RawValue>(raw)?.get(), false, 0)
    } else {
        let mut value = serde_json::from_str(raw)?;
        normalize_floats(&mut value);
        Ok(value)
    }
}

// Before arbitrary_precision, parsed finite decimals were IEEE floats. Keep
// that identity (including .0 and negative zero) while retaining large integers.
fn normalize_number(number: &mut serde_json::Number) {
    if number.as_str().contains(['.', 'e', 'E'])
        && let Some(value) = number.as_f64().filter(|n| n.is_finite())
        && let Some(normalized) = serde_json::Number::from_f64(value)
    {
        *number = normalized;
    }
}
fn normalize_floats(value: &mut Value) {
    match value {
        Value::Number(number) => normalize_number(number),
        Value::Array(values) => values.iter_mut().for_each(normalize_floats),
        Value::Object(values) => values.values_mut().for_each(normalize_floats),
        _ => {}
    }
}

pub fn parse_slice(raw: &[u8]) -> Result<Value, serde_json::Error> {
    parse_value(std::str::from_utf8(raw).map_err(|_| error("Invalid UTF-8"))?)
}

pub fn parse_unique_value(raw: &str) -> Result<Value, serde_json::Error> {
    parse_raw(serde_json::from_str::<&RawValue>(raw)?.get(), true, 0)
}

fn parse_raw(raw: &str, unique: bool, depth: usize) -> Result<Value, serde_json::Error> {
    if depth >= 128 {
        return Err(error("JSON nesting limit reached"));
    }
    match raw.as_bytes().first() {
        Some(b'{') => {
            struct Object {
                unique: bool,
                depth: usize,
            }
            impl<'de> Visitor<'de> for Object {
                type Value = Value;
                fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                    f.write_str("JSON object")
                }
                fn visit_map<A: MapAccess<'de>>(self, mut input: A) -> Result<Value, A::Error> {
                    let mut output = Map::new();
                    while let Some((key, raw)) = input.next_entry::<String, &'de RawValue>()? {
                        if self.unique && output.contains_key(&key) {
                            return Err(de::Error::custom("Duplicate key"));
                        }
                        let value = parse_raw(raw.get(), self.unique, self.depth + 1)
                            .map_err(de::Error::custom)?;
                        output.insert(key, value);
                    }
                    Ok(Value::Object(output))
                }
            }
            let mut decoder = serde_json::Deserializer::from_str(raw);
            let value = decoder.deserialize_map(Object { unique, depth })?;
            decoder.end()?;
            Ok(value)
        }
        Some(b'[') => {
            let values: Vec<&RawValue> = serde_json::from_str(raw)?;
            values
                .into_iter()
                .map(|v| parse_raw(v.get(), unique, depth + 1))
                .collect::<Result<Vec<_>, _>>()
                .map(Value::Array)
        }
        Some(b'"') => serde_json::from_str::<String>(raw).map(Value::String),
        Some(b'n') if raw == "null" => Ok(Value::Null),
        Some(b't') if raw == "true" => Ok(Value::Bool(true)),
        Some(b'f') if raw == "false" => Ok(Value::Bool(false)),
        _ => {
            let mut number = raw.parse::<serde_json::Number>()?;
            normalize_number(&mut number);
            Ok(Value::Number(number))
        }
    }
}

/// Las claves `keys` de un objeto JSON de nivel superior como texto crudo prestado de
/// `raw`, sin copiar ni decodificar el resto: serde valida cada valor y descarta los
/// que no se piden (una línea de transcript de varios MiB no se materializa). Una
/// clave repetida se queda con su último valor, como `json.loads`. `Err` si `raw` no
/// es un objeto o serde no lo acepta (p. ej. `NaN`, que `json.loads` sí admite): quien
/// llama puede reintentar con `workspace_loads`.
pub fn object_fields<'a>(
    raw: &'a str,
    keys: &[&str],
) -> Result<Vec<Option<&'a RawValue>>, serde_json::Error> {
    struct Fields<'k> {
        keys: &'k [&'k str],
    }
    impl<'de> Visitor<'de> for Fields<'_> {
        type Value = Vec<Option<&'de RawValue>>;
        fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("JSON object")
        }
        fn visit_map<A: MapAccess<'de>>(self, mut input: A) -> Result<Self::Value, A::Error> {
            let mut out = vec![None; self.keys.len()];
            while let Some(key) = input.next_key::<String>()? {
                match self.keys.iter().position(|k| *k == key) {
                    Some(i) => {
                        let value = input.next_value::<&'de RawValue>()?;
                        if let Some(slot) = out.get_mut(i) {
                            *slot = Some(value);
                        }
                    }
                    None => {
                        input.next_value::<de::IgnoredAny>()?;
                    }
                }
            }
            Ok(out)
        }
    }
    let mut decoder = serde_json::Deserializer::from_str(raw);
    let fields = decoder.deserialize_map(Fields { keys })?;
    decoder.end()?;
    Ok(fields)
}
