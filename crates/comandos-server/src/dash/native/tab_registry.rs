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
    files::{FileLock, LOCK_WAIT, Strict, read_json_strict, write_json_atomic},
    light, py,
    states::gather::session_labels,
    target, workspace,
};
use comandos_runtime::ssh_config;
use serde_json::{Map, Value, json};
use std::{
    io,
    path::{Path, PathBuf},
    sync::OnceLock,
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
fn meta_lock() -> &'static tokio::sync::Mutex<()> {
    static LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| tokio::sync::Mutex::new(()))
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

/// `with file_lock(path): job()`. Camino rápido: el candado libre se toma en el
/// mismo hilo que hace el trabajo (un salto). Si otro lo tiene (cc-app, el
/// Python), se espera en la cola de `acquire_timeout` y el trabajo va en un
/// segundo salto con el candado ya tomado.
async fn with_file_lock<T, F>(path: PathBuf, job: F) -> Result<T, RegistryError>
where
    F: FnOnce() -> Result<T, RegistryError> + Send + 'static,
    T: Send + 'static,
{
    enum First<T, F> {
        Done(Result<T, RegistryError>),
        Busy(F),
    }
    let target = path.clone();
    let first = blocking(move || match FileLock::try_acquire(&target) {
        Ok(Some(lock)) => {
            let out = job();
            drop(lock);
            Ok(First::Done(out))
        }
        Ok(None) => Ok(First::Busy(job)),
        Err(error) => Err(RegistryError::Io(error)),
    })
    .await?;
    let job = match first {
        First::Done(out) => return out,
        First::Busy(job) => job,
    };
    let lock = FileLock::acquire_timeout(&path, LOCK_WAIT)
        .await
        .map_err(RegistryError::Io)?;
    blocking(move || {
        let out = job();
        drop(lock);
        out
    })
    .await
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
    let data = load_object(path)?;
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
    let path = hook(native.options(), TABS_META_FILE);
    let item = meta_item(kind, host, cwd);
    let (sess, written) = (sess.to_owned(), item.clone());
    let _guard = meta_lock().lock().await;
    blocking(move || {
        let mut metadata = read_tab_metadata(&path)?;
        metadata.insert(sess, written);
        write_json_atomic(&path, &Value::Object(metadata)).map_err(RegistryError::Io)
    })
    .await?;
    Ok(Some(item))
}

