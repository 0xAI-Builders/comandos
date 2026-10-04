//! Portable workspace arrangement and per-device state rules.
//!
//! Documents retain unknown JSON metadata. Time and process/session operations
//! are supplied by the caller; this module never opens a database or terminal.
use crate::json::{
    number_cmp, python_eq as json_eq, truthy, validate_workspace_depth, workspace_dumps,
};
use serde_json::{Map, Value, json};
use std::collections::{HashMap, HashSet};
use std::fmt;

pub const SCHEMA: u64 = 1;
pub const MAX_DEPTH: usize = 64;
pub const MAX_ID: usize = 200;
pub const MAX_CLIENT_BYTES: usize = 256 * 1024;
pub const MAX_DRAFTS: usize = 64;
pub const MAX_DRAFT_CHARS: usize = 20_000;
pub const MAX_ANCHORS: usize = 128;
pub const PHASES: [&str; 3] = ["restoring", "ready", "failed"];

#[derive(Debug, Clone, PartialEq)]
pub enum WorkspaceError {
    Invalid(String),
    Conflict(Option<Value>),
    Callback(String),
}
impl fmt::Display for WorkspaceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Invalid(s) | Self::Callback(s) => f.write_str(s),
            Self::Conflict(_) => f.write_str("Revisión desactualizada"),
        }
    }
}
impl std::error::Error for WorkspaceError {}
pub type Result<T> = std::result::Result<T, WorkspaceError>;
fn invalid(message: &str) -> WorkspaceError {
    WorkspaceError::Invalid(message.into())
}
fn ident<'a>(value: &'a Value, what: &str) -> Result<&'a str> {
    value
        .as_str()
        .filter(|s| !s.is_empty() && s.chars().count() <= MAX_ID)
        .ok_or_else(|| invalid(&format!("{what} inválido")))
}
fn object<'a>(value: &'a Value, error: &str) -> Result<&'a Map<String, Value>> {
    value.as_object().ok_or_else(|| invalid(error))
}
fn array<'a>(value: &'a Value, error: &str) -> Result<&'a Vec<Value>> {
    value.as_array().ok_or_else(|| invalid(error))
}

pub fn tab_ids(node: &Value) -> Result<Vec<String>> {
    fn visit(node: &Value, depth: usize, out: &mut Vec<String>) -> Result<()> {
        if depth > MAX_DEPTH {
            return Err(invalid("Distribución demasiado profunda"));
        }
        object(node, "Distribución inválida")?;
        if node["type"] == "tab" {
            out.push(ident(&node["tabId"], "tabId")?.into());
            return Ok(());
        }
        if node["type"] != "split" || !matches!(node["axis"].as_str(), Some("x" | "y")) {
            return Err(invalid("Distribución inválida"));
        }
        if !node["ratio"]
            .as_f64()
            .is_some_and(|r| r.is_finite() && r > 0.0 && r < 1.0)
        {
            return Err(invalid("Proporción inválida"));
        }
        visit(&node["first"], depth + 1, out)?;
        visit(&node["second"], depth + 1, out)
    }
    let mut out = Vec::new();
    visit(node, 0, &mut out)?;
    Ok(out)
}

