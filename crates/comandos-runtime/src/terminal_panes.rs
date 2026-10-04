//! Callback-owned pane controls; this library never invokes tmux.
use crate::{pane_typing::TmuxResult, terminal_history::friendly_path};
use comandos_core::json::dumps;
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use std::sync::{Condvar, Mutex};
use std::thread::ThreadId;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaneError(pub String);
impl std::fmt::Display for PaneError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for PaneError {}
pub type Result<T> = std::result::Result<T, PaneError>;
pub const FOCUS_KEY_BASE: usize = 900;
pub const FOCUS_KEY_COUNT: usize = 32;
const INVENTORY_FORMAT: &str = "#{pane_id}\t#{pane_active}\t#{pane_current_command}\t#{pane_index}\t#{pane_current_path}\t#{pane_left}\t#{pane_top}\t#{pane_width}\t#{pane_height}";
const STABLE_FIELDS: [&str; 6] = [
    "socket_path",
    "pid",
    "server_start",
    "session_id",
    "pane_id",
    "pane_pid",
];
// Match Python's global RLock, including callback reentry on the owning thread.
static SERIAL: Mutex<(Option<ThreadId>, usize)> = Mutex::new((None, 0));
static READY: Condvar = Condvar::new();
struct SerialGuard;
impl SerialGuard {
    fn acquire() -> Self {
        let owner = std::thread::current().id();
        let mut state = SERIAL
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        while state.0.is_some_and(|current| current != owner) {
            state = READY
                .wait(state)
                .unwrap_or_else(std::sync::PoisonError::into_inner);
        }
        state.0 = Some(owner);
        state.1 += 1;
        Self
    }
}
impl Drop for SerialGuard {
    fn drop(&mut self) {
        let mut state = SERIAL
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.1 -= 1;
        if state.1 == 0 {
            state.0 = None;
            READY.notify_all();
        }
    }
}
fn error(message: &str) -> PaneError {
    PaneError(message.into())
}
fn digits(value: &str) -> bool {
    !value.is_empty() && value.bytes().all(|b| b.is_ascii_digit())
}
fn pane_id(value: &str) -> bool {
    value.strip_prefix('%').is_some_and(digits)
}
fn machine_number(value: &str) -> bool {
    digits(value.strip_prefix(['$', '%']).unwrap_or(value))
}
fn machine_string(value: &Value) -> Option<String> {
    match value {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) if !n.as_str().contains(['.', 'e', 'E']) => Some(n.to_string()),
        _ => None,
    }
}
fn splitlines(text: &str) -> Vec<&str> {
    let mut rows = Vec::new();
    let mut start = 0;
    let mut iter = text.char_indices().peekable();
    while let Some((at, ch)) = iter.next() {
        if matches!(
            ch,
            '\n' | '\r'
                | '\u{b}'
                | '\u{c}'
                | '\u{1c}'
                | '\u{1d}'
                | '\u{1e}'
                | '\u{85}'
                | '\u{2028}'
                | '\u{2029}'
        ) {
            rows.push(&text[start..at]);
            start = at + ch.len_utf8();
            if ch == '\r'
                && iter.peek().is_some_and(|(_, c)| *c == '\n')
                && let Some((i, c)) = iter.next()
            {
                start = i + c.len_utf8();
            }
        }
    }
    if start < text.len() {
        rows.push(&text[start..]);
    }
    rows
}
/// Version includes only stable identity fields with Python's sorted default ASCII JSON.
pub fn version(identity: &Value) -> Result<String> {
    let object = identity
        .as_object()
        .ok_or_else(|| error("No se pudo verificar la identidad del panel"))?;
    let stable: Map<String, Value> = STABLE_FIELDS
        .iter()
        .map(|key| {
            (
                (*key).into(),
                object.get(*key).cloned().unwrap_or_else(|| json!("")),
            )
        })
        .collect();
    let raw = dumps(&Value::Object(stable), true, false).map_err(PaneError)?;
    Ok(format!("{:x}", Sha256::digest(raw.as_bytes())))
}
pub fn focus_key(index: usize) -> String {
    format!("\u{1b}[4242;{index}~")
}
struct Pane {
    public: Value,
    raw: Value,
}
fn inventory(
    tmux: &mut impl FnMut(&[&str]) -> Result<TmuxResult>,
    identify: &mut impl FnMut(&str, &str) -> Result<Value>,
    session: &str,
    home: &str,
) -> Result<Vec<Pane>> {
    let target = format!("={session}:");
    let result = tmux(&["list-panes", "-t", &target, "-F", INVENTORY_FORMAT])?;
    if result.returncode != 0 {
        return Err(error("No se encuentra la sesión"));
    }
    let mut panes = Vec::new();
    for line in splitlines(&result.stdout) {
        let head = line.splitn(5, '\t').collect::<Vec<_>>();
        if head.len() != 5 || !pane_id(head[0]) || !digits(head[3]) {
            return Err(error("No se pudo leer la lista de paneles"));
        }
        let index = head[3]
            .parse::<u64>()
            .map_err(|_| error("No se pudo leer la lista de paneles"))?;
        let mut public = json!({"id":head[0],"active":head[1]=="1","title":head[2].chars().take(100).collect::<String>(),"index":index});
        let mut parts = head[4].rsplitn(5, '\t').collect::<Vec<_>>();
        parts.reverse();
        let path = if parts.len() == 5 && parts[1..].iter().all(|p| digits(p)) {
            for (key, raw) in ["left", "top", "width", "height"].iter().zip(&parts[1..]) {
                public[*key] = json!(
                    raw.parse::<u64>()
                        .map_err(|_| error("No se pudo leer la lista de paneles"))?
                );
            }
            parts[0]
        } else {
            head[4]
        };
        public["path"] = json!(
            friendly_path(path, home)
                .chars()
                .take(300)
                .collect::<String>()
        );
        let raw = identify(session, head[0])?;
        for key in ["pid", "session_id", "pane_id", "pane_pid"] {
            if !machine_string(&raw[key]).is_some_and(|s| machine_number(&s)) {
                return Err(error("No se pudo verificar la identidad del panel"));
            }
        }
        if raw["pane_id"] != head[0]
            || raw
                .get("session_name")
                .is_some_and(|value| value != session)
        {
            return Err(error("El panel no pertenece a esta sesión"));
        }
        public["identity"] = json!(version(&raw)?);
        panes.push(Pane { public, raw });
    }
    if panes.is_empty() {
        return Err(error("No hay paneles disponibles"));
    }
    Ok(panes)
}
fn public(panes: &[Pane]) -> Value {
    Value::Array(panes.iter().map(|p| p.public.clone()).collect())
}
fn ensure_focus_keys(tmux: &mut impl FnMut(&[&str]) -> Result<TmuxResult>) -> Result<()> {
    let mut args = Vec::new();
    for i in 0..FOCUS_KEY_COUNT {
        args.extend([
            "set-option".into(),
            "-s".into(),
            format!("user-keys[{}]", FOCUS_KEY_BASE + i),
            focus_key(i),
            ";".into(),
            "bind-key".into(),
            "-n".into(),
            format!("User{}", FOCUS_KEY_BASE + i),
            "select-pane".into(),
            "-t".into(),
            format!(":.{i}"),
            ";".into(),
        ]);
    }
    args.pop();
    let refs = args.iter().map(String::as_str).collect::<Vec<_>>();
    if tmux(&refs)?.returncode != 0 {
        return Err(error("No se pudo preparar el foco del dispositivo"));
    }
    Ok(())
}
fn remote_focus(
    tmux: &mut impl FnMut(&[&str]) -> Result<TmuxResult>,
    session: &str,
) -> Result<Value> {
    let target = format!("={session}");
    let result = tmux(&[
        "list-clients",
        "-t",
        &target,
        "-F",
        "#{client_flags}\t#{pane_id}",
    ])?;
    if result.returncode != 0 {
        return Ok(Value::Null);
    }
    let mut seen = std::collections::HashSet::new();
    for row in splitlines(&result.stdout) {
        if let Some((flags, pane)) = row.split_once('\t')
            && flags.split(',').any(|f| f == "active-pane")
            && pane_id(pane)
        {
            seen.insert(pane);
        }
    }
    Ok(if seen.len() == 1 {
        seen.into_iter().next().map_or(Value::Null, |p| json!(p))
    } else {
        Value::Null
    })
}
pub fn execute(
    mut tmux: impl FnMut(&[&str]) -> Result<TmuxResult>,
    mut identify: impl FnMut(&str, &str) -> Result<Value>,
    mut save_snapshot: impl FnMut(&str, &str) -> Result<Value>,
    data: &Value,
    home: &str,
) -> Result<Value> {
    let data = data
        .as_object()
        .ok_or_else(|| error("Solicitud inválida"))?;
    let session = data
        .get("session")
        .and_then(Value::as_str)
        .filter(|s| {
            !s.is_empty()
                && s.len() <= 120
                && s.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
        })
        .ok_or_else(|| error("Sesión inválida"))?;
    let action = match data.get("action") {
        None => "list",
        Some(value) => value
            .as_str()
            .ok_or_else(|| error("Acción de panel inválida"))?,
    };
    if !["list", "select", "close", "split", "resize"].contains(&action) {
        return Err(error("Acción de panel inválida"));
    }
    let axis = if action == "resize" {
        let axis = match data.get("axis").and_then(Value::as_str) {
            Some("x") => "-x",
            Some("y") => "-y",
            _ => return Err(error("Tamaño de panel inválido")),
        };
        let size = data
            .get("size")
            .and_then(Value::as_u64)
            .filter(|n| (2..=1000).contains(n))
            .ok_or_else(|| error("Tamaño de panel inválido"))?;
        Some((axis, size.to_string()))
    } else {
        None
    };
    let split_flag = if action == "split" {
        Some(match data.get("direction").and_then(Value::as_str) {
            Some("right") => "-h",
            Some("down") => "-v",
            _ => return Err(error("Dirección de split inválida")),
        })
    } else {
        None
    };
    let _guard = SerialGuard::acquire();
    let panes = inventory(&mut tmux, &mut identify, session, home)?;
    if action == "list" {
        return Ok(
            json!({"ok":true,"panes":public(&panes),"remoteFocus":remote_focus(&mut tmux,session)?}),
        );
    }
    let pane = panes
        .iter()
        .find(|p| data.get("pane") == Some(&p.public["id"]));
    if let Some((axis, size)) = axis {
        let pane = pane.ok_or_else(|| error("Ese panel ya no existe"))?;
        let id = pane.public["id"]
            .as_str()
            .ok_or_else(|| error("No se pudo leer la lista de paneles"))?;
        if tmux(&["resize-pane", "-t", id, axis, &size])?.returncode != 0 {
            return Err(error("tmux no pudo cambiar el tamaño del panel"));
        }
        return Ok(
            json!({"ok":true,"panes":public(&inventory(&mut tmux,&mut identify,session,home)?)}),
        );
    }
    let pane = pane
        .filter(|p| data.get("identity") == Some(&p.public["identity"]))
        .ok_or_else(|| error("El panel cambió. Abre Paneles y vuelve a elegirlo"))?;
    if action == "close" && panes.len() <= 1 {
        return Err(error("El último panel permanece abierto"));
    }
    let id = pane.public["id"]
        .as_str()
        .ok_or_else(|| error("No se pudo leer la lista de paneles"))?;
    let snapshot = if action == "close" {
        let saved = save_snapshot(session, id)?;
        let fresh = inventory(&mut tmux, &mut identify, session, home)?;
        if fresh.len() <= 1
            || !fresh.iter().any(|p| {
                p.public["id"] == pane.public["id"]
                    && p.public["identity"] == pane.public["identity"]
            })
        {
            return Err(error(
                "El panel cambió mientras se guardaba. No se ha cerrado",
            ));
        }
        Some(saved)
    } else {
        None
    };
    if action == "select" && data.get("scope") == Some(&json!("client")) {
        let index = pane.public["index"]
            .as_u64()
            .filter(|i| *i < FOCUS_KEY_COUNT as u64)
            .ok_or_else(|| error("Este panel no admite foco por dispositivo"))?;
        ensure_focus_keys(&mut tmux)?;
        return Ok(
            json!({"ok":true,"panes":public(&panes),"clientKeys":focus_key(index as usize)}),
        );
    }
    let mut checks = Vec::new();
    for key in ["pid", "session_id", "pane_id", "pane_pid"] {
        let value = machine_string(&pane.raw[key])
            .filter(|s| machine_number(s))
            .ok_or_else(|| error("No se pudo verificar la identidad del panel"))?;
        checks.push(format!("#{{==:#{{{key}}},{value}}}"));
    }
    if action == "close" {
        checks.push("#{>:#{window_panes},1}".into());
    }
    let mut condition = checks
        .pop()
        .ok_or_else(|| error("No se pudo verificar la identidad del panel"))?;
    for check in checks {
        condition = format!("#{{&&:{check},{condition}}}");
    }
    let command = if let Some(flag) = split_flag {
        format!("split-window {flag} -t {id} -c \"#{{pane_current_path}}\"")
    } else {
        format!(
            "{} -t {id}",
            if action == "close" {
                "kill-pane"
            } else {
                "select-pane"
            }
        )
    };
    let result = tmux(&[
        "if-shell",
        "-F",
        "-t",
        id,
        &condition,
        &command,
        "display-message -p COMANDOS_PANE_CHANGED",
    ])?;
    if result.returncode != 0 || result.stdout.contains("COMANDOS_PANE_CHANGED") {
        return Err(error("El panel cambió. La acción no se ha aplicado"));
    }
    let after = inventory(&mut tmux, &mut identify, session, home)?;
    let mut response = json!({"ok":true,"panes":public(&after)});
    if action == "split" {
        response["opened"] = after
            .iter()
            .find(|p| {
                !panes
                    .iter()
                    .any(|before| before.public["id"] == p.public["id"])
            })
            .map_or(Value::Null, |p| p.public["id"].clone());
    }
    if let Some(snapshot) = snapshot {
        response["closed"] = json!(id);
        response["snapshot"] = snapshot;
    }
    Ok(response)
}
