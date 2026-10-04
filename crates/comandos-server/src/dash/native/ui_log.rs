//! F. `POST /ui-log` (8942, `ui_log_append` 64): anexa eventos de uso a
//! `~/.claude/hooks/ui-events.jsonl` bajo el `flock` del Python y rota a 30
//! días al pasar de 2 MB. Las conversiones van antes del candado (como en el
//! Python); candado tomado o rotación incierta → se declina sin escribir.
use super::{
    Answer, Entry, Fault, Key, Native, NativeRoute, Verb,
    files::{self, FileLock, Strict},
    light::data,
    py, reply,
};
use crate::{HandlerError, Request};
use comandos_core::json::{response_dumps_unicode, truthy};
use http::StatusCode;
use serde_json::{Map, Value, json};
use std::{fs, io::Write, path::Path};

pub const ROUTES: &[Entry] = &[Entry {
    verb: Verb::Post,
    key: Key::Raw("/ui-log"),
    route: NativeRoute::UiLog,
}];

const ROTATE_BYTES: u64 = 2_000_000;
const KEEP_LINES: usize = 20_000;
const KEEP_SECONDS: f64 = 30.0 * 86_400.0;

fn failure() -> Fault {
    Fault::Error(HandlerError::Failure)
}

pub async fn answer(native: &Native, request: &Request) -> Answer {
    let data = data(request)?;
    let now = (native.options().clock)() as f64 / 1000.0;
    let keep = records(data.get("events"), now)?;
    if keep.is_empty() {
        return reply(StatusCode::OK, &json!({"ok": true, "n": 0}));
    }
    let mut text = String::new();
    for record in &keep {
        text.push_str(&response_dumps_unicode(record).map_err(|_| Fault::Decline)?);
        text.push('\n');
    }
    let n = keep.len();
    let path = native.options().hooks.join("ui-events.jsonl");
    let done = tokio::task::spawn_blocking(move || append(&path, &text, now))
        .await
        .map_err(|_| failure())?;
    match done {
        Appended::Done => reply(StatusCode::OK, &json!({"ok": true, "n": n})),
        Appended::Contended | Appended::Unsure => Err(Fault::Decline),
        Appended::Failed => Err(failure()),
    }
}

/// `keep` del Python: dicts de `events[:200]`, campos en su orden.
fn records(events: Option<&Value>, now: f64) -> Result<Vec<Value>, Fault> {
    let items = match events {
        Some(v) if !truthy(v) => return Ok(Vec::new()),
        None => return Ok(Vec::new()),
        Some(Value::Array(items)) => items,
        // `str[:200]` itera caracteres; ninguno es un dict.
        Some(Value::String(_)) => return Ok(Vec::new()),
        // Objeto, número o booleano: `x[:200]` → TypeError no capturado.
        Some(_) => return Err(failure()),
    };
    let mut keep = Vec::new();
    for item in items.iter().take(200) {
        let Value::Object(e) = item else { continue };
        let mut record = Map::new();
        record.insert("ts".into(), ts(e.get("ts"), now)?);
        record.insert("k".into(), Value::String(text(e.get("k"), 24)?));
        record.insert("n".into(), Value::String(text(e.get("n"), 80)?));
        record.insert("c".into(), Value::String(text(e.get("c"), 80)?));
        record.insert("d".into(), json!(int(e.get("d"))?));
        record.insert("s".into(), Value::String(text(e.get("s"), 80)?));
        keep.push(Value::Object(record));
    }
    Ok(keep)
}

/// `float(e.get("ts") or now)`. No finito → declina (el `repr` de inf/nan
/// en el archivo no se arriesga); entero enorme → OverflowError → 500.
fn ts(value: Option<&Value>, now: f64) -> Result<Value, Fault> {
    let x = match value {
        None => now,
        Some(v) if !truthy(v) => now,
        Some(Value::Bool(_)) => 1.0,
        Some(Value::Number(n)) => {
            let raw = n.as_str();
            let x: f64 = raw.parse().map_err(|_| Fault::Decline)?;
            if !x.is_finite() && !raw.contains(['.', 'e', 'E', 'N', 'I']) {
                return Err(failure());
            }
            x
        }
        Some(Value::String(s)) => match py::float(s) {
            Ok(x) => x,
            Err(py::NumError::Invalid) => return Err(failure()),
            Err(py::NumError::Exotic) => return Err(Fault::Decline),
        },
        Some(_) => return Err(failure()),
    };
    if !x.is_finite() {
        return Err(Fault::Decline);
    }
    serde_json::Number::from_f64(x)
        .map(Value::Number)
        .ok_or(Fault::Decline)
}

