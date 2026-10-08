//! Cliente ACP (Agent Client Protocol) de `lib/acp.py`, la parte cliente que
//! usan los Resúmenes de noticias (plan 2f-4, Tarea 3): `agent_specs`,
//! `build_command`, `open_session` (`AcpSession` y `AgyStreamSession`),
//! `new_session`, `prompt` y `close`.
//!
//! Es síncrono: corre en el hilo del trabajo de agente (nunca en el runtime de
//! tokio). El agente es un proceso hijo con `stdin`/`stdout` en JSON-RPC por
//! líneas; dos hilos leen sus salidas y un tercero (el vigía) lo mata si una
//! escritura o una espera pasan de su plazo.
//!
//! Lo que se escribe es el `json.dumps` del Python (orden de inserción,
//! `ensure_ascii`, separadores `", "`/`": "`): el agente recibe los mismos
//! bytes que del cliente Python. Los textos de error son los `str(exc)` del
//! Python en los casos que se pueden reproducir; en respuestas malformadas del
//! agente se aproximan (solo acaban en el texto de un fallo).
//!
//! Endurecimiento deliberado frente al Python:
//! - `session/request_permission` NUNCA elige una opción que permite: se elige
//!   la de rechazo (`deny_agent_tools`) y, si no hay ninguna, se responde
//!   `cancelled` (el Python caería a la primera opción `allow`).
//! - `fs/read_text_file` y `fs/write_text_file` del agente se rechazan como
//!   no soportados (el Python leería o escribiría el archivo que pida).
//! - Cerrar mata el GRUPO de procesos del agente (grupo propio al lanzarlo):
//!   no quedan nietos huérfanos. Una sesión que se suelta sin cerrar se cierra
//!   (`Drop`), y `new_session` fallida también cierra el proceso.
use comandos_core::json::{float_repr, python_eq, response_dumps, truthy};
use comandos_core::text::strip;
use nix::{
    sys::signal::{Signal, killpg},
    unistd::Pid,
};
use serde_json::{Map, Value, json};
use std::{
    borrow::Cow,
    ffi::{OsStr, OsString},
    fmt,
    io::{BufRead, BufReader, Read, Write},
    os::unix::process::CommandExt,
    path::{Path, PathBuf},
    process::{Child, ChildStdin, Command, Stdio},
    sync::{
        Arc, Condvar, Mutex, MutexGuard,
        mpsc::{self, Receiver, RecvTimeoutError},
    },
    time::{Duration, Instant},
};

/// `_USER_BIN_DIRS` de `lib/acp.py` (incluye `~/.grok/bin`, que el de
/// `lib/providers.py` no tiene).
const USER_BIN_DIRS: [&str; 8] = [
    "~/.local/bin",
    "~/.bun/bin",
    "~/.cargo/bin",
    "~/.npm-global/bin",
    "~/.opencode/bin",
    "~/.grok/bin",
    "~/bin",
    "/usr/local/bin",
];

/// Líneas de stderr que se guardan (`del self.stderr[:-200]`).
const STDERR_KEEP: usize = 200;
/// Tope de una línea del agente (2 MiB sin contar el salto). Una línea de
/// `stdout` mayor es fin de flujo (la sesión falla y se cierra); una de
/// `stderr` se recorta y se sigue leyendo. El Python no tiene tope: un agente
/// que no para de escribir sin salto de línea agotaría la memoria del
/// tablero. Lo que se usa de una respuesta tiene su propio tope de 1 MiB
/// (`news_agents::MAX_REPLY`); 2 MiB deja sitio a un trozo así entero.
pub const MAX_LINE: usize = 2 * 1024 * 1024;
/// Lo que se guarda de cada línea de stderr (solo acaba en textos de error).
const STDERR_LINE_KEEP: usize = 4096;
/// Presupuesto de líneas de `stdout` leídas y aún no terminadas de atender:
/// el lector espera (y con él el agente, por la tubería llena) hasta que la
/// sesión las suelte. Cada línea cuesta lo que puede ocupar ya convertida a
/// `Value` (con números de precisión arbitraria y mapas con orden, `[1,1,…]`
/// ocupa unas 56 veces su tamaño) más un fijo: una línea de 2 MiB llena el
/// presupuesto y no convive con otra.
const LINES_BUDGET: usize = 128 * 1024 * 1024;
const LINE_COST: usize = 64;
const LINE_OVERHEAD: usize = 256;
/// Líneas en la cola a lo sumo (además del presupuesto en bytes).
const LINES_QUEUE: usize = 256;
/// Paso de la espera de una línea (`queue.get(timeout=0.25)`).
const POLL: Duration = Duration::from_millis(250);
/// Margen del vigía sobre el plazo de una espera: solo desbloquea una
/// escritura colgada (el plazo normal lo vence la propia espera).
const WATCH_GRACE: Duration = Duration::from_secs(2);
/// `proc.wait(timeout=3)` de `close`.
const CLOSE_WAIT: Duration = Duration::from_secs(3);

/// `AcpError` (o la excepción del Python que se reproduce): su `str(exc)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcpError(pub String);

impl fmt::Display for AcpError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for AcpError {}

fn fail<T>(text: impl Into<String>) -> Result<T, AcpError> {
    Err(AcpError(text.into()))
}

// ---------------------------------------------------------------- str() del Python

/// `type(x).__name__` de lo que entrega `json.loads`.
pub fn py_type_name(value: &Value) -> &'static str {
    match value {
        Value::Null => "NoneType",
        Value::Bool(_) => "bool",
        Value::Number(n) if is_int_number(n) => "int",
        Value::Number(_) => "float",
        Value::String(_) => "str",
        Value::Array(_) => "list",
        Value::Object(_) => "dict",
    }
}

fn is_int_number(n: &serde_json::Number) -> bool {
    n.is_i64() || n.is_u64() || !n.to_string().contains(['.', 'e', 'E'])
}

/// `str(x)` del Python sobre un valor JSON.
pub fn py_str(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        _ => py_repr(value),
    }
}

/// `str(x or "")`.
pub fn py_str_or_empty(value: &Value) -> String {
    if truthy(value) {
        py_str(value)
    } else {
        String::new()
    }
}

/// `repr(x)` del Python sobre un valor JSON (aproximado en los caracteres no
/// imprimibles fuera de ASCII).
pub fn py_repr(value: &Value) -> String {
    match value {
        Value::Null => "None".into(),
        Value::Bool(true) => "True".into(),
        Value::Bool(false) => "False".into(),
        Value::Number(n) if is_int_number(n) => int_text(&n.to_string()),
        Value::Number(n) => float_repr(n.as_f64().unwrap_or(f64::NAN)),
        Value::String(s) => repr_str(s),
        Value::Array(items) => {
            let inner: Vec<String> = items.iter().map(py_repr).collect();
            format!("[{}]", inner.join(", "))
        }
        Value::Object(map) => {
            let inner: Vec<String> = map
                .iter()
                .map(|(k, v)| format!("{}: {}", repr_str(k), py_repr(v)))
                .collect();
            format!("{{{}}}", inner.join(", "))
        }
    }
}

