//! Registro de pestañas (plan 2f-1, Tarea 1): `load_json_file` (5034),
//! `read_tab_metadata` (5252), `write_tab_metadata` (5274),
//! `remove_tab_metadata` (5289), `tab_metadata_for_session` (5297),
//! `write_app_tab` (5239), `register_app_tab` (5321), `remember_tab` (5223) y
//! `close_app_tab` (5350) de `bin/cc-dash`.
//!
//! Dueño único (E1 del plan): con el corte `tabs` activo, el frente es el
//! único que escribe `app-tabs-meta.json`, bajo `meta_lock` (el
//! `TAB_METADATA_LOCK` del Python). `app-tabs.json` y `app-tabs-history.json`
//! llevan el `flock` de `<archivo>.lock` que también toman el Python y cc-app.
//! Mismo volcado que `write_json_file` (`json.dumps` con ASCII escapado y los
//! separadores por omisión) y la misma escritura atómica.
//!
//! Sin `Decline` (ruling 2 del plan): todo lo que el frente no puede
//! reproducir con certeza tras leer un archivo del registro es un 500 con una
//! línea en stderr (`RegistryError` → `Fault`).
//!
//! Ligereza: cada leer-modificar-escribir es un solo salto de bloqueo cuando
//! el `flock` está libre (`try_acquire` en el mismo hilo); solo si otro lo
//! tiene se espera en la cola de `FileLock::acquire_timeout` (un hilo por
//! ruta como mucho) y se hace un segundo salto. `meta_lock` nunca se retiene
//! a través de `Native::with_state` (P28 del pre-flight).
// Submódulo de `tabs` (ver su `#[path]`): las piezas hermanas por la ruta entera.
use crate::HandlerError;
use crate::dash::native::{
    Fault, Native, NativeOptions,
    files::{DomainDocument, FileLock, LOCK_WAIT, Strict, read_json_strict, write_json_atomic},
    light, py,
    states::gather::session_labels,
    target, workspace,
};
use comandos_runtime::ssh_config;
use serde_json::{Map, Value, json};
use std::{
    collections::HashSet,
    io,
    path::{Path, PathBuf},
    sync::{Arc, OnceLock},
};

/// `TAB_KINDS` (5248).
pub const TAB_KINDS: [&str; 5] = ["project", "scratch", "shell", "ssh", "ssh-tab"];

pub const TABS_FILE: &str = "app-tabs.json";
pub const TABS_META_FILE: &str = "app-tabs-meta.json";
pub const TAB_HISTORY_FILE: &str = "app-tabs-history.json";
pub const TAB_OPEN_FILE: &str = "app-tab-open.json";
pub const TAB_CLOSE_FILE: &str = "app-tab-close.json";

/// Por qué una función del registro no terminó como el Python.
pub enum RegistryError {
    /// Un archivo del registro que el Python leería de otra forma
    /// (`files::Strict::Unsure`): anidamiento profundo, sustitutos sueltos,
    /// bytes no UTF-8 u otro error de E/S.
    Unsure(PathBuf),
    /// Escritura o candado fallidos (el Python lanzaría `OSError`; el candado
    /// vencido es la espera de 30 s del frente, que el Python no tiene).
    Io(io::Error),
    /// Lo que devuelven las piezas compartidas (`target`, `light`, tmux): un
    /// error que el Python también lanzaría, o un `Decline` de algo que no se
    /// reproduce con certeza. Aquí ninguno declina: los dos son 500/504.
    Fault(Fault),
}

impl std::fmt::Debug for RegistryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RegistryError::Unsure(path) => write!(f, "Unsure({})", path.display()),
            RegistryError::Io(error) => write!(f, "Io({error})"),
            RegistryError::Fault(Fault::Decline) => f.write_str("Fault(Decline)"),
            RegistryError::Fault(Fault::Error(error)) => write!(f, "Fault({error:?})"),
        }
    }
}

impl From<Fault> for RegistryError {
    fn from(fault: Fault) -> Self {
        RegistryError::Fault(fault)
    }
}

impl RegistryError {
    /// El `Fault` de la ruta `route`, con la línea de stderr del ruling 2.
    pub fn into_fault(self, route: &str) -> Fault {
        match self {
            RegistryError::Unsure(path) => {
                eprintln!(
                    "comandos dash: {} incierto; {route} responde 500",
                    path.display()
                );
                Fault::Error(HandlerError::Failure)
            }
            RegistryError::Io(error) => {
                eprintln!("comandos dash: registro de pestañas: {error}; {route} responde 500");
                Fault::Error(HandlerError::Failure)
            }
            RegistryError::Fault(Fault::Decline) => {
                eprintln!("comandos dash: registro de pestañas incierto; {route} responde 500");
                Fault::Error(HandlerError::Failure)
            }
            RegistryError::Fault(fault) => fault,
        }
    }
}

