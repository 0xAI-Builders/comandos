//! `valid_snapshot` y `remap_layout` de `lib/tmux_snapshot.py`, para leer
//! `app-sessions-v2.json` exactamente como el Python. Lo que dependería de
//! reglas de Python difíciles de reproducir (bool como int, `\d` Unicode) es
//! `Exotic`: quien llama declina.
use serde_json::Value;
use std::collections::HashSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Snapshot {
    Valid,
    Invalid,
    Exotic,
}

fn digit_run(b: &[u8], from: usize) -> usize {
    let mut end = from;
    while end < b.len() && b[end].is_ascii_digit() {
        end += 1;
    }
    end
}

/// Una hoja `(\d+x\d+,\d+,\d+),(\d+)(?=[,}\]]|$)` que empieza en `i`:
/// `(inicio del grupo 2, fin)`. Cada `\d+` va seguido de un no-dígito literal,
/// así que el retroceso nunca da otra coincidencia.
fn leaf_at(b: &[u8], i: usize) -> Option<(usize, usize)> {
    let mut at = i;
    for sep in [b'x', b',', b',', b','] {
        let end = digit_run(b, at);
        if end == at || b.get(end) != Some(&sep) {
            return None;
        }
        at = end + 1;
    }
    let end = digit_run(b, at);
    if end == at {
        return None;
    }
    let lookahead = match b.get(end) {
        None => true,
        Some(b',' | b'}' | b']') => true,
        // `$` sin MULTILINE también casa antes de un `\n` final.
        Some(b'\n') => end + 1 == b.len(),
        Some(_) => false,
    };
    lookahead.then_some((at, end))
}

/// Los grupos 2 de `_LEAF.finditer(layout)` (solo ASCII).
pub fn leaf_ids(layout: &str) -> Vec<&str> {
    let b = layout.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        match leaf_at(b, i) {
            Some((start, end)) => {
                out.push(&layout[start..end]);
                i = end;
            }
            None => i += 1,
        }
    }
    out
}

/// El checksum de tmux que recalcula `remap_layout`.
pub fn layout_checksum(body: &str) -> u16 {
    let mut sum: u32 = 0;
    for c in body.chars() {
        sum = (sum >> 1) | ((sum & 1) << 15);
        sum = (sum + c as u32) & 0xffff;
    }
    sum as u16
}

fn is_int_text(raw: &str) -> bool {
    !raw.contains(['.', 'e', 'E']) && !matches!(raw, "NaN" | "Infinity" | "-Infinity")
}

/// `isinstance(v, int) and v >= 1`, o exótico si es bool.
fn positive_int(v: Option<&Value>) -> Result<bool, Snapshot> {
    match v {
        Some(Value::Bool(_)) => Err(Snapshot::Exotic),
        Some(Value::Number(n)) if is_int_text(n.as_str()) => {
            Ok(!n.as_str().starts_with('-') && n.as_str() != "0")
        }
        None => Err(Snapshot::Invalid),
        Some(_) => Ok(false),
    }
}

fn active_count(items: &[Value]) -> Option<usize> {
    let mut n = 0;
    for item in items {
        // `w['active']` sobre un no-dict o sin la clave: excepción → False.
        let active = item.as_object()?.get("active")?;
        if crate::json::truthy(active) {
            n += 1;
        }
    }
    Some(n)
}

#[derive(PartialEq, Eq, Hash)]
enum IndexKey {
    Int(String),
    Str(String),
    Null,
}

fn check_window(window: &Value) -> Result<(), Snapshot> {
    use Snapshot::{Exotic, Invalid};
    let window = window.as_object().ok_or(Invalid)?;
    let panes = match window.get("panes") {
        Some(Value::Array(p)) if !p.is_empty() => p,
        _ => return Err(Invalid),
    };
    if active_count(panes) != Some(1) {
        return Err(Invalid);
    }
    for key in ["width", "height"] {
        if !positive_int(window.get(key))? {
            return Err(Invalid);
        }
    }
    match window.get("index") {
        Some(Value::Number(n)) if is_int_text(n.as_str()) => {}
        Some(Value::Bool(_)) => return Err(Exotic),
        _ => return Err(Invalid),
    }
    if !matches!(window.get("name"), Some(Value::String(_))) {
        return Err(Invalid);
    }
    let mut ids = HashSet::new();
    for pane in panes {
        match pane.get("id") {
            Some(Value::String(id)) => {
                ids.insert(id.clone());
            }
            // Ids no-str nunca igualan a las hojas: el Python acaba en False.
            _ => return Err(Invalid),
        }
    }
    let Some(Value::String(layout)) = window.get("layout") else {
        return Err(Invalid);
    };
    if !layout.is_ascii() {
        return Err(Exotic);
    }
    let leaves: Vec<String> = leaf_ids(layout)
        .into_iter()
        .map(|d| format!("%{d}"))
        .collect();
    let leaf_set: HashSet<String> = leaves.iter().cloned().collect();
    if ids.len() != panes.len() || leaves.len() != panes.len() || leaf_set != ids {
        return Err(Invalid);
    }
    // remap_layout con la identidad: el cuerpo no cambia; solo el checksum.
    let Some((_, body)) = layout.split_once(',') else {
        return Err(Invalid);
    };
    if format!("{:04x},{body}", layout_checksum(body)) != *layout {
        return Err(Invalid);
    }
    Ok(())
}

fn check_session(session: &Value) -> Result<(), Snapshot> {
    use Snapshot::{Exotic, Invalid};
    let windows = match session.as_object().and_then(|s| s.get("windows")) {
        Some(Value::Array(w)) if !w.is_empty() => w,
        _ => return Err(Invalid),
    };
    if active_count(windows) != Some(1) {
        return Err(Invalid);
    }
    let mut indexes = HashSet::new();
    for window in windows {
        let key = match window.get("index") {
            Some(Value::Number(n)) if is_int_text(n.as_str()) => {
                IndexKey::Int(if n.as_str() == "-0" {
                    "0".into()
                } else {
                    n.as_str().to_owned()
                })
            }
            Some(Value::String(s)) => IndexKey::Str(s.clone()),
            Some(Value::Null) => IndexKey::Null,
            // bool y float colisionan con enteros en un set de Python.
            Some(Value::Bool(_) | Value::Number(_)) => return Err(Exotic),
            _ => return Err(Invalid),
        };
        indexes.insert(key);
    }
    if indexes.len() != windows.len() {
        return Err(Invalid);
    }
    windows.iter().try_for_each(check_window)
}

pub fn check_snapshot(data: &Value) -> Snapshot {
    let Some(obj) = data.as_object() else {
        return Snapshot::Invalid;
    };
    let version_two = match obj.get("version") {
        Some(Value::Number(n)) => n.as_f64() == Some(2.0),
        _ => false,
    };
    let Some(Value::Object(sessions)) = obj.get("sessions") else {
        return Snapshot::Invalid;
    };
    if !version_two {
        return Snapshot::Invalid;
    }
    match sessions.values().try_for_each(check_session) {
        Ok(()) => Snapshot::Valid,
        Err(result) => result,
    }
}