/// `str(int(texto))`: sin ceros ni signo sobrantes.
fn int_text(raw: &str) -> String {
    let (negative, digits) = match raw.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, raw),
    };
    let digits = digits.trim_start_matches('0');
    if digits.is_empty() {
        "0".into()
    } else if negative {
        format!("-{digits}")
    } else {
        digits.to_owned()
    }
}

/// `repr(str)`: comillas simples salvo que haya `'` y no `"`.
fn repr_str(s: &str) -> String {
    let quote = if s.contains('\'') && !s.contains('"') {
        '"'
    } else {
        '\''
    };
    let mut out = String::with_capacity(s.len() + 2);
    out.push(quote);
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c == quote => {
                out.push('\\');
                out.push(c);
            }
            c if c.is_control() || (c.is_whitespace() && c != ' ') => {
                let code = u32::from(c);
                if code < 0x100 {
                    out.push_str(&format!("\\x{code:02x}"));
                } else if code < 0x10000 {
                    out.push_str(&format!("\\u{code:04x}"));
                } else {
                    out.push_str(&format!("\\U{code:08x}"));
                }
            }
            c => out.push(c),
        }
    }
    out.push(quote);
    out
}

/// `for x in value` del Python sobre un valor JSON: lista, claves de un
/// `dict`, letras de un `str`; lo demás es `TypeError`.
pub fn py_iter(value: &Value) -> Result<Vec<Value>, AcpError> {
    Ok(py_iter_lazy(value)?.map(Cow::into_owned).collect())
}

/// `py_iter` sin copiar: los elementos de una lista se prestan; las claves
/// de un `dict` y las letras de un `str` se crean de una en una.
fn py_iter_lazy(value: &Value) -> Result<Box<dyn Iterator<Item = Cow<'_, Value>> + '_>, AcpError> {
    match value {
        Value::Array(items) => Ok(Box::new(items.iter().map(Cow::Borrowed))),
        Value::Object(map) => Ok(Box::new(
            map.keys().map(|k| Cow::Owned(Value::String(k.clone()))),
        )),
        Value::String(s) => Ok(Box::new(
            s.chars().map(|c| Cow::Owned(Value::String(c.into()))),
        )),
        other => fail(format!("'{}' object is not iterable", py_type_name(other))),
    }
}

/// `x.get(...)` sobre algo que no es `dict`: `AttributeError`.
fn as_dict(value: &Value) -> Result<&Map<String, Value>, AcpError> {
    value.as_object().ok_or_else(|| no_get(value))
}

fn no_get(value: &Value) -> AcpError {
    AcpError(format!(
        "'{}' object has no attribute 'get'",
        py_type_name(value)
    ))
}

/// `x or {}` y después `.get`: lo falso es un `dict` vacío.
fn dict_or_empty(value: Option<&Value>) -> Result<Map<String, Value>, AcpError> {
    match value {
        Some(v) if truthy(v) => as_dict(v).cloned(),
        _ => Ok(Map::new()),
    }
}

fn get<'a>(map: &'a Map<String, Value>, key: &str) -> &'a Value {
    map.get(key).unwrap_or(&Value::Null)
}

/// `map.get(key) or default`.
fn or(map: &Map<String, Value>, key: &str, default: Value) -> Value {
    match map.get(key) {
        Some(v) if truthy(v) => v.clone(),
        _ => default,
    }
}

/// `map.get(key) or default`, prestado.
fn or_ref<'a>(map: &'a Map<String, Value>, key: &str, default: &'a Value) -> &'a Value {
    match map.get(key) {
        Some(v) if truthy(v) => v,
        _ => default,
    }
}

/// `map.get(key) or default` sacando el valor del mapa: lo que llega del
/// agente se mueve, no se copia.
fn take(map: &mut Map<String, Value>, key: &str, default: Value) -> Value {
    match map.remove(key) {
        Some(v) if truthy(&v) => v,
        _ => default,
    }
}

/// `as_dict` de un valor propio: el mapa mismo, sin copiarlo.
fn into_dict(value: Value) -> Result<Map<String, Value>, AcpError> {
    match value {
        Value::Object(map) => Ok(map),
        other => Err(no_get(&other)),
    }
}

/// `dict_or_empty` de un valor propio.
fn take_dict(value: Option<Value>) -> Result<Map<String, Value>, AcpError> {
    match value {
        Some(v) if truthy(&v) => into_dict(v),
        _ => Ok(Map::new()),
    }
}

/// `dict_or_empty` solo por su excepción, sin copiar.
fn check_dict(value: Option<&Value>) -> Result<(), AcpError> {
    match value {
        Some(v) if truthy(v) => as_dict(v).map(|_| ()),
        _ => Ok(()),
    }
}

/// Un objeto JSON con sus valores movidos (`json!` los copiaría) y las
/// claves en orden.
fn object<const N: usize>(entries: [(&str, Value); N]) -> Value {
    Value::Object(
        entries
            .into_iter()
            .map(|(k, v)| (k.to_owned(), v))
            .collect(),
    )
}

/// La respuesta JSON-RPC a una petición del agente.
fn reply(id: Value, outcome: Result<Value, Value>) -> Value {
    let (key, value) = match outcome {
        Ok(result) => ("result", result),
        Err(error) => ("error", error),
    };
    object([("jsonrpc", json!("2.0")), ("id", id), (key, value)])
}

// ---------------------------------------------------------------- agentes

/// Un agente del registro (`acpAgents.<id>`): el objeto tal cual.
pub type AgentSpec = Value;

/// `agent_specs(registry)`: `dict(registry.get("acpAgents") or {})`.
pub fn agent_specs(registry: &Value) -> Map<String, AgentSpec> {
    registry
        .get("acpAgents")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default()
}

/// `os.path.expanduser` sin base de usuarios: `~` y `~/…` → `home`.
fn expanduser(raw: &str, home: &Path) -> String {
    match raw.strip_prefix('~') {
        Some("") => home.display().to_string(),
        Some(rest) if rest.starts_with('/') => format!("{}{rest}", home.display()),
        _ => raw.to_owned(),
    }
}

/// `which(name)` de `lib/acp.py`: `shutil.which` sobre el `PATH` dado y
/// después los directorios de binarios de usuario.
pub fn which(name: &str, search_path: Option<&OsStr>, home: &Path) -> Option<PathBuf> {
    if name.is_empty() {
        return None;
    }
    if let Some(hit) = crate::providers::which_path(name, search_path) {
        return Some(hit);
    }
    USER_BIN_DIRS.iter().find_map(|dir| {
        let candidate = Path::new(&expanduser(dir, home)).join(name);
        let regular = std::fs::metadata(&candidate).is_ok_and(|m| m.is_file());
        (regular && nix::unistd::access(&candidate, nix::unistd::AccessFlags::X_OK).is_ok())
            .then_some(candidate)
    })
}

