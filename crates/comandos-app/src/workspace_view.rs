use serde_json::{Value, json};
use std::collections::BTreeSet;

#[derive(Debug, Clone, PartialEq)]
pub struct DockTarget {
    pub group: String,
    pub tab: String,
    pub edge: DockEdge,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DockEdge {
    Left,
    Right,
    Top,
    Bottom,
    Center,
}

pub fn prune(tree: &Value, present: &BTreeSet<String>) -> Value {
    match tree.get("type").and_then(Value::as_str) {
        Some("tab") => tree
            .get("tabId")
            .and_then(Value::as_str)
            .filter(|tab| present.contains(*tab))
            .map(|_| tree.clone())
            .unwrap_or(Value::Null),
        Some("split") => {
            let first = prune(&tree["first"], present);
            let second = prune(&tree["second"], present);
            match (first.is_null(), second.is_null()) {
                (true, true) => Value::Null,
                (true, false) => second,
                (false, true) => first,
                (false, false) => {
                    let mut out = tree.clone();
                    if let Some(object) = out.as_object_mut() {
                        object.insert("first".to_string(), first);
                        object.insert("second".to_string(), second);
                    }
                    out
                }
            }
        }
        _ => Value::Null,
    }
}

pub fn shape(tree: &Value) -> Value {
    match tree.get("type").and_then(Value::as_str) {
        Some("tab") => json!({"type":"tab"}),
        Some("split") => json!({
            "type": "split",
            "axis": tree.get("axis").and_then(Value::as_str).unwrap_or("x"),
            "ratio": tree.get("ratio").and_then(Value::as_f64).unwrap_or(0.5),
            "first": shape(&tree["first"]),
            "second": shape(&tree["second"]),
        }),
        _ => Value::Null,
    }
}

pub fn split_paths(tree: &Value) -> Vec<(Vec<usize>, f64)> {
    fn walk(node: &Value, path: &mut Vec<usize>, out: &mut Vec<(Vec<usize>, f64)>) {
        if node.get("type").and_then(Value::as_str) != Some("split") {
            return;
        }
        out.push((
            path.clone(),
            node.get("ratio").and_then(Value::as_f64).unwrap_or(0.5),
        ));
        path.push(0);
        walk(&node["first"], path, out);
        path.pop();
        path.push(1);
        walk(&node["second"], path, out);
        path.pop();
    }
    let mut out = Vec::new();
    walk(tree, &mut Vec::new(), &mut out);
    out
}

pub fn dock_target(layout: &Value, x: f64, y: f64, moved: &BTreeSet<String>) -> Option<DockTarget> {
    let groups = layout.get("groups")?.as_array()?;
    for group in groups {
        let group_id = group.get("id")?.as_str()?;
        let Some(tab) = first_tab(&group["tree"], moved) else {
            continue;
        };
        let edge = edge_for(x, y);
        return Some(DockTarget {
            group: group_id.to_string(),
            tab,
            edge,
        });
    }
    None
}

fn first_tab(tree: &Value, moved: &BTreeSet<String>) -> Option<String> {
    match tree.get("type").and_then(Value::as_str) {
        Some("tab") => tree
            .get("tabId")
            .and_then(Value::as_str)
            .filter(|tab| !moved.contains(*tab))
            .map(ToString::to_string),
        Some("split") => {
            first_tab(&tree["first"], moved).or_else(|| first_tab(&tree["second"], moved))
        }
        _ => None,
    }
}

fn edge_for(x: f64, y: f64) -> DockEdge {
    let x = x.clamp(0.0, 1.0);
    let y = y.clamp(0.0, 1.0);
    let left = x;
    let right = 1.0 - x;
    let top = y;
    let bottom = 1.0 - y;
    let min = left.min(right).min(top).min(bottom);
    if min > 0.25 {
        DockEdge::Center
    } else if (min - left).abs() < f64::EPSILON {
        DockEdge::Left
    } else if (min - right).abs() < f64::EPSILON {
        DockEdge::Right
    } else if (min - top).abs() < f64::EPSILON {
        DockEdge::Top
    } else {
        DockEdge::Bottom
    }
}

#[derive(Clone)]
pub struct WorkspaceView {
    doc: std::rc::Rc<std::cell::RefCell<Value>>,
}

impl Default for WorkspaceView {
    fn default() -> Self {
        Self {
            doc: std::rc::Rc::new(std::cell::RefCell::new(Value::Null)),
        }
    }
}

impl WorkspaceView {
    pub fn apply(&self, doc: &Value) {
        *self.doc.borrow_mut() = doc.clone();
    }

    pub fn select(&self, key: &str) -> bool {
        contains_tab(&self.doc.borrow(), key)
    }

    pub fn commit(&self, doc: &Value, _focus: Option<&str>) {
        self.apply(doc);
    }
}

fn contains_tab(node: &Value, key: &str) -> bool {
    if node.get("type").and_then(Value::as_str) == Some("tab") {
        return node.get("tabId").and_then(Value::as_str) == Some(key);
    }
    node.get("groups")
        .and_then(Value::as_array)
        .is_some_and(|groups| groups.iter().any(|g| contains_tab(&g["tree"], key)))
        || contains_tab(&node["first"], key)
        || contains_tab(&node["second"], key)
}
