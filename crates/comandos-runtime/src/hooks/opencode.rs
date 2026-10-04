//! `comandos hook opencode`: lo que hacía el plugin `adapters/opencode-comandos.js`,
//! que ahora solo reenvía `{directory, event, session}` por stdin (la sesión la
//! consulta con el SDK de OpenCode, que solo existe dentro del plugin). Registra el
//! proceso de OpenCode (el padre de este proceso) en `native-processes/<pid>.json`
//! y entrega `working|waiting|done` al pipeline de `hook claude`, lo que antes
//! hacía el plugin con un POST a `/event` de cc-dash.
use super::adapter::{arg, notify};
use super::input::{clock, env_bytes};
use super::state_file::mktemp;
use serde_json::{Map, Value, json};
use std::ffi::OsStr;
use std::io::{Read, Write};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::DirBuilderExt;
use std::path::{Path, PathBuf};

/// Verdad de JavaScript sobre un valor de JSON.
fn js_truthy(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_f64().is_some_and(|f| f != 0.0 && !f.is_nan()),
        Some(Value::String(s)) => !s.is_empty(),
        Some(_) => true,
    }
}

/// `a || b` de JavaScript.
fn js_or<'a>(a: Option<&'a Value>, b: Option<&'a Value>) -> Option<&'a Value> {
    if js_truthy(a) { a } else { b }
}

/// `String(x)` de JavaScript para un valor de JSON.
fn js_string(value: Option<&Value>) -> String {
    match value {
        None => "undefined".into(),
        Some(Value::Null) => "null".into(),
        Some(Value::Bool(b)) => b.to_string(),
        Some(Value::String(s)) => s.clone(),
        Some(Value::Number(n)) => js_number(n.as_f64().unwrap_or(f64::NAN)),
        Some(Value::Array(items)) => items
            .iter()
            .map(|v| {
                if v.is_null() {
                    String::new()
                } else {
                    js_string(Some(v))
                }
            })
            .collect::<Vec<_>>()
            .join(","),
        Some(Value::Object(_)) => "[object Object]".into(),
    }
}

/// `Number.prototype.toString()`.
fn js_number(f: f64) -> String {
    if f.is_nan() {
        return "NaN".into();
    }
    if f.is_infinite() {
        return if f > 0.0 { "Infinity" } else { "-Infinity" }.into();
    }
    if f == 0.0 {
        return "0".into();
    }
    let sci = format!("{:e}", f.abs());
    let (mantissa, exp) = sci.split_once('e').unwrap_or((&sci, "0"));
    let digits: String = mantissa.chars().filter(char::is_ascii_digit).collect();
    let n = exp.parse::<i32>().unwrap_or(0) + 1;
    let k = digits.len() as i32;
    let sign = if f < 0.0 { "-" } else { "" };
    let body = if k <= n && n <= 21 {
        format!("{digits}{}", "0".repeat((n - k) as usize))
    } else if 0 < n && n <= 21 {
        format!("{}.{}", &digits[..n as usize], &digits[n as usize..])
    } else if -6 < n && n <= 0 {
        format!("0.{}{digits}", "0".repeat((-n) as usize))
    } else {
        let frac = if k > 1 {
            format!(".{}", &digits[1..])
        } else {
            String::new()
        };
        format!(
            "{}{frac}e{}{}",
            &digits[..1],
            if n > 0 { '+' } else { '-' },
            (n - 1).abs()
        )
    };
    format!("{sign}{body}")
}

