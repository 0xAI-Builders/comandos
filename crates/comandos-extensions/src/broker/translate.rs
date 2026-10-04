//! Utilidades de (de)serialización JSON-RPC usadas por el multiplexor. Sin estado.
use serde_json::{Map, Value, json};

pub(crate) enum Parsed {
    Object(Map<String, Value>),
    NotJson,
    NotObject,
}

/// Una línea MCP es un único objeto JSON. Los lotes (arrays) no se admiten: MCP 2025-06-18
/// los eliminó, así que un JSON válido que no sea objeto se descarta sin respuesta.
pub(crate) fn parse(line: &[u8]) -> Parsed {
    match serde_json::from_slice::<Value>(line) {
        Ok(Value::Object(m)) => Parsed::Object(m),
        Ok(_) => Parsed::NotObject,
        Err(_) => Parsed::NotJson,
    }
}

pub(crate) fn bytes(m: Map<String, Value>) -> Vec<u8> {
    serde_json::to_vec(&Value::Object(m)).unwrap_or_default()
}

pub(crate) fn error(code: i64, message: &str) -> Value {
    json!({"code": code, "message": message})
}

fn envelope(id: Value, key: &str, body: Value) -> Vec<u8> {
    let mut m = Map::new();
    m.insert("jsonrpc".into(), "2.0".into());
    m.insert("id".into(), id);
    m.insert(key.into(), body);
    bytes(m)
}

pub(crate) fn response(id: Value, result: Value) -> Vec<u8> {
    envelope(id, "result", result)
}

pub(crate) fn error_response(id: Value, error: Value) -> Vec<u8> {
    envelope(id, "error", error)
}

pub(crate) fn cancelled(request_id: u64, reason: &str) -> Vec<u8> {
    serde_json::to_vec(&json!({"jsonrpc":"2.0","method":"notifications/cancelled",
        "params":{"requestId":request_id,"reason":reason}}))
    .unwrap_or_default()
}

/// Referencia mutable a un campo anidado (`["params","_meta","progressToken"]`).
pub(crate) fn slot<'a>(msg: &'a mut Map<String, Value>, path: &[&str]) -> Option<&'a mut Value> {
    let mut cur = msg.get_mut(*path.first()?)?;
    for k in &path[1..] {
        cur = cur.get_mut(*k)?;
    }
    Some(cur)
}
