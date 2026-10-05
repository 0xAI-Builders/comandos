//! Procesos de agentes, inventario de panes de tmux, dueños por pane y cuentas
//! por pid: port de las funciones de `bin/cc-dash` que usa `read_states`
//! (`agent_procs`, `parent_pid`, `_proc_cmdline`, `_read_environ`,
//! `_process_start`, `tmux_pane_inventory`, `_tmux_process_owners`,
//! `agent_pane_maps`, el bucle de agentes externos y `account_for_pid`).
//!
//! Todas las lecturas de `/proc` van contra una raíz inyectable para probarlas
//! sobre un árbol falso. Solo Linux: el frente no tiene la rama `ps`/`lsof`.
use crate::Unsure;
use crate::tui_state::{Obs, loads_bytes, loads_text};
use comandos_core::json::truthy;
use comandos_core::text::splitlines;
use serde_json::{Map, Value};
use std::collections::{HashMap, HashSet};
use std::ffi::OsStr;
use std::fs;
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

/// Un proceso de agente vivo: `(pid, cwd, agent)` del Python.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentProc {
    pub pid: i64,
    pub cwd: String,
    pub agent: String,
}

/// Blanco de `str.split()` de Python: Unicode más U+001C–U+001F.
pub(crate) fn is_py_space(c: char) -> bool {
    c.is_whitespace() || ('\u{1c}'..='\u{1f}').contains(&c)
}

/// `str.split()` sin argumentos.
pub(crate) fn py_split(s: &str) -> impl Iterator<Item = &str> {
    s.split(is_py_space).filter(|t| !t.is_empty())
}