pub fn validate_document(document: &Value) -> Result<()> {
    // Python's schema comparison accepts true and 1.0 as schema 1.
    if !document.is_object()
        || !(document["schema"].as_f64() == Some(SCHEMA as f64) || document["schema"] == true)
    {
        return Err(invalid("Esquema de workspace no soportado"));
    }
    let groups = array(&document["groups"], "Workspace incompleto")?;
    let tabs = object(&document["tabs"], "Workspace incompleto")?;
    let mut group_ids = HashSet::new();
    let mut ids = HashSet::new();
    for group in groups {
        object(group, "Grupo inválido")?;
        let gid = ident(&group["id"], "Grupo")?;
        if !group_ids.insert(gid) {
            return Err(invalid("Grupo duplicado"));
        }
        for id in tab_ids(&group["tree"])? {
            if !ids.insert(id) {
                return Err(invalid("Cada tab debe aparecer exactamente una vez"));
            }
        }
    }
    if ids.len() != tabs.len() || tabs.keys().any(|k| !ids.contains(k)) {
        return Err(invalid("Cada tab debe aparecer exactamente una vez"));
    }
    let mut pane_keys = HashSet::new();
    for tab in tabs.values() {
        let fields = object(tab, "Tab inválida")?;
        ident(&tab["session"], "Sesión")?;
        if let Some(keys) = fields.get("paneKeys") {
            for key in array(keys, "paneKeys inválido")? {
                if !pane_keys.insert(ident(key, "paneKey")?) {
                    return Err(invalid("Un pane pertenece a una sola tab"));
                }
            }
        }
        if fields.get("label").is_some_and(|label| !label.is_string()) {
            return Err(invalid("Etiqueta inválida"));
        }
    }
    if let Some(bindings) = document.get("bindings")
        && object(bindings, "Enlaces de pane inválidos")?
            .keys()
            .any(|k| !pane_keys.contains(k.as_str()))
    {
        return Err(invalid("Enlaces de pane inválidos"));
    }
    Ok(())
}

pub fn empty_document() -> Value {
    json!({"schema":SCHEMA,"groups":[],"tabs":{}})
}
pub fn is_empty(document: &Value) -> bool {
    !truthy(&document["groups"]) && !truthy(&document["tabs"])
}

pub fn pane_bindings(snapshot: &Value) -> Result<Vec<(String, Value)>> {
    let mut out = Vec::new();
    let Some(windows) = snapshot.get("windows") else {
        return Ok(out);
    };
    for window in array(windows, "Inventario de ventanas inválido")? {
        let Some(panes) = window.get("panes") else {
            continue;
        };
        for pane in array(panes, "Inventario de panes inválido")? {
            let Some(key) = pane["key"].as_str().filter(|s| !s.is_empty()) else {
                continue;
            };
            let conversation = if truthy(&pane["resume_id"]) {
                json!({"agent":pane["agent"],"id":pane["resume_id"]})
            } else if truthy(&pane["acp"]["sessionId"]) {
                json!({"agent":"acp","id":pane["acp"]["sessionId"]})
            } else {
                Value::Null
            };
            out.push((key.into(), json!({"paneId":pane["id"],"pid":pane["pid"],"startTime":pane["start"],"conversation":conversation})));
        }
    }
    Ok(out)
}