/// `remove_tab_metadata(sess)` (5289): escribe solo si la clave estaba.
pub async fn remove_tab_metadata(native: &Native, sess: &str) -> Result<(), RegistryError> {
    let path = hook(native.options(), TABS_META_FILE);
    let sess = sess.to_owned();
    let _guard = meta_lock().lock().await;
    blocking(move || {
        let mut metadata = read_tab_metadata(&path)?;
        // `pop`: el resto conserva su orden.
        if metadata.shift_remove(&sess).is_some() {
            write_json_atomic(&path, &Value::Object(metadata)).map_err(RegistryError::Io)?;
        }
        Ok(())
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
fn derived_metadata(opts: &NativeOptions, sess: &str) -> Result<Value, RegistryError> {
    let path = hook(opts, TABS_META_FILE);
    if let Some(saved) = read_tab_metadata(&path)?.get(sess) {
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
    blocking(move || derived_metadata(&opts, &sess)).await
}

/// `write_app_tab(sess, label)` (5239): `tabs[sess] = label or sess`.
pub async fn write_app_tab(native: &Native, sess: &str, label: &str) -> Result<(), RegistryError> {
    let path = hook(native.options(), TABS_FILE);
    let (sess, label) = (sess.to_owned(), label.to_owned());
    let target = path.clone();
    with_file_lock(path, move || {
        let mut tabs = load_object(&target)?;
        let label = if label.is_empty() {
            sess.clone()
        } else {
            label
        };
        tabs.insert(sess, json!(label));
        write_json_atomic(&target, &Value::Object(tabs)).map_err(RegistryError::Io)
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
    let path = hook(opts, TABS_FILE);
    let shown = label_or(label, sess);
    let (owned, mirrored, target) = (sess.to_owned(), shown.clone(), path.clone());
    with_file_lock(path, move || {
        let mut tabs = load_object(&target)?;
        if !tabs.contains_key(&owned) {
            tabs.insert(owned, json!(mirrored));
            write_json_atomic(&target, &Value::Object(tabs)).map_err(RegistryError::Io)?;
        }
        Ok(())
    })
    .await?;
    let mut meta = if TAB_KINDS.contains(&kind) {
        write_tab_metadata(native, sess, kind, host, cwd).await?
    } else {
        Some(tab_metadata_for_session(native, sess).await?)
    };
    if let Some(found) = meta.clone() {
        let meta_path = hook(opts, TABS_META_FILE);
        let owned = sess.to_owned();
        let saved =
            blocking(move || Ok(read_tab_metadata(&meta_path)?.contains_key(&owned))).await?;
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
    let _ = blocking(move || {
        write_json_atomic(&open, &Value::Object(event)).map_err(RegistryError::Io)
    })
    .await;
    Ok(())
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
    if !py::is_session(sess) {
        return Ok(());
    }
    let opts = native.options();
    let ts = (opts.clock_seconds)();
    if !ts.is_finite() {
        return Err(Fault::Error(HandlerError::Failure).into());
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
    // `int(time.time())`: trunca hacia cero.
    item.insert("ts".into(), json!(ts.trunc() as i64));
    let hooks = opts.hooks.clone();
    let path = hook(opts, TAB_HISTORY_FILE);
    let (sess, target) = (sess.to_owned(), path.clone());
    with_file_lock(path, move || {
        let history = light::read_tab_history(&hooks).map_err(|fault| match fault {
            Fault::Decline => RegistryError::Unsure(target.clone()),
            other => RegistryError::Fault(other),
        })?;
        let mut out = vec![Value::Object(item)];
        out.extend(
            history
                .into_iter()
                .filter(|h| h.get("session").and_then(Value::as_str) != Some(sess.as_str()))
                .take(79)
                .map(Value::Object),
        );
        write_json_atomic(&target, &Value::Array(out)).map_err(RegistryError::Io)
    })
    .await
}

/// Lo que `close_app_tab` lee antes de tocar nada: `tab_labels()` y
/// `read_tab_history()` de `session_labels()` y `state_agent(sess)`.
struct CloseReads {
    tabs: Vec<(String, String)>,
    history: Vec<Map<String, Value>>,
    agent: String,
}

fn close_reads(opts: &NativeOptions, sess: &str) -> Result<CloseReads, RegistryError> {
    let unsure = |name: &str| {
        let path = hook(opts, name);
        move |fault: Fault| match fault {
            Fault::Decline => RegistryError::Unsure(path),
            other => RegistryError::Fault(other),
        }
    };
    let tabs = light::tab_labels(&opts.hooks).map_err(unsure(TABS_FILE))?;
    let history = light::read_tab_history(&opts.hooks).map_err(unsure(TAB_HISTORY_FILE))?;
    let agent = target::state_agent(&opts.hooks.join("state"), sess)?;
    Ok(CloseReads {
        tabs,
        history,
        agent,
    })
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
    if !ephemeral {
        let (owned, reads_opts) = (sess.to_owned(), opts.clone());
        let reads = blocking(move || close_reads(&reads_opts, &owned)).await?;
        // `session_labels()`: `tmux_sessions()` entre las dos lecturas.
        let live = light::tmux_sessions(&opts.tmux).await?;
        let labels = session_labels(reads.tabs, &live, &reads.history);
        let label = labels.get(sess).cloned().unwrap_or_else(|| sess.to_owned());
        let out = opts
            .tmux
            .run(&[
                "display-message",
                "-p",
                "-t",
                &format!("={sess}:"),
                "#{pane_current_path}",
            ])
            .await
            .map_err(|e| Fault::Error(e.uncaught()))?;
        let cwd = if out.ok {
            py::strip(&out.stdout).to_owned()
        } else {
            String::new()
        };
        remember_tab(native, sess, Some(&label), &cwd, &reads.agent, "closed").await?;
    }
    let path = hook(opts, TABS_FILE);
    let (owned, target) = (sess.to_owned(), path.clone());
    with_file_lock(path, move || {
        if let Value::Object(mut tabs) = load_json_file(&target, json!({}))?
            && tabs.shift_remove(&owned).is_some()
        {
            write_json_atomic(&target, &Value::Object(tabs)).map_err(RegistryError::Io)?;
        }
        Ok(())
    })
    .await?;
    remove_tab_metadata(native, sess).await?;
    sync_after_close(native, sess).await;
    let event = json!({"session": sess, "ts": seconds_value(opts)?});
    let close = hook(opts, TAB_CLOSE_FILE);
    // `except Exception: pass`.
    let _ = blocking(move || write_json_atomic(&close, &event).map_err(RegistryError::Io)).await;
    Ok(None)
}

/// `workspace_sync(reason="user")` del cierre: un fallo se anota en stderr y
/// el cierre sigue, como el `except Exception` del Python. Lo que el frente no
/// reproduce con certeza (un `Decline` del worker o de la sincronización)
/// sigue el mismo camino: la siguiente `workspace_sync` (cualquier GET
/// `/workspace`) ajusta el documento al registro, que ya no tiene la pestaña.
async fn sync_after_close(native: &Native, sess: &str) {
    let hooks = native.options().hooks.clone();
    let now_seconds = (native.options().clock)() as f64 / 1000.0;
    let synced = native
        .with_state(move |b| workspace::sync_with_reason(b, &hooks, now_seconds, "user"))
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