/// `os.path.basename` de POSIX: lo que sigue a la última `/`.
pub(crate) fn basename(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// `f"{proc}/{pid}"`.
pub(crate) fn proc_dir(proc_root: &Path, pid: &str) -> PathBuf {
    proc_root.join(pid)
}

/// Campos de `stat` que siguen al `)` del nombre (texto UTF-8 estricto, como
/// `read_text()`/`open()` en modo texto: lo que no decodifica es `ValueError`).
pub(crate) fn stat_fields(proc_root: &Path, pid: &str) -> Option<Vec<String>> {
    let text = fs::read_to_string(proc_dir(proc_root, pid).join("stat")).ok()?;
    let (_, after) = text.rsplit_once(')')?;
    Some(py_split(after).map(str::to_owned).collect())
}

/// `argv` de `cmdline` sin vacíos, con `decode("utf-8", "replace")`.
fn split_argv(raw: &[u8]) -> Vec<String> {
    raw.split(|b| *b == 0)
        .filter(|a| !a.is_empty())
        .map(|a| String::from_utf8_lossy(a).into_owned())
        .collect()
}

/// `agent_procs` (cc-dash): `<proc>/[0-9]*/cmdline` en el orden de `read_dir`;
/// el primer alias de los tres primeros argumentos fija el agente y
/// `readlink(<pid>/cwd)` el directorio (si falla, el proceso se salta).
pub fn agent_procs(
    proc_root: &Path,
    aliases: &HashMap<String, String>,
) -> Result<Vec<AgentProc>, Unsure> {
    let mut out = Vec::new();
    let Ok(entries) = fs::read_dir(proc_root) else {
        return Ok(out);
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        // `glob("[0-9]*")` y luego `int(nombre)`: solo nombres todo dígitos.
        let Some(name) = name.to_str() else {
            continue;
        };
        if name.is_empty() || !name.bytes().all(|b| b.is_ascii_digit()) {
            continue;
        }
        let dir = proc_dir(proc_root, name);
        let Ok(raw) = fs::read(dir.join("cmdline")) else {
            continue;
        };
        // `argv[:3]` de `split(b"\0")` y después se quitan los vacíos.
        let names: Vec<String> = raw
            .split(|b| *b == 0)
            .take(3)
            .filter(|a| !a.is_empty())
            .map(|a| basename(&String::from_utf8_lossy(a)).to_owned())
            .collect();
        let Some(hit) = names
            .iter()
            .find_map(|n| aliases.get(n))
            .filter(|h| !h.is_empty())
        else {
            continue;
        };
        let Ok(pid) = name.parse::<i64>() else {
            continue;
        };
        let Ok(cwd) = fs::read_link(dir.join("cwd")) else {
            continue;
        };
        // El Python lo guarda con `surrogateescape`: no se puede reproducir.
        let cwd = cwd.into_os_string().into_string().map_err(|_| Unsure)?;
        out.push(AgentProc {
            pid,
            cwd,
            agent: hit.clone(),
        });
    }
    Ok(out)
}

/// `parent_pid`: campo 1 tras `)`; cualquier fallo → 0. Se leen bytes: un
/// nombre que no es UTF-8 hace que el Python pregunte a `ps`, que da el mismo
/// padre que el campo numérico; un pid que no existe da 0 por ambos caminos.
pub fn parent_pid(proc_root: &Path, pid: i64) -> i64 {
    let Ok(raw) = fs::read(proc_dir(proc_root, &pid.to_string()).join("stat")) else {
        return 0;
    };
    let Some(close) = raw.iter().rposition(|b| *b == b')') else {
        return 0;
    };
    let after = raw.get(close + 1..).unwrap_or(&[]);
    after
        .split(|b| b.is_ascii_whitespace() || matches!(b, 0x0b | 0x1c..=0x1f))
        .filter(|t| !t.is_empty())
        .nth(1)
        .and_then(|t| std::str::from_utf8(t).ok())
        .and_then(|t| t.parse::<i64>().ok())
        .unwrap_or(0)
}

/// `_proc_cmdline`: argumentos no vacíos de `cmdline`; fallo → `[]`.
pub fn proc_cmdline(proc_root: &Path, pid: i64) -> Vec<String> {
    fs::read(proc_dir(proc_root, &pid.to_string()).join("cmdline"))
        .map(|raw| split_argv(&raw))
        .unwrap_or_default()
}

/// `_read_environ`: `dict` de `environ` (gana la última); fallo → `{}`.
pub fn read_environ(proc_root: &Path, pid: i64) -> HashMap<Vec<u8>, Vec<u8>> {
    let mut env = HashMap::new();
    let Ok(raw) = fs::read(proc_dir(proc_root, &pid.to_string()).join("environ")) else {
        return env;
    };
    for item in raw.split(|b| *b == 0) {
        if let Some(eq) = item.iter().position(|b| *b == b'=') {
            let (key, value) = item.split_at(eq);
            env.insert(key.to_vec(), value.get(1..).unwrap_or(&[]).to_vec());
        }
    }
    env
}

/// `_process_start`: campo 19 tras `)` o `""`.
pub fn process_start(proc_root: &Path, pid: i64) -> String {
    stat_fields(proc_root, &pid.to_string())
        .and_then(|f| f.into_iter().nth(19))
        .unwrap_or_default()
}

/// Una fila de `tmux_pane_inventory`.
#[derive(Debug, Clone, PartialEq)]
pub struct PaneRow {
    pub session: String,
    pub pane: String,
    pub pane_pid: i64,
    pub command: String,
    pub cwd: String,
    pub activity: f64,
    pub pane_active: bool,
}

/// Formato de `list-panes -a -F` de `tmux_pane_inventory`.
pub const PANE_FORMAT: &str = "#{session_name}|#{pane_id}|#{pane_pid}|#{pane_current_command}|#{pane_current_path}|#{session_activity}|#{pane_active}|#{window_active}";

/// `SESSION_RE = ^[A-Za-z0-9._-]{1,80}\Z`.
fn is_session(s: &str) -> bool {
    (1..=80).contains(&s.len())
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
}

/// `PANE_RE = ^%\d{1,7}\Z` (tmux solo escribe dígitos ASCII).
fn is_pane(s: &str) -> bool {
    s.strip_prefix('%')
        .is_some_and(|d| (1..=7).contains(&d.len()) && d.bytes().all(|b| b.is_ascii_digit()))
}

/// `tmux_pane_inventory` sobre la salida de `list-panes` ya leída.
pub fn parse_pane_inventory(stdout: &str) -> Vec<PaneRow> {
    let mut panes = Vec::new();
    for line in splitlines(stdout) {
        let mut values: Vec<&str> = line.splitn(8, '|').collect();
        let pid_ok = values
            .get(2)
            .is_some_and(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()));
        if values.len() < 3 || !pid_ok {
            continue;
        }
        values.resize(8, "");
        let [
            session,
            pane,
            pid,
            command,
            cwd,
            activity,
            pane_active,
            window_active,
        ] = values.as_slice()
        else {
            continue;
        };
        if !is_session(session) || !is_pane(pane) {
            continue;
        }
        let Ok(pane_pid) = pid.parse::<i64>() else {
            continue;
        };
        // `float(activity or 0)`; `ValueError` → 0.
        let activity = if activity.is_empty() {
            0.0
        } else {
            comandos_core::text::float(activity).unwrap_or(0.0)
        };
        panes.push(PaneRow {
            session: (*session).to_owned(),
            pane: (*pane).to_owned(),
            pane_pid,
            command: (*command).to_owned(),
            cwd: (*cwd).to_owned(),
            activity,
            pane_active: *pane_active == "1" && *window_active == "1",
        });
    }
    panes
}

