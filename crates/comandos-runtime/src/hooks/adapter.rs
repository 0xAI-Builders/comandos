//! Piezas comunes de los adaptadores de la Tarea 11 (codex, gemini, agy, opencode):
//! la lectura `jq -r` de sus payloads, el `$PWD` de bash, la clave de estado que
//! calculan los scripts de codex y la entrega a `comandos hook claude` en proceso
//! (los scripts hacían `exec`/`&` de `~/.claude/hooks/cc-notify.sh`).
use super::input::env_bytes;
use super::jq::jq_dump;
use super::text::{any_line, basename, head_c, jq_lossy, strip_nl, tr};
use super::which;
use serde_json::Value;
use std::ffi::OsStr;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Error de jq: descarta la salida de esa entrada.
pub type Jq<T> = Result<T, ()>;

/// Los valores JSON que jq lee de un flujo: todos hasta el primer error de sintaxis
/// (jq imprime lo de los anteriores y después falla).
pub fn jq_values(raw: &[u8]) -> Vec<Value> {
    let text = jq_lossy(raw);
    let mut out = Vec::new();
    for value in serde_json::Deserializer::from_str(&text).into_iter::<Value>() {
        match value {
            Ok(v) => out.push(v),
            Err(_) => break,
        }
    }
    out
}

/// `$(jq -r PROGRAMA)` sobre el flujo: cada salida con su `\n`, sin los finales.
/// Una entrada cuyo programa falla no imprime nada (jq sigue con la siguiente).
pub fn jq_get(values: &[Value], program: impl Fn(&Value) -> Jq<Option<Value>>) -> Vec<u8> {
    let mut out = Vec::new();
    for value in values {
        if let Ok(Some(result)) = program(value) {
            match &result {
                Value::String(s) => out.extend_from_slice(s.as_bytes()),
                other => {
                    let mut text = String::new();
                    jq_dump(other, Some(2), &mut text);
                    out.extend_from_slice(text.as_bytes());
                }
            }
            out.push(b'\n');
        }
    }
    strip_nl(&out).to_vec()
}

/// `.campo` de jq: `null` si falta, error si la entrada no es objeto ni `null`.
pub fn get<'a>(value: &'a Value, key: &str) -> Jq<&'a Value> {
    super::transcript::field(value, key)
}

/// Verdad de jq: todo menos `null` y `false`.
pub fn truthy(value: &Value) -> bool {
    !matches!(value, Value::Null | Value::Bool(false))
}

/// `.a // .b // ... // RESPALDO`: el primero verdadero (los errores no se perdonan).
pub fn first_of(value: &Value, keys: &[&str], fallback: Option<&str>) -> Jq<Option<Value>> {
    for key in keys {
        let found = get(value, key)?;
        if truthy(found) {
            return Ok(Some(found.clone()));
        }
    }
    Ok(fallback.map(|f| Value::String(f.into())))
}

/// `... | strings`: solo si el resultado es una cadena.
pub fn strings(result: Jq<Option<Value>>) -> Jq<Option<Value>> {
    Ok(result?.filter(Value::is_string))
}

/// `$PWD` de bash: el heredado si apunta al directorio actual; si no, `getcwd`.
pub fn pwd() -> Vec<u8> {
    let current = std::env::current_dir().unwrap_or_default();
    let inherited = PathBuf::from(OsStr::from_bytes(&env_bytes("PWD")));
    if inherited.is_absolute() && inherited.canonicalize().ok() == current.canonicalize().ok() {
        inherited.into_os_string().into_encoded_bytes()
    } else {
        current.into_os_string().into_encoded_bytes()
    }
}

/// `$(tmux display-message -p -t PANE '#S' 2>/dev/null || true)`.
fn tmux_session(tmux: &Path, pane: &[u8]) -> Vec<u8> {
    Command::new(tmux)
        .arg("display-message")
        .arg("-p")
        .arg("-t")
        .arg(OsStr::from_bytes(pane))
        .arg("#S")
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .map(|o| strip_nl(&o.stdout).to_vec())
        .unwrap_or_default()
}

