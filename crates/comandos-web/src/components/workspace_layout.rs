use serde_json::{Value, json};

const MIN_RATIO: f64 = 0.1;
const MAX_RATIO: f64 = 0.9;

fn fail<T>(message: &str) -> Result<T, String> {
    Err(message.to_string())
}

fn typ(node: &Value) -> &str {
    node.get("type").and_then(Value::as_str).unwrap_or("")
}

pub fn tab_ids(node: &Value) -> Vec<String> {
    if typ(node) == "tab" {
        return node
            .get("tabId")
            .and_then(Value::as_str)
            .map(|s| vec![s.to_string()])
            .unwrap_or_default();
    }
    let mut out = node.get("first").map(tab_ids).unwrap_or_default();
    out.extend(node.get("second").map(tab_ids).unwrap_or_default());
    out
}

fn remove(node: &Value, ids: &[String]) -> Option<Value> {
    if typ(node) == "tab" {
        let id = node.get("tabId").and_then(Value::as_str).unwrap_or("");
        return (!ids.iter().any(|x| x == id)).then_some(node.clone());
    }
    let first = node.get("first").and_then(|n| remove(n, ids));
    let second = node.get("second").and_then(|n| remove(n, ids));
    match (first, second) {
        (Some(a), Some(b)) => {
            let mut next = node.clone();
            if let Some(o) = next.as_object_mut() {
                o.insert("first".into(), a);
                o.insert("second".into(), b);
            }
            Some(next)
        }
        (Some(a), None) | (None, Some(a)) => Some(a),
        (None, None) => None,
    }
}

fn replace_leaf(node: &Value, tab_id: &str, next_leaf: &Value) -> Value {
    if typ(node) == "tab" {
        if node.get("tabId").and_then(Value::as_str) == Some(tab_id) {
            return next_leaf.clone();
        }
        return node.clone();
    }
    let mut next = node.clone();
    if let Some(o) = next.as_object_mut() {
        if let Some(first) = node.get("first") {
            o.insert("first".into(), replace_leaf(first, tab_id, next_leaf));
        }
        if let Some(second) = node.get("second") {
            o.insert("second".into(), replace_leaf(second, tab_id, next_leaf));
        }
    }
    next
}

fn find_leaf<'a>(node: &'a Value, tab_id: &str) -> Option<&'a Value> {
    if typ(node) == "tab" {
        return (node.get("tabId").and_then(Value::as_str) == Some(tab_id)).then_some(node);
    }
    node.get("first")
        .and_then(|n| find_leaf(n, tab_id))
        .or_else(|| node.get("second").and_then(|n| find_leaf(n, tab_id)))
}

fn groups(doc: &Value) -> Vec<Value> {
    doc.get("groups")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
}

fn source(doc: &Value, src: &str) -> Result<Value, String> {
    if let Some(group_id) = src.strip_prefix("group:") {
        return groups(doc)
            .into_iter()
            .find(|g| g.get("id").and_then(Value::as_str) == Some(group_id))
            .and_then(|g| g.get("tree").cloned())
            .ok_or_else(|| "Grupo de origen inexistente".to_string());
    }
    for group in groups(doc) {
        if let Some(leaf) = group.get("tree").and_then(|t| find_leaf(t, src)) {
            return Ok(leaf.clone());
        }
    }
    fail("Tab de origen inexistente")
}

fn without(doc: &Value, ids: &[String]) -> Vec<Value> {
    groups(doc)
        .into_iter()
        .filter_map(|mut g| {
            let tree = g.get("tree").and_then(|t| remove(t, ids))?;
            if let Some(o) = g.as_object_mut() {
                o.insert("tree".into(), tree);
            }
            Some(g)
        })
        .collect()
}