/// `_tmux_process_owners`: pid → (índice de la fila del pane, profundidad).
/// Con dos filas del mismo `pane_pid` gana la última, como el `dict` del Python.
pub fn process_owners(
    procs: &[AgentProc],
    panes: &[PaneRow],
    parent: &mut dyn FnMut(i64) -> i64,
) -> HashMap<i64, (usize, usize)> {
    let roots: HashMap<i64, usize> = panes
        .iter()
        .enumerate()
        .map(|(i, row)| (row.pane_pid, i))
        .collect();
    let mut parents: HashMap<i64, i64> = HashMap::new();
    let mut owners = HashMap::new();
    for proc in procs {
        let mut ancestor = proc.pid;
        let mut seen = HashSet::new();
        for depth in 0..128 {
            if let Some(&row) = roots.get(&ancestor) {
                owners.insert(proc.pid, (row, depth));
                break;
            }
            if ancestor <= 1 || !seen.insert(ancestor) {
                break;
            }
            ancestor = *parents.entry(ancestor).or_insert_with(|| parent(ancestor));
        }
    }
    owners
}

/// El agente principal de un pane, tal como lo devuelve `agent_pane_maps`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentInfo {
    pub session: String,
    pub pane: String,
    pub cwd: String,
    pub agent: String,
    pub pid: i64,
}

impl AgentInfo {
    /// `{session, pane, cwd, agent, pid}` en ese orden.
    pub fn to_obs(&self) -> Obs {
        let mut obs = Map::new();
        obs.insert("session".into(), Value::from(self.session.clone()));
        obs.insert("pane".into(), Value::from(self.pane.clone()));
        obs.insert("cwd".into(), Value::from(self.cwd.clone()));
        obs.insert("agent".into(), Value::from(self.agent.clone()));
        obs.insert("pid".into(), Value::from(self.pid));
        obs
    }
}

/// `(by_session, by_cwd)` de `agent_pane_maps` con el orden de inserción de
/// sus `dict`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AgentMaps {
    pub by_session: Vec<(String, AgentInfo)>,
    pub by_cwd: Vec<(String, Vec<AgentInfo>)>,
}

/// `(session, pane)`.
type PaneKey = (String, String);

const PANE_SHELLS: [&str; 7] = ["zsh", "bash", "sh", "fish", "dash", "ksh", "ssh"];
const WRAPPERS: [&str; 5] = ["node", "nodejs", "python", "python3", "bun"];

/// `min(items, key=...)` de Python: el primero de los mínimos.
fn first_min<T, K: PartialOrd>(items: &[T], key: impl Fn(&T) -> K) -> Option<&T> {
    let mut best: Option<(&T, K)> = None;
    for item in items {
        let k = key(item);
        if best.as_ref().is_none_or(|(_, b)| k < *b) {
            best = Some((item, k));
        }
    }
    best.map(|(item, _)| item)
}

