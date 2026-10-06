//! Estante de extensiones: identidad exacta, borradores y operaciones durables.
use super::super::{Answer, Fault, Native, NativeOptions, light, py, query::Query, reply};
use super::{OpsRoute, configure};
use crate::{HandlerError, Request};
use comandos_core::json::{python_eq, truthy};
use comandos_runtime::{
    capabilities::Paths,
    extension_launch::{self as launch, LaunchError},
    extension_observations,
    pane_extensions::{self as drafts, ExtensionStore},
    pane_snapshot::{PaneInspector, PaneRef},
    session_configuration::{self as sc, Env, Proc},
    session_operations::{self as ops, OperationStore},
};
use http::StatusCode;
use rusqlite::Connection;
use serde_json::{Map, Value, json};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Save,
    Apply,
    Template,
    Cancel,
    Recover,
}
impl Action {
    pub const fn path(self) -> &'static str {
        match self {
            Self::Save => "/pane-extensions",
            Self::Apply => "/pane-extensions/apply",
            Self::Template => "/pane-extensions/template",
            Self::Cancel => "/pane-extensions/cancel",
            Self::Recover => "/pane-extensions/recover",
        }
    }
    fn fields(self) -> &'static [&'static str] {
        match self {
            Self::Save => &["desired"],
            Self::Apply => &["requestId", "interrupt"],
            Self::Template => &["name", "templateId"],
            Self::Cancel | Self::Recover => &["operationId"],
        }
    }
}
const COMMON: &[&str] = &[
    "session",
    "pane",
    "harness",
    "expectedIdentity",
    "expectedConversationId",
    "revision",
];
const TERMINAL: &[&str] = &["confirmed", "failed", "rolled_back"];
enum Error {
    Value(String),
    Other,
}
type Result<T> = std::result::Result<T, Error>;
impl From<sc::Fail> for Error {
    fn from(e: sc::Fail) -> Self {
        match e {
            sc::Fail::Py(s) => Self::Value(s),
            sc::Fail::Unsure => Self::Other,
        }
    }
}
impl From<LaunchError> for Error {
    fn from(e: LaunchError) -> Self {
        match e {
            LaunchError::Value(s) => Self::Value(s),
            _ => Self::Other,
        }
    }
}
impl From<drafts::Fault> for Error {
    fn from(e: drafts::Fault) -> Self {
        match e {
            drafts::Fault::Invalid(s) | drafts::Fault::Conflict(s) => Self::Value(s),
            _ => Self::Other,
        }
    }
}
impl From<ops::Error> for Error {
    fn from(e: ops::Error) -> Self {
        match e {
            ops::Error::Conflict(s) => Self::Value(s),
            _ => Self::Other,
        }
    }
}
fn err(s: &str) -> Error {
    Error::Value(s.into())
}
fn text(v: &Value) -> &str {
    v.as_str().unwrap_or("")
}
fn string(v: &Value) -> String {
    comandos_runtime::acp_client::py_str_or_empty(v)
}
fn failure() -> Fault {
    Fault::Error(HandlerError::Failure)
}
fn respond_error(e: Error, get: bool) -> Answer {
    match e {
        Error::Value(s) => reply(
            StatusCode::CONFLICT,
            &json!({"ok":false,"error":if get {"No se pudo identificar el panel y su conversación."}else{&s}}),
        ),
        Error::Other if !get => reply(
            StatusCode::SERVICE_UNAVAILABLE,
            &json!({"ok":false,"error":"No se pudo consultar el estado de extensiones; el panel se conserva."}),
        ),
        Error::Other => Err(failure()),
    }
}
async fn blocking<T: Send + 'static>(f: impl FnOnce() -> Result<T> + Send + 'static) -> Result<T> {
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|_| Error::Other)?
}
fn paths(env: &Env) -> Paths {
    let mut paths = Paths::new(&env.home, &env.cwd);
    paths.env = env
        .environ
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    paths
}
fn with_operations<T>(
    conn: &Connection,
    opts: &NativeOptions,
    f: impl FnOnce(&OperationStore<'_>) -> ops::Result<T>,
) -> Result<T> {
    let owner = || i64::from(std::process::id());
    let clock = || (opts.clock_seconds)();
    let store = OperationStore::new(conn, &owner, &clock)?;
    Ok(f(&store)?)
}
fn canonical(inventory: &Value, desired: &Value) -> Value {
    let mut result = Map::new();
    for kind in ["mcps", "skills"] {
        let rows = inventory[kind]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|r| r["enabled"].is_boolean())
            .filter_map(|r| {
                let id = r["id"].as_str()?;
                Some((
                    id.to_owned(),
                    if truthy(&r["toggleable"]) {
                        desired[kind].get(id).cloned().unwrap_or(false.into())
                    } else {
                        r["enabled"].clone()
                    },
                ))
            })
            .collect();
        result.insert(kind.into(), Value::Object(rows));
    }
    Value::Object(result)
}
struct Target {
    identity: Map<String, Value>,
    info: Option<Proc>,
    harness: String,
    account: String,
    inventory: Value,
    loaded: Value,
    draft: Value,
    conversation: String,
    active: bool,
}
fn valid_target(data: &Value) -> Result<(String, String)> {
    let sess = string(&data["session"]);
    let pane = string(&data["pane"]);
    if !py::is_session(&sess) || py::is_pane(&pane) != Some(true) {
        return Err(err("sesión y panel exactos requeridos"));
    }
    Ok((sess, pane))
}
fn target(env: &Env, opts: &NativeOptions, data: &Value) -> Result<Target> {
    let (sess, pane) = valid_target(data)?;
    let identity = sc::pane_identity(env, &sess, &pane)?;
    let identity_key = sc::identity_key(&identity);
    let conn = configure::open_store(opts).map_err(|_| Error::Other)?;
    let operation = with_operations(&conn, opts, |s| s.latest_for_target(&sess, &pane))?
        .filter(|o| o["pane_key"] == identity_key && !TERMINAL.contains(&text(&o["state"])));
    let info = sc::agent_info_for_pane(env, &pane)?;
    let observed = if info.is_some() {
        Value::Object(sc::observe_pane(env, &sess, &pane, info.as_ref(), None)?)
    } else {
        json!({})
    };
    let harness = info
        .as_ref()
        .map(|i| i.agent.as_str())
        .filter(|s| !s.is_empty())
        .unwrap_or(text(&data["harness"]))
        .to_owned();
    if !["claude", "codex", "grok", "opencode", "agy"].contains(&harness.as_str()) {
        return Err(err("elige un CLI compatible para este panel"));
    }
    if info.is_some() && truthy(&data["harness"]) && data["harness"] != harness {
        return Err(err("el CLI del panel cambió"));
    }
    if info.is_none()
        && !shell(text(
            identity.get("pane_current_command").unwrap_or(&Value::Null),
        ))
        && operation.is_none()
    {
        return Err(err("hay un proceso sin identificar en el panel"));
    }
    let mut account = observed["harnessAccount"]
        .as_str()
        .filter(|s| !s.is_empty())
        .unwrap_or(if info.is_none() { "main" } else { "unknown" })
        .to_owned();
    if account == "unknown"
        && let Some(op) = &operation
    {
        account = op["snapshot"]["destination"]["harnessAccount"]
            .as_str()
            .filter(|s| !s.is_empty())
            .unwrap_or("unknown")
            .into();
    }
    if account == "unknown" {
        return Err(err("no se identificó la cuenta del proceso"));
    }
    let cwd = identity
        .get("pane_current_path")
        .and_then(Value::as_str)
        .ok_or(Error::Other)?;
    let inventory = launch::inventory(
        &env.registry,
        &harness,
        &account,
        Path::new(cwd),
        &paths(env),
    )?;
    let bundle = match info
        .as_ref()
        .and_then(|i| u32::try_from(i.pid).ok())
        .filter(|p| *p != 0)
    {
        Some(pid) => launch::launch_from_pid(pid).map_err(|_| Error::Other)?,
        None => None,
    };
    let loaded = bundle
        .filter(|b| b["harness"] == harness)
        .map(|b| b["selection"].clone())
        .unwrap_or(Value::Null);
    let defaults = if truthy(&loaded) {
        loaded.clone()
    } else {
        Value::Object(
            ["mcps", "skills"]
                .into_iter()
                .map(|kind| {
                    (
                        kind.into(),
                        Value::Object(
                            inventory[kind]
                                .as_array()
                                .into_iter()
                                .flatten()
                                .filter(|r| r["enabled"].is_boolean())
                                .filter_map(|r| {
                                    r["id"].as_str().map(|id| (id.into(), r["enabled"].clone()))
                                })
                                .collect(),
                        ),
                    )
                })
                .collect(),
        )
    };
    let conversation = text(&observed["conversationId"]).to_owned();
    let clock = || (opts.clock_seconds)();
    let store = ExtensionStore::new(&conn, &clock)?;
    let mut draft = if let Some(op) = operation
        .as_ref()
        .filter(|o| truthy(&o["request"]["extensionDraftKey"]))
    {
        store.require_revision(
            text(&op["request"]["extensionDraftKey"]),
            &op["request"]["revision"],
        )?
    } else {
        store.state(&identity_key, &conversation, &harness, &defaults)?
    };
    draft["desired"] = canonical(&inventory, &draft["desired"]);
    Ok(Target {
        identity,
        info,
        harness,
        account,
        inventory,
        loaded,
        draft,
        conversation,
        active: operation.is_some(),
    })
}
fn shell(command: &str) -> bool {
    ["bash", "zsh", "sh", "fish", "dash"].contains(&command)
}
fn source(env: &Env, target: &Target, pane: &str) -> Option<PathBuf> {
    if target.info.is_none() || target.conversation.is_empty() {
        return None;
    }
    let inspector = PaneInspector::new(&env.home, &env.proc_root).ok()?;
    let identity = &target.identity;
    let pid = identity.get("pane_pid")?.as_str()?.parse().ok()?;
    let snap = inspector
        .inspect(&PaneRef {
            id: pane,
            pid,
            command: identity.get("pane_current_command")?.as_str()?,
        })
        .ok()?;
    let source = PathBuf::from(sc::snapshot_transcript(env, &snap).ok()?);
    match target.harness.as_str() {
        "grok" => Some(source.parent()?.join("events.jsonl")),
        "agy" => Some(
            source
                .parent()?
                .parent()?
                .join("brain")
                .join(&target.conversation)
                .join(".system_generated/logs/transcript_full.jsonl"),
        ),
        _ => Some(source),
    }
}
async fn state(native: &Arc<Native>, env: Env, data: Value) -> Result<Value> {
    let opts = native.options().clone();
    let input = data.clone();
    let (mut body, operation) = blocking(move || {
        let t = target(&env, &opts, &input)?;
        let sess = text(&input["session"]);
        let pane = text(&input["pane"]);
        let src = source(&env, &t, pane);
        let usage_inventory = if src.is_some() {
            let cwd = t
                .identity
                .get("pane_current_path")
                .and_then(Value::as_str)
                .unwrap_or("");
            launch::internal_inventory(
                &env.registry,
                &t.harness,
                &t.account,
                Path::new(cwd),
                &paths(&env),
            )
            .map(|(_, v)| v)
            .unwrap_or_else(|_| t.inventory.clone())
        } else {
            t.inventory.clone()
        };
        let usage = extension_observations::conversation_usage(
            &t.harness,
            &t.conversation,
            src.as_deref(),
            &usage_inventory,
            2_000_000,
        );
        let conn = configure::open_store(&opts).map_err(|_| Error::Other)?;
        let operation = with_operations(&conn, &opts, |s| s.latest_for_target(sess, pane))?
            .filter(|o| o["pane_key"] == sc::identity_key(&t.identity));
        let command = t
            .identity
            .get("pane_current_command")
            .and_then(Value::as_str)
            .unwrap_or("");
        let supported = (t.info.is_none() || !t.conversation.is_empty())
            && !t.active
            && (t.info.is_some() || shell(command));
        let clock = || (opts.clock_seconds)();
        let templates = ExtensionStore::new(&conn, &clock)?.templates()?;
        let busy = t.info.is_some() && sc::harness_pane_busy(&env, sess, pane, &t.harness)?;
        let pid = t.info.as_ref().and_then(|i| u32::try_from(i.pid).ok());
        let body = json!({
            "ok": true,
            "session": input["session"],
            "pane": input["pane"],
            "identity": sc::identity_key(&t.identity),
            "conversationId": t.conversation,
            "harness": t.harness,
            "account": t.account,
            "inventory": t.inventory,
            "desired": t.draft["desired"],
            "loaded": t.loaded,
            "revision": t.draft["revision"],
            "configurationStatus": launch::configuration_status(pid, !t.loaded.is_null()),
            "operation": null,
            "usage": usage,
            "templates": templates,
            "busy": busy,
            "applySupported": supported,
            "reason": if supported { "" } else {
                "La conversación exacta todavía no está identificada; el agente sigue abierto."
            },
        });
        Ok((body, operation))
    })
    .await?;
    if let Some(row) = operation {
        let row = configure::refresh_session_confirmation(native, row)
            .await
            .map_err(|_| Error::Other)?;
        let state = text(&row["state"]);
        let result = &row["result"];
        body["operation"] = json!({"id":row["id"],"operationId":row["id"],"state":row["state"],"pending":!TERMINAL.contains(&state),"recoveryAllowed":(["recovery_required","awaiting_confirmation"].contains(&state)),"error":if truthy(&result["error"]){result["error"].clone()}else{json!("")},"rolledBack":truthy(&result["rolledBack"])});
    }
    Ok(body)
}
fn guards(data: &Value, target: &Target) -> Result<()> {
    if data["expectedIdentity"] != sc::identity_key(&target.identity)
        || data.get("expectedConversationId").is_none()
        || data["expectedConversationId"] != target.conversation
    {
        return Err(err(
            "cambió el panel o la conversación; vuelve a cargar el estante",
        ));
    }
    Ok(())
}
fn template_name(row: &Value) -> String {
    if truthy(&row["group"]) {
        format!("plugin-group:{}", text(&row["plugin"]))
    } else if truthy(&row["plugin"]) {
        format!(
            "plugin-skill:{}:{}",
            text(&row["plugin"]),
            text(&row["name"])
        )
    } else if ["project", "local"].contains(&text(&row["scope"])) {
        format!("project:{}", text(&row["name"]))
    } else {
        text(&row["name"]).into()
    }
}
fn request_fields(data: &Value) -> Value {
    Value::Object(
        data.as_object()
            .into_iter()
            .flatten()
            .filter(|(k, _)| {
                COMMON.contains(&k.as_str()) || ["requestId", "interrupt"].contains(&k.as_str())
            })
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect(),
    )
}
enum Next {
    State(Option<Value>),
    Reply(Value),
    Configure(Value),
    Recover(Value),
}
async fn write(
    native: &Arc<Native>,
    env: Env,
    action: Action,
    data: Value,
) -> Result<(StatusCode, Value)> {
    let input = data.clone();
    let opts = native.options().clone();
    let env_for_work = env.clone();
    let next = blocking(move || {
        let t = target(&env_for_work, &opts, &input)?;
        guards(&input, &t)?;
        let conn = configure::open_store(&opts).map_err(|_| Error::Other)?;
        let clock = || (opts.clock_seconds)();
        let store = ExtensionStore::new(&conn, &clock)?;
        let key = text(&t.draft["key"]);
        let revision = &input["revision"];
        match action {
            Action::Save => {
                launch::normalize(&t.inventory, &input["desired"])?;
                store.save(key, revision, &canonical(&t.inventory, &input["desired"]))?;
                Ok(Next::State(None))
            }
            Action::Apply => {
                if t.info.is_some() && t.conversation.is_empty() {
                    return Err(err(
                        "no se identificó la conversación exacta; el agente sigue abierto",
                    ));
                }
                if input.get("interrupt").is_some_and(|v| !v.is_boolean()) {
                    return Err(err("interrupt inválido"));
                }
                let mut request = request_fields(&input);
                request["extensionsOnly"] = true.into();
                request["extensionDraftKey"] = t.draft["key"].clone();
                request["toHarness"] = t.harness.into();
                Ok(Next::Configure(request))
            }
            Action::Template => {
                let mut draft = store.require_revision(key, revision)?;
                draft["desired"] = canonical(&t.inventory, &draft["desired"]);
                if truthy(&input["name"]) == truthy(&input["templateId"]) {
                    return Err(err("indica nombre o plantilla"));
                }
                if truthy(&input["name"]) {
                    let portable = Value::Object(
                        ["mcps", "skills"]
                            .into_iter()
                            .map(|kind| {
                                let rows = t.inventory[kind]
                                    .as_array()
                                    .into_iter()
                                    .flatten()
                                    .filter(|r| truthy(&r["toggleable"]))
                                    .filter_map(|r| {
                                        let id = r["id"].as_str()?;
                                        Some((
                                            template_name(r),
                                            draft["desired"][kind].get(id)?.clone(),
                                        ))
                                    })
                                    .collect();
                                (kind.into(), Value::Object(rows))
                            })
                            .collect(),
                    );
                    let template = store.save_template(&input["name"], &portable)?;
                    return Ok(Next::Reply(json!({"ok": true, "template": template})));
                }
                let template = store
                    .templates()?
                    .into_iter()
                    .find(|t| t["id"] == input["templateId"])
                    .ok_or_else(|| err("plantilla no encontrada"))?;
                let mut desired = draft["desired"].clone();
                let mut missing = json!({"mcps": [], "skills": []});
                let mut unavailable = missing.clone();
                for kind in ["mcps", "skills"] {
                    let rows: Map<String, Value> = t.inventory[kind]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .map(|r| (template_name(r), r.clone()))
                        .collect();
                    for (name, enabled) in template["selection"][kind]
                        .as_object()
                        .into_iter()
                        .flatten()
                    {
                        if let Some(row) = rows.get(name) {
                            if truthy(&row["toggleable"]) {
                                desired[kind][text(&row["id"])] = enabled.clone();
                            } else if let Some(list) = unavailable[kind].as_array_mut() {
                                list.push(name.clone().into());
                            }
                        } else if let Some(list) = missing[kind].as_array_mut() {
                            list.push(name.clone().into());
                        }
                    }
                }
                let desired = launch::normalize(&t.inventory, &canonical(&t.inventory, &desired))?;
                store.save(key, revision, &desired)?;
                Ok(Next::State(Some(
                    json!({"missing": missing, "unavailable": unavailable}),
                )))
            }
            Action::Cancel | Action::Recover => {
                let row = with_operations(&conn, &opts, |s| s.get(&string(&input["operationId"])))?
                    .filter(|r| {
                        r["pane_key"] == t.draft["identity"]
                            && r["request"]["session"] == input["session"]
                            && r["request"]["pane"] == input["pane"]
                    })
                    .ok_or_else(|| err("la operación no pertenece a este panel"))?;
                if action == Action::Cancel {
                    if !with_operations(&conn, &opts, |s| s.cancel_waiting(text(&row["id"])))? {
                        return Err(err("la operación ya empezó; usa recuperación"));
                    }
                    Ok(Next::Reply(
                        json!({"ok": true, "cancelled": true, "operationId": row["id"]}),
                    ))
                } else {
                    Ok(Next::Recover(json!({"operationId": row["id"]})))
                }
            }
        }
    })
    .await?;
    match next {
        Next::State(extra) => {
            let mut body = state(native, env, data).await?;
            if let Some(Value::Object(extra)) = extra
                && let Some(body) = body.as_object_mut()
            {
                body.extend(extra);
            }
            Ok((StatusCode::OK, body))
        }
        Next::Reply(body) => Ok((StatusCode::OK, body)),
        Next::Configure(data) => configure::session_configure(native, data)
            .await
            .map_err(|_| Error::Other),
        Next::Recover(data) => configure::session_recover(native, data)
            .await
            .map_err(|_| Error::Other),
    }
}
pub async fn answer(native: &Arc<Native>, route: OpsRoute, request: &Request) -> Answer {
    let (action, data) = match route {
        OpsRoute::ExtensionsGet => {
            let q = Query::parse(&request.target)?;
            let data = Value::Object(
                ["session", "pane", "harness"]
                    .into_iter()
                    .filter_map(|k| q.all(k).last().map(|v| (k.into(), json!(v))))
                    .collect(),
            );
            (None, data)
        }
        OpsRoute::ExtensionsWrite(action) => {
            (Some(action), Value::Object(light::data(request)?.clone()))
        }
        _ => return Err(Fault::Decline),
    };
    if let Some(action) = action
        && data
            .as_object()
            .into_iter()
            .flatten()
            .any(|(k, _)| !COMMON.contains(&k.as_str()) && !action.fields().contains(&k.as_str()))
    {
        return reply(
            StatusCode::BAD_REQUEST,
            &json!({"ok":false,"error":"campos de extensiones inválidos"}),
        );
    }
    // La tarea conserva las escrituras y el registro de confirmación si el cliente se desconecta.
    let native = Arc::clone(native);
    let task_native = Arc::clone(&native);
    let job = native
        .tasks()
        .spawn_handle(async move {
            let native = task_native;
            if action == Some(Action::Apply) && truthy(&data["requestId"]) {
                let opts = native.options().clone();
                let id = string(&data["requestId"]);
                let previous = blocking(move || {
                    let conn = configure::open_store(&opts).map_err(|_| Error::Other)?;
                    with_operations(&conn, &opts, |s| s.get(&id))
                })
                .await;
                match previous {
                    Err(e) => return respond_error(e, false),
                    Ok(Some(old)) => {
                        if !python_eq(&request_fields(&data), &request_fields(&old["request"])) {
                            return respond_error(
                                err("requestId ya se usó con otra configuración"),
                                false,
                            );
                        }
                        return match configure::session_configure(&native, old["request"].clone())
                            .await
                        {
                            Ok((code, body)) => reply(code, &body),
                            Err(_) => respond_error(Error::Other, false),
                        };
                    }
                    Ok(None) => {}
                }
            }
            if let Err(e) = valid_target(&data) {
                return respond_error(e, action.is_none());
            }
            let env = match configure::build_env(&native).await {
                Ok(env) => env,
                // La búsqueda de idempotencia abre el diario y puede recuperar filas;
                // después de ese efecto no se entrega la misma petición al heredado.
                Err(_) if action == Some(Action::Apply) && truthy(&data["requestId"]) => {
                    return respond_error(Error::Other, false);
                }
                Err(fault) => return Err(fault),
            };
            match action {
                None => match state(&native, env, data).await {
                    Ok(body) => reply(StatusCode::OK, &body),
                    Err(e) => respond_error(e, true),
                },
                Some(action) => match write(&native, env, action, data).await {
                    Ok((code, body)) => reply(code, &body),
                    Err(e) => respond_error(e, false),
                },
            }
        })
        .map_err(|_| failure())?;
    job.await.map_err(|_| failure())?
}
