//! Shared workspace renderer. Terminal iframes are positioned, never reparented.
use super::extensions::{field, rows, text};
use super::workspace_layout::tab_ids;
use serde_json::{Value, json};
use std::collections::BTreeMap;
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub left: f64,
    pub top: f64,
    pub width: f64,
    pub height: f64,
}
impl Rect {
    pub fn right(self) -> f64 {
        self.left + self.width
    }
    pub fn bottom(self) -> f64 {
        self.top + self.height
    }
}
pub const TRAYS: &[(&str, &str, &str)] = &[
    ("frozen", "Aparcar", "❄"),
    ("awaiting_reply", "Esperando", "⏳"),
    ("resolved", "Hecho", "✓"),
    ("none", "Quitar", "✕"),
];
pub fn edge_scroll(x: f64, left: f64, right: f64, zone: f64, max: f64) -> f64 {
    if x < left + zone {
        -(((left + zone - x) / zone).min(1.0) * max).ceil()
    } else if x > right - zone {
        (((x - (right - zone)) / zone).min(1.0) * max).ceil()
    } else {
        0.0
    }
}
fn nearest(d: [(&'static str, f64); 4]) -> (&'static str, f64) {
    d.into_iter()
        .reduce(|a, b| if b.1 < a.1 { b } else { a })
        .unwrap_or(("", f64::NAN))
}
pub fn edge_for(r: Rect, x: f64, y: f64, fraction: f64) -> Option<&'static str> {
    let (e, d) = nearest([
        ("left", (x - r.left) / r.width),
        ("right", (r.right() - x) / r.width),
        ("top", (y - r.top) / r.height),
        ("bottom", (r.bottom() - y) / r.height),
    ]);
    (d >= 0.0 && d <= fraction.max(0.5)).then_some(e)
}
pub fn outer_edge(r: Rect, x: f64, y: f64, px: f64) -> Option<&'static str> {
    let (e, d) = nearest([
        ("left", x - r.left),
        ("right", r.right() - x),
        ("top", y - r.top),
        ("bottom", r.bottom() - y),
    ]);
    (d >= 0.0 && d < px).then_some(e)
}
pub fn preview_rect(mut r: Rect, edge: &str) -> Rect {
    if matches!(edge, "left" | "right") {
        r.width /= 2.0;
        if edge == "right" {
            r.left += r.width
        }
    } else {
        r.height /= 2.0;
        if edge == "bottom" {
            r.top += r.height
        }
    }
    r
}
#[derive(Debug, Clone)]
pub struct Measurement {
    pub axis: &'static str,
    pub height: f64,
    pub a: Option<Box<Measurement>>,
    pub b: Option<Box<Measurement>>,
}
impl Measurement {
    pub fn json(&self) -> Value {
        if self.axis.is_empty() {
            json!({"height":self.height})
        } else {
            json!({"height":self.height,"axis":self.axis,"a":self.a.as_ref().map(|a|a.json()),"b":self.b.as_ref().map(|b|b.json())})
        }
    }
}
pub fn measure(n: &Value, width: f64, leaf_min: Option<f64>) -> Measurement {
    let min = leaf_min.unwrap_or(if width < 560.0 { 440.0 } else { 240.0 });
    if field(n, "type").as_str() == Some("tab") {
        return Measurement {
            axis: "",
            height: min,
            a: None,
            b: None,
        };
    }
    let axis = if field(n, "axis").as_str() == Some("x") && width >= 560.0 {
        "x"
    } else {
        "y"
    };
    let ratio = field(n, "ratio").as_f64().unwrap_or(0.5);
    let a = measure(
        field(n, "first"),
        if axis == "x" { width * ratio } else { width },
        Some(min),
    );
    let b = measure(
        field(n, "second"),
        if axis == "x" {
            width * (1.0 - ratio)
        } else {
            width
        },
        Some(min),
    );
    let height = if axis == "x" {
        a.height.max(b.height)
    } else if field(n, "axis").as_str() == Some("y") {
        (a.height / ratio).max(b.height / (1.0 - ratio))
    } else {
        a.height + b.height
    };
    Measurement {
        axis,
        height,
        a: Some(Box::new(a)),
        b: Some(Box::new(b)),
    }
}
pub fn strip_ratios(n: &Value) -> Value {
    if field(n, "type").as_str() == Some("tab") {
        field(n, "tabId").clone()
    } else {
        json!([
            field(n, "axis"),
            strip_ratios(field(n, "first")),
            strip_ratios(field(n, "second"))
        ])
    }
}
#[derive(Default)]
pub struct Dock {
    pub doc: Option<Value>,
    pub revision: i64,
    pub focus: BTreeMap<String, String>,
    pub pending: bool,
    pub gesturing: bool,
    pub signature: String,
}
impl Dock {
    pub fn adopt(&mut self, state: Value) -> bool {
        if !field(&state, "groups").is_array() || self.gesturing || self.pending {
            return false;
        }
        let revision = field(&state, "revision").as_i64().unwrap_or(0);
        let changed = self.doc.is_none() || revision != self.revision;
        let mut doc = json!({"schema":field(&state,"schema"),"groups":field(&state,"groups"),"tabs":field(&state,"tabs")});
        if let Some(bindings) = state.get("bindings")
            && let Some(m) = doc.as_object_mut()
        {
            m.insert("bindings".into(), bindings.clone());
        }
        self.doc = Some(doc);
        self.revision = revision;
        changed
    }
    pub fn groups(&self) -> Vec<&Value> {
        self.doc
            .as_ref()
            .map(|d| rows(field(d, "groups")))
            .unwrap_or_default()
    }
    pub fn group(&self, tab: &str) -> Option<&Value> {
        self.groups()
            .into_iter()
            .find(|g| tab_ids(field(g, "tree")).iter().any(|t| t == tab))
    }
    pub fn session(&self, tab: &str) -> String {
        self.doc
            .as_ref()
            .map(|d| text(field(field(field(d, "tabs"), tab), "session")))
            .filter(|s| !s.is_empty())
            .unwrap_or(tab.into())
    }
    pub fn visible_tabs(&self, shown: &str) -> Vec<String> {
        if let Some(g) = self.group(shown) {
            tab_ids(field(g, "tree"))
                .iter()
                .map(|t| self.session(t))
                .collect()
        } else if !shown.is_empty() {
            vec![shown.into()]
        } else {
            vec![]
        }
    }
}
#[cfg(target_arch = "wasm32")]
#[path = "workspace_dock_web.rs"]
mod web;
#[cfg(target_arch = "wasm32")]
pub use web::{attach, mount};
#[cfg(not(target_arch = "wasm32"))]
pub fn mount() -> Result<(), wasm_bindgen::JsValue> {
    Ok(())
}
#[cfg(not(target_arch = "wasm32"))]
pub fn attach() -> Result<(), wasm_bindgen::JsValue> {
    Ok(())
}