/// `agent_pane_maps`: un agente por pane (sin shells); un envoltorio
/// (`node`, `python`, `bun`…) cede ante su hijo directo nativo del mismo agente.
pub fn agent_pane_maps(
    procs: &[AgentProc],
    panes: &[PaneRow],
    owners: &HashMap<i64, (usize, usize)>,
    cmdline: &mut dyn FnMut(i64) -> Vec<String>,
    parent: &mut dyn FnMut(i64) -> i64,
) -> AgentMaps {
    let mut argv_cache: HashMap<i64, Vec<String>> = HashMap::new();
    let mut wrapper = |pid: i64| -> bool {
        let argv = argv_cache.entry(pid).or_insert_with(|| cmdline(pid));
        argv.first()
            .is_some_and(|a| WRAPPERS.contains(&basename(a)))
    };
    // (info, profundidad) por pane, en el orden en que aparece el primer proceso.
    let mut candidates: Vec<(PaneKey, Vec<(AgentInfo, usize)>)> = Vec::new();
    for proc in procs {
        let Some(&(index, depth)) = owners.get(&proc.pid) else {
            continue;
        };
        let Some(row) = panes.get(index) else {
            continue;
        };
        if PANE_SHELLS.contains(&row.command.as_str()) {
            continue;
        }
        let cwd = if proc.cwd.is_empty() {
            row.cwd.clone()
        } else {
            proc.cwd.clone()
        };
        let info = AgentInfo {
            session: row.session.clone(),
            pane: row.pane.clone(),
            cwd,
            agent: proc.agent.clone(),
            pid: proc.pid,
        };
        let key = (row.session.clone(), row.pane.clone());
        match candidates.iter_mut().find(|(k, _)| *k == key) {
            Some((_, options)) => options.push((info, depth)),
            None => candidates.push((key, vec![(info, depth)])),
        }
    }
    let mut maps = AgentMaps::default();
    for (_, options) in &candidates {
        let Some(mut selected) = first_min(options, |(i, d)| (*d, -i.pid)) else {
            continue;
        };
        if wrapper(selected.0.pid) {
            let mut native = Vec::new();
            for option in options {
                if option.0.agent == selected.0.agent
                    && option.1 == selected.1 + 1
                    && parent(option.0.pid) == selected.0.pid
                    && !wrapper(option.0.pid)
                {
                    native.push(option);
                }
            }
            if let Some(best) = first_min(&native, |o| o.0.pid) {
                selected = *best;
            }
        }
        let info = selected.0.clone();
        if !maps.by_session.iter().any(|(s, _)| *s == info.session) {
            maps.by_session.push((info.session.clone(), info.clone()));
        }
        match maps.by_cwd.iter_mut().find(|(c, _)| *c == info.cwd) {
            Some((_, rows)) => rows.push(info),
            None => maps.by_cwd.push((info.cwd.clone(), vec![info])),
        }
    }
    maps
}

/// Bucle de agentes externos de `read_states`: procesos sin pane cuyo linaje
/// llega a pid ≤ 1 sin pasar por otro proceso de agente.
pub fn external_agents(
    procs: &[AgentProc],
    owners: &HashMap<i64, (usize, usize)>,
    parent: &mut dyn FnMut(i64) -> i64,
) -> HashSet<(String, String)> {
    let agent_pids: HashSet<i64> = procs.iter().map(|p| p.pid).collect();
    let mut parent_cache: HashMap<i64, i64> = HashMap::new();
    let mut external = HashSet::new();
    for proc in procs {
        if owners.contains_key(&proc.pid) {
            continue;
        }
        let mut ancestor = proc.pid;
        let mut seen = HashSet::new();
        for _ in 0..128 {
            ancestor = *parent_cache
                .entry(ancestor)
                .or_insert_with(|| parent(ancestor));
            if ancestor <= 1 {
                external.insert((proc.cwd.clone(), proc.agent.clone()));
                break;
            }
            if agent_pids.contains(&ancestor) || !seen.insert(ancestor) {
                break;
            }
        }
    }
    external
}

// ---- Rutas con la semántica de `posixpath` (sobre bytes, como `surrogateescape`).