/// `str(e.get(k) or "")[:n]`.
fn text(value: Option<&Value>, n: usize) -> Result<String, Fault> {
    match value {
        Some(v) if truthy(v) => py::str_scalar(v)
            .map(|s| py::take_chars(&s, n))
            .ok_or(Fault::Decline),
        _ => Ok(String::new()),
    }
}

/// `int(e.get("d") or 0)`.
fn int(value: Option<&Value>) -> Result<i64, Fault> {
    match value {
        Some(v) if truthy(v) => py::int_of(v).map_err(|c| match c {
            py::Conversion::Exotic => Fault::Decline,
            _ => failure(),
        }),
        _ => Ok(0),
    }
}

enum Appended {
    Done,
    Contended,
    Unsure,
    Failed,
}

enum Rotation {
    Skip,
    Write(String),
    Unsure,
}

/// Bajo el candado. La rotación se decide ANTES de anexar sobre lo viejo + lo
/// nuevo (nadie más escribe con el candado tomado): así una línea incierta
/// declina sin haber escrito nada.
fn append(path: &Path, text: &str, now: f64) -> Appended {
    let lock = match FileLock::try_acquire(path) {
        Ok(Some(lock)) => lock,
        Ok(None) => return Appended::Contended,
        Err(_) => return Appended::Failed,
    };
    let before = fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    let rotation = if before + text.len() as u64 > ROTATE_BYTES {
        rotated(path, text, now)
    } else {
        Rotation::Skip
    };
    if matches!(rotation, Rotation::Unsure) {
        drop(lock);
        return Appended::Unsure;
    }
    let appended = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .and_then(|mut f| f.write_all(text.as_bytes()));
    if appended.is_err() {
        return Appended::Failed;
    }
    if let Rotation::Write(fresh) = rotation {
        // `except Exception: pass` del Python.
        let _ = files::write_text_atomic(path, &fresh);
    }
    drop(lock);
    Appended::Done
}

/// Lo que haría la rotación del Python con el archivo ya anexado.
fn rotated(path: &Path, appended: &str, now: f64) -> Rotation {
    let old = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(_) => return Rotation::Skip,
    };
    // UnicodeDecodeError dentro del `try` externo: no se rota.
    let Ok(old) = String::from_utf8(old) else {
        return Rotation::Skip;
    };
    let all = format!("{old}{appended}")
        .replace("\r\n", "\n")
        .replace('\r', "\n");
    let lines: Vec<&str> = all
        .split_inclusive('\n')
        .filter(|l| !py::strip(l).is_empty())
        .collect();
    let start = lines.len().saturating_sub(KEEP_LINES);
    let cutoff = now - KEEP_SECONDS;
    let mut fresh = String::new();
    for line in lines.iter().skip(start) {
        let keep = match files::loads_strict(line) {
            Strict::Value(Value::Object(o)) => match o.get("ts") {
                None => 0.0 >= cutoff,
                Some(Value::Bool(b)) => f64::from(u8::from(*b)) >= cutoff,
                Some(Value::Number(n)) => n.as_str().parse::<f64>().is_ok_and(|x| x >= cutoff),
                // `str >= float`, `None >= float`…: TypeError → se aborta.
                Some(_) => return Rotation::Skip,
            },
            // `.get` de una lista o un escalar: AttributeError → se aborta.
            Strict::Value(_) => return Rotation::Skip,
            Strict::Missing | Strict::Unreadable => false,
            Strict::Unsure => return Rotation::Unsure,
        };
        if keep {
            fresh.push_str(line);
        }
    }
    Rotation::Write(fresh)
}
