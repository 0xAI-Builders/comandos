//! Corte `tabs` (plan 2f-1, Tarea 2): registro y cierre de pestañas.
//! POST `/tab-register` (9503), `/tab-metadata` (9516), `/tab-metadata-remove`
//! (9525), `/tab-close` (9529) y `/workspace/close-group` (8725) de
//! `bin/cc-dash`; el registro en sí es `tab_registry` (Tarea 1).
//!
//! Las cuatro primeras van tras el preámbulo de `do_POST` (`session` validada
//! con `SESSION_RE.match`) sin `resolve_project_session`.
//!
//! Sin `Decline` tras leer el registro (ruling 2 del sub-plan): lo único que
//! declina es un `str()` que el frente no reproduce (un flotante o un
//! contenedor verdadero en `label`/`kind`/`host`/`cwd`), antes de cualquier
//! lectura o efecto. Lo demás incierto es un 500 con una línea en stderr.
//!
//! Cancelación (regla de la revisión de la T1): cada petición corre en su
//! propia tarea (`tokio::spawn`). Si el cliente se va, o vence el plazo del
//! manejador, el cierre termina igual: nunca queda a medias (historial escrito
//! y espejo sin tocar, o un cierre de grupo sin su señal agregada).
//!
//! Efectos en vivo (los mismos que el Python, en el mismo orden): ninguna
//! orden de tmux que mute. `/tab-register` hace `has-session -t =<s>`; los
//! cierres, `list-sessions -F #{session_name}` y `display-message -p -t =<s>:
//! #{pane_current_path}`; el cierre de grupo, además, `display-message -p -t
//! =<s>: #{session_id}` por miembro. Cerrar una pestaña nunca mata su sesión.
use super::{
    Answer, Entry, Fault, Key, Native, NativeOptions, NativeRoute, Verb, light, py, reply,
    state::StateBackend, tmux::Tmux, workspace,
};
use crate::{HandlerError, Request};
use comandos_core::{
    json::{response_dumps, truthy},
    workspace::{CloseGroupState, WorkspaceError, close_group as close_group_core},
};
use comandos_store::workspace::WorkspaceStore;
use http::StatusCode;
use serde_json::{Map, Value, json};
use std::{cell::RefCell, sync::Arc};
use tab_registry::blocking::Failure;
use tokio::runtime::Handle;

/// El registro de pestañas (Tarea 1): declarado aquí para no tocar `native/mod.rs`.
#[path = "tab_registry.rs"]
pub mod tab_registry;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TabsRoute {
    Register,
    Metadata,
    MetadataRemove,
    Close,
    CloseGroup,
}

impl TabsRoute {
    /// La ruta del Python (`self.path == …`).
    pub const fn path(self) -> &'static str {
        match self {
            TabsRoute::Register => "/tab-register",
            TabsRoute::Metadata => "/tab-metadata",
            TabsRoute::MetadataRemove => "/tab-metadata-remove",
            TabsRoute::Close => "/tab-close",
            TabsRoute::CloseGroup => "/workspace/close-group",
        }
    }
}

const fn entry(route: TabsRoute) -> Entry {
    Entry {
        verb: Verb::Post,
        key: Key::Raw(route.path()),
        route: NativeRoute::Tabs(route),
    }
}

pub const ROUTES: &[Entry] = &[
    entry(TabsRoute::Register),
    entry(TabsRoute::Metadata),
    entry(TabsRoute::MetadataRemove),
    entry(TabsRoute::Close),
    entry(TabsRoute::CloseGroup),
];

fn failure() -> Fault {
    Fault::Error(HandlerError::Failure)
}

/// Ruling 2: dentro de estas rutas un `Decline` tras leer el registro es un
/// 500 con su línea en stderr (el Python lo habría hecho él mismo, escribiendo
/// `app-tabs-meta.json`, del que el frente es el único dueño).
fn no_decline(path: &str, fault: Fault) -> Fault {
    match fault {
        Fault::Decline => {
            eprintln!("comandos dash: registro de pestañas incierto; {path} responde 500");
            failure()
        }
        other => other,
    }
}