pub fn move_tab(document: &Value, src: &str, target_id: &str, edge: &str) -> Result<Value, String> {
    let (axis, before) = match edge {
        "left" => ("x", true),
        "right" => ("x", false),
        "top" => ("y", true),
        "bottom" => ("y", false),
        _ => return fail("Borde inválido"),
    };
    let moved = source(document, src)?;
    let ids = tab_ids(&moved);
    let mut next_groups = without(document, &ids);
    let outer = target_id.starts_with("group:");
    let target_group = target_id.strip_prefix("group:").unwrap_or(target_id);
    for group in &mut next_groups {
        let kept = if outer {
            (group.get("id").and_then(Value::as_str) == Some(target_group))
                .then(|| group.get("tree").cloned())
                .flatten()
        } else {
            group
                .get("tree")
                .and_then(|t| find_leaf(t, target_id))
                .cloned()
        };
        let Some(kept) = kept else {
            continue;
        };
        let split = if before {
            json!({"type": "split", "axis": axis, "ratio": 0.5, "first": moved, "second": kept})
        } else {
            json!({"type": "split", "axis": axis, "ratio": 0.5, "first": kept, "second": moved})
        };
        let tree = if outer {
            split
        } else {
            group
                .get("tree")
                .map(|t| replace_leaf(t, target_id, &split))
                .unwrap_or(split)
        };
        if let Some(o) = group.as_object_mut() {
            o.insert("tree".into(), tree);
        }
        let out = with_groups(document, next_groups);
        if &out == document {
            return fail("Sin cambios");
        }
        return Ok(out);
    }
    fail("Destino inexistente o dentro de lo que se mueve")
}

fn with_groups(document: &Value, groups: Vec<Value>) -> Value {
    let mut out = document.clone();
    if let Some(o) = out.as_object_mut() {
        o.insert("groups".into(), Value::Array(groups));
    }
    out
}

pub fn detach_tab(document: &Value, src: &str, index: i64) -> Result<Value, String> {
    if index < 0 {
        return fail("Posición inválida");
    }
    let doc_groups = groups(document);
    let before = doc_groups
        .get(usize::try_from(index).unwrap_or(usize::MAX))
        .and_then(|g| g.get("id"))
        .and_then(Value::as_str)
        .map(str::to_string);
    let (entry, mut next_groups) = if let Some(group_id) = src.strip_prefix("group:") {
        let Some(entry) = doc_groups
            .iter()
            .find(|g| g.get("id").and_then(Value::as_str) == Some(group_id))
            .cloned()
        else {
            return fail("Grupo de origen inexistente");
        };
        let kept = doc_groups
            .into_iter()
            .filter(|g| g.get("id").and_then(Value::as_str) != Some(group_id))
            .collect();
        (entry, kept)
    } else {
        let moved = source(document, src)?;
        let owner = doc_groups
            .iter()
            .find(|g| g.get("tree").and_then(|t| find_leaf(t, src)).is_some())
            .cloned()
            .ok_or_else(|| "Tab de origen inexistente".to_string())?;
        if owner.get("tree") == Some(&moved) {
            let owner_id = owner.get("id").and_then(Value::as_str);
            let kept = doc_groups
                .into_iter()
                .filter(|g| g.get("id").and_then(Value::as_str) != owner_id)
                .collect();
            (owner, kept)
        } else {
            let kept = without(document, &[src.to_string()]);
            let id = new_group_id(&doc_groups, src);
            (json!({"id": id, "tree": moved}), kept)
        }
    };
    let at = before
        .as_deref()
        .and_then(|id| {
            next_groups
                .iter()
                .position(|g| g.get("id").and_then(Value::as_str) == Some(id))
        })
        .unwrap_or(next_groups.len());
    next_groups.insert(at, entry);
    let out = with_groups(document, next_groups);
    if &out == document {
        return fail("Sin cambios");
    }
    Ok(out)
}

fn new_group_id(groups: &[Value], tab_id: &str) -> String {
    let mut id = format!("group-{tab_id}");
    let mut n = 1;
    while groups
        .iter()
        .any(|g| g.get("id").and_then(Value::as_str) == Some(id.as_str()))
    {
        n += 1;
        id = format!("group-{tab_id}-{n}");
    }
    id
}

pub fn resize_split(
    document: &Value,
    group_id: &str,
    path: &[String],
    ratio: f64,
) -> Result<Value, String> {
    if !ratio.is_finite() {
        return fail("Proporción inválida");
    }
    let mut out = document.clone();
    let group = out
        .get_mut("groups")
        .and_then(Value::as_array_mut)
        .and_then(|gs| {
            gs.iter_mut()
                .find(|g| g.get("id").and_then(Value::as_str) == Some(group_id))
        })
        .ok_or_else(|| "Grupo inexistente".to_string())?;
    let mut node = group
        .get_mut("tree")
        .ok_or_else(|| "Ruta inválida".to_string())?;
    for step in path {
        if step != "first" && step != "second" || typ(node) != "split" {
            return fail("Ruta inválida");
        }
        node = node
            .get_mut(step)
            .ok_or_else(|| "Ruta inválida".to_string())?;
    }
    if typ(node) != "split" {
        return fail("La ruta no es una división");
    }
    if let Some(o) = node.as_object_mut() {
        o.insert("ratio".into(), json!(ratio.clamp(MIN_RATIO, MAX_RATIO)));
    }
    Ok(out)
}

