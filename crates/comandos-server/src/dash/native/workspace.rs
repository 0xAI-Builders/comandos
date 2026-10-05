//! C. Workspace (`bin/cc-dash`: `workspace_store` 6329, `_tab_registry` 6348,
//! `workspace_inventory` 6361, `workspace_panes` 6373, `workspace_sort` 6379,
//! `workspace_sync` 6415, `workspace_session_identity` 6439, `workspace_payload`
//! 6448; GET 8487, 8530, 8539; POST 8662, 8665, 8751).
//!
//! `workspace_sync` corre entera en el worker; la identidad de sesión (tmux)
//! fuera. POST /workspace/close-group sigue en el Python (`close_app_tab`).
//!
//! POST /workspace/sort en modo `by` (Fase 2d) calcula `/state` con la caché
//! del frente ANTES de su trabajo en la base (excepción 3a de los rulings: la
//! escritura de `app-tab-models.json` es idempotente). El Python sincroniza
//! antes de leer el estado y lo relee en cada reintento; aquí se lee una vez
//! (diferencia aceptada, A8 del preflight).
//!
//! Sobre declinar: `workspace_sync` puede confirmar una revisión (efecto en un
//! GET) con el `requestId` determinista del Python (`sync-<rev>-<sha256[:24]>`).
//! Si tras ella se declina, reenviar es seguro porque esa revisión no depende
//! de la petición (solo del registro, las preferencias y el snapshot) y
//! `reconcile` es idempotente: la `workspace_sync` del Python que recibe la
//! petición ve `wanted == document` y no confirma nada.
use super::{
    Answer, Entry, Fault, Key, Native, NativeRoute, Verb,
    files::{self, Strict},
    light::{HIDDEN_SESSIONS, data, error, favorites_set, ordered_tab_keys, read_prefs},
    py,
    query::Query,
    reply,
    state::StateBackend,
    states::{PyFloat, py_float},
    tmux::Tmux,
};
use crate::{HandlerError, Request};
use comandos_core::{
    json::{python_eq, truthy, workspace_dumps, workspace_dumps_with_options},
    workspace::{
        close_group_preview, empty_document,
        layout::{restore_order, sort_groups},
        pane_bindings, reconcile,
        snapshot::{Snapshot, check_snapshot},
        validate_document,
    },
};
use comandos_store::workspace::{Error as WsError, WorkspaceState, WorkspaceStore};
use http::StatusCode;
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{HashMap, HashSet, hash_map::Entry::Vacant},
    path::{Path, PathBuf},
    sync::Arc,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkspaceRoute {
    Get,
    ClosePreview,
    ClientGet,
    Save,
    Sort,
    ClientSave,
}

const fn entry(verb: Verb, key: Key, route: WorkspaceRoute) -> Entry {
    Entry {
        verb,
        key,
        route: NativeRoute::Workspace(route),
    }
}

/// POST `/workspace/close-group` NO está: cierra pestañas (`close_app_tab`).
pub const ROUTES: &[Entry] = &[
    entry(Verb::Get, Key::Path("/workspace"), WorkspaceRoute::Get),
    entry(
        Verb::Get,
        Key::Path("/workspace/close-group"),
        WorkspaceRoute::ClosePreview,
    ),
    entry(
        Verb::Get,
        Key::Path("/workspace/client"),
        WorkspaceRoute::ClientGet,
    ),
    entry(Verb::Post, Key::Raw("/workspace"), WorkspaceRoute::Save),
    entry(
        Verb::Post,
        Key::Raw("/workspace/sort"),
        WorkspaceRoute::Sort,
    ),
    entry(
        Verb::Post,
        Key::Raw("/workspace/client"),
        WorkspaceRoute::ClientSave,
    ),
];

fn failure() -> Fault {
    Fault::Error(HandlerError::Failure)
}

/// `workspace_store()`: fase `ready` desde el primer uso.
fn store(conn: &rusqlite::Connection) -> Result<WorkspaceStore<'_>, Fault> {
    let mut store = WorkspaceStore::new(conn);
    store.set_phase("ready").map_err(|_| failure())?;
    Ok(store)
}