pub async fn answer(native: &Arc<Native>, route: TabsRoute, request: &Request) -> Answer {
    let data = light::data(request)?.clone();
    let job = tokio::spawn({
        let native = Arc::clone(native);
        async move { run(&native, route, &data).await }
    });
    job.await.map_err(|_| failure())?
}

async fn run(native: &Native, route: TabsRoute, data: &Map<String, Value>) -> Answer {
    if route == TabsRoute::CloseGroup {
        return close_group(native, data).await;
    }
    // `data.get("session", "")` sin `str()`: un no-texto en `SESSION_RE.match`
    // es un `TypeError` (500).
    let sess = match data.get("session") {
        None => "",
        Some(Value::String(s)) => s.as_str(),
        Some(_) => return Err(failure()),
    };
    if !py::is_session(sess) {
        return reply(
            StatusCode::BAD_REQUEST,
            &json!({"error": "Nombre de sesion invalido"}),
        );
    }
    let path = route.path();
    // Los `str()` van antes de toda lectura: su `Decline` es el único que sale.
    let texts = match route {
        TabsRoute::Register => Some((
            text_or(data.get("label"), sess)?,
            text_or(data.get("kind"), "")?,
            text_or(data.get("host"), "")?,
            text_or(data.get("cwd"), "")?,
        )),
        TabsRoute::Metadata => Some((
            String::new(),
            text_or(data.get("kind"), "")?,
            text_or(data.get("host"), "")?,
            text_or(data.get("cwd"), "")?,
        )),
        _ => None,
    };
    let (label, kind, host, cwd) = texts.unwrap_or_default();
    let answered = match route {
        TabsRoute::Register => register(native, sess, &label, &kind, &host, &cwd).await,
        TabsRoute::Metadata => {
            match tab_registry::write_tab_metadata(native, sess, &kind, &host, &cwd)
                .await
                .map_err(|e| e.into_fault(path))?
            {
                None => reply(
                    StatusCode::BAD_REQUEST,
                    &json!({"error": "metadata de pestana invalida"}),
                ),
                Some(item) => reply(StatusCode::OK, &json!({"ok": true, "metadata": item})),
            }
        }
        TabsRoute::MetadataRemove => {
            tab_registry::remove_tab_metadata(native, sess)
                .await
                .map_err(|e| e.into_fault(path))?;
            reply(StatusCode::OK, &json!({"ok": true}))
        }
        TabsRoute::Close => {
            let ephemeral = matches!(data.get("ephemeral"), Some(Value::Bool(true)));
            match tab_registry::close_app_tab(native, sess, ephemeral)
                .await
                .map_err(|e| e.into_fault(path))?
            {
                Some(error) => reply(StatusCode::BAD_REQUEST, &json!({"error": error})),
                None => reply(StatusCode::OK, &json!({"ok": true})),
            }
        }
        TabsRoute::CloseGroup => Err(failure()),
    };
    answered.map_err(|fault| no_decline(path, fault))
}

/// `str(value or fallback)`: un valor falso cae a `fallback`; un escalar
/// verdadero da su `str()`; un flotante o un contenedor verdadero declina
/// (su `repr` no se reproduce con certeza). Va antes de toda lectura.
fn text_or(value: Option<&Value>, fallback: &str) -> Result<String, Fault> {
    match value {
        Some(value) if truthy(value) => py::str_scalar(value).ok_or(Fault::Decline),
        _ => Ok(fallback.to_owned()),
    }
}

