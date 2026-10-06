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
    let home = PathBuf::from(OsStr::from_bytes(&env_bytes("HOME")));
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64();
    save_data(&home, &data, now)
}
fn save_data(home: &std::path::Path, data: &Value, now: f64) -> Option<()> {
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
    let hooks = home.join(".claude/hooks");
    let path = hooks.join("agy-quota.json");
    let mut out = Map::new();
    out.insert(
        "plan_tier".into(),
        Value::String(short(data.get("plan_tier"))),
    );
    out.insert("quota".into(), Value::Object(buckets));
    let access = comandos_store::domains::caller::CallerAccess::open(home, "quota-docs").ok()?;
    let mut lock = path.as_os_str().to_owned();
    lock.push(".lock");
    let transform = |old: Option<&[u8]>| -> comandos_store::Result<Option<Vec<u8>>> {
        if let Some(old) = old.and_then(py::json_load)
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
                    Some(_) => return Ok(None),
                };
                if now - captured < 60.0 {
                    return Ok(None); // agy repinta seguido: no reescribir lo mismo
                }
            }
        }
        out.insert("captured_at".into(), json!(now as i64));
        let mut text = String::new();
        py::dump(&Value::Object(out), true, (", ", ": "), &mut text);
        Ok(Some(text.into_bytes()))
    };
    let legacy_write = |bytes: &[u8]| -> comandos_store::Result<()> {
        std::fs::create_dir_all(&hooks)?;
        let mut tmp_name = path.clone().into_os_string();
        tmp_name.push(format!(".{}.tmp", std::process::id()));
        let tmp = PathBuf::from(tmp_name);
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&tmp)?;
        file.write_all(bytes)?;
        drop(file);
        std::fs::rename(&tmp, &path)?;
        Ok(())
    };
    if access.mode() == comandos_store::unified::Mode::Legacy {
        let old = access.read_document("hooks/agy-quota.json", &path).ok()?;
        if let Some(bytes) = transform(old.as_deref()).ok()? {
            legacy_write(&bytes).ok()?;
        }
        Some(())
    } else {
        access
            .update_document(
                "hooks/agy-quota.json",
                "quota-docs",
                &path,
                std::path::Path::new(&lock),
                (now * 1000.0) as i64,
                transform,
                legacy_write,
            )
            .ok()
            .map(|_| ())
    }
}

#[cfg(test)]
mod domain_tests {
    use super::*;
    use comandos_store::unified::{self, Mode};
    use std::os::unix::fs::DirBuilderExt;
    #[test]
    fn quota_document_uses_authority_and_preserves_exact_bytes_every_mode() {
        let home = std::env::temp_dir().join(format!("agy-quota-domain-{}", std::process::id()));
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&home)
            .unwrap();
        let db = unified::open_unified(&unified::unified_path(&home)).unwrap();
        let path = home.join(".claude/hooks/agy-quota.json");
        for (i, mode) in [Mode::Legacy, Mode::Mirror, Mode::Unified, Mode::Sealed]
            .into_iter()
            .enumerate()
        {
            unified::set_mode(&db, "quota-docs", mode, "test", 1).unwrap();
            if mode == Mode::Sealed {
                std::fs::remove_file(&path).unwrap();
            }
            let now = 1000.0 + i as f64 * 100.0;
            let data = json!({"plan_tier":"PRO","quota":{"fast":{"remaining_fraction":0.5,"reset_time":"tomorrow"}}});
            assert!(save_data(&home, &data, now).is_some());
            let expected=format!("{{\"plan_tier\": \"PRO\", \"quota\": {{\"fast\": {{\"remaining_fraction\": 0.5, \"reset_time\": \"tomorrow\"}}}}, \"captured_at\": {}}}",now as i64).into_bytes();
            if mode != Mode::Legacy {
                assert_eq!(
                    unified::doc_get(&db, "hooks/agy-quota.json")
                        .unwrap()
                        .unwrap()
                        .body,
                    expected
                );
            }
            assert_eq!(path.exists(), mode != Mode::Sealed);
            if mode != Mode::Sealed {
                assert_eq!(std::fs::read(&path).unwrap(), expected);
            }
            let before = unified::doc_get(&db, "hooks/agy-quota.json")
                .unwrap()
                .map(|d| d.revision);
            assert!(save_data(&home, &data, now + 10.0).is_some());
            assert_eq!(
                unified::doc_get(&db, "hooks/agy-quota.json")
                    .unwrap()
                    .map(|d| d.revision),
                before
            );
        }
        std::fs::remove_dir_all(home).unwrap();
    }
}
