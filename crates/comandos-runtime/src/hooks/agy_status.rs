//! `comandos hook agy-status`: transcripción de `adapters/agy-statusline.py`, la
//! barra de estado de agy que solo guarda su cuota. Del JSON de stdin toma
//! `quota` (cubetas con `remaining_fraction` y `reset_time`) y `plan_tier`, y los
//! deja en `~/.claude/hooks/agy-quota.json` (nada de correo, rutas ni modelo). No
//! imprime nada y nunca falla: agy no debe ver errores.
use super::input::env_bytes;
use super::py;
use serde_json::{Map, Value, json};
use std::ffi::OsStr;
use std::io::{Read, Write};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::OpenOptionsExt;
use std::path::PathBuf;

pub fn run(_args: &[String]) -> i32 {
    let _ = save();
    0
}

/// `str(x or "")[:40]`.
fn short(value: Option<&Value>) -> String {
    match value {
        Some(v) if py::truthy(v) => py::take(&py::str_of(v), 40),
        _ => String::new(),
    }
}

/// `main()`; `None` corta como el `return` (o la excepción que se traga el script).
fn save() -> Option<()> {
    let mut raw = Vec::new();
    std::io::stdin().read_to_end(&mut raw).ok()?;
    let data = if raw.is_empty() {
        Value::Null
    } else {
        py::json_load(&raw)?
    };
    let quota = data.get("quota")?.as_object()?;
    let mut buckets = Map::new();
    for (name, bucket) in quota {
        let Some(bucket) = bucket.as_object() else {
            continue;
        };
        let fraction = match bucket.get("remaining_fraction") {
            Some(Value::Bool(b)) => f64::from(u8::from(*b)),
            Some(Value::Number(n)) => {
                let f = py::as_f64(n);
                // `float(int)` desborda con enteros enormes: la excepción aborta.
                if f.is_infinite() && !py::is_float(n) {
                    return None;
                }
                f
            }
            _ => continue,
        };
        buckets.insert(
            py::take(name, 40),
            json!({
                "remaining_fraction": py::float_value(fraction),
                "reset_time": short(bucket.get("reset_time")),
            }),
        );
    }
    if buckets.is_empty() {
        return None;
    }
    let hooks = PathBuf::from(OsStr::from_bytes(&env_bytes("HOME"))).join(".claude/hooks");
    let path = hooks.join("agy-quota.json");
    let mut out = Map::new();
    out.insert(
        "plan_tier".into(),
        Value::String(short(data.get("plan_tier"))),
    );
    out.insert("quota".into(), Value::Object(buckets));
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64();
    if let Some(old) = std::fs::read(&path).ok().and_then(|b| py::json_load(&b))
        && let Some(old) = old.as_object()
    {
        let same = out
            .iter()
            .all(|(k, v)| py::eq(old.get(k).unwrap_or(&Value::Null), v));
        if same {
            let captured = match old.get("captured_at") {
                None => 0.0,
                Some(Value::Bool(b)) => f64::from(u8::from(*b)),
                Some(Value::Number(n)) => py::as_f64(n),
                // `time.time() - None`: TypeError, que el script se traga sin escribir.
                Some(_) => return None,
            };
            if now - captured < 60.0 {
                return None; // agy repinta seguido: no reescribir lo mismo
            }
        }
    }
    out.insert("captured_at".into(), json!(now as i64));
    std::fs::create_dir_all(&hooks).ok()?;
    let mut text = String::new();
    py::dump(&Value::Object(out), true, (", ", ": "), &mut text);
    let mut tmp_name = path.clone().into_os_string();
    tmp_name.push(format!(".{}.tmp", std::process::id()));
    let tmp = PathBuf::from(tmp_name);
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&tmp)
        .ok()?;
    file.write_all(text.as_bytes()).ok()?;
    drop(file);
    std::fs::rename(&tmp, &path).ok()
}