/// POST `/tab-register` (9503). El `SESSION_RE.match` repetido de la rama no
/// puede fallar (el preámbulo ya lo hizo).
async fn register(
    native: &Native,
    sess: &str,
    label: &str,
    kind: &str,
    host: &str,
    cwd: &str,
) -> Answer {
    let target = format!("={sess}");
    let out = native
        .options()
        .tmux
        .run(&["has-session", "-t", &target])
        .await
        .map_err(|e| Fault::Error(e.uncaught()))?;
    if !out.ok {
        return reply(
            StatusCode::NOT_FOUND,
            &json!({"error": format!("'{sess}' no existe")}),
        );
    }
    let label = py::take_chars(label, 80);
    tab_registry::register_app_tab(native, sess, Some(&label), kind, host, cwd)
        .await
        .map_err(|e| e.into_fault(TabsRoute::Register.path()))?;
    reply(StatusCode::OK, &json!({"ok": true}))
}

/// `isinstance(v, int) and not isinstance(v, bool)` sobre el número crudo.
fn is_int(value: Option<&Value>) -> bool {
    matches!(value, Some(Value::Number(n)) if !n.as_str().contains(['.', 'e', 'E'])
        && !matches!(n.as_str(), "NaN" | "Infinity" | "-Infinity"))
}

/// Lo que sale del trabajo del worker.
enum CloseOutcome {
    Done(Value),
    /// `Conflict(current)` con `workspace_payload(current)`.
    Conflict(Value),
    /// `ValueError`/`TypeError`: `str(exc)`.
    Invalid(String),
}

/// POST `/workspace/close-group` (8725).
async fn close_group(native: &Native, data: &Map<String, Value>) -> Answer {
    const PATH: &str = "/workspace/close-group";
    if !is_int(data.get("expectedRevision"))
        || !matches!(data.get("members"), Some(Value::Array(_)))
    {
        return reply(
            StatusCode::BAD_REQUEST,
            &json!({"error": "Solicitud de cierre inválida"}),
        );
    }
    let opts = native.options().clone();
    let handle = Handle::current();
    let request = data.clone();
    // Un `Decline` de `with_state` (base apagada o worker retirado) es de
    // antes de leer nada: se reenvía como el resto de rutas de la base.
    let outcome = native
        .with_state(move |backend| close_group_job(backend, &opts, &handle, &request))
        .await?
        .map_err(|fault| no_decline(PATH, fault))?;
    match outcome {
        CloseOutcome::Done(result) => {
            let closed = result.get("closed").cloned().unwrap_or(Value::Null);
            if truthy(&closed) && !result.get("replayed").is_some_and(truthy) {
                write_close_signal(native.options(), closed).await;
            }
            reply(StatusCode::OK, &result)
        }
        CloseOutcome::Conflict(current) => reply(
            StatusCode::CONFLICT,
            &json!({
                "error": "El workspace cambió. Revisa la lista antes de cerrar",
                "current": current,
            }),
        ),
        CloseOutcome::Invalid(message) => {
            reply(StatusCode::BAD_REQUEST, &json!({"error": message}))
        }
    }
}

/// La señal agregada a la app (`app-tab-close.json`): `except Exception: pass`.
async fn write_close_signal(opts: &NativeOptions, closed: Value) {
    let path = opts.hooks.join(tab_registry::TAB_CLOSE_FILE);
    let ts = serde_json::Number::from_f64((opts.clock_seconds)()).map(Value::Number);
    let Some(last) = closed.as_array().and_then(|c| c.last()).cloned() else {
        return;
    };
    let Some(ts) = ts else {
        return;
    };
    let signal = json!({"session": last, "sessions": closed, "ts": ts});
    let _ =
        tokio::task::spawn_blocking(move || super::files::write_json_atomic(&path, &signal)).await;
}

/// `workspace_payload(state)` sobre el JSON de `CloseGroupState::current`.
fn payload_of(state: &Value) -> Result<Value, Fault> {
    let mut doc = state
        .get("document")
        .and_then(Value::as_object)
        .cloned()
        .ok_or_else(failure)?;
    doc.insert(
        "revision".into(),
        state.get("revision").cloned().unwrap_or(Value::Null),
    );
    doc.insert("ready".into(), json!(true));
    Ok(Value::Object(doc))
}

