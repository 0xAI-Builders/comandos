//! Copy-on-write docking and stable top-strip ordering.
use super::{Result, array, invalid, json_eq, number_cmp, object, tab_ids, validate_document};
use crate::json::truthy;
use serde_json::{Value, json};
use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};

pub const MIN_RATIO: f64 = 0.1;
pub const MAX_RATIO: f64 = 0.9;
pub const SORTS: [&str; 4] = ["fav", "recent", "need", "alpha"];

pub(super) fn remove(node: &Value, ids: &HashSet<String>) -> Option<Value> {
    if node["type"] == "tab" {
        return if node["tabId"].as_str().is_some_and(|t| ids.contains(t)) {
            None
        } else {
            Some(node.clone())
        };
    }
    match (remove(&node["first"], ids), remove(&node["second"], ids)) {
        (None, second) => second,
        (first, None) => first,
        (Some(first), Some(second)) => {
            let mut out = node.clone();
            out["first"] = first;
            out["second"] = second;
            Some(out)
        }
    }
}
fn find_leaf<'a>(node: &'a Value, tab_id: &str) -> Option<&'a Value> {
    if node["type"] == "tab" {
        return (node["tabId"] == tab_id).then_some(node);
    }
    find_leaf(&node["first"], tab_id).or_else(|| find_leaf(&node["second"], tab_id))
}
fn replace_leaf(node: &Value, tab_id: &str, replacement: &Value) -> Value {
    if node["type"] == "tab" {
        return if node["tabId"] == tab_id {
            replacement.clone()
        } else {
            node.clone()
        };
    }
    let mut out = node.clone();
    out["first"] = replace_leaf(&node["first"], tab_id, replacement);
    out["second"] = replace_leaf(&node["second"], tab_id, replacement);
    out
}
fn groups(document: &Value) -> Result<&Vec<Value>> {
    array(&document["groups"], "Workspace incompleto")
}
fn source<'a>(document: &'a Value, source: &str) -> Result<&'a Value> {
    if let Some(gid) = source.strip_prefix("group:") {
        return groups(document)?
            .iter()
            .find(|g| g["id"] == gid)
            .map(|g| &g["tree"])
            .ok_or_else(|| invalid("Grupo de origen inexistente"));
    }
    groups(document)?
        .iter()
        .find_map(|g| find_leaf(&g["tree"], source))
        .ok_or_else(|| invalid("Tab de origen inexistente"))
}
fn without(document: &Value, ids: &HashSet<String>) -> Result<Vec<Value>> {
    Ok(groups(document)?
        .iter()
        .filter_map(|g| {
            remove(&g["tree"], ids).map(|tree| {
                let mut g = g.clone();
                g["tree"] = tree;
                g
            })
        })
        .collect())
}
pub(super) fn new_group_id(groups: &[Value], tab_id: &str) -> String {
    let mut id = format!("group-{tab_id}");
    let mut n = 1;
    while groups.iter().any(|g| g["id"] == id) {
        n += 1;
        id = format!("group-{tab_id}-{n}");
    }
    id
}

pub fn move_tab(document: &Value, source_id: &str, target_id: &str, edge: &str) -> Result<Value> {
    let (axis, before) = match edge {
        "left" => ("x", true),
        "right" => ("x", false),
        "top" => ("y", true),
        "bottom" => ("y", false),
        _ => return Err(invalid("Borde inválido")),
    };
    validate_document(document)?;
    let moved = source(document, source_id)?;
    let ids = tab_ids(moved)?.into_iter().collect();
    let mut groups = without(document, &ids)?;
    for group in &mut groups {
        let kept = if let Some(gid) = target_id.strip_prefix("group:") {
            if group["id"] != gid {
                continue;
            }
            &group["tree"]
        } else {
            let Some(leaf) = find_leaf(&group["tree"], target_id) else {
                continue;
            };
            leaf
        };
        let split = json!({"type":"split","axis":axis,"ratio":0.5,"first":if before {moved} else {kept},"second":if before {kept} else {moved}});
        group["tree"] = if target_id.starts_with("group:") {
            split
        } else {
            replace_leaf(&group["tree"], target_id, &split)
        };
        let mut out = document.clone();
        out["groups"] = json!(groups);
        if json_eq(&out, document) {
            return Err(invalid("Sin cambios"));
        }
        validate_document(&out)?;
        return Ok(out);
    }
    Err(invalid("Destino inexistente o dentro de lo que se mueve"))
}