/// `read_snapshot` (`lib/tmux_snapshot.py` 244): el archivo, luego su `.bak`,
/// el primero que pase `valid_snapshot`; si ninguno, vacío. Lo que el Python
/// lee distinto (bytes no UTF-8, anidamiento, snapshots exóticos) declina.
pub fn read_snapshot(path: &Path) -> Result<Value, Fault> {
    let backup = PathBuf::from(format!("{}.bak", path.display()));
    for candidate in [path.to_path_buf(), backup] {
        match files::read_json_strict(&candidate) {
            // `except (OSError, ValueError, AttributeError): pass`.
            Strict::Missing | Strict::Unreadable => {}
            Strict::Unsure => return Err(Fault::Decline),
            Strict::Value(data) => match check_snapshot(&data) {
                Snapshot::Valid => return Ok(data),
                Snapshot::Exotic => return Err(Fault::Decline),
                Snapshot::Invalid => {}
            },
        }
    }
    Ok(json!({"version": 2, "sessions": {}}))
}

/// `(sesión, etiqueta)` en el orden de `workspace_inventory`.
type Inventory = Vec<(String, Option<String>)>;

/// `workspace_inventory` (6361); `None` = `_RegistryUnreadable`.
fn inventory(hooks: &Path) -> Result<Option<Inventory>, Fault> {
    let labels: Vec<(String, String)> = match files::read_json_strict(&hooks.join("app-tabs.json"))
    {
        Strict::Missing => Vec::new(),
        Strict::Value(Value::Object(map)) => map
            .into_iter()
            .filter_map(|(k, v)| match v {
                Value::String(s) if !s.is_empty() => Some((k, s)),
                _ => None,
            })
            .collect(),
        // JSON roto o no-objeto: `_RegistryUnreadable`.
        Strict::Value(_) | Strict::Unreadable => return Ok(None),
        Strict::Unsure => return Err(Fault::Decline),
    };
    let favorites = favorites_set(&read_prefs(hooks)?)?;
    let mut out = vec![("local".to_owned(), Some("⌂ local".to_owned()))];
    for (sess, label) in ordered_tab_keys(&labels, &favorites) {
        if !HIDDEN_SESSIONS.contains(&sess.as_str()) && py::is_session(sess) {
            out.push((sess.clone(), Some(label.clone())));
        }
    }
    Ok(Some(out))
}

/// `(pane.get("acp") or {}).get("sessionId")` sobre un `acp` verdadero que no
/// es objeto: AttributeError no capturado en el Python (500). El core no lo
/// reproduce; aquí se detecta antes de confirmar nada.
fn acp_breaks(session: &Value) -> bool {
    let Some(windows) = session.get("windows").and_then(Value::as_array) else {
        return false;
    };
    windows
        .iter()
        .filter_map(|w| w.get("panes").and_then(Value::as_array))
        .flatten()
        .any(|pane| {
            pane.get("key")
                .and_then(Value::as_str)
                .is_some_and(|k| !k.is_empty())
                && !pane.get("resume_id").is_some_and(truthy)
                && pane
                    .get("acp")
                    .is_some_and(|acp| truthy(acp) && !acp.is_object())
        })
}

/// `workspace_panes` (6373).
fn panes(
    hooks: &Path,
    inventory: &[(String, Option<String>)],
) -> Result<HashMap<String, Vec<(String, Value)>>, Fault> {
    let snapshot = read_snapshot(&hooks.join("app-sessions-v2.json"))?;
    let sessions = &snapshot["sessions"];
    let mut out = HashMap::new();
    for (tab, _) in inventory {
        let session = sessions.get(tab).unwrap_or(&Value::Null);
        if acp_breaks(session) {
            return Err(failure());
        }
        let bindings = pane_bindings(session).map_err(|_| failure())?;
        out.insert(tab.clone(), bindings);
    }
    Ok(out)
}