/// Cómo se lanza el agente.
#[derive(Debug, Clone, Default)]
pub struct OpenOptions {
    /// `model=` de `open_session` (`""` = el del agente).
    pub model: String,
    /// `extra_env` (p. ej. `COMANDOS_SILENT_AGENT=1`).
    pub extra_env: Vec<(String, String)>,
    /// `PATH` de `shutil.which` (el del frente al arrancar).
    pub search_path: Option<OsString>,
    /// `HOME` de `expanduser`.
    pub home: PathBuf,
    /// Entorno base del hijo (`os.environ`). `None`: el del proceso; `Some`:
    /// solo ese (pruebas confinadas).
    pub base_env: Option<Vec<(OsString, OsString)>>,
}

/// `argv` (con `argv[0]` resuelto) y las variables que se añaden al entorno.
pub type BuiltCommand = (Vec<String>, Vec<(String, String)>);

/// `build_command(spec, model=…)` (sin esfuerzo ni modo peligroso: los
/// Resúmenes nunca los piden): `argv` (con `argv[0]` resuelto) y entorno.
pub fn build_command(spec: &AgentSpec, opts: &OpenOptions) -> Result<BuiltCommand, AcpError> {
    let spec = as_dict(spec)?;
    let template: Vec<String> = py_iter(&or(spec, "command", json!([])))?
        .iter()
        .map(py_str)
        .collect();
    if template.is_empty() {
        return fail("agente sin comando");
    }
    let model = opts.model.as_str();
    let model_args: Vec<String> = if model.is_empty() {
        Vec::new()
    } else {
        py_iter(&or(spec, "modelArgs", json!([])))?
            .iter()
            .map(|a| py_str(a).replace("{model}", model))
            .collect()
    };
    // Los grupos van donde el template ponga su marcador; sin marcador, al
    // final. Esfuerzo y modo peligroso: siempre vacíos.
    let mut argv = Vec::new();
    let mut placed_model = false;
    for token in template {
        match token.as_str() {
            "{model_args}" => {
                argv.extend(model_args.iter().cloned());
                placed_model = true;
            }
            "{effort_args}" | "{danger_args}" => {}
            _ => argv.push(token),
        }
    }
    if !placed_model {
        argv.extend(model_args);
    }
    let Some(first) = argv.first_mut() else {
        return fail("list index out of range");
    };
    let search = opts.search_path.as_deref();
    let Some(resolved) = which(first, search, &opts.home) else {
        return fail(format!("{first} no está instalado"));
    };
    *first = resolved.display().to_string();
    let mut env: Vec<(String, String)> = Vec::new();
    let mut set = |key: String, value: String| {
        env.retain(|(k, _)| *k != key);
        env.push((key, value));
    };
    for (key, value) in &dict_or_empty(spec.get("env"))? {
        let mut value = py_str(value);
        if let Some(name) = value.strip_prefix("which:") {
            match which(name, search, &opts.home) {
                Some(hit) => value = hit.display().to_string(),
                None => continue,
            }
        }
        set(key.clone(), expanduser(&value, &opts.home));
    }
    if !model.is_empty() && truthy(get(spec, "modelEnv")) {
        let template = py_str(&or(spec, "modelEnvTemplate", json!("{model}")));
        set(
            py_str(get(spec, "modelEnv")),
            template.replace("{model}", model),
        );
    }
    for (key, value) in &opts.extra_env {
        set(key.clone(), value.clone());
    }
    Ok((argv, env))
}

// ---------------------------------------------------------------- sesión

/// Eventos normalizados que emite una sesión (los `dict` del Python).
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    Text(Value),
    Thought(Value),
    Tool {
        title: Value,
        status: Value,
        kind: Value,
        id: Value,
    },
    Plan(Value),
    Permission {
        title: Value,
        decision: Option<Value>,
    },
    End(Value),
}

/// Lo que los Resúmenes necesitan de una sesión abierta (`prompt`, `close`).
pub trait AgentSession: Send {
    fn prompt(
        &mut self,
        text: &str,
        on_event: &mut dyn FnMut(&Event),
        timeout: Duration,
    ) -> Result<Value, AcpError>;
    fn close(&mut self);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Transport {
    Acp,
    /// `AgyStreamSession` (Antigravity, stream-json).
    Agy,
}

/// Una línea de `stdout` ya leída (JSON y su coste en el presupuesto) o fin.
enum Line {
    Json(Value, Ticket),
    Eof,
}

/// El coste de una línea en el presupuesto: vuelve al soltarse, cuando la
/// sesión ya atendió la línea (no al sacarla de la cola), así no se lee la
/// siguiente mientras la anterior sigue en memoria.
struct Ticket {
    flow: Arc<Flow>,
    cost: usize,
}

impl Drop for Ticket {
    fn drop(&mut self) {
        self.flow.release(self.cost);
    }
}

/// El presupuesto en bytes de la cola de líneas (`LINES_BUDGET`).
struct Flow {
    state: Mutex<FlowState>,
    room: Condvar,
}

struct FlowState {
    used: usize,
    closed: bool,
}

impl Flow {
    fn new() -> Flow {
        Flow {
            state: Mutex::new(FlowState {
                used: 0,
                closed: false,
            }),
            room: Condvar::new(),
        }
    }

    fn lock(&self) -> MutexGuard<'_, FlowState> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Espera sitio para `cost` (una línea sola siempre cabe con la cola
    /// vacía). `false` si la sesión se cerró.
    fn acquire(&self, cost: usize) -> bool {
        let mut state = self.lock();
        while !state.closed && state.used > 0 && state.used.saturating_add(cost) > LINES_BUDGET {
            state = self.room.wait(state).unwrap_or_else(|e| e.into_inner());
        }
        if state.closed {
            return false;
        }
        state.used = state.used.saturating_add(cost);
        true
    }

    fn release(&self, cost: usize) {
        let mut state = self.lock();
        state.used = state.used.saturating_sub(cost);
        drop(state);
        self.room.notify_all();
    }

    fn close(&self) {
        self.lock().closed = true;
        self.room.notify_all();
    }
}

/// El vigía: mata el grupo del agente si vence el plazo armado. Mata solo
/// mientras el hijo no se ha recogido (`done`), así el grupo sigue siendo
/// suyo (el líder sin recoger reserva el identificador).
struct Watch {
    state: Mutex<WatchState>,
    wake: Condvar,
}

struct WatchState {
    deadline: Option<Instant>,
    done: bool,
}

impl Watch {
    fn lock(&self) -> MutexGuard<'_, WatchState> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn arm(&self, deadline: Option<Instant>) {
        self.lock().deadline = deadline;
        self.wake.notify_all();
    }

    fn run(&self, group: Pid) {
        let mut state = self.lock();
        loop {
            if state.done {
                return;
            }
            match state.deadline {
                None => {
                    state = self.wake.wait(state).unwrap_or_else(|e| e.into_inner());
                }
                Some(deadline) => {
                    let now = Instant::now();
                    if now >= deadline {
                        let _ = killpg(group, Signal::SIGKILL);
                        state.deadline = None;
                    } else {
                        state = self
                            .wake
                            .wait_timeout(state, deadline - now)
                            .map(|(guard, _)| guard)
                            .unwrap_or_else(|e| e.into_inner().0);
                    }
                }
            }
        }
    }
}