impl From<RegistryError> for Fault {
    fn from(error: RegistryError) -> Self {
        error.into_fault("la ruta")
    }
}

/// `TAB_METADATA_LOCK` del Python: solo el frente escribe `app-tabs-meta.json`
/// con el corte `tabs` activo (D2 del plan maestro). Global del proceso: hay un
/// solo archivo por HOME y un solo frente.
///
/// El guardia se toma con `lock_owned` y viaja DENTRO del trabajo de bloqueo
/// que lee y reescribe el archivo: si el futuro de la ruta se suelta a mitad
/// (cliente que se va, plazo del manejador, apagado), el hilo sigue
/// escribiendo con el candado tomado y nadie más entra hasta que termina.
fn meta_lock() -> &'static Arc<tokio::sync::Mutex<()>> {
    static LOCK: OnceLock<Arc<tokio::sync::Mutex<()>>> = OnceLock::new();
    LOCK.get_or_init(|| Arc::new(tokio::sync::Mutex::new(())))
}

fn hook(opts: &NativeOptions, name: &str) -> PathBuf {
    opts.hooks.join(name)
}

/// Un trabajo de disco en un hilo de bloqueo.
async fn blocking<T, F>(job: F) -> Result<T, RegistryError>
where
    F: FnOnce() -> Result<T, RegistryError> + Send + 'static,
    T: Send + 'static,
{
    tokio::task::spawn_blocking(job)
        .await
        .map_err(|_| RegistryError::Fault(Fault::Error(HandlerError::Failure)))?
}

pub(crate) fn domain_document(
    opts: &NativeOptions,
    name: &str,
) -> Result<DomainDocument, RegistryError> {
    DomainDocument::new(&opts.home, &opts.hooks, name).map_err(RegistryError::Io)
}
pub(crate) fn load_document(
    doc: &DomainDocument,
    access: Option<&comandos_store::domains::caller::CallerAccess>,
    default: Value,
) -> Result<Value, RegistryError> {
    match access.map_or_else(|| doc.strict(), |a| doc.strict_under(a)) {
        Strict::Value(Value::Null) | Strict::Missing | Strict::Unreadable => Ok(default),
        Strict::Value(value) => Ok(value),
        Strict::Unsure => Err(RegistryError::Unsure(doc.file.clone())),
    }
}
fn write_document(
    doc: &DomainDocument,
    access: &comandos_store::domains::caller::CallerAccess,
    value: &Value,
) -> Result<(), RegistryError> {
    let text = comandos_core::json::response_dumps(value)
        .map_err(|e| RegistryError::Io(io::Error::other(e)))?;
    doc.write_under(access, text.as_bytes(), 0)
        .map_err(RegistryError::Io)
}
fn with_document_sync<T>(
    opts: &NativeOptions,
    name: &str,
    job: impl FnOnce(
        &DomainDocument,
        &comandos_store::domains::caller::CallerAccess,
    ) -> Result<T, RegistryError>,
) -> Result<T, RegistryError> {
    let doc = domain_document(opts, name)?;
    let access = doc
        .access()
        .map_err(|e| RegistryError::Io(io::Error::other(e)))?;
    let _lock = if access.mode() == comandos_store::unified::Mode::Sealed {
        None
    } else {
        let deadline = std::time::Instant::now() + LOCK_WAIT;
        let lock = loop {
            if let Some(lock) = FileLock::try_acquire(&doc.file).map_err(RegistryError::Io)? {
                break lock;
            }
            if std::time::Instant::now() >= deadline {
                return Err(RegistryError::Io(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "registro ocupado",
                )));
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        };
        Some(lock)
    };
    access
        .with_write_transaction(|| job(&doc, &access))
        .map_err(|e| RegistryError::Io(io::Error::other(e)))?
}
async fn with_document<T: Send + 'static>(
    opts: NativeOptions,
    name: &'static str,
    job: impl FnOnce(
        &DomainDocument,
        &comandos_store::domains::caller::CallerAccess,
    ) -> Result<T, RegistryError>
    + Send
    + 'static,
) -> Result<T, RegistryError> {
    blocking(move || with_document_sync(&opts, name, job)).await
}
pub(crate) fn read_metadata_domain(
    opts: &NativeOptions,
    access: Option<&comandos_store::domains::caller::CallerAccess>,
) -> Result<Map<String, Value>, RegistryError> {
    let doc = domain_document(opts, TABS_META_FILE)?;
    metadata_from(load_document(&doc, access, json!({}))?)
}