/// `posixpath.join(a, b)` con `b` relativo o absoluto.
fn path_join(a: &[u8], b: &[u8]) -> Vec<u8> {
    if b.starts_with(b"/") || a.is_empty() {
        return b.to_vec();
    }
    let mut out = a.to_vec();
    if !a.ends_with(b"/") {
        out.push(b'/');
    }
    out.extend_from_slice(b);
    out
}

/// `posixpath.split(p)`: cabeza sin barras finales (salvo si es todo barras).
fn path_split(p: &[u8]) -> (Vec<u8>, Vec<u8>) {
    let cut = p.iter().rposition(|b| *b == b'/').map_or(0, |i| i + 1);
    let (head, tail) = p.split_at(cut);
    let mut head = head.to_vec();
    if head.iter().any(|b| *b != b'/') {
        while head.last() == Some(&b'/') {
            head.pop();
        }
    }
    (head, tail.to_vec())
}

/// `posixpath.dirname`.
fn dirname(p: &[u8]) -> Vec<u8> {
    path_split(p).0
}

/// `p.rstrip('/')`.
fn rstrip_slash(p: &[u8]) -> &[u8] {
    let end = p.iter().rposition(|b| *b != b'/').map_or(0, |i| i + 1);
    p.get(..end).unwrap_or(&[])
}

/// `posixpath.normpath`.
fn normpath(p: &[u8]) -> Vec<u8> {
    if p.is_empty() {
        return b".".to_vec();
    }
    let initial = if p.starts_with(b"//") && !p.starts_with(b"///") {
        2
    } else {
        usize::from(p.starts_with(b"/"))
    };
    let mut comps: Vec<&[u8]> = Vec::new();
    for comp in p.split(|b| *b == b'/') {
        if comp.is_empty() || comp == b"." {
            continue;
        }
        if comp != b".." || (initial == 0 && comps.is_empty()) || comps.last() == Some(&&b".."[..])
        {
            comps.push(comp);
        } else if !comps.is_empty() {
            comps.pop();
        }
    }
    let mut out = vec![b'/'; initial];
    out.extend_from_slice(&comps.join(&b'/'));
    if out.is_empty() { b".".to_vec() } else { out }
}

fn os(p: &[u8]) -> &Path {
    Path::new(OsStr::from_bytes(p))
}

/// `posixpath._joinrealpath` (no estricto).
fn join_realpath(
    mut path: Vec<u8>,
    rest: &[u8],
    seen: &mut HashMap<Vec<u8>, Option<Vec<u8>>>,
) -> (Vec<u8>, bool) {
    let mut rest = rest.to_vec();
    if rest.starts_with(b"/") {
        rest.remove(0);
        path = b"/".to_vec();
    }
    while !rest.is_empty() {
        let (name, tail) = match rest.iter().position(|b| *b == b'/') {
            Some(i) => (
                rest.get(..i).unwrap_or(&[]).to_vec(),
                rest.get(i + 1..).unwrap_or(&[]).to_vec(),
            ),
            None => (rest.clone(), Vec::new()),
        };
        rest = tail;
        if name.is_empty() || name == b"." {
            continue;
        }
        if name == b".." {
            if path.is_empty() {
                path = b"..".to_vec();
            } else {
                let (head, last) = path_split(&path);
                path = if last == b".." {
                    path_join(&path_join(&head, b".."), b"..")
                } else {
                    head
                };
            }
            continue;
        }
        let newpath = path_join(&path, &name);
        let is_link = fs::symlink_metadata(os(&newpath)).is_ok_and(|m| m.file_type().is_symlink());
        if !is_link {
            path = newpath;
            continue;
        }
        if let Some(known) = seen.get(&newpath) {
            match known {
                Some(resolved) => {
                    path = resolved.clone();
                    continue;
                }
                None => return (path_join(&newpath, &rest), false),
            }
        }
        seen.insert(newpath.clone(), None);
        // `os.readlink` fuera del `try` del Python: un fallo aquí (carrera) es
        // una excepción allí; se trata como un camino sin resolver.
        let Ok(target) = fs::read_link(os(&newpath)) else {
            return (path_join(&newpath, &rest), false);
        };
        let (resolved, ok) = join_realpath(path, &target.into_os_string().into_vec(), seen);
        if !ok {
            return (path_join(&resolved, &rest), false);
        }
        seen.insert(newpath, Some(resolved.clone()));
        path = resolved;
    }
    (path, true)
}