/// Una conversación con un agente ACP (o `agy`) por stdio.
pub struct Session {
    child: Child,
    group: Pid,
    stdin: Option<ChildStdin>,
    lines: Receiver<Line>,
    stderr: Arc<Mutex<Vec<String>>>,
    flow: Arc<Flow>,
    watch: Arc<Watch>,
    /// Plazo de las escrituras de la llamada en curso y su texto de fallo.
    limit: Option<(Instant, String)>,
    next_id: i64,
    session_id: Value,
    cwd: PathBuf,
    transport: Transport,
    closed: bool,
}

/// `str()` de un `OSError` del Python: `[Errno N] texto: 'ruta'`.
fn os_error(error: &std::io::Error, path: &str) -> AcpError {
    match error.raw_os_error() {
        Some(code) => {
            let text = std::io::Error::from_raw_os_error(code).to_string();
            let text = text.split(" (os error").next().unwrap_or(&text).to_owned();
            AcpError(format!("[Errno {code}] {text}: {}", repr_str(path)))
        }
        None => AcpError(error.to_string()),
    }
}

/// Bytes de una línea → los trozos que ve `for line in proc.stdout` del
/// Python en modo texto (`\r` también separa) o `None` si no es UTF-8 (el
/// `UnicodeDecodeError` que termina la lectura).
fn universal_lines(raw: &[u8]) -> Option<Vec<String>> {
    let text = std::str::from_utf8(raw).ok()?;
    Some(text.split(['\n', '\r']).map(str::to_owned).collect())
}

fn pump_stdout(
    stdout: impl Read,
    lines: mpsc::SyncSender<Line>,
    stderr: Arc<Mutex<Vec<String>>>,
    flow: Arc<Flow>,
) {
    let mut reader = BufReader::new(stdout);
    let mut raw = Vec::new();
    loop {
        raw.clear();
        // `MAX_LINE` bytes y el salto caben; un byte más, no.
        match (&mut reader)
            .take(MAX_LINE as u64 + 2)
            .read_until(b'\n', &mut raw)
        {
            Ok(0) | Err(_) => break,
            Ok(_) => {}
        }
        let content = raw.len() - usize::from(raw.last() == Some(&b'\n'));
        if content > MAX_LINE {
            // Fin de flujo: la sesión falla («cerró stdout») y quien la usa
            // la cierra; el resto de la línea no se lee.
            let text = format!("línea de más de {} MiB en stdout", MAX_LINE >> 20);
            push_stderr(&stderr, &text);
            break;
        }
        let Some(pieces) = universal_lines(&raw) else {
            break;
        };
        for piece in pieces {
            let line = strip(&piece);
            if line.is_empty() {
                continue;
            }
            let cost = line
                .len()
                .saturating_mul(LINE_COST)
                .saturating_add(LINE_OVERHEAD);
            if !flow.acquire(cost) {
                return;
            }
            let ticket = Ticket {
                flow: Arc::clone(&flow),
                cost,
            };
            match comandos_store::news::py_loads(line) {
                Ok(Some(value)) => {
                    if lines.send(Line::Json(value, ticket)).is_err() {
                        return;
                    }
                }
                _ => {
                    drop(ticket);
                    push_stderr(&stderr, line);
                }
            }
        }
    }
    let _ = lines.send(Line::Eof);
}

fn pump_stderr(stream: impl Read, stderr: Arc<Mutex<Vec<String>>>) {
    let mut reader = BufReader::new(stream);
    let mut kept: Vec<u8> = Vec::with_capacity(STDERR_LINE_KEEP);
    loop {
        // Una línea: se guardan sus primeros `STDERR_LINE_KEEP` bytes y el
        // resto se descarta sin copiarlo; nunca se deja de leer (el agente no
        // se queda bloqueado escribiendo en stderr).
        kept.clear();
        let mut seen = false;
        let mut ended = false;
        loop {
            let buf = match reader.fill_buf() {
                Ok(buf) => buf,
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(_) => return,
            };
            if buf.is_empty() {
                ended = true;
                break;
            }
            seen = true;
            let newline = buf.iter().position(|b| *b == b'\n');
            let part = buf.get(..newline.unwrap_or(buf.len())).unwrap_or_default();
            let room = STDERR_LINE_KEEP.saturating_sub(kept.len());
            kept.extend_from_slice(part.get(..room.min(part.len())).unwrap_or_default());
            let used = part.len() + usize::from(newline.is_some());
            reader.consume(used);
            if newline.is_some() {
                break;
            }
        }
        if seen {
            let text = String::from_utf8_lossy(&kept);
            push_stderr(&stderr, text.trim_end_matches('\r'));
        }
        if ended {
            return;
        }
    }
}

/// Guarda una línea de stderr recortada a `STDERR_LINE_KEEP` bytes (en un
/// límite de carácter, sin copiar el resto) y solo las 200 últimas.
fn push_stderr(stderr: &Mutex<Vec<String>>, line: &str) {
    let mut cut = line.len().min(STDERR_LINE_KEEP);
    while !line.is_char_boundary(cut) {
        cut -= 1;
    }
    let line = line.get(..cut).unwrap_or_default().to_owned();
    let mut lines = stderr.lock().unwrap_or_else(|e| e.into_inner());
    lines.push(line);
    let extra = lines.len().saturating_sub(STDERR_KEEP);
    lines.drain(..extra);
}

/// Plazo de una escritura fuera de una llamada (no ocurre en el uso actual).
const WRITE_WAIT: Duration = Duration::from_secs(30);
/// Tope de cualquier plazo (un `timeout` infinito o enorme se recorta aquí).
const MAX_WAIT: Duration = Duration::from_secs(24 * 60 * 60);

/// El plazo de `timeout` segundos desde ahora, sin pánico: negativo o `NaN`
/// → ya vencido; infinito o enorme → `MAX_WAIT`.
fn deadline_after(timeout: f64) -> Instant {
    let wait = Duration::try_from_secs_f64(timeout.max(0.0))
        .unwrap_or(MAX_WAIT)
        .min(MAX_WAIT);
    let now = Instant::now();
    now.checked_add(wait).unwrap_or(now)
}

/// Por qué no se pudo escribir.
enum WriteFail {
    /// Venció el plazo con la tubería llena.
    Late,
    /// El agente cerró su `stdin` (o falló la escritura).
    Closed,
}

/// `O_NONBLOCK` en el extremo de escritura de `stdin` (solo es nuestro).
fn set_nonblocking(fd: &impl std::os::fd::AsFd) {
    use nix::fcntl::{FcntlArg, OFlag, fcntl};
    if let Ok(bits) = fcntl(fd, FcntlArg::F_GETFL) {
        let flags = OFlag::from_bits_truncate(bits) | OFlag::O_NONBLOCK;
        let _ = fcntl(fd, FcntlArg::F_SETFL(flags));
    }
}