/// `load_json_file(path, default)`: ausente, ilegible o `null` → `default`;
/// incierto → `Unsure` (ruling 2: el Python lo trataría con su `except`).
pub fn load_json_file(path: &Path, default: Value) -> Result<Value, RegistryError> {
    match read_json_strict(path) {
        Strict::Value(Value::Null) | Strict::Missing | Strict::Unreadable => Ok(default),
        Strict::Value(value) => Ok(value),
        Strict::Unsure => Err(RegistryError::Unsure(path.to_path_buf())),
    }
}

/// `load_json_file(path, {})` y `if not isinstance(x, dict): x = {}`.
fn load_object(path: &Path) -> Result<Map<String, Value>, RegistryError> {
    Ok(match load_json_file(path, json!({}))? {
        Value::Object(map) => map,
        _ => Map::new(),
    })
}

/// `kind not in TAB_KINDS`: `Some(sí/no)`; `None` si el valor no es hashable
/// (lista u objeto: `TypeError` en el Python).
fn known_kind(kind: Option<&Value>) -> Option<bool> {
    match kind {
        Some(Value::String(s)) => Some(TAB_KINDS.contains(&s.as_str())),
        Some(Value::Array(_) | Value::Object(_)) => None,
        _ => Some(false),
    }
}

/// `read_tab_metadata()` (5252) sobre `path`. Bloquea.
pub fn read_tab_metadata(path: &Path) -> Result<Map<String, Value>, RegistryError> {
    metadata_from(Value::Object(load_object(path)?))
}
fn metadata_from(data: Value) -> Result<Map<String, Value>, RegistryError> {
    let data = data.as_object().cloned().unwrap_or_default();
    let mut out = Map::new();
    for (sess, value) in data {
        let Value::Object(value) = value else {
            continue;
        };
        match known_kind(value.get("kind")) {
            None => return Err(Fault::Error(HandlerError::Failure).into()),
            Some(false) => continue,
            Some(true) => {}
        }
        let mut item = Map::new();
        item.insert(
            "kind".into(),
            value.get("kind").cloned().unwrap_or_default(),
        );
        if let Some(Value::String(host)) = value.get("host")
            && !host.is_empty()
        {
            item.insert("host".into(), json!(host));
        }
        if let Some(Value::String(cwd)) = value.get("cwd")
            && cwd.starts_with('/')
        {
            item.insert("cwd".into(), json!(cwd));
        }
        out.insert(sess, Value::Object(item));
    }
    Ok(out)
}

/// `{"kind": kind}` + `host` si no vacío + `cwd` si es absoluto.
fn meta_item(kind: &str, host: &str, cwd: &str) -> Value {
    let mut item = Map::new();
    item.insert("kind".into(), json!(kind));
    if !host.is_empty() {
        item.insert("host".into(), json!(host));
    }
    // `cwd and os.path.isabs(cwd)`.
    if cwd.starts_with('/') {
        item.insert("cwd".into(), json!(cwd));
    }
    Value::Object(item)
}

/// `write_tab_metadata(sess, kind, host, cwd)` (5274): `None` si la sesión o
/// el tipo no valen; si no, el item escrito.
pub async fn write_tab_metadata(
    native: &Native,
    sess: &str,
    kind: &str,
    host: &str,
    cwd: &str,
) -> Result<Option<Value>, RegistryError> {
    if !py::is_session(sess) || !TAB_KINDS.contains(&kind) {
        return Ok(None);
    }
    let opts = native.options().clone();
    let item = meta_item(kind, host, cwd);
    let (sess, written) = (sess.to_owned(), item.clone());
    let guard = Arc::clone(meta_lock()).lock_owned().await;
    with_document(opts.clone(), TABS_META_FILE, move |doc, access| {
        let _guard = guard;
        let mut metadata = read_metadata_domain(&opts, Some(access))?;
        metadata.insert(sess, written);
        write_document(doc, access, &Value::Object(metadata))
    })
    .await?;
    Ok(Some(item))
}

/// `remove_tab_metadata(sess)` (5289): escribe solo si la clave estaba.
pub async fn remove_tab_metadata(native: &Native, sess: &str) -> Result<(), RegistryError> {
    let opts = native.options().clone();
    let sess = sess.to_owned();
    let guard = Arc::clone(meta_lock()).lock_owned().await;
    with_document(opts, TABS_META_FILE, move |doc, access| {
        let _guard = guard;
        unmeta_locked(doc, access, &sess)
    })
    .await
}