/// `workspace_session_identity(sess)` (6439) desde el worker. Las excepciones
/// de tmux no se capturan (500/504).
fn session_identity(tmux: &Tmux, handle: &Handle, sess: &str) -> Result<Value, Fault> {
    if sess == "local" {
        return Ok(json!("local"));
    }
    if !py::is_session(sess) {
        return Ok(Value::Null);
    }
    let target = format!("={sess}:");
    let out = tmux
        .run_blocking(
            handle,
            &["display-message", "-p", "-t", &target, "#{session_id}"],
        )
        .map_err(|e| Fault::Error(e.uncaught()))?;
    let value = if out.ok { py::strip(&out.stdout) } else { "" };
    // `re.fullmatch(r"\$\d+", value)`: el `\d` de Python casa dígitos Unicode.
    if !value.is_ascii() {
        return Err(Fault::Decline);
    }
    let is_id = value
        .strip_prefix('$')
        .is_some_and(|d| !d.is_empty() && d.bytes().all(|b| b.is_ascii_digit()));
    Ok(if is_id { json!(value) } else { Value::Null })
}

/// `CloseGroupState` sobre la conexión del worker que no guarda el resultado
/// si un cierre se abortó (ruling 2): el Python habría guardado un error que
/// el frente no sabe escribir.
struct Store<'a> {
    inner: WorkspaceStore<'a>,
    abort: &'a RefCell<Option<Fault>>,
}

impl CloseGroupState for Store<'_> {
    fn current(&mut self) -> comandos_core::workspace::Result<Option<Value>> {
        CloseGroupState::current(&mut self.inner)
    }
    fn meta(&mut self, key: &str) -> comandos_core::workspace::Result<Option<Value>> {
        CloseGroupState::meta(&mut self.inner, key)
    }
    /// `store.set_meta(key, json.dumps(result))`: el texto del Python (orden de
    /// inserción, ASCII escapado, separadores por omisión), no la forma
    /// canónica ordenada del adaptador de `comandos-store`. La repetición lo
    /// devuelve tal cual y el Python leería el que escriba el frente.
    fn set_meta(&mut self, key: &str, value: &Value) -> comandos_core::workspace::Result<()> {
        if self.abort.borrow().is_some() {
            return Err(WorkspaceError::Callback("cierre abortado".into()));
        }
        let text = response_dumps(value).map_err(WorkspaceError::Callback)?;
        WorkspaceStore::set_meta(&self.inner, key, &text)
            .map_err(|e| WorkspaceError::Callback(e.to_string()))
    }
}

/// Primer miembro (objeto) cuyo `tabId` no es hashable en el Python: el
/// `{m.get("tabId"): m …}` lanza `TypeError: unhashable type: '<tipo>'`.
fn unhashable_tab_id(members: &Value) -> Option<&'static str> {
    members
        .as_array()?
        .iter()
        .filter_map(Value::as_object)
        .find_map(|m| match m.get("tabId") {
            Some(Value::Array(_)) => Some("list"),
            Some(Value::Object(_)) => Some("dict"),
            _ => None,
        })
}

/// `member.get('label') or session` del mensaje «… cambió»: el Python hace
/// `str()` de cualquier valor verdadero. Los escalares seguros se pasan ya
/// como texto (el núcleo solo lee texto); devuelve si queda alguno exótico
/// (flotante o contenedor verdadero).
fn normalize_labels(members: &mut Value) -> bool {
    let mut exotic = false;
    let Some(list) = members.as_array_mut() else {
        return false;
    };
    for member in list.iter_mut().filter_map(Value::as_object_mut) {
        let Some(label) = member.get_mut("label") else {
            continue;
        };
        if label.is_string() || !truthy(label) {
            continue;
        }
        match py::str_scalar(label) {
            Some(text) => *label = Value::String(text),
            None => exotic = true,
        }
    }
    exotic
}