/// Fresh empty pane captures leave the last known pane keys and bindings intact.
pub fn reconcile(
    document: &Value,
    live_tabs: &[(String, Option<String>)],
    panes: &HashMap<String, Vec<(String, Value)>>,
) -> Result<Value> {
    object(document, "Workspace incompleto")?;
    let live: HashSet<&str> = live_tabs.iter().map(|(t, _)| t.as_str()).collect();
    let mut groups = Vec::new();
    if let Some(existing) = document.get("groups") {
        for group in array(existing, "Workspace incompleto")? {
            let missing: HashSet<String> = tab_ids(&group["tree"])?
                .into_iter()
                .filter(|t| !live.contains(t.as_str()))
                .collect();
            if let Some(tree) = layout::remove(&group["tree"], &missing) {
                let mut g = group.clone();
                g["tree"] = tree;
                groups.push(g);
            }
        }
    }
    let mut tabs = Map::new();
    if let Some(existing) = document.get("tabs") {
        for (key, tab) in object(existing, "Workspace incompleto")? {
            if live.contains(key.as_str()) {
                object(tab, "Tab inválida")?;
                tabs.insert(key.clone(), tab.clone());
            }
        }
    }
    let mut known = HashSet::new();
    for group in &groups {
        known.extend(tab_ids(&group["tree"])?);
    }
    for (tab_id, label) in live_tabs {
        let entry = tabs
            .entry(tab_id.clone())
            .or_insert_with(|| json!({"session":tab_id,"paneKeys":[]}));
        if let Some(label) = label.as_ref().filter(|s| !s.is_empty()) {
            entry["label"] = json!(label);
        }
        if known.insert(tab_id.clone()) {
            groups.push(json!({"id":layout::new_group_id(&groups, tab_id),"tree":{"type":"tab","tabId":tab_id}}));
        }
    }
    let mut bindings = match document.get("bindings") {
        Some(v) => object(v, "Enlaces de pane inválidos")?.clone(),
        None => Map::new(),
    };
    for (tab_id, found) in panes {
        let Some(tab) = tabs.get_mut(tab_id) else {
            continue;
        };
        if found.is_empty() {
            continue;
        }
        tab["paneKeys"] = json!(found.iter().map(|(k, _)| k).collect::<Vec<_>>());
        for (key, binding) in found {
            object(binding, "Enlace de pane inválido")?;
            let mut binding = binding.clone();
            binding["session"] = tab["session"].clone();
            bindings.insert(key.clone(), binding);
        }
    }
    let mut keys = HashSet::new();
    for tab in tabs.values() {
        if let Some(v) = tab.get("paneKeys") {
            for k in array(v, "paneKeys inválido")? {
                keys.insert(ident(k, "paneKey")?.to_string());
            }
        }
    }
    bindings.retain(|k, _| keys.contains(k));
    let mut out = document.clone();
    out["schema"] = json!(SCHEMA);
    out["groups"] = json!(groups);
    out["tabs"] = Value::Object(tabs);
    if document.get("bindings").is_some() || !bindings.is_empty() {
        out["bindings"] = Value::Object(bindings);
    }
    validate_document(&out)?;
    Ok(out)
}

/// A pane lacking an exact conversation id is never resumed with a latest fallback.
pub fn restore_workspace(
    document: &Value,
    mut inspect: impl FnMut(&Value) -> Result<Option<Value>>,
    mut resume_exact: impl FnMut(&Value) -> Result<Option<Value>>,
) -> Result<Value> {
    validate_document(document)?;
    let mut attached = Vec::new();
    let mut resumed = Map::new();
    let mut unavailable = Vec::new();
    for tab in object(&document["tabs"], "Tab inválida")?.values() {
        let Some(keys) = tab.get("paneKeys") else {
            continue;
        };
        for key in array(keys, "paneKeys inválido")? {
            let key = ident(key, "paneKey")?;
            let binding = &document["bindings"][key];
            if !truthy(binding) {
                unavailable.push(key.to_string());
                continue;
            }
            object(binding, "Enlace de pane inválido")?;
            if let Some(live) = inspect(binding)?
                && truthy(&live)
                && json_eq(&live["pid"], &binding["pid"])
                && json_eq(&live["startTime"], &binding["startTime"])
            {
                attached.push(key.to_string());
                continue;
            }
            if !truthy(&binding["conversation"]["id"]) {
                unavailable.push(key.to_string());
                continue;
            }
            match resume_exact(binding) {
                Ok(Some(value)) if truthy(&value) => {
                    resumed.insert(key.into(), value);
                }
                _ => unavailable.push(key.to_string()),
            }
        }
    }
    Ok(json!({"attached":attached,"resumed":resumed,"unavailable":unavailable}))
}