#[cfg(target_arch = "wasm32")]
pub fn mount() -> Result<(), wasm_bindgen::JsValue> {
    use comandos_web_dom::{bridge::global_set, port::*};
    let api = object();
    set(
        &api,
        "EDGES",
        &from_json(
            &json!({"left":["x",true],"right":["x",false],"top":["y",true],"bottom":["y",false]}),
        )?,
    )?;
    set(&api, "MIN_RATIO", &MIN_RATIO.into())?;
    set(&api, "MAX_RATIO", &MAX_RATIO.into())?;
    for name in ["tabIds", "moveTab", "detachTab", "resizeSplit"] {
        method(&api, name, move |args| {
            let doc = to_json(&args.get(0));
            let src = string(&args.get(1));
            let result = match name {
                "tabIds" => Ok(json!(tab_ids(&doc))),
                "moveTab" => move_tab(&doc, &src, &string(&args.get(2)), &string(&args.get(3))),
                "detachTab" => {
                    let n = args.get(2).as_f64().unwrap_or(f64::NAN);
                    if !n.is_finite() || n.fract() != 0.0 || n < 0.0 {
                        Err("Posición inválida".into())
                    } else {
                        detach_tab(&doc, &src, n as i64)
                    }
                }
                "resizeSplit" => {
                    let path = to_json(&args.get(2))
                        .as_array()
                        .map(|a| {
                            a.iter()
                                .map(|v| v.as_str().unwrap_or("").into())
                                .collect::<Vec<_>>()
                        })
                        .unwrap_or_default();
                    resize_split(&doc, &src, &path, args.get(3).as_f64().unwrap_or(f64::NAN))
                }
                _ => Err("Operación desconocida".into()),
            };
            from_json(&result.map_err(|e| js_sys::Error::new(&e))?)
        })?;
    }
    global_set("WorkspaceLayout", &api)
}

#[cfg(not(target_arch = "wasm32"))]
pub fn mount() -> Result<(), wasm_bindgen::JsValue> {
    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::unreachable)]
mod tests {
    use super::{detach_tab, move_tab, resize_split, tab_ids};
    use serde_json::Value;

    #[test]
    fn workspace_layout_checks() {
        let cases: Value = serde_json::from_str(include_str!(
            "../../../../tests/fixtures/workspace_layout.json"
        ))
        .unwrap();
        for case in cases.as_array().unwrap() {
            let doc = &case["doc"];
            let before = doc.clone();
            let args = case["args"].as_array().unwrap();
            let result = match case["op"].as_str().unwrap() {
                "move" => move_tab(
                    doc,
                    args[0].as_str().unwrap(),
                    args[1].as_str().unwrap(),
                    args[2].as_str().unwrap(),
                ),
                "detach" => detach_tab(doc, args[0].as_str().unwrap(), args[1].as_i64().unwrap()),
                "resize" => {
                    let path = args[1]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|v| v.as_str().unwrap().to_string())
                        .collect::<Vec<_>>();
                    resize_split(
                        doc,
                        args[0].as_str().unwrap(),
                        &path,
                        args[2].as_f64().unwrap(),
                    )
                }
                _ => unreachable!(),
            };
            if case.get("error").is_some() {
                assert!(result.is_err(), "{}", case["name"]);
            } else {
                let out = result.unwrap();
                assert_eq!(out, case["expect"], "{}", case["name"]);
                let mut got = out["groups"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .flat_map(|g| tab_ids(&g["tree"]))
                    .collect::<Vec<_>>();
                got.sort();
                let mut want = doc["groups"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .flat_map(|g| tab_ids(&g["tree"]))
                    .collect::<Vec<_>>();
                want.sort();
                assert_eq!(got, want, "{}", case["name"]);
            }
            assert_eq!(doc, &before, "{}: input mutated", case["name"]);
        }
    }
}