/// `str.isdigit()`: exacto en ASCII; con no-ASCII, «no» seguro si algún
/// carácter no es numérico para Unicode y `None` (no se sabe) si todos lo son.
fn py_isdigit(text: &str) -> Option<bool> {
    if text.is_ascii() {
        return Some(!text.is_empty() && text.bytes().all(|b| b.is_ascii_digit()));
    }
    if text.chars().all(char::is_numeric) {
        None
    } else {
        Some(false)
    }
}

/// `tab_metadata_for_session` sobre las opciones (bloquea).
fn derived_metadata(
    opts: &NativeOptions,
    sess: &str,
    access: Option<&comandos_store::domains::caller::CallerAccess>,
) -> Result<Value, RegistryError> {
    if let Some(saved) = read_metadata_domain(opts, access)?.get(sess) {
        return Ok(saved.clone());
    }
    if let Some(project) = target::find_project_dir(&opts.home, sess)? {
        let cwd = project.to_str().ok_or(Fault::Decline)?;
        return Ok(json!({"kind": "project", "cwd": cwd}));
    }
    let entry = |host: &str| -> Result<bool, RegistryError> {
        Ok(ssh_config::host_entry(&opts.home, host)
            .map_err(RegistryError::Io)?
            .is_some())
    };
    if let Some(host) = sess.strip_prefix("ssh-")
        && entry(host)?
    {
        return Ok(json!({"kind": "ssh", "host": host}));
    }
    if let Some(rest) = sess.strip_prefix("sshtab-")
        && let Some((host, index)) = rest.rsplit_once('-')
        && py_isdigit(index).ok_or(Fault::Decline)?
        && entry(host)?
    {
        return Ok(json!({"kind": "ssh-tab", "host": host}));
    }
    if sess.starts_with("term-") {
        return Ok(json!({"kind": "scratch"}));
    }
    Ok(json!({"kind": "shell"}))
}

/// `tab_metadata_for_session(sess)` (5297): la identidad guardada o, si no
/// hay, la derivada (proyecto, ssh, ssh-tab, scratch, shell).
pub async fn tab_metadata_for_session(native: &Native, sess: &str) -> Result<Value, RegistryError> {
    let opts = native.options().clone();
    let sess = sess.to_owned();
    blocking(move || derived_metadata(&opts, &sess, None)).await
}

/// `write_app_tab(sess, label)` (5239): `tabs[sess] = label or sess`.
pub async fn write_app_tab(native: &Native, sess: &str, label: &str) -> Result<(), RegistryError> {
    let opts = native.options().clone();
    let (sess, label) = (sess.to_owned(), label.to_owned());
    with_document(opts, TABS_FILE, move |doc, access| {
        let mut tabs = load_document(doc, Some(access), json!({}))?
            .as_object()
            .cloned()
            .unwrap_or_default();
        let label = if label.is_empty() {
            sess.clone()
        } else {
            label
        };
        tabs.insert(sess, json!(label));
        write_document(doc, access, &Value::Object(tabs))
    })
    .await
}

/// `(label or sess)[:80]` (caracteres).
fn label_or(label: Option<&str>, sess: &str) -> String {
    let text = label.filter(|l| !l.is_empty()).unwrap_or(sess);
    py::take_chars(text, 80)
}

/// `time.time()` como número JSON.
fn seconds_value(opts: &NativeOptions) -> Result<Value, RegistryError> {
    serde_json::Number::from_f64((opts.clock_seconds)())
        .map(Value::Number)
        .ok_or(RegistryError::Fault(Fault::Error(HandlerError::Failure)))
}