fn patch_entries(target: &mut Value, patch: &Value, draft: bool, now_ms: f64) -> Result<()> {
    let target = target
        .as_object_mut()
        .ok_or_else(|| invalid("Estado de cliente inválido"))?;
    for (key, value) in object(patch, "Cambio inválido")? {
        ident(&json!(key), "clave")?;
        if value.is_null() {
            target.shift_remove(key);
            continue;
        }
        object(value, "Cambio inválido")?;
        let text = value["text"]
            .as_str()
            .filter(|t| t.chars().count() <= if draft { MAX_DRAFT_CHARS } else { 300 })
            .ok_or_else(|| {
                invalid(if draft {
                    "Borrador inválido o demasiado largo"
                } else {
                    "Ancla de lectura inválida"
                })
            })?;
        let stamp = value["updatedAt"]
            .as_f64()
            .filter(|v| v.is_finite())
            .map(|_| value["updatedAt"].clone())
            .unwrap_or_else(|| json!(now_ms));
        let mut entry = json!({"text":text,"updatedAt":stamp});
        if draft {
            for key in ["selStart", "selEnd"] {
                if value[key]
                    .as_u64()
                    .is_some_and(|pos| pos <= text.chars().count() as u64)
                {
                    entry[key] = value[key].clone();
                }
            }
        } else if let Some(ratio) = value["ratio"].as_f64().filter(|r| *r >= 0.0 && *r <= 1.0) {
            entry["ratio"] = json!(ratio);
        }
        target.insert(key.clone(), entry);
    }
    let limit = if draft { MAX_DRAFTS } else { MAX_ANCHORS };
    let mut ordered: Vec<_> = target
        .iter()
        .map(|(k, v)| {
            (
                k.clone(),
                v.get("updatedAt").cloned().unwrap_or_else(|| json!(0)),
            )
        })
        .collect();
    ordered.sort_by(|a, b| number_cmp(&a.1, &b.1));
    for (key, _) in ordered.into_iter().take(target.len().saturating_sub(limit)) {
        target.shift_remove(&key);
    }
    Ok(())
}

/// Merge per-device state without erasing omitted fields; clock is milliseconds.
pub fn clean_client_state(
    device_id: &str,
    previous: &Value,
    state: &Value,
    now_ms: f64,
) -> Result<Value> {
    ident(&json!(device_id), "deviceId")?;
    object(state, "Estado de cliente inválido")?;
    if !now_ms.is_finite() {
        return Err(invalid("Reloj inválido"));
    }
    let mut out = Map::new();
    for (key, default) in [
        ("activeTabId", Value::Null),
        ("activePaneKey", Value::Null),
        ("drafts", json!({})),
        ("readingAnchors", json!({})),
    ] {
        let selected = state
            .get(key)
            .or_else(|| previous.get(key))
            .unwrap_or(&default);
        if matches!(key, "activeTabId" | "activePaneKey") && !selected.is_null() {
            ident(selected, key)?;
        }
        if matches!(key, "drafts" | "readingAnchors") {
            // Count the resulting root object before cloning retained entries.
            validate_workspace_depth(selected, 1).map_err(|e| invalid(&e))?;
        }
        out.insert(key.into(), selected.clone());
    }
    for key in ["drafts", "readingAnchors"] {
        if !truthy(&out[key]) {
            out.insert(key.into(), json!({}));
        }
        object(&out[key], "Estado de cliente inválido")?;
    }
    if let Some(patch) = state.get("draftsPatch") {
        patch_entries(
            out.get_mut("drafts").expect("initialized"),
            patch,
            true,
            now_ms,
        )?;
    }
    if let Some(patch) = state.get("anchorsPatch") {
        patch_entries(
            out.get_mut("readingAnchors").expect("initialized"),
            patch,
            false,
            now_ms,
        )?;
    }
    let value = Value::Object(out);
    if workspace_dumps(&value).map_err(|e| invalid(&e))?.len() > MAX_CLIENT_BYTES {
        return Err(invalid("Estado de cliente demasiado grande"));
    }
    Ok(value)
}

fn group<'a>(document: &'a Value, group_id: &str) -> Result<&'a Value> {
    array(&document["groups"], "El grupo ya no existe")?
        .iter()
        .find(|g| g["id"] == group_id)
        .ok_or_else(|| invalid("El grupo ya no existe"))
}
fn tab_session<'a>(document: &'a Value, tab_id: &'a str) -> &'a str {
    document["tabs"][tab_id]["session"]
        .as_str()
        .filter(|s| !s.is_empty())
        .unwrap_or(tab_id)
}
pub fn close_group_preview(
    document: &Value,
    group_id: &str,
    mut session_identity: impl FnMut(&str) -> Value,
) -> Result<Value> {
    let mut members = Vec::new();
    for tab_id in tab_ids(&group(document, group_id)?["tree"])? {
        let session = tab_session(document, &tab_id);
        let label = document["tabs"][&tab_id]["label"]
            .as_str()
            .filter(|s| !s.is_empty())
            .unwrap_or(session);
        members.push(json!({"tabId":tab_id,"session":session,"label":label,"sessionId":session_identity(session),"kept":session=="local"}));
    }
    Ok(json!({"groupId":group_id,"members":members}))
}