/// `/^[A-Za-z0-9_-]{1,256}$/.test(String(x || ''))`.
fn session_id_ok(id: Option<&Value>) -> bool {
    let text = if js_truthy(id) {
        js_string(id)
    } else {
        String::new()
    };
    (1..=256).contains(&text.chars().count())
        && text
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

struct Hook {
    home: PathBuf,
    directory: Option<Value>,
    session: Option<Value>,
}

impl Hook {
    /// `persist`: registro del proceso, solo para la sesión raíz que existe.
    fn persist(&self, session_id: Option<&Value>, patch: Map<String, Value>) -> Option<()> {
        let pid = std::os::unix::process::parent_id();
        let start = start_tick(pid).unwrap_or_default();
        if start.is_empty() || !session_id_ok(session_id) {
            return None;
        }
        let id = session_id?;
        let session = self.session.as_ref().filter(|s| js_truthy(Some(s)))?;
        if session.get("id") != Some(id) || js_truthy(session.get("parentID")) {
            return None;
        }
        let root = self.home.join(".claude/hooks/native-processes");
        let path = root.join(format!("{pid}.json"));
        // El plugin guardaba el registro vigente en memoria; aquí se relee el suyo.
        let mut current = std::fs::read(&path)
            .ok()
            .and_then(|b| serde_json::from_slice::<Map<String, Value>>(&b).ok())
            .filter(|c| {
                c.get("sessionId") == Some(id)
                    && c.get("pid") == Some(&json!(pid))
                    && c.get("start") == Some(&json!(start))
            })
            .unwrap_or_default();
        for (key, value) in [
            ("pid", json!(pid)),
            ("start", json!(start)),
            ("harness", json!("opencode")),
            ("sessionId", id.clone()),
            ("parentId", json!("")),
            ("updatedAt", json!(clock().1)),
        ] {
            current.insert(key.into(), value);
        }
        current.extend(patch);
        mkdir_all(&root, 0o700)?;
        let name = format!("{pid}.json.");
        let (temp, mut file) = mktemp(&root, name.as_bytes(), 10, ".tmp")?;
        file.write_all(serde_json::to_string(&current).ok()?.as_bytes())
            .ok()?;
        drop(file);
        std::fs::rename(&temp, &path).ok()
    }

    /// El POST a `/event` de cc-dash, que validaba y lanzaba `cc-notify.sh --agent`.
    fn post(&self, event: &str) {
        let Some(Value::String(cwd)) = &self.directory else {
            return;
        };
        let cwd: String = cwd.chars().take(300).collect();
        if !cwd.starts_with('/') {
            return;
        }
        notify(&[
            arg("--agent"),
            arg("opencode"),
            arg("--event"),
            arg(event),
            arg("--cwd"),
            cwd.into_bytes(),
        ]);
    }
}

/// El tick de arranque como lo saca el plugin: tras el PRIMER `)` de `stat`.
fn start_tick(pid: u32) -> Option<String> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let rest = stat.split_once(')')?.1;
    rest.split_whitespace().nth(19).map(str::to_string)
}

/// `mkdir(..., { recursive: true, mode })`: el modo va en cada directorio creado.
fn mkdir_all(dir: &Path, mode: u32) -> Option<()> {
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(mode)
        .create(dir)
        .ok()
}

pub fn run(_args: &[String]) -> i32 {
    if std::env::var_os("COMANDOS_SILENT_AGENT").is_some_and(|v| v == "1") {
        return 0;
    }
    let mut raw = Vec::new();
    let _ = std::io::stdin().read_to_end(&mut raw);
    let Ok(Value::Object(mut input)) = serde_json::from_slice::<Value>(&raw) else {
        return 0;
    };
    let event = input.remove("event").unwrap_or(Value::Null);
    let hook = Hook {
        home: PathBuf::from(OsStr::from_bytes(&env_bytes("HOME"))),
        directory: input.remove("directory"),
        session: input.remove("session"),
    };
    let empty = Value::Object(Map::new());
    let properties = event.get("properties");
    let p = if js_truthy(properties) {
        properties.unwrap_or(&empty)
    } else {
        &empty
    };
    let info = p.get("info");
    let session_id = js_or(p.get("sessionID"), info.and_then(|i| i.get("sessionID")));
    let mut patch = Map::new();
    match event.get("type").and_then(Value::as_str) {
        Some("session.idle") => {
            patch.insert("busy".into(), json!(false));
            let _ = hook.persist(session_id, patch);
            hook.post("done");
        }
        Some("permission.asked" | "permission.updated" | "session.error") => {
            patch.insert("busy".into(), json!(true));
            let _ = hook.persist(session_id, patch);
            hook.post("waiting");
        }
        Some("message.updated") => {
            let info = info.filter(|i| js_truthy(Some(i))).unwrap_or(&empty);
            let (model_id, provider_id) = (info.get("modelID"), info.get("providerID"));
            if js_truthy(model_id) && js_truthy(provider_id) {
                let model = format!("{}/{}", js_string(provider_id), js_string(model_id));
                patch.insert("model".into(), json!(model));
            }
            if info.get("role").and_then(Value::as_str) == Some("user") {
                patch.insert("busy".into(), json!(true));
                hook.post("working");
            }
            let _ = hook.persist(session_id, patch);
        }
        _ => {}
    }
    0
}