/// `register_app_tab(sess, label, kind, host, cwd)` (5321): espejo en
/// `app-tabs.json` si falta, identidad en `app-tabs-meta.json` y aviso a la app
/// viva en `app-tab-open.json`.
pub async fn register_app_tab(
    native: &Native,
    sess: &str,
    label: Option<&str>,
    kind: &str,
    host: &str,
    cwd: &str,
) -> Result<(), RegistryError> {
    if sess == "local" {
        return Ok(());
    }
    let opts = native.options();
    let shown = label_or(label, sess);
    let known = TAB_KINDS.contains(&kind);
    let (owned, mirrored) = (sess.to_owned(), shown.clone());
    let reads = opts.clone();
    // Todo lo que puede responder 500 se lee ANTES de escribir el espejo:
    // `app-tabs-meta.json` (que `write_tab_metadata` y la derivación leen
    // después) y, con un tipo desconocido, la identidad derivada (sus lecturas
    // no dependen de `app-tabs.json`; el Python la calcula después del espejo).
    let derived = with_document(opts.clone(), TABS_FILE, move |doc, access| {
        let derived = if known {
            read_metadata_domain(&reads, Some(access))?;
            None
        } else {
            Some(derived_metadata(&reads, &owned, Some(access))?)
        };
        let mut tabs = load_document(doc, Some(access), json!({}))?
            .as_object()
            .cloned()
            .unwrap_or_default();
        if !tabs.contains_key(&owned) {
            tabs.insert(owned, json!(mirrored));
            write_document(doc, access, &Value::Object(tabs))?;
        }
        Ok(derived)
    })
    .await?;
    let mut meta = match derived {
        None => write_tab_metadata(native, sess, kind, host, cwd).await?,
        Some(found) => Some(found),
    };
    if let Some(found) = meta.clone() {
        let meta_opts = opts.clone();
        let owned = sess.to_owned();
        let saved =
            blocking(move || Ok(read_metadata_domain(&meta_opts, None)?.contains_key(&owned)))
                .await?;
        if !saved {
            let text = |key: &str| {
                found
                    .get(key)
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_owned()
            };
            meta = write_tab_metadata(native, sess, &text("kind"), &text("host"), &text("cwd"))
                .await?;
        }
    }
    let mut event = Map::new();
    event.insert("session".into(), json!(sess));
    event.insert("label".into(), json!(shown));
    event.insert("ts".into(), seconds_value(opts)?);
    if let Some(meta) = meta {
        event.insert("meta".into(), meta);
    }
    let open = hook(opts, TAB_OPEN_FILE);
    // `except OSError: pass`.
    let (home, now) = (opts.home.clone(), (opts.clock)());
    let _ = blocking(move || {
        let event = Value::Object(event);
        let body = comandos_core::json::response_dumps(&event)
            .map_err(|e| RegistryError::Io(io::Error::other(e)))?;
        comandos_store::domains::commands::publish(
            &home,
            TAB_OPEN_FILE,
            body.as_bytes(),
            now,
            || Ok(write_json_atomic(&open, &event)?),
        )
        .map_err(|e| RegistryError::Io(io::Error::other(e)))
    })
    .await;
    Ok(())
}

/// El item de `remember_tab` (5223); `None` si la sesión no vale.
fn history_item(
    opts: &NativeOptions,
    sess: &str,
    label: Option<&str>,
    cwd: &str,
    agent: &str,
    reason: &str,
) -> Option<Map<String, Value>> {
    if !py::is_session(sess) {
        return None;
    }
    let mut item = Map::new();
    item.insert("session".into(), json!(sess));
    item.insert("label".into(), json!(label_or(label, sess)));
    item.insert(
        "cwd".into(),
        json!(if cwd.starts_with('/') { cwd } else { "" }),
    );
    let agent = if agent.is_empty() { "claude" } else { agent };
    item.insert("agent".into(), json!(py::take_chars(agent, 16)));
    item.insert("reason".into(), json!(py::take_chars(reason, 32)));
    // `int(time.time())`: trunca hacia cero (un reloj no finito solo existe en
    // pruebas; satura como cualquier conversión de Rust).
    item.insert("ts".into(), json!(((opts.clock_seconds)()).trunc() as i64));
    Some(item)
}

/// El cuerpo de `remember_tab` con el `flock` del historial ya tomado.
fn remember_locked(
    doc: &DomainDocument,
    access: &comandos_store::domains::caller::CallerAccess,
    sess: &str,
    item: Map<String, Value>,
) -> Result<(), RegistryError> {
    let history = light::value_from(doc.strict_under(access))
        .and_then(light::tab_history_from)
        .map_err(|fault| match fault {
            Fault::Decline => RegistryError::Unsure(doc.file.clone()),
            other => RegistryError::Fault(other),
        })?;
    let mut out = vec![Value::Object(item)];
    out.extend(
        history
            .into_iter()
            .filter(|h| h.get("session").and_then(Value::as_str) != Some(sess))
            .take(79)
            .map(Value::Object),
    );
    write_document(doc, access, &Value::Array(out))
}

/// `remember_tab(sess, label, cwd, agent, reason)` (5223): la sesión entra al
/// principio del historial (80 como mucho), sin duplicados.
pub async fn remember_tab(
    native: &Native,
    sess: &str,
    label: Option<&str>,
    cwd: &str,
    agent: &str,
    reason: &str,
) -> Result<(), RegistryError> {
    let opts = native.options();
    let Some(item) = history_item(opts, sess, label, cwd, agent, reason) else {
        return Ok(());
    };
    let sess = sess.to_owned();
    with_document(opts.clone(), TAB_HISTORY_FILE, move |doc, access| {
        remember_locked(doc, access, &sess, item)
    })
    .await
}