/// The adapter provides revision/state and persistent idempotency results.
pub trait CloseGroupState {
    fn current(&mut self) -> Result<Option<Value>>;
    /// Return every saved JSON value as `Some`, including null and empty values.
    /// Absent or empty raw text returns `None`; malformed nonempty text errors.
    fn meta(&mut self, key: &str) -> Result<Option<Value>>;
    fn set_meta(&mut self, key: &str, value: &Value) -> Result<()>;
}

/// Revalidate the complete confirmed list before invoking any close callback.
pub fn close_group(
    store: &mut impl CloseGroupState,
    group_id: &str,
    expected_revision: &Value,
    identities: &Value,
    request_id: &str,
    mut session_identity: impl FnMut(&str) -> Value,
    mut close_tab: impl FnMut(&str) -> std::result::Result<Option<String>, String>,
) -> Result<Value> {
    ident(&json!(request_id), "requestId")?;
    let key = format!("close-group:{request_id}");
    if let Some(seen) = store.meta(&key)? {
        return Ok(seen);
    }
    let state = store.current()?;
    let Some(ref current) = state else {
        return Err(WorkspaceError::Conflict(state));
    };
    if !json_eq(&current["revision"], expected_revision) {
        return Err(WorkspaceError::Conflict(state));
    }
    let document = &current["document"];
    let order = tab_ids(&group(document, group_id)?["tree"])?;
    let members = array(
        identities,
        "Las pestañas del grupo cambiaron. Revisa la lista antes de cerrar",
    )?;
    let mut confirmed = HashMap::new();
    for member in members {
        if member.is_object() {
            confirmed.insert(member["tabId"].as_str().unwrap_or(""), member);
        }
    }
    if confirmed.len() != members.len()
        || confirmed.len() != order.len()
        || order.iter().any(|t| !confirmed.contains_key(t.as_str()))
    {
        return Err(invalid(
            "Las pestañas del grupo cambiaron. Revisa la lista antes de cerrar",
        ));
    }
    for tab_id in &order {
        let member = confirmed[tab_id.as_str()];
        let session = tab_session(document, tab_id);
        let live = session_identity(session);
        if member["session"] != session || !truthy(&live) || !json_eq(&live, &member["sessionId"]) {
            let label = member["label"]
                .as_str()
                .filter(|s| !s.is_empty())
                .unwrap_or(session);
            return Err(invalid(&format!("«{label}» cambió. No se ha cerrado nada")));
        }
    }
    let mut closed = Vec::new();
    let mut remaining = Vec::new();
    let mut kept = Vec::new();
    let mut error = None;
    for (index, tab_id) in order.iter().enumerate() {
        let session = tab_session(document, tab_id);
        if session == "local" {
            kept.push(session.to_string());
            continue;
        }
        let close_error = match close_tab(session) {
            Ok(error) => error.filter(|s| !s.is_empty()),
            Err(error) => Some(if error.is_empty() {
                "Error al cerrar".into()
            } else {
                error
            }),
        };
        if let Some(e) = close_error {
            error = Some(e);
            for tab_id in &order[index..] {
                let s = tab_session(document, tab_id);
                if s == "local" {
                    kept.push(s.to_string());
                } else {
                    remaining.push(s.to_string());
                }
            }
            break;
        }
        closed.push(session.to_string());
    }
    let result = json!({"ok":error.is_none(),"closed":closed,"remaining":remaining,"kept":kept,"error":error});
    store.set_meta(&key, &result)?;
    Ok(result)
}

pub mod layout;
pub mod snapshot;