/// `os.path.realpath` (no estricto) sobre bytes.
pub(crate) fn realpath(p: &[u8]) -> Vec<u8> {
    let (path, _) = join_realpath(Vec::new(), p, &mut HashMap::new());
    // `abspath`: relativo al directorio de trabajo, luego `normpath`.
    if path.starts_with(b"/") {
        normpath(&path)
    } else {
        let cwd = std::env::current_dir()
            .map(|c| c.into_os_string().into_vec())
            .unwrap_or_default();
        normpath(&path_join(&cwd, &path))
    }
}

// ---- Cuentas por pid.

/// `AGENT_ACCOUNT_ENV`: variable del directorio de configuración y su defecto.
fn account_env(agent: &str) -> Option<(&'static str, &'static str)> {
    match agent {
        "claude" => Some(("CLAUDE_CONFIG_DIR", "/.claude")),
        "codex" => Some(("CODEX_HOME", "/.codex")),
        "grok" => Some(("GROK_HOME", "/.grok")),
        _ => None,
    }
}

/// `os.stat` → `(dev, ino, mtime_ns, size)`.
type Signature = (u64, u64, i128, u64);

fn signature(path: &[u8]) -> Option<Signature> {
    let m = fs::metadata(os(path)).ok()?;
    let mtime = i128::from(m.mtime()) * 1_000_000_000 + i128::from(m.mtime_nsec());
    Some((m.dev(), m.ino(), mtime, m.size()))
}

/// `base64.urlsafe_b64decode` (alfabeto `-_` traducido a `+/` y luego
/// `binascii.a2b_base64` sin validar: lo que no es del alfabeto se ignora).
fn urlsafe_b64decode(text: &str) -> Option<Vec<u8>> {
    if !text.is_ascii() {
        return None;
    }
    let value = |c: u8| -> Option<u8> {
        match c {
            b'A'..=b'Z' => Some(c - b'A'),
            b'a'..=b'z' => Some(c - b'a' + 26),
            b'0'..=b'9' => Some(c - b'0' + 52),
            b'+' | b'-' => Some(62),
            b'/' | b'_' => Some(63),
            _ => None,
        }
    };
    let mut out = Vec::new();
    let (mut quad, mut left, mut pads) = (0u8, 0u8, 0u8);
    for c in text.bytes() {
        if c == b'=' {
            if quad >= 2 {
                pads += 1;
                if quad + pads >= 4 {
                    return Some(out);
                }
            }
            continue;
        }
        let Some(v) = value(c) else {
            continue;
        };
        pads = 0;
        match quad {
            0 => {
                quad = 1;
                left = v;
            }
            1 => {
                quad = 2;
                out.push((left << 2) | (v >> 4));
                left = v & 0x0f;
            }
            2 => {
                quad = 3;
                out.push((left << 4) | (v >> 2));
                left = v & 0x03;
            }
            _ => {
                quad = 0;
                out.push((left << 6) | v);
                left = 0;
            }
        }
    }
    (quad == 0).then_some(out)
}

/// Resultado de un `json.loads`: el valor y si se cambió un sustituto suelto.
type Loaded = Option<(Value, bool)>;

/// El valor elegido como correo; con un sustituto suelto cambiado por U+FFFD
/// que llega al correo, el Python emitiría otra cosa.
fn checked(value: Value, touched: bool) -> Result<Value, Unsure> {
    match &value {
        Value::String(s) if touched && s.contains('\u{fffd}') => Err(Unsure),
        _ => Ok(value),
    }
}