/// `codex_state_file` de los scripts de codex: la misma clave que `cc-notify.sh`
/// con una sola consulta a tmux (`#S`).
pub fn codex_state_file(cwd: &[u8]) -> PathBuf {
    let proj = basename(cwd);
    let proj_file: Vec<u8> = proj
        .iter()
        .map(|&b| {
            if b.is_ascii_alphanumeric() || b"._-".contains(&b) {
                b
            } else {
                b'-'
            }
        })
        .collect();
    let mut key = head_c(&proj_file, 80).to_vec();
    let pane = env_bytes("TMUX_PANE");
    let pane_ok = |l: &[u8]| l.len() > 1 && l[0] == b'%' && l[1..].iter().all(u8::is_ascii_digit);
    if any_line(&pane, pane_ok)
        && let Some(tmux) = which("tmux")
    {
        let mut sess = tmux_session(&tmux, &pane);
        let sess_ok = |l: &[u8]| {
            (1..=80).contains(&l.len())
                && l.iter()
                    .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(b))
        };
        if !any_line(&sess, sess_ok) {
            sess = head_c(&tr(&tr(&proj, b'.', b'-'), b':', b'-'), 60).to_vec();
        }
        if !sess.is_empty() {
            key.extend_from_slice(b"--");
            key.extend_from_slice(&sess);
            key.extend_from_slice(b"--");
            key.extend_from_slice(pane.strip_prefix(b"%").unwrap_or(&pane));
        }
    }
    key.extend_from_slice(b".json");
    PathBuf::from(OsStr::from_bytes(&env_bytes("HOME")))
        .join(".claude/hooks/state")
        .join(OsStr::from_bytes(&key))
}

/// `tonumber` de jq 1.6.
fn tonumber(value: &Value) -> Jq<f64> {
    match value {
        Value::Number(n) => n.as_f64().ok_or(()),
        Value::String(s) => s.trim_start().parse::<f64>().map_err(|_| ()),
        _ => Err(()),
    }
}

/// El `jq -e` de los scripts de codex: el estado es un `done` de codex de hace
/// 15 s o menos. Cualquier error de jq cuenta como "no".
pub fn recent_codex_done(state: &Path, now: i64) -> bool {
    let Ok(raw) = std::fs::read(state) else {
        return false;
    };
    let text = jq_lossy(&raw);
    let mut last = None;
    for value in serde_json::Deserializer::from_str(&text).into_iter::<Value>() {
        let Ok(value) = value else { return false };
        let check = || -> Jq<bool> {
            // `(.campo // "") == "valor"`.
            let is = |key: &str, want: &str| -> Jq<bool> {
                let v = get(&value, key)?;
                Ok(truthy(v) && v.as_str() == Some(want))
            };
            if !is("agent", "codex")? || !is("status", "done")? {
                return Ok(false);
            }
            let ts = get(&value, "ts")?;
            let ts = if truthy(ts) { tonumber(ts)? } else { 0.0 };
            Ok(now as f64 - ts <= 15.0)
        };
        match check() {
            Ok(result) => last = Some(result),
            Err(()) => return false,
        }
    }
    last.unwrap_or(false)
}

/// `exec ~/.claude/hooks/cc-notify.sh ARGS`: el mismo pipeline, en proceso.
pub fn notify(args: &[Vec<u8>]) -> i32 {
    let args: Vec<String> = args.iter().map(|a| jq_lossy(a)).collect();
    super::claude::run(&args)
}

pub fn arg(text: &str) -> Vec<u8> {
    text.as_bytes().to_vec()
}

/// `/proc/PID/stat`: el padre (campo 4) y el tick de arranque (campo 22), tras el
/// último `)` como hace `rsplit(")", 1)` en Python.
pub fn proc_stat(pid: u32) -> Option<(u32, String)> {
    #[cfg(target_os = "macos")]
    {
        use crate::procs::ProcSource;
        let p = crate::procs::system().process(i32::try_from(pid).ok()?)?;
        return Some((u32::try_from(p.ppid).ok()?, p.start.to_string()));
    }
    #[cfg(not(target_os = "macos"))]
    {
        let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
        let rest = stat.rsplit_once(')')?.1;
        let fields: Vec<&str> = rest.split_whitespace().collect();
        Some((fields.get(1)?.parse().ok()?, (*fields.get(19)?).to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn stream_and_raw_output_like_jq() {
        let values = jq_values(br#"{"a":1} {"a":"x\n"} garbage {"a":3}"#);
        assert_eq!(values.len(), 2);
        assert_eq!(jq_get(&values, |v| Ok(Some(get(v, "a")?.clone()))), b"1\nx");
        assert_eq!(
            jq_get(&[json!("s")], |v| Ok(Some(get(v, "a")?.clone()))),
            b""
        );
        assert_eq!(
            jq_get(&[json!({"b":{"c":[1]}})], |v| Ok(Some(
                get(v, "b")?.clone()
            ))),
            b"{\n  \"c\": [\n    1\n  ]\n}"
        );
        assert_eq!(
            first_of(&json!({"a":false,"b":0}), &["a", "b"], Some("")),
            Ok(Some(json!(0)))
        );
        assert_eq!(strings(Ok(Some(json!(5)))), Ok(None));
    }
}