/// `tabs_map.pop(sess)` de `close_app_tab` con el `flock` del espejo tomado:
/// escribe solo si estaba.
fn unmirror_locked(
    doc: &DomainDocument,
    access: &comandos_store::domains::caller::CallerAccess,
    sess: &str,
) -> Result<(), RegistryError> {
    if let Value::Object(mut tabs) = load_document(doc, Some(access), json!({}))?
        && tabs.shift_remove(sess).is_some()
    {
        write_document(doc, access, &Value::Object(tabs))?;
    }
    Ok(())
}

/// El cuerpo de `remove_tab_metadata` con `meta_lock` tomado.
fn unmeta_locked(
    doc: &DomainDocument,
    access: &comandos_store::domains::caller::CallerAccess,
    sess: &str,
) -> Result<(), RegistryError> {
    let mut metadata = metadata_from(load_document(doc, Some(access), json!({}))?)?;
    // `pop`: el resto conserva su orden.
    if metadata.shift_remove(sess).is_some() {
        write_document(doc, access, &Value::Object(metadata))?;
    }
    Ok(())
}

/// `app-tab-close.json` de un cierre; `except Exception: pass`.
fn write_close_event(opts: &NativeOptions, sess: &str) {
    if let Ok(ts) = seconds_value(opts) {
        let event = json!({"session": sess, "ts": ts});
        if let Ok(body) = comandos_core::json::response_dumps(&event) {
            let _ = comandos_store::domains::commands::publish(
                &opts.home,
                TAB_CLOSE_FILE,
                body.as_bytes(),
                (opts.clock)(),
                || Ok(write_json_atomic(&hook(opts, TAB_CLOSE_FILE), &event)?),
            );
        }
    }
}

/// Lo que `close_app_tab` lee antes de tocar nada: `tab_labels()` y
/// `read_tab_history()` de `session_labels()`, `state_agent(sess)` y, para que
/// un `app-tabs-meta.json` incierto sea un 500 ANTES de cualquier escritura,
/// los metadatos que `remove_tab_metadata` leerá después.
struct CloseReads {
    tabs: Vec<(String, String)>,
    history: Vec<Map<String, Value>>,
    agent: String,
}

fn unsure_in(opts: &NativeOptions, name: &str) -> impl FnOnce(Fault) -> RegistryError {
    let path = hook(opts, name);
    move |fault: Fault| match fault {
        Fault::Decline => RegistryError::Unsure(path),
        other => RegistryError::Fault(other),
    }
}

fn close_reads(opts: &NativeOptions, sess: &str) -> Result<CloseReads, RegistryError> {
    let tabs =
        light::tab_labels_domain(&opts.home, &opts.hooks).map_err(unsure_in(opts, TABS_FILE))?;
    let history = light::read_tab_history_domain(&opts.home, &opts.hooks)
        .map_err(unsure_in(opts, TAB_HISTORY_FILE))?;
    let agent = target::state_agent_domain(&opts.home, &opts.hooks.join("state"), sess)?;
    read_metadata_domain(opts, None)?;
    Ok(CloseReads {
        tabs,
        history,
        agent,
    })
}

/// Una efímera no lee el historial: solo se adelantan las lecturas del espejo
/// y de los metadatos (el mismo motivo que en `close_reads`).
fn ephemeral_reads(opts: &NativeOptions) -> Result<(), RegistryError> {
    load_document(&domain_document(opts, TABS_FILE)?, None, json!({}))?;
    read_metadata_domain(opts, None)?;
    Ok(())
}

/// `session_labels().get(sess, sess)`.
fn close_label(reads: CloseReads, live: &HashSet<String>, sess: &str) -> String {
    session_labels(reads.tabs, live, &reads.history)
        .get(sess)
        .cloned()
        .unwrap_or_else(|| sess.to_owned())
}

/// `display-message -p -t =<s>: #{pane_current_path}` del cierre.
fn cwd_args(sess: &str) -> [String; 5] {
    [
        "display-message".into(),
        "-p".into(),
        "-t".into(),
        format!("={sess}:"),
        "#{pane_current_path}".into(),
    ]
}