/// Extrae el correo del documento ya leído (cualquier `AttributeError` → `""`).
fn email_of(data: &Value, agent: &str, touched: bool) -> Result<Value, Unsure> {
    let empty = || Value::from("");
    let Value::Object(map) = data else {
        return Ok(empty());
    };
    match agent {
        "codex" => {
            let tokens = map.get("tokens").filter(|t| truthy(t));
            let token = match tokens {
                None => return Ok(empty()),
                Some(Value::Object(t)) => t.get("id_token").cloned().unwrap_or_else(empty),
                Some(_) => return Ok(empty()),
            };
            let Value::String(token) = token else {
                return Ok(empty());
            };
            if token.matches('.').count() < 2 {
                return Ok(empty());
            }
            let mut part = token.split('.').nth(1).unwrap_or("").to_owned();
            let pad = (4 - part.chars().count() % 4) % 4;
            part.push_str(&"=".repeat(pad));
            let Some(raw) = urlsafe_b64decode(&part) else {
                return Ok(empty());
            };
            let loaded: Loaded = loads_bytes(&raw).unwrap_or(None);
            match loaded {
                Some((Value::Object(claims), inner)) => match claims.get("email") {
                    // El sustituto de fuera no llega: `encode('ascii')` ya falla.
                    Some(v) => checked(v.clone(), inner),
                    None => Ok(empty()),
                },
                _ => Ok(empty()),
            }
        }
        "grok" => {
            let rec = map.values().find_map(|v| v.as_object());
            let Some(rec) = rec else {
                return Ok(empty());
            };
            let chosen = ["email", "user_id", "principal_id"]
                .iter()
                .find_map(|k| rec.get(*k).filter(|v| truthy(v)));
            chosen.map_or_else(|| Ok(empty()), |v| checked(v.clone(), touched))
        }
        _ => match map.get("oauthAccount").filter(|v| truthy(v)) {
            None => Ok(empty()),
            Some(Value::Object(oauth)) => oauth
                .get("emailAddress")
                .filter(|v| truthy(v))
                .map_or_else(|| Ok(empty()), |v| checked(v.clone(), touched)),
            Some(_) => Ok(empty()),
        },
    }
}

/// Correo leído y la firma del archivo del que salió.
type CachedEmail = (Result<Value, Unsure>, Option<Signature>);

/// Cachés de `account_for_pid` y `account_email_for_dir`.
#[derive(Default)]
pub struct AccountCache {
    pids: HashMap<(i64, String, String), (String, Vec<u8>)>,
    emails: HashMap<(Vec<u8>, String), CachedEmail>,
}

impl AccountCache {
    /// `account_for_pid`: el proceso fija su directorio de cuenta (caché por
    /// pid, inicio y agente); el correo se relee cuando cambia su archivo.
    /// `Unsure` si el correo lleva un sustituto suelto que el Python emitiría.
    pub fn account_for_pid(
        &mut self,
        home: &Path,
        proc_root: &Path,
        pid: i64,
        agent: &str,
    ) -> Result<Obs, Unsure> {
        let Some((envvar, default)) = account_env(agent) else {
            return Ok(Map::new());
        };
        if pid == 0 {
            return Ok(Map::new());
        }
        let key = (pid, process_start(proc_root, pid), agent.to_owned());
        let (alias, path) = match self.pids.get(&key) {
            Some(hit) => hit.clone(),
            None => {
                let env = read_environ(proc_root, pid);
                // `os.path.expanduser(default)`.
                let mut base = rstrip_slash(home.as_os_str().as_bytes()).to_vec();
                base.extend_from_slice(default.as_bytes());
                let path = match env.get(envvar.as_bytes()).filter(|v| !v.is_empty()) {
                    Some(raw) => String::from_utf8_lossy(raw).into_owned().into_bytes(),
                    None => base.clone(),
                };
                let alias = if realpath(&path) == realpath(&base) {
                    "main".to_owned()
                } else {
                    let name = String::from_utf8_lossy(rstrip_slash(&path)).into_owned();
                    match basename(&name) {
                        "" => "main".to_owned(),
                        other => other.to_owned(),
                    }
                };
                let hit = (alias, path);
                if self.pids.len() > 4096 {
                    self.pids.clear();
                }
                self.pids.insert(key, hit.clone());
                hit
            }
        };
        let email = self.email_for_dir(&path, agent)?;
        let mut obs = Map::new();
        obs.insert("account".into(), Value::from(alias));
        obs.insert("accountEmail".into(), email);
        Ok(obs)
    }