/// Escribe todo antes de `deadline`, esperando con `poll` mientras la
/// tubería esté llena.
fn write_until(stdin: &mut ChildStdin, bytes: &[u8], deadline: Instant) -> Result<(), WriteFail> {
    use nix::poll::{PollFd, PollFlags, PollTimeout, poll};
    use std::os::fd::AsFd;
    let mut rest = bytes;
    while !rest.is_empty() {
        match stdin.write(rest) {
            Ok(0) => return Err(WriteFail::Closed),
            Ok(n) => rest = rest.get(n..).unwrap_or_default(),
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                let left = deadline.saturating_duration_since(Instant::now());
                if left.is_zero() {
                    return Err(WriteFail::Late);
                }
                let wait = u16::try_from(left.min(POLL).as_millis()).unwrap_or(250);
                let mut fds = [PollFd::new(stdin.as_fd(), PollFlags::POLLOUT)];
                let _ = poll(&mut fds, PollTimeout::from(wait));
            }
            Err(_) => return Err(WriteFail::Closed),
        }
    }
    Ok(())
}

/// `ETXTBSY` (26): el ejecutable sigue abierto para escribir en algún
/// proceso (típico justo después de crearlo mientras otro hilo hace `fork`).
const TEXT_FILE_BUSY: i32 = 26;

/// `spawn` que reintenta unas veces si el ejecutable está ocupado
/// (`ETXTBSY`); cualquier otro fallo se devuelve enseguida.
fn spawn_retrying(command: &mut Command) -> std::io::Result<Child> {
    let mut attempt = 0;
    loop {
        match command.spawn() {
            Err(error) if error.raw_os_error() == Some(TEXT_FILE_BUSY) && attempt < 5 => {
                attempt += 1;
                std::thread::sleep(Duration::from_millis(50));
            }
            other => return other,
        }
    }
}