/// `close_app_tab(sess, ephemeral)` (5350): `Some(error)` si no se cierra;
/// `None` tras quitarla del espejo y de los metadatos, recordarla en el
/// historial (salvo efímeras), sincronizar el workspace y avisar a la app.
pub async fn close_app_tab(
    native: &Native,
    sess: &str,
    ephemeral: bool,
) -> Result<Option<String>, RegistryError> {
    if sess == "local" {
        return Ok(Some("La pestaña local permanece abierta".into()));
    }
    if ephemeral && !sess.starts_with("comandos-e2e-") {
        return Ok(Some("ephemeral requiere comandos-e2e-".into()));
    }
    let opts = native.options();
    let (owned, reads_opts) = (sess.to_owned(), opts.clone());
    let reads = blocking(move || {
        if ephemeral {
            ephemeral_reads(&reads_opts).map(|()| None)
        } else {
            close_reads(&reads_opts, &owned).map(Some)
        }
    })
    .await?;
    if let Some(reads) = reads {
        let agent = reads.agent.clone();
        // `session_labels()`: `tmux_sessions()` entre las dos lecturas.
        let live = light::tmux_sessions(&opts.tmux).await?;
        let label = close_label(reads, &live, sess);
        let args = cwd_args(sess);
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        let out = opts
            .tmux
            .run(&args)
            .await
            .map_err(|e| Fault::Error(e.uncaught()))?;
        let cwd = if out.ok {
            py::strip(&out.stdout).to_owned()
        } else {
            String::new()
        };
        remember_tab(native, sess, Some(&label), &cwd, &agent, "closed").await?;
    }
    let owned = sess.to_owned();
    with_document(opts.clone(), TABS_FILE, move |doc, access| {
        unmirror_locked(doc, access, &owned)
    })
    .await?;
    remove_tab_metadata(native, sess).await?;
    sync_after_close(native, sess).await;
    let (owned, event_opts) = (sess.to_owned(), opts.clone());
    let _ = blocking(move || {
        write_close_event(&event_opts, &owned);
        Ok(())
    })
    .await;
    Ok(None)
}

/// `workspace_sync(reason="user")` del cierre: un fallo se anota en stderr y
/// el cierre sigue, como el `except Exception` del Python. Lo que el frente no
/// reproduce con certeza (un `Decline` del worker o de la sincronización)
/// sigue el mismo camino: la siguiente `workspace_sync` (cualquier GET
/// `/workspace`) ajusta el documento al registro, que ya no tiene la pestaña.
async fn sync_after_close(native: &Native, sess: &str) {
    let hooks = native.options().hooks.clone();
    let home = native.options().home.clone();
    let now_seconds = (native.options().clock)() as f64 / 1000.0;
    let synced = native
        .with_state(move |b| workspace::sync_domain(b, &home, &hooks, now_seconds, "user"))
        .await;
    match synced {
        Ok(Ok(_)) => {}
        Ok(Err(fault)) | Err(fault) => {
            let what = match fault {
                Fault::Decline => "no reproducible en el frente",
                Fault::Error(HandlerError::Timeout) => "tiempo agotado",
                Fault::Error(_) => "error interno",
            };
            eprintln!("workspace close {sess}: {what}");
        }
    }
}

/// Las mismas funciones sin `spawn_blocking`, para el trabajo del worker de
/// app-state de POST `/workspace/close-group` (2f-1/T2): `close_group` necesita
/// la conexión y llama a `close_app_tab` en medio, así que todo corre en un
/// trabajo del worker. tmux va por `Tmux::run_blocking` (lo conduce el hilo del
/// runtime, como en `terminal.rs`); la sincronización del workspace, sobre el
/// mismo `StateBackend`.
///
/// Candados desde el hilo del worker: el `flock` de `<archivo>.lock` con
/// sondeo y plazo (`LOCK_WAIT`, como las rutas: un dueño colgado no retiene la
/// base para siempre) y `meta_lock` con `blocking_lock`. Sin ciclo posible:
/// ningún camino asíncrono retiene un `flock` o `meta_lock` mientras espera
/// al worker (los trabajos de bloqueo los sueltan al terminar y
/// `sync_after_close` corre sin ninguno).
pub mod blocking {
    use super::{
        CloseReads, RegistryError, TAB_HISTORY_FILE, TABS_FILE, TABS_META_FILE, close_label,
        close_reads, cwd_args, ephemeral_reads, history_item, meta_lock, remember_locked,
        unmeta_locked, unmirror_locked, with_document_sync, write_close_event,
    };
    use crate::HandlerError;
    use crate::dash::native::{
        Fault, NativeOptions, py,
        state::StateBackend,
        tmux::{Output, TmuxError},
        workspace,
    };
    use std::collections::HashSet;
    use tokio::runtime::Handle;