/// `workspace_sync()` + `workspace_state.close_group(...)` en un trabajo del
/// worker de app-state.
fn close_group_job(
    backend: &mut StateBackend,
    opts: &NativeOptions,
    handle: &Handle,
    data: &Map<String, Value>,
) -> Result<CloseOutcome, Fault> {
    let now_seconds = (opts.clock)() as f64 / 1000.0;
    workspace::sync(backend, &opts.hooks, now_seconds)?;
    let backend: &StateBackend = backend;
    let text = |key: &str| match data.get(key) {
        Some(Value::String(s)) => s.clone(),
        // Un `groupId` que no es texto nunca casa un grupo (sus ids son texto
        // no vacío) y un `requestId` que no es texto falla `_ident` igual que "".
        _ => String::new(),
    };
    let (group_id, request_id) = (text("groupId"), text("requestId"));
    let expected = data.get("expectedRevision").cloned().unwrap_or(Value::Null);
    let mut members = data.get("members").cloned().unwrap_or(Value::Null);
    let abort: RefCell<Option<Fault>> = RefCell::new(None);
    let mut store = Store {
        inner: WorkspaceStore::new(&backend.conn),
        abort: &abort,
    };
    if let Some(kind) = unhashable_tab_id(&members) {
        // El Python lanza el `TypeError` tras `_ident`, la repetición guardada
        // y el `Conflict`, pero antes de buscar el grupo: con un grupo que no
        // existe el núcleo hace esos tres pasos y se para sin cerrar nada.
        let result = close_group_core(
            &mut store,
            "",
            &expected,
            &json!([]),
            &request_id,
            |_| Value::Null,
            |_| Ok(None),
        );
        return match result {
            Err(WorkspaceError::Invalid(message)) if message == "El grupo ya no existe" => {
                Ok(CloseOutcome::Invalid(format!("unhashable type: '{kind}'")))
            }
            other => settle(other),
        };
    }
    let exotic = normalize_labels(&mut members);
    let result = close_group_core(
        &mut store,
        &group_id,
        &expected,
        &members,
        &request_id,
        |sess| {
            if abort.borrow().is_some() {
                return Value::Null;
            }
            match session_identity(&opts.tmux, handle, sess) {
                Ok(id) => id,
                Err(fault) => {
                    *abort.borrow_mut() = Some(fault);
                    Value::Null
                }
            }
        },
        |sess| match tab_registry::blocking::close_app_tab(backend, opts, handle, sess, false) {
            Ok(error) => Ok(error),
            Err(Failure::Caught(message)) => Err(message),
            Err(Failure::Registry(error)) => {
                *abort.borrow_mut() = Some(error.into_fault(TabsRoute::CloseGroup.path()));
                Err("cierre abortado".into())
            }
        },
    );
    if let Some(fault) = abort.borrow_mut().take() {
        return Err(fault);
    }
    if exotic
        && let Err(WorkspaceError::Invalid(message)) = &result
        && message.ends_with("cambió. No se ha cerrado nada")
    {
        eprintln!(
            "comandos dash: etiqueta de miembro no reproducible; /workspace/close-group responde 500"
        );
        return Err(failure());
    }
    settle(result)
}

fn settle(result: comandos_core::workspace::Result<Value>) -> Result<CloseOutcome, Fault> {
    match result {
        Ok(value) => Ok(CloseOutcome::Done(value)),
        Err(WorkspaceError::Conflict(Some(state))) => {
            Ok(CloseOutcome::Conflict(payload_of(&state)?))
        }
        // `workspace_payload(None)` revienta dentro del `except` (500).
        Err(WorkspaceError::Conflict(None)) => Err(failure()),
        Err(WorkspaceError::Invalid(message)) => Ok(CloseOutcome::Invalid(message)),
        // Errores de la base: excepciones de `sqlite3` sin capturar.
        Err(WorkspaceError::Callback(_)) => Err(failure()),
    }
}