impl Session {
    /// `open_session(spec, cwd, model=…, permission_handler=deny_agent_tools,
    /// extra_env=…)`: lanza el agente en su propio grupo de procesos.
    pub fn open(spec: &AgentSpec, cwd: &Path, opts: &OpenOptions) -> Result<Session, AcpError> {
        let (argv, env) = build_command(spec, opts)?;
        let transport = if spec.get("transport").and_then(Value::as_str) == Some("agy-stream") {
            Transport::Agy
        } else {
            Transport::Acp
        };
        let program = argv.first().cloned().unwrap_or_default();
        let mut command = Command::new(&program);
        command
            .args(argv.iter().skip(1))
            .current_dir(cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .process_group(0);
        if let Some(base) = &opts.base_env {
            command.env_clear();
            command.envs(base.iter().map(|(k, v)| (k, v)));
        }
        // El ajuste de malloc del frente no pasa a los agentes (como
        // `tmux::CHILD_ENV_REMOVE` en los hijos del tablero).
        command.env_remove(comandos_core::malloc_tuning::GLIBC_TUNABLES_ENV);
        command.envs(env.iter().map(|(k, v)| (k, v)));
        let mut child = spawn_retrying(&mut command).map_err(|e| os_error(&e, &program))?;
        let group = Pid::from_raw(i32::try_from(child.id()).unwrap_or(0));
        let stderr = Arc::new(Mutex::new(Vec::new()));
        let (tx, lines) = mpsc::sync_channel(LINES_QUEUE);
        let flow = Arc::new(Flow::new());
        let watch = Arc::new(Watch {
            state: Mutex::new(WatchState {
                deadline: None,
                done: false,
            }),
            wake: Condvar::new(),
        });
        let stdin = child.stdin.take();
        if let Some(stdin) = &stdin {
            set_nonblocking(stdin);
        }
        let mut session = Session {
            child,
            group,
            stdin,
            lines,
            stderr: Arc::clone(&stderr),
            flow: Arc::clone(&flow),
            watch: Arc::clone(&watch),
            limit: None,
            next_id: 0,
            session_id: json!(""),
            cwd: cwd.to_path_buf(),
            transport,
            closed: false,
        };
        let out = session.child.stdout.take();
        let err = session.child.stderr.take();
        let started = (|| -> std::io::Result<()> {
            if let Some(out) = out {
                let stderr = Arc::clone(&stderr);
                std::thread::Builder::new()
                    .name("comandos-acp-out".into())
                    .spawn(move || pump_stdout(out, tx, stderr, flow))?;
            }
            if let Some(err) = err {
                let stderr = Arc::clone(&stderr);
                std::thread::Builder::new()
                    .name("comandos-acp-err".into())
                    .spawn(move || pump_stderr(err, stderr))?;
            }
            std::thread::Builder::new()
                .name("comandos-acp-watch".into())
                .spawn(move || watch.run(group))?;
            Ok(())
        })();
        match started {
            Ok(()) => Ok(session),
            Err(error) => {
                session.close();
                Err(AcpError(format!("can't start new thread: {error}")))
            }
        }
    }

    /// La siguiente línea de `stdout`; su coste vuelve al presupuesto al
    /// soltar su `Ticket`.
    fn next_line(&self, wait: Duration) -> Result<Line, RecvTimeoutError> {
        self.lines.recv_timeout(wait)
    }

    /// `" | ".join(self.proc.stderr[-3:])`.
    fn tail(&self) -> String {
        let lines = self.stderr.lock().unwrap_or_else(|e| e.into_inner());
        let start = lines.len().saturating_sub(3);
        lines.get(start..).unwrap_or_default().join(" | ")
    }

    /// `proc.poll() is None`, sin recoger al hijo (el grupo sigue reservado).
    fn alive(&self) -> bool {
        if self.closed {
            return false;
        }
        matches!(crate::procs::child_exited_unreaped(&self.child), Ok(false))
    }

    /// `proc.stdin.write(json.dumps(obj) + "\n")`, sin bloquear más allá del
    /// plazo de la llamada en curso: `stdin` es no bloqueante y se espera con
    /// `poll`. Un descendiente que se escapó del grupo (`setsid`) con `stdin`
    /// abierto sin leer no puede dejar colgado el hilo (el Python sí).
    fn write(&mut self, message: &Value) -> Result<(), AcpError> {
        let mut text = response_dumps(message).map_err(AcpError)?;
        text.push('\n');
        let (deadline, late) = match &self.limit {
            Some((deadline, late)) => (*deadline, late.clone()),
            None => (
                Instant::now() + WRITE_WAIT,
                format!(
                    "timeout esperando respuesta ({:.0}s)",
                    WRITE_WAIT.as_secs_f64()
                ),
            ),
        };
        let written = match self.stdin.as_mut() {
            Some(stdin) => write_until(stdin, text.as_bytes(), deadline),
            None => Err(WriteFail::Closed),
        };
        match written {
            Ok(()) => Ok(()),
            Err(WriteFail::Late) => Err(AcpError(late)),
            Err(WriteFail::Closed) => fail(format!("el agente cerró la conexión: {}", self.tail())),
        }
    }

    fn request(&mut self, method: &str, params: Value) -> Result<i64, AcpError> {
        self.next_id += 1;
        let id = self.next_id;
        self.write(&object([
            ("jsonrpc", json!("2.0")),
            ("id", json!(id)),
            ("method", json!(method)),
            ("params", params),
        ]))?;
        Ok(id)
    }

    fn respond(&mut self, id: Value, outcome: Result<Value, Value>) -> Result<(), AcpError> {
        self.write(&reply(id, outcome))
    }

    /// Escribe una petición y espera su respuesta, con el vigía armado.
    fn call(
        &mut self,
        method: &str,
        params: Value,
        on_event: Option<&mut (dyn FnMut(&Event) + '_)>,
        timeout: f64,
    ) -> Result<Value, AcpError> {
        let deadline = deadline_after(timeout);
        self.watch.arm(Some(deadline + WATCH_GRACE));
        self.limit = Some((
            deadline,
            format!("timeout esperando respuesta ({timeout:.0}s)"),
        ));
        let result = self
            .request(method, params)
            .and_then(|id| self.pump_until(id, on_event, deadline, timeout));
        self.limit = None;
        self.watch.arm(None);
        result
    }

    /// `_pump_until(want_id, on_event, timeout)`. Cada línea se atiende sin
    /// copiarla: sus partes se sacan del mapa (una línea hostil de 2 MiB
    /// ocupa más de 100 MiB ya convertida) y su `Ticket` se suelta al acabar.
    fn pump_until(
        &mut self,
        want: i64,
        mut on_event: Option<&mut (dyn FnMut(&Event) + '_)>,
        deadline: Instant,
        timeout: f64,
    ) -> Result<Value, AcpError> {
        loop {
            let now = Instant::now();
            if now >= deadline {
                break;
            }
            let (message, _ticket) = match self.next_line(POLL.min(deadline - now)) {
                Err(RecvTimeoutError::Timeout) => {
                    if !self.alive() {
                        return fail(format!("el agente murió: {}", self.tail()));
                    }
                    continue;
                }
                Err(RecvTimeoutError::Disconnected)
                | Ok(Line::Eof)
                | Ok(Line::Json(Value::Null, _)) => {
                    return fail(format!("el agente cerró stdout: {}", self.tail()));
                }
                Ok(Line::Json(message, ticket)) => (message, ticket),
            };
            let mut map = match message {
                Value::Object(map) => map,
                other @ (Value::Array(_) | Value::String(_)) => return Err(no_get(&other)),
                other => {
                    return fail(format!(
                        "argument of type '{}' is not iterable",
                        py_type_name(&other)
                    ));
                }
            };
            if map.contains_key("method") {
                self.handle_incoming(map, on_event.as_deref_mut())?;
                continue;
            }
            if python_eq(get(&map, "id"), &json!(want)) {
                if map.contains_key("error") {
                    return Err(agent_error(get(&map, "error"))?);
                }
                return Ok(take(&mut map, "result", json!({})));
            }
        }
        fail(format!("timeout esperando respuesta ({timeout:.0}s)"))
    }

    fn handle_incoming(
        &mut self,
        mut message: Map<String, Value>,
        on_event: Option<&mut (dyn FnMut(&Event) + '_)>,
    ) -> Result<(), AcpError> {
        let method = message.remove("method").unwrap_or(Value::Null);
        let params = take(&mut message, "params", json!({}));
        let rid = message.remove("id").unwrap_or(Value::Null);
        match method.as_str() {
            Some("session/update") => {
                let mut params = into_dict(params)?;
                let theirs = get(&params, "sessionId");
                if truthy(&self.session_id)
                    && truthy(theirs)
                    && !python_eq(theirs, &self.session_id)
                {
                    return Ok(());
                }
                session_update(take(&mut params, "update", json!({})), on_event)
            }
            Some("session/request_permission") => self.permission(params, rid, on_event),
            // Endurecimiento: el agente de un resumen no lee ni escribe
            // archivos por el cliente (el Python lo haría).
            Some(name @ ("fs/read_text_file" | "fs/write_text_file")) => {
                let error =
                    json!({"code": -32601, "message": format!("{name} no soportado por cc-acp")});
                self.respond(rid, Err(error))
            }
            _ if !rid.is_null() => {
                let error = json!({"code": -32601, "message": format!("{} no soportado por cc-acp", py_str(&method))});
                self.respond(rid, Err(error))
            }
            _ => Ok(()),
        }
    }

    /// `_permission` con `deny_agent_tools`: nunca se permite nada. La
    /// opción elegida sale de la petición sin copiarla, va en la respuesta y
    /// de ahí al evento.
    fn permission(
        &mut self,
        params: Value,
        rid: Value,
        on_event: Option<&mut (dyn FnMut(&Event) + '_)>,
    ) -> Result<(), AcpError> {
        let mut params = into_dict(params)?;
        let mut options = take(&mut params, "options", json!([]));
        let mut call = take_dict(params.remove("toolCall"))?;
        let title = take(&mut call, "title", json!(""));
        let decision = deny_position(&options)?.and_then(|index| {
            options
                .get_mut(index)
                .and_then(Value::as_object_mut)
                .and_then(|option| option.remove("optionId"))
        });
        let decision = match decision {
            Some(option) if truthy(&option) => {
                let outcome = object([("outcome", json!("selected")), ("optionId", option)]);
                let mut message = reply(rid, Ok(object([("outcome", outcome)])));
                self.write(&message)?;
                message
                    .pointer_mut("/result/outcome/optionId")
                    .map(Value::take)
            }
            other => {
                self.respond(rid, Ok(json!({"outcome": {"outcome": "cancelled"}})))?;
                other
            }
        };
        if let Some(on_event) = on_event {
            on_event(&Event::Permission { title, decision });
        }
        Ok(())
    }

    /// `new_session(timeout)`: `session/new` con el `cwd` y sin servidores MCP
    /// (en `agy`, la sesión existe en cuanto arranca el proceso).
    pub fn new_session(&mut self, timeout: f64) -> Result<Value, AcpError> {
        if self.transport == Transport::Agy {
            self.session_id = json!("agy-pending");
            return Ok(self.session_id.clone());
        }
        let cwd = self.cwd.display().to_string();
        let result = self.call(
            "session/new",
            json!({"cwd": cwd, "mcpServers": []}),
            None,
            timeout,
        )?;
        let mut result = into_dict(result)?;
        self.session_id = take(&mut result, "sessionId", json!(""));
        if !truthy(&self.session_id) {
            return fail("session/new sin sessionId");
        }
        absorb_session(&result)?;
        Ok(self.session_id.clone())
    }

    fn prompt_acp(
        &mut self,
        text: &str,
        on_event: &mut dyn FnMut(&Event),
        timeout: f64,
    ) -> Result<Value, AcpError> {
        let params = object([
            ("sessionId", self.session_id.clone()),
            ("prompt", json!([{"type": "text", "text": text}])),
        ]);
        let result = self.call("session/prompt", params, Some(&mut *on_event), timeout)?;
        let stop = take(&mut into_dict(result)?, "stopReason", json!("end_turn"));
        // El evento lleva el valor y después se devuelve, sin copiarlo.
        let end = Event::End(stop);
        on_event(&end);
        match end {
            Event::End(stop) => Ok(stop),
            _ => Ok(json!("end_turn")),
        }
    }

    /// `AgyStreamSession.prompt`.
    fn prompt_agy(
        &mut self,
        text: &str,
        on_event: &mut dyn FnMut(&Event),
        timeout: f64,
    ) -> Result<Value, AcpError> {
        let deadline = deadline_after(timeout);
        self.watch.arm(Some(deadline + WATCH_GRACE));
        self.limit = Some((deadline, format!("timeout esperando a agy ({timeout:.0}s)")));
        let result = self.agy_turn(text, on_event, deadline, timeout);
        self.limit = None;
        self.watch.arm(None);
        result
    }

    fn agy_turn(
        &mut self,
        text: &str,
        on_event: &mut dyn FnMut(&Event),
        deadline: Instant,
        timeout: f64,
    ) -> Result<Value, AcpError> {
        self.write(&json!({"event": "user", "message": {"role": "user", "content": text}}))?;
        loop {
            let now = Instant::now();
            if now >= deadline {
                break;
            }
            let (message, _ticket) = match self.next_line(POLL.min(deadline - now)) {
                Err(RecvTimeoutError::Timeout) => {
                    if !self.alive() {
                        return fail(format!("agy murió: {}", self.tail()));
                    }
                    continue;
                }
                Err(RecvTimeoutError::Disconnected)
                | Ok(Line::Eof)
                | Ok(Line::Json(Value::Null, _)) => {
                    return fail(format!("agy cerró stdout: {}", self.tail()));
                }
                Ok(Line::Json(message, ticket)) => (message, ticket),
            };
            // Como en `pump_until`: las partes se sacan, no se copian.
            let mut map = into_dict(message)?;
            let event = match get(&map, "event").as_str() {
                Some("init") => "init",
                Some("step_update") => "step_update",
                Some("result") => "result",
                _ => "",
            };
            match event {
                "init" => {
                    if let Some(id) = map.remove("conversation_id").filter(truthy) {
                        self.session_id = id;
                    }
                }
                "step_update" => {
                    let mut step = take_dict(map.remove("step_update"))?;
                    if let Some(delta) = step.remove("text_delta").filter(truthy) {
                        on_event(&Event::Text(delta));
                    } else {
                        let kind = get(&step, "step_type");
                        if !matches!(kind.as_str(), Some("user_input" | "agent_response")) {
                            on_event(&Event::Tool {
                                title: json!(py_str_or_empty(kind)),
                                status: json!(py_str_or_empty(get(&step, "state")).to_lowercase()),
                                kind: json!(""),
                                id: json!(match step.get("step_index") {
                                    Some(v) => py_str(v),
                                    None => String::new(),
                                }),
                            });
                        }
                    }
                }
                "result" => {
                    let mut result = take_dict(map.remove("result"))?;
                    if let Some(id) = result.remove("conversation_id").filter(truthy) {
                        self.session_id = id;
                    }
                    if get(&result, "status").as_str() != Some("SUCCESS") {
                        let error = match (get(&result, "error"), get(&result, "status")) {
                            (e, _) if truthy(e) => py_str(e),
                            (_, s) if truthy(s) => py_str(s),
                            _ => "agy error".into(),
                        };
                        return fail(error);
                    }
                    on_event(&Event::End(json!("end_turn")));
                    return Ok(json!("end_turn"));
                }
                _ => {}
            }
        }
        fail(format!("timeout esperando a agy ({timeout:.0}s)"))
    }

    /// `close()`: cierra `stdin`, `SIGTERM` al grupo, espera hasta 3 s y
    /// después `SIGKILL` al grupo (también a nietos que sigan vivos) y recoge
    /// al hijo. Idempotente.
    pub fn close(&mut self) {
        if self.closed {
            return;
        }
        self.closed = true;
        self.flow.close();
        drop(self.stdin.take());
        let valid = self.group.as_raw() > 1;
        if valid {
            let _ = killpg(self.group, Signal::SIGTERM);
            let started = Instant::now();
            while started.elapsed() < CLOSE_WAIT {
                if !matches!(crate::procs::child_exited_unreaped(&self.child), Ok(false)) {
                    break;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            // El líder sigue sin recoger: el grupo es todavía el suyo.
            let _ = killpg(self.group, Signal::SIGKILL);
        } else {
            let _ = self.child.kill();
        }
        {
            let mut state = self.watch.lock();
            state.done = true;
            state.deadline = None;
        }
        self.watch.wake.notify_all();
        let _ = self.child.wait();
    }
}

impl AgentSession for Session {
    fn prompt(
        &mut self,
        text: &str,
        on_event: &mut dyn FnMut(&Event),
        timeout: Duration,
    ) -> Result<Value, AcpError> {
        let seconds = timeout.as_secs_f64();
        match self.transport {
            Transport::Acp => self.prompt_acp(text, on_event, seconds),
            Transport::Agy => self.prompt_agy(text, on_event, seconds),
        }
    }

    fn close(&mut self) {
        Session::close(self);
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        self.close();
    }
}

/// El texto de una respuesta de error del agente (`_pump_until`).
fn agent_error(error: &Value) -> Result<AcpError, AcpError> {
    let empty = Map::new();
    let error = if truthy(error) {
        as_dict(error)?
    } else {
        &empty
    };
    let fallback = json!("error del agente");
    let mut text = py_str(or_ref(error, "message", &fallback));
    if let Some(detail) = error.get("data").and_then(Value::as_object) {
        let message = get(detail, "message");
        if truthy(message) {
            let clipped: String = py_str(message).chars().take(400).collect();
            text.push_str(": ");
            text.push_str(&clipped);
        }
    }
    Ok(AcpError(text))
}

/// `_session_update`: los eventos que ve el que pregunta. El estado del
/// agente (modelos, modos, comandos) no se guarda: los Resúmenes no lo usan;
/// solo se reproducen las excepciones que el Python lanzaría al absorberlo.
fn session_update(
    update: Value,
    on_event: Option<&mut (dyn FnMut(&Event) + '_)>,
) -> Result<(), AcpError> {
    const KINDS: [&str; 6] = [
        "config_option_update",
        "agent_message_chunk",
        "agent_thought_chunk",
        "tool_call",
        "tool_call_update",
        "plan",
    ];
    let mut update = into_dict(update)?;
    let kind = get(&update, "sessionUpdate")
        .as_str()
        .and_then(|kind| KINDS.into_iter().find(|known| *known == kind))
        .unwrap_or_default();
    if kind == "config_option_update" {
        let empty = json!([]);
        absorb_config_options(or_ref(&update, "configOptions", &empty))?;
    }
    let Some(on_event) = on_event else {
        return Ok(());
    };
    match kind {
        "agent_message_chunk" | "agent_thought_chunk" => {
            let mut content = take_dict(update.remove("content"))?;
            if get(&content, "type").as_str() == Some("text") {
                let text = content.remove("text").unwrap_or_else(|| json!(""));
                on_event(&if kind == "agent_message_chunk" {
                    Event::Text(text)
                } else {
                    Event::Thought(text)
                });
            }
        }
        "tool_call" | "tool_call_update" => on_event(&Event::Tool {
            title: take(&mut update, "title", json!("")),
            status: take(&mut update, "status", json!("")),
            kind: take(&mut update, "kind", json!("")),
            id: take(&mut update, "toolCallId", json!("")),
        }),
        "plan" => on_event(&Event::Plan(take(&mut update, "entries", json!([])))),
        _ => {}
    }
    Ok(())
}

/// `_absorb_session`: solo sus excepciones posibles.
fn absorb_session(result: &Map<String, Value>) -> Result<(), AcpError> {
    for key in ["models", "modes"] {
        check_dict(result.get(key))?;
    }
    if let Some(options) = result.get("configOptions") {
        absorb_config_options(options)?;
    }
    Ok(())
}

/// `_absorb_config_options`: solo sus excepciones posibles. Recorre sin
/// copiar: la primera opción que no es `dict` es la excepción del Python
/// (que antes junta todas, así un fallo del recorrido manda).
fn absorb_config_options(options: &Value) -> Result<(), AcpError> {
    let mut selects: Vec<&Map<String, Value>> = Vec::new();
    for option in py_iter_lazy(options)? {
        if let Cow::Borrowed(Value::Object(map)) = option
            && get(map, "type").as_str() == Some("select")
        {
            selects.push(map);
        }
    }
    let empty = json!([]);
    for category in ["model", "mode", "thought_level"] {
        let Some(option) = selects
            .iter()
            .find(|o| get(o, "category").as_str() == Some(category))
        else {
            continue;
        };
        let mut bad: Option<AcpError> = None;
        let mut check = |value: &Value| {
            if category != "thought_level" && bad.is_none() {
                bad = as_dict(value).err();
            }
        };
        for item in py_iter_lazy(or_ref(option, "options", &empty))? {
            let nested = match &*item {
                Value::Object(map) => map.contains_key("options"),
                Value::Array(items) => items.iter().any(|i| i.as_str() == Some("options")),
                Value::String(s) => s.contains("options"),
                other => {
                    return fail(format!(
                        "argument of type '{}' is not iterable",
                        py_type_name(other)
                    ));
                }
            };
            if nested {
                let Value::Object(map) = &*item else {
                    return fail(format!(
                        "{} indices must be integers",
                        if item.is_string() { "string" } else { "list" }
                    ));
                };
                for value in py_iter_lazy(get(map, "options"))? {
                    check(&value);
                }
            } else {
                check(&item);
            }
        }
        if let Some(error) = bad {
            return Err(error);
        }
    }
    Ok(())
}

/// La posición de la primera opción cuyo `kind` contiene `reject` o `deny`
/// (solo una lista puede tenerla: con un `dict` o un `str` la primera
/// opción ya no es un `dict`). No copia las opciones.
fn deny_position(options: &Value) -> Result<Option<usize>, AcpError> {
    if !truthy(options) {
        return Ok(None);
    }
    for (index, option) in py_iter_lazy(options)?.enumerate() {
        let option = as_dict(&option)?;
        let kind = py_str_or_empty(get(option, "kind"));
        if kind.contains("reject") || kind.contains("deny") {
            return Ok(Some(index));
        }
    }
    Ok(None)
}

/// `deny_agent_tools(request)`: el `optionId` de la primera opción cuyo
/// `kind` contiene `reject` o `deny`; `None` si no hay ninguna.
pub fn deny_agent_tools(options: &Value) -> Result<Option<Value>, AcpError> {
    Ok(deny_position(options)?.and_then(|index| options.get(index)?.get("optionId").cloned()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn py_str_follows_python() {
        assert_eq!(py_str(&json!(null)), "None");
        assert_eq!(py_str(&json!(true)), "True");
        assert_eq!(py_str(&json!(5)), "5");
        assert_eq!(py_str(&json!(1.5)), "1.5");
        assert_eq!(py_str(&json!("a")), "a");
        assert_eq!(py_str(&json!([1, "a'b", null])), "[1, \"a'b\", None]");
        assert_eq!(py_str(&json!({"k": "v"})), "{'k': 'v'}");
        assert_eq!(py_str(&json!("x\ny")), "x\ny");
        assert_eq!(py_repr(&json!("x\ny")), "'x\\ny'");
        assert_eq!(py_str_or_empty(&json!(0)), "");
    }

    #[test]
    fn iteration_follows_python() {
        assert_eq!(py_iter(&json!("ab")).unwrap(), vec![json!("a"), json!("b")]);
        assert_eq!(py_iter(&json!({"a": 1})).unwrap(), vec![json!("a")]);
        assert_eq!(
            py_iter(&json!(5)).unwrap_err().0,
            "'int' object is not iterable"
        );
    }

    #[test]
    fn build_command_places_model_args() {
        let dir = std::env::temp_dir().join(format!("laneB-acp-unit-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let bin = dir.join("agente");
        std::fs::write(&bin, "#!/bin/sh\n").unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o700)).unwrap();
        let opts = OpenOptions {
            model: "m1".into(),
            extra_env: vec![("COMANDOS_SILENT_AGENT".into(), "1".into())],
            search_path: Some(dir.clone().into_os_string()),
            home: dir.clone(),
            base_env: Some(Vec::new()),
        };
        let spec = json!({"command": ["agente", "x", "{model_args}", "{effort_args}", "stdio"],
                          "modelArgs": ["-m", "{model}"], "env": {"A": "~/b", "B": "which:no-existe"},
                          "modelEnv": "M", "modelEnvTemplate": "{\"model\":\"{model}\"}"});
        let (argv, env) = build_command(&spec, &opts).unwrap();
        assert_eq!(
            argv,
            vec![
                bin.display().to_string(),
                "x".into(),
                "-m".into(),
                "m1".into(),
                "stdio".into()
            ]
        );
        assert_eq!(
            env,
            vec![
                ("A".to_owned(), format!("{}/b", dir.display())),
                ("M".to_owned(), "{\"model\":\"m1\"}".to_owned()),
                ("COMANDOS_SILENT_AGENT".to_owned(), "1".to_owned()),
            ]
        );
        let spec = json!({"command": ["agente"], "modelArgs": ["-m", "{model}"]});
        let (argv, _) = build_command(&spec, &opts).unwrap();
        assert_eq!(
            argv,
            vec![bin.display().to_string(), "-m".into(), "m1".into()]
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn agent_errors_carry_their_detail() {
        assert_eq!(
            agent_error(&json!({"message": "malo", "data": {"message": "x".repeat(500)}}))
                .unwrap()
                .0,
            format!("malo: {}", "x".repeat(400))
        );
        assert_eq!(agent_error(&json!(null)).unwrap().0, "error del agente");
        assert_eq!(
            agent_error(&json!([1])).unwrap_err().0,
            "'list' object has no attribute 'get'"
        );
    }
}