/// `hashlib.sha256(json.dumps(wanted, sort_keys=True).encode()).hexdigest()[:24]`.
fn sync_request_id(revision: i64, wanted: &Value) -> Result<String, Fault> {
    let encoded = workspace_dumps_with_options(wanted, true, false).map_err(|_| Fault::Decline)?;
    let digest: String = Sha256::digest(encoded.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    let short = digest.get(..24).ok_or(Fault::Decline)?;
    Ok(format!("sync-{revision}-{short}"))
}

/// `workspace_sync` (6415). Solo declina antes de su commit.
pub fn sync(
    backend: &StateBackend,
    hooks: &Path,
    now_seconds: f64,
) -> Result<WorkspaceState, Fault> {
    let store = store(&backend.conn)?;
    for _ in 0..3 {
        let current = store
            .current()
            .map_err(|_| failure())?
            .unwrap_or_else(|| WorkspaceState {
                revision: 0,
                document: empty_document(),
                recovered: false,
            });
        let Some(inventory) = inventory(hooks)? else {
            return Ok(current);
        };
        let panes = panes(hooks, &inventory)?;
        // Un ValueError de reconcile no se captura en el Python: 500.
        let wanted = reconcile(&current.document, &inventory, &panes).map_err(|_| failure())?;
        if current.revision != 0 && python_eq(&wanted, &current.document) {
            return Ok(current);
        }
        let request_id = sync_request_id(current.revision, &wanted)?;
        match store.commit(
            &json!(current.revision),
            &wanted,
            &request_id,
            "auto",
            now_seconds,
        ) {
            Ok(saved) => return Ok(saved),
            Err(WsError::Conflict { .. }) => continue,
            Err(WsError::EmptyInventory | WsError::NotReady(_)) => return Ok(current),
            Err(_) => return Err(failure()),
        }
    }
    // `return store.current()`: un None revienta después en el Python (500).
    store.current().map_err(|_| failure())?.ok_or_else(failure)
}

/// `workspace_payload` (6448): el documento + `revision` + `ready`.
pub fn payload(state: &WorkspaceState) -> Value {
    let mut doc = state.document.as_object().cloned().unwrap_or_default();
    doc.insert("revision".into(), json!(state.revision));
    doc.insert("ready".into(), json!(true));
    Value::Object(doc)
}

/// `{"error": str(exc) or fallback}`.
fn message_or(error: impl ToString, fallback: &str) -> Value {
    let message = error.to_string();
    json!({"error": if message.is_empty() { fallback.to_owned() } else { message }})
}

/// `isinstance(v, int) and not isinstance(v, bool)`.
fn is_int(value: Option<&Value>) -> bool {
    matches!(value, Some(Value::Number(n)) if !n.as_str().contains(['.', 'e', 'E'])
        && !matches!(n.as_str(), "NaN" | "Infinity" | "-Infinity"))
}

/// `secrets.token_hex(12)`.
fn token_hex12() -> Result<String, Fault> {
    let mut bytes = [0u8; 12];
    getrandom::fill(&mut bytes).map_err(|_| failure())?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

/// `workspace_session_identity` (6439). Las excepciones de tmux no se capturan.
async fn session_identity(tmux: &Tmux, sess: &str) -> Result<Value, Fault> {
    if sess == "local" {
        return Ok(json!("local"));
    }
    if !py::is_session(sess) {
        return Ok(Value::Null);
    }
    let out = tmux
        .run(&[
            "display-message",
            "-p",
            "-t",
            &format!("={sess}:"),
            "#{session_id}",
        ])
        .await
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

/// Anidamiento a partir del que el `json.dumps` de CPython agota la recursión
/// (`RecursionError`, 500 sin escribir) y el core responde otra cosa.
const MAX_CLIENT_DEPTH: usize = 900;

/// Profundidad de contenedores de un valor, sin recursión.
fn depth(value: &Value) -> usize {
    let mut deepest = 0;
    let mut stack = vec![(value, 0usize)];
    while let Some((v, d)) = stack.pop() {
        let children: Box<dyn Iterator<Item = &Value>> = match v {
            Value::Array(items) => Box::new(items.iter()),
            Value::Object(map) => Box::new(map.values()),
            _ => continue,
        };
        deepest = deepest.max(d + 1);
        stack.extend(children.map(|c| (c, d + 1)));
    }
    deepest
}

/// `updatedAt` que el `sorted(...)` de `_patch` compara como el core: ausente
/// (vale 0), booleano o número finito. Texto, null, listas o NaN harían que
/// el Python compare tipos mezclados (`TypeError`, 500) u ordene distinto.
fn comparable_stamp(entry: &Value) -> bool {
    match entry.get("updatedAt") {
        None | Some(Value::Bool(_)) => true,
        Some(Value::Number(n)) => n.as_f64().is_some_and(f64::is_finite),
        Some(_) => false,
    }
}

/// `save_client` sobre `drafts`/`readingAnchors` que el Python trata distinto
/// del core: `dict(x or {})` de un no-objeto verdadero (lista de pares, texto,
/// número), anidamiento extremo y, con un parche, entradas no-objeto
/// (`target[k].get` revienta) o con `updatedAt` no comparable.
fn client_state_exotic(state: &Map<String, Value>, previous: &Value) -> bool {
    [
        ("drafts", "draftsPatch"),
        ("readingAnchors", "anchorsPatch"),
    ]
    .into_iter()
    .any(|(key, patch)| {
        let selected = state.get(key).or_else(|| previous.get(key));
        match selected {
            Some(v) if depth(v) > MAX_CLIENT_DEPTH => true,
            Some(v) if truthy(v) && !v.is_object() => true,
            Some(Value::Object(entries)) if state.contains_key(patch) => entries
                .values()
                .any(|e| !e.is_object() || !comparable_stamp(e)),
            _ => false,
        }
    })
}

/// `[g["id"] for g in doc.get("groups", [])]`, fuera del `try`: un grupo
/// sin `id` es un `KeyError` no capturado (500).
fn group_ids(document: &Value) -> Result<Vec<Value>, Fault> {
    match document.get("groups").and_then(Value::as_array) {
        None => Ok(Vec::new()),
        Some(groups) => groups
            .iter()
            .map(|g| g.get("id").cloned().ok_or_else(failure))
            .collect(),
    }
}

/// `_tab_registry` (6348) dentro del `try` de `workspace_sort`: ausente → vacío;
/// no-objeto → 400 con su texto. JSON roto u otro `OSError` → 400 con el texto
/// de Python, que no se reproduce: declina.
fn sort_registry(hooks: &Path) -> Result<Result<HashMap<String, String>, &'static str>, Fault> {
    match files::read_json_strict(&hooks.join("app-tabs.json")) {
        Strict::Missing => Ok(Ok(HashMap::new())),
        Strict::Value(Value::Object(map)) => Ok(Ok(map
            .into_iter()
            .filter_map(|(k, v)| match v {
                Value::String(s) if !s.is_empty() => Some((k, s)),
                _ => None,
            })
            .collect())),
        Strict::Value(_) => Ok(Err("app-tabs.json no es un objeto")),
        Strict::Unreadable | Strict::Unsure => Err(Fault::Decline),
    }
}

/// `set((read_prefs() or {}).get("favorites") or [])`. Un `TypeError` (no
/// iterable, elementos no hashables) es un 400 con texto de Python: declina;
/// iterar un texto o un objeto también.
fn sort_favorites(prefs: &Map<String, Value>) -> Result<HashSet<String>, Fault> {
    match prefs.get("favorites") {
        Some(v) if truthy(v) => match v {
            Value::Array(items) => {
                let mut set = HashSet::new();
                for item in items {
                    match item {
                        Value::String(s) => {
                            set.insert(s.clone());
                        }
                        Value::Array(_) | Value::Object(_) => return Err(Fault::Decline),
                        // Números, booleanos y None nunca igualan a una pestaña.
                        _ => {}
                    }
                }
                Ok(set)
            }
            _ => Err(Fault::Decline),
        },
        _ => Ok(HashSet::new()),
    }
}

/// El `info` de `workspace_sort` (6390-6402): `activeAt`/`need` por sesión de
/// las tarjetas de `/state` y `label`/`fav` por pestaña del documento.
fn sort_info(
    items: &[Value],
    document: &Value,
    labels: &HashMap<String, String>,
    favorites: &HashSet<String>,
) -> Result<Value, Fault> {
    let mut info: Map<String, Value> = Map::new();
    for row in items {
        // Las tarjetas siempre son objetos; otra cosa sería un AttributeError.
        let row = row.as_object().ok_or(Fault::Decline)?;
        let Some(sess) = row.get("session").filter(|v| truthy(v)) else {
            continue;
        };
        // `info.setdefault(sess, …)` con una lista u objeto: TypeError (400).
        if sess.is_array() || sess.is_object() {
            return Err(Fault::Decline);
        }
        // `float(row.get("ts") or 0)`: ValueError/TypeError serían un 400 con
        // texto de Python (las tarjetas ya traen `ts` validado): declina.
        let ts = match row.get("ts").filter(|v| truthy(v)) {
            None => 0.0,
            Some(v) => match py_float(v) {
                PyFloat::Value(f) => f,
                PyFloat::Raises | PyFloat::Unsure => return Err(Fault::Decline),
            },
        };
        let waiting = row.get("status").and_then(Value::as_str) == Some("waiting");
        // Una sesión numérica nunca coincide con un id de pestaña (texto).
        let Value::String(sess) = sess else {
            continue;
        };
        let cur = info
            .entry(sess.clone())
            .or_insert_with(|| json!({"activeAt": 0, "need": false}))
            .as_object_mut()
            .ok_or(Fault::Decline)?;
        let previous = cur.get("activeAt").and_then(Value::as_f64).unwrap_or(0.0);
        // `max(cur, ts)`: solo cambia si `ts > cur` (NaN nunca).
        if ts > previous {
            let number = serde_json::Number::from_f64(ts).ok_or(Fault::Decline)?;
            cur.insert("activeAt".into(), Value::Number(number));
        }
        if waiting {
            cur.insert("need".into(), json!(true));
        }
    }
    match document.get("tabs") {
        None => {}
        Some(Value::Object(tabs)) => {
            for tab in tabs.keys() {
                let cur = info
                    .entry(tab.clone())
                    .or_insert_with(|| json!({"activeAt": 0, "need": false}))
                    .as_object_mut()
                    .ok_or(Fault::Decline)?;
                cur.insert("label".into(), json!(labels.get(tab).unwrap_or(tab)));
                cur.insert("fav".into(), json!(favorites.contains(tab)));
            }
        }
        // Iterar otra cosa: TypeError o claves de Python; no ocurre con un
        // documento validado.
        Some(_) => return Err(Fault::Decline),
    }
    Ok(Value::Object(info))
}

/// El resto de `workspace_sort` en modo `by`, dentro del worker.
fn sort_by(b: &mut StateBackend, hooks: &Path, now_seconds: f64, by: &str, items: &[Value]) -> Job {
    for _ in 0..3 {
        let current = sync(b, hooks, now_seconds)?;
        let store = store(&b.conn)?;
        let previous = group_ids(&current.document)?;
        let labels = match sort_registry(hooks)? {
            Ok(labels) => labels,
            Err(message) => return Ok((StatusCode::BAD_REQUEST, json!({"error": message}))),
        };
        let favorites = sort_favorites(&read_prefs(hooks)?)?;
        let info = sort_info(items, &current.document, &labels, &favorites)?;
        let wanted = match sort_groups(&current.document, by, &info) {
            Ok(w) => w,
            Err(e) => return Ok((StatusCode::BAD_REQUEST, message_or(e, "Orden inválido"))),
        };
        match commit_sorted(&store, &current, &wanted, previous, now_seconds)? {
            Some(answer) => return Ok(answer),
            None => continue,
        }
    }
    Ok((
        StatusCode::CONFLICT,
        json!({"error": "El acomodo cambió mientras ordenaba; intenta de nuevo"}),
    ))
}

/// `store.commit(..., secrets.token_hex(12), reason="user")` de los dos modos
/// de `workspace_sort`; `None` = `Conflict` (reintentar).
fn commit_sorted(
    store: &WorkspaceStore<'_>,
    current: &WorkspaceState,
    wanted: &Value,
    previous: Vec<Value>,
    now_seconds: f64,
) -> Result<Option<(StatusCode, Value)>, Fault> {
    match store.commit(
        &json!(current.revision),
        wanted,
        &token_hex12()?,
        "user",
        now_seconds,
    ) {
        Ok(saved) => {
            let mut body = payload(&saved);
            if let Some(map) = body.as_object_mut() {
                map.insert("previous".into(), Value::Array(previous));
            }
            Ok(Some((StatusCode::OK, body)))
        }
        Err(WsError::Conflict { .. }) => Ok(None),
        Err(WsError::Invalid(m)) => Ok(Some((
            StatusCode::BAD_REQUEST,
            message_or(m, "Orden inválido"),
        ))),
        Err(_) => Err(failure()),
    }
}

type Job = Result<(StatusCode, Value), Fault>;

async fn run(
    native: &Native,
    job: impl FnOnce(&mut StateBackend) -> Job + Send + 'static,
) -> Answer {
    let (status, body) = native.with_state(job).await??;
    reply(status, &body)
}

pub async fn answer(native: &Native, route: WorkspaceRoute, request: &Request) -> Answer {
    let hooks = native.options().hooks.clone();
    let now_seconds = (native.options().clock)() as f64 / 1000.0;
    match route {
        WorkspaceRoute::Get => {
            run(native, move |b| {
                Ok((StatusCode::OK, payload(&sync(b, &hooks, now_seconds)?)))
            })
            .await
        }
        WorkspaceRoute::ClosePreview => {
            let query = Query::parse(&request.target)?;
            let group = query.first("groupId").unwrap_or("").to_owned();
            let state = native
                .with_state(move |b| sync(b, &hooks, now_seconds))
                .await??;
            // Primer pase, puro: qué sesiones preguntaría el Python, en orden,
            // y si es un 404 (sin tocar tmux).
            let mut asked: Vec<String> = Vec::new();
            if let Err(e) = close_group_preview(&state.document, &group, |sess| {
                asked.push(sess.to_owned());
                Value::Null
            }) {
                return error(StatusCode::NOT_FOUND, &e.to_string());
            }
            let mut identities: HashMap<String, Value> = HashMap::new();
            for sess in asked {
                if let Vacant(slot) = identities.entry(sess) {
                    let id = session_identity(&native.options().tmux, slot.key()).await?;
                    slot.insert(id);
                }
            }
            let preview = close_group_preview(&state.document, &group, |sess| {
                identities.get(sess).cloned().unwrap_or(Value::Null)
            })
            .map_err(|_| failure())?;
            let mut out: Map<String, Value> = preview.as_object().cloned().ok_or_else(failure)?;
            out.insert("revision".into(), json!(state.revision));
            reply(StatusCode::OK, &Value::Object(out))
        }
        WorkspaceRoute::ClientGet => {
            let query = Query::parse(&request.target)?;
            let device = query.first("deviceId").unwrap_or("").to_owned();
            run(native, move |b| match store(&b.conn)?.client(&device) {
                Ok(Some(state)) if truthy(&state) => Ok((StatusCode::OK, state)),
                Ok(_) => Ok((StatusCode::OK, json!({}))),
                // `json.loads` de un estado roto: el texto del ValueError es de Python.
                Err(WsError::Json(_)) => Err(Fault::Decline),
                Err(_) => Err(failure()),
            })
            .await
        }
        WorkspaceRoute::Save => {
            let data = data(request)?.clone();
            if !is_int(data.get("expectedRevision")) {
                return error(StatusCode::BAD_REQUEST, "expectedRevision inválido");
            }
            let document = data.get("document").cloned().unwrap_or(Value::Null);
            // Lo que el codificador portado no escribe (anidamiento, números) el
            // Python lo resuelve a su manera: se declina antes de sincronizar.
            if workspace_dumps(&document).is_err() {
                return Err(Fault::Decline);
            }
            run(native, move |b| {
                let current = sync(b, &hooks, now_seconds)?;
                let store = store(&b.conn)?;
                if let Err(e) = validate_document(&document) {
                    return Ok((StatusCode::BAD_REQUEST, message_or(e, "Workspace inválido")));
                }
                let tabs = |d: &Value| -> Option<HashSet<String>> {
                    d.get("tabs")
                        .and_then(Value::as_object)
                        .map(|m| m.keys().cloned().collect())
                };
                if tabs(&document) != tabs(&current.document) {
                    return Ok((
                        StatusCode::BAD_REQUEST,
                        json!({"error": "La distribución no coincide con las tabs abiertas"}),
                    ));
                }
                // `_ident(request_id)` de un no-str: el mismo 400 que el store.
                let Some(Value::String(request_id)) = data.get("requestId") else {
                    return Ok((
                        StatusCode::BAD_REQUEST,
                        json!({"error": "requestId inválido"}),
                    ));
                };
                let expected = data.get("expectedRevision").cloned().unwrap_or(Value::Null);
                match store.commit(&expected, &document, request_id, "user", now_seconds) {
                    Ok(saved) => Ok((StatusCode::OK, payload(&saved))),
                    Err(WsError::Conflict {
                        current: Some(c),
                        message,
                    }) => Ok((
                        StatusCode::CONFLICT,
                        json!({"error": message, "current": payload(&c)}),
                    )),
                    Err(WsError::Invalid(m)) => {
                        Ok((StatusCode::BAD_REQUEST, message_or(m, "Workspace inválido")))
                    }
                    // `workspace_payload(None)` dentro del except: 500.
                    Err(_) => Err(failure()),
                }
            })
            .await
        }
        WorkspaceRoute::Sort => {
            let data = data(request)?;
            let Some(Value::Array(restore)) = data.get("restore") else {
                // Modo `by`: `data.get("by")` va tal cual a `sort_groups`; sin
                // él (`None`) es «Orden desconocido». Otro tipo: el texto del
                // TypeError o la pertenencia a SORTS de Python; se declina.
                let by = match data.get("by") {
                    None | Some(Value::Null) => "<sin orden>".to_owned(),
                    Some(Value::String(s)) => s.clone(),
                    Some(_) => return Err(Fault::Decline),
                };
                // `read_states_cached()` dentro del `try` (A8): el 400 con el
                // texto de la excepción no se reproduce (declina); el plazo
                // vencido de tmux no se captura (504).
                let states = match native.states_cached().await {
                    Ok(states) => states,
                    Err(Fault::Error(HandlerError::Timeout)) => {
                        return Err(Fault::Error(HandlerError::Timeout));
                    }
                    Err(_) => return Err(Fault::Decline),
                };
                let items: Arc<Vec<Value>> = states.items.clone();
                // El instante tras `/state`, que puede tardar segundos.
                let now_seconds = (native.options().clock)() as f64 / 1000.0;
                return run(native, move |b| {
                    sort_by(b, &hooks, now_seconds, &by, &items)
                })
                .await;
            };
            // `str(x)` de un no-str: repr de Python, se declina.
            let ids: Vec<String> = restore
                .iter()
                .map(|v| v.as_str().map(str::to_owned))
                .collect::<Option<_>>()
                .ok_or(Fault::Decline)?;
            run(native, move |b| {
                for _ in 0..3 {
                    let current = sync(b, &hooks, now_seconds)?;
                    let store = store(&b.conn)?;
                    let previous = group_ids(&current.document)?;
                    let wanted = match restore_order(&current.document, &ids) {
                        Ok(w) => w,
                        Err(e) => {
                            return Ok((StatusCode::BAD_REQUEST, message_or(e, "Orden inválido")));
                        }
                    };
                    match commit_sorted(&store, &current, &wanted, previous, now_seconds)? {
                        Some(answer) => return Ok(answer),
                        None => continue,
                    }
                }
                Ok((
                    StatusCode::CONFLICT,
                    json!({"error": "El acomodo cambió mientras ordenaba; intenta de nuevo"}),
                ))
            })
            .await
        }
        WorkspaceRoute::ClientSave => {
            let data = data(request)?.clone();
            let Some(Value::String(device)) = data.get("deviceId").cloned() else {
                return error(StatusCode::BAD_REQUEST, "deviceId inválido");
            };
            run(native, move |b| {
                let store = store(&b.conn)?;
                // Se lee lo guardado antes de escribir para declinar sin efectos.
                let previous = match store.client(&device) {
                    Ok(previous) => previous.unwrap_or_else(|| json!({})),
                    Err(WsError::Json(_)) => return Err(Fault::Decline),
                    Err(_) => return Err(failure()),
                };
                if client_state_exotic(&data, &previous) {
                    return Err(Fault::Decline);
                }
                match store.save_client(&device, &Value::Object(data), now_seconds) {
                    Ok(state) => Ok((StatusCode::OK, state)),
                    Err(WsError::Invalid(m)) => Ok((StatusCode::BAD_REQUEST, json!({"error": m}))),
                    Err(_) => Err(failure()),
                }
            })
            .await
        }
    }
}