    /// `account_email_for_dir`: identidad pública solo cuando cambia su fuente.
    fn email_for_dir(&mut self, path: &[u8], agent: &str) -> Result<Value, Unsure> {
        let name: &[u8] = if matches!(agent, "codex" | "grok") {
            b"auth.json"
        } else {
            b".claude.json"
        };
        let mut cfg = path_join(path, name);
        if agent == "claude" && fs::metadata(os(&cfg)).is_err() {
            cfg = path_join(&dirname(rstrip_slash(path)), b".claude.json");
        }
        let sig = signature(&cfg);
        let key = (path.to_vec(), agent.to_owned());
        if let Some((email, cached)) = self.emails.get(&key)
            && *cached == sig
        {
            return email.clone();
        }
        let email = read_email(&cfg, agent);
        if self.emails.len() > 4096 {
            self.emails.clear();
        }
        self.emails.insert(key, (email.clone(), sig));
        email
    }
}

/// Lectura del archivo de identidad; todo lo que el `except Exception` del
/// Python atrapa (incluido `RecursionError`) da `""`.
fn read_email(cfg: &[u8], agent: &str) -> Result<Value, Unsure> {
    let Ok(raw) = fs::read(os(cfg)) else {
        return Ok(Value::from(""));
    };
    let Ok(text) = String::from_utf8(raw) else {
        return Ok(Value::from(""));
    };
    match loads_text(&text) {
        Ok(Some((data, touched))) => email_of(&data, agent, touched),
        _ => Ok(Value::from("")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn realpath_follows_python_rules() {
        assert_eq!(normpath(b"/a/./b/../c//"), b"/a/c");
        assert_eq!(normpath(b"//a"), b"//a");
        assert_eq!(normpath(b"///a/.."), b"/");
        assert_eq!(normpath(b"../x"), b"../x");
        assert_eq!(realpath(b"/nonexistent-zz/../q"), b"/q");
        assert_eq!(dirname(b"/a/b/"), b"/a/b");
        assert_eq!(dirname(b"x"), b"");
        assert_eq!(path_join(b"", b".claude.json"), b".claude.json");
    }

    #[test]
    fn b64_matches_binascii() {
        assert_eq!(
            urlsafe_b64decode("eyJh+IjoxfQ==").unwrap(),
            b"{\"a\xf8\x88\xe8\xc5\xf4"
        );
        assert_eq!(urlsafe_b64decode("YQ==").unwrap(), b"a");
        assert_eq!(urlsafe_b64decode("Y!Q==").unwrap(), b"a");
        assert!(urlsafe_b64decode("YQ=").is_none());
        assert!(urlsafe_b64decode("Y").is_none());
        assert_eq!(urlsafe_b64decode("YQ==YWJj").unwrap(), b"a");
    }

    #[test]
    fn external_agents_walk_to_init() {
        let procs = vec![
            AgentProc {
                pid: 10,
                cwd: "/a".into(),
                agent: "claude".into(),
            },
            AgentProc {
                pid: 11,
                cwd: "/a".into(),
                agent: "codex".into(),
            },
            AgentProc {
                pid: 12,
                cwd: "/b".into(),
                agent: "codex".into(),
            },
            AgentProc {
                pid: 13,
                cwd: "/c".into(),
                agent: "grok".into(),
            },
        ];
        // 10 → 5 → 1 (externo); 11 → 10 (envoltorio, no); 12 en un pane; 13 → 13 (ciclo).
        let parents: HashMap<i64, i64> = [(10, 5), (5, 1), (11, 10), (13, 13)].into();
        let owners: HashMap<i64, (usize, usize)> = [(12, (0, 0))].into();
        let mut calls = 0;
        let mut parent = |pid: i64| {
            calls += 1;
            parents.get(&pid).copied().unwrap_or(0)
        };
        let got = external_agents(&procs, &owners, &mut parent);
        assert_eq!(got, HashSet::from([("/a".to_owned(), "claude".to_owned())]));
    }
}