    /// Por qué un cierre dentro de `close_group` no terminó.
    pub enum Failure {
        /// Una excepción que `close_app_tab` lanza en el Python y que
        /// `close_group` captura (`error = str(exc)`): su texto.
        Caught(String),
        /// Lo que el frente no reproduce (ruling 2): la ruta responde 500/504.
        Registry(RegistryError),
    }

    impl From<RegistryError> for Failure {
        fn from(error: RegistryError) -> Self {
            Failure::Registry(error)
        }
    }

    impl From<Fault> for Failure {
        fn from(fault: Fault) -> Self {
            Failure::Registry(RegistryError::Fault(fault))
        }
    }

    /// `remember_tab` (5223).
    pub fn remember_tab(
        opts: &NativeOptions,
        sess: &str,
        label: Option<&str>,
        cwd: &str,
        agent: &str,
        reason: &str,
    ) -> Result<(), RegistryError> {
        let Some(item) = history_item(opts, sess, label, cwd, agent, reason) else {
            return Ok(());
        };
        with_document_sync(opts, TAB_HISTORY_FILE, |doc, access| {
            remember_locked(doc, access, sess, item)
        })
    }

    /// `remove_tab_metadata` (5289).
    pub fn remove_tab_metadata(opts: &NativeOptions, sess: &str) -> Result<(), RegistryError> {
        let _guard = meta_lock().blocking_lock();
        with_document_sync(opts, TABS_META_FILE, |doc, access| {
            unmeta_locked(doc, access, sess)
        })
    }

    /// `tmux(...)` del Python desde el worker: `TimeoutExpired` y
    /// `FileNotFoundError` con el texto que `close_group` guardaría; lo demás
    /// no se reproduce.
    fn tmux(opts: &NativeOptions, handle: &Handle, args: &[&str]) -> Result<Output, Failure> {
        opts.tmux
            .run_blocking(handle, args)
            .map_err(|error: TmuxError| match error.python_message() {
                Some(message) => Failure::Caught(message),
                None => Fault::Error(error.uncaught()).into(),
            })
    }

    /// `close_app_tab(sess, ephemeral)` (5350), igual que la asíncrona: mismas
    /// lecturas antes de escribir, mismas órdenes de tmux en el mismo orden.
    pub fn close_app_tab(
        backend: &StateBackend,
        opts: &NativeOptions,
        handle: &Handle,
        sess: &str,
        ephemeral: bool,
    ) -> Result<Option<String>, Failure> {
        if sess == "local" {
            return Ok(Some("La pestaña local permanece abierta".into()));
        }
        if ephemeral && !sess.starts_with("comandos-e2e-") {
            return Ok(Some("ephemeral requiere comandos-e2e-".into()));
        }
        if ephemeral {
            ephemeral_reads(opts)?;
        } else {
            let reads: CloseReads = close_reads(opts, sess)?;
            let agent = reads.agent.clone();
            // `tmux_sessions()` de `session_labels()`.
            let listed = tmux(opts, handle, &["list-sessions", "-F", "#{session_name}"])?;
            let live: HashSet<String> = if listed.ok {
                py::splitlines(&listed.stdout)
                    .into_iter()
                    .filter(|l| !l.is_empty())
                    .map(str::to_owned)
                    .collect()
            } else {
                HashSet::new()
            };
            let label = close_label(reads, &live, sess);
            let args = cwd_args(sess);
            let args: Vec<&str> = args.iter().map(String::as_str).collect();
            let out = tmux(opts, handle, &args)?;
            let cwd = if out.ok {
                py::strip(&out.stdout).to_owned()
            } else {
                String::new()
            };
            remember_tab(opts, sess, Some(&label), &cwd, &agent, "closed")?;
        }
        with_document_sync(opts, TABS_FILE, |doc, access| {
            unmirror_locked(doc, access, sess)
        })?;
        remove_tab_metadata(opts, sess)?;
        // `workspace_sync(reason="user")` con `except Exception: print`.
        let now_seconds = (opts.clock)() as f64 / 1000.0;
        if let Err(fault) =
            workspace::sync_domain(backend, &opts.home, &opts.hooks, now_seconds, "user")
        {
            let what = match fault {
                Fault::Decline => "no reproducible en el frente",
                Fault::Error(HandlerError::Timeout) => "tiempo agotado",
                Fault::Error(_) => "error interno",
            };
            eprintln!("workspace close {sess}: {what}");
        }
        write_close_event(opts, sess);
        Ok(None)
    }
}