pub fn detach_tab(document: &Value, source_id: &str, index: &Value) -> Result<Value> {
    let index = index.as_u64().ok_or_else(|| invalid("Posición inválida"))?;
    validate_document(document)?;
    let existing = groups(document)?;
    let before = usize::try_from(index)
        .ok()
        .and_then(|i| existing.get(i))
        .map(|g| g["id"].clone());
    let (entry, mut groups) = if let Some(gid) = source_id.strip_prefix("group:") {
        let entry = existing
            .iter()
            .find(|g| g["id"] == gid)
            .ok_or_else(|| invalid("Grupo de origen inexistente"))?;
        (
            entry.clone(),
            existing
                .iter()
                .filter(|g| g["id"] != gid)
                .cloned()
                .collect::<Vec<_>>(),
        )
    } else {
        let moved = source(document, source_id)?;
        let owner = existing
            .iter()
            .find(|g| find_leaf(&g["tree"], source_id).is_some())
            .ok_or_else(|| invalid("Tab de origen inexistente"))?;
        if owner["tree"]["type"] == "tab" {
            (
                owner.clone(),
                existing
                    .iter()
                    .filter(|g| g["id"] != owner["id"])
                    .cloned()
                    .collect(),
            )
        } else {
            (
                json!({"id":new_group_id(existing,source_id),"tree":moved}),
                without(document, &HashSet::from([source_id.into()]))?,
            )
        }
    };
    let at = before
        .and_then(|b| groups.iter().position(|g| g["id"] == b))
        .unwrap_or(groups.len());
    groups.insert(at, entry);
    let mut out = document.clone();
    out["groups"] = json!(groups);
    if json_eq(&out, document) {
        return Err(invalid("Sin cambios"));
    }
    validate_document(&out)?;
    Ok(out)
}

pub fn resize_split(
    document: &Value,
    group_id: &str,
    path: &Value,
    ratio: &Value,
) -> Result<Value> {
    let ratio = ratio
        .as_f64()
        .filter(|r| r.is_finite())
        .ok_or_else(|| invalid("Proporción inválida"))?;
    validate_document(document)?;
    let mut out = document.clone();
    let group = out["groups"]
        .as_array_mut()
        .expect("validated")
        .iter_mut()
        .find(|g| g["id"] == group_id)
        .ok_or_else(|| invalid("Grupo inexistente"))?;
    let mut node = &mut group["tree"];
    for step in array(path, "Ruta inválida")? {
        let step = step
            .as_str()
            .filter(|s| matches!(*s, "first" | "second"))
            .ok_or_else(|| invalid("Ruta inválida"))?;
        if node["type"] != "split" {
            return Err(invalid("Ruta inválida"));
        }
        node = &mut node[step];
    }
    if node["type"] != "split" {
        return Err(invalid("La ruta no es una división"));
    }
    node["ratio"] = json!(ratio.clamp(MIN_RATIO, MAX_RATIO));
    validate_document(&out)?;
    Ok(out)
}

// Python int conversion used by move_tab_group permits numeric strings, bools,
// and truncates finite floats. Dock/detach retain strict integer validation.
fn position(value: &Value) -> Result<i128> {
    match value {
        Value::Bool(b) => Ok(i128::from(*b)),
        Value::String(s) => s.trim().parse().map_err(|_| invalid("Posición inválida")),
        Value::Number(n) => n
            .to_string()
            .parse()
            .or_else(|_| {
                n.as_f64()
                    .filter(|v| v.is_finite())
                    .map(|v| v.trunc() as i128)
                    .ok_or(())
            })
            .map_err(|_| invalid("Posición inválida")),
        _ => Err(invalid("Posición inválida")),
    }
}
pub fn move_tab_group(document: &Value, tab_id: &str, index: &Value) -> Result<Option<Value>> {
    let mut groups = match document.get("groups") {
        Some(v) => array(v, "Workspace incompleto")?.clone(),
        None => Vec::new(),
    };
    let mut at = None;
    for (i, g) in groups.iter().enumerate() {
        if tab_ids(&g["tree"])?.iter().any(|t| t == tab_id) {
            at = Some(i);
            break;
        }
    }
    let Some(at) = at else {
        return Ok(None);
    };
    let entry = groups.remove(at);
    let at = position(index)?.clamp(0, groups.len() as i128) as usize;
    groups.insert(at, entry);
    let mut out = document.clone();
    object(&out, "Workspace incompleto")?;
    out["groups"] = json!(groups);
    Ok(Some(out))
}

struct Facts {
    local: bool,
    fav: bool,
    need: bool,
    active_at: Value,
    label: String,
}
fn facts(group: &Value, info: &Value) -> Result<Facts> {
    let ids = tab_ids(&group["tree"])?;
    let mut facts = Facts {
        local: ids.iter().any(|t| t == "local"),
        fav: false,
        need: false,
        active_at: Value::Null,
        label: String::new(),
    };
    for (index, tab) in ids.iter().enumerate() {
        let row = &info[tab];
        facts.fav |= truthy(&row["fav"]);
        facts.need |= truthy(&row["need"]);
        let active = if truthy(&row["activeAt"]) {
            row["activeAt"].clone()
        } else {
            json!(0)
        };
        if !active.is_number() && !active.is_boolean() {
            return Err(invalid("Fecha de actividad inválida"));
        }
        if index == 0 || number_cmp(&active, &facts.active_at).is_gt() {
            facts.active_at = active;
        }
        let label = row["label"]
            .as_str()
            .filter(|s| !s.is_empty())
            .unwrap_or(tab)
            .to_lowercase();
        if index == 0 || label < facts.label {
            facts.label = label;
        }
    }
    Ok(facts)
}

pub fn sort_groups(document: &Value, by: &str, info: &Value) -> Result<Value> {
    if !SORTS.contains(&by) {
        return Err(invalid("Orden desconocido"));
    }
    let mut decorated = Vec::new();
    if let Some(groups) = document.get("groups") {
        for g in array(groups, "Workspace incompleto")? {
            decorated.push((g.clone(), facts(g, info)?));
        }
    }
    decorated.sort_by(|a, b| {
        let local = b.1.local.cmp(&a.1.local);
        if local != Ordering::Equal {
            return local;
        }
        match by {
            "fav" => b.1.fav.cmp(&a.1.fav),
            "recent" => number_cmp(&b.1.active_at, &a.1.active_at),
            "need" => b.1.need.cmp(&a.1.need).then_with(|| b.1.fav.cmp(&a.1.fav)),
            "alpha" => a.1.label.cmp(&b.1.label),
            _ => unreachable!(),
        }
    });
    let mut out = document.clone();
    object(&out, "Workspace incompleto")?;
    out["groups"] = json!(decorated.into_iter().map(|(g, _)| g).collect::<Vec<_>>());
    Ok(out)
}
pub fn restore_order(document: &Value, group_ids: &[String]) -> Result<Value> {
    let pos: HashMap<&str, usize> = group_ids
        .iter()
        .enumerate()
        .map(|(i, g)| (g.as_str(), i))
        .collect();
    let mut groups = match document.get("groups") {
        Some(g) => array(g, "Workspace incompleto")?.clone(),
        None => Vec::new(),
    };
    groups.sort_by_key(|g| {
        pos.get(g["id"].as_str().unwrap_or(""))
            .copied()
            .unwrap_or(pos.len())
    });
    let mut out = document.clone();
    object(&out, "Workspace incompleto")?;
    out["groups"] = json!(groups);
    Ok(out)
}
