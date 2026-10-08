//! Marcas propias de una ventana; las respuestas no cambian la selección.
use comandos_core::work_marks as core;
use serde_json::{Value, json};
use std::collections::BTreeMap;
#[derive(Clone, Debug)]
pub struct MarkRequest {
    pub body: Value,
    scope: String,
    key: String,
    generation: u64,
}
#[derive(Default, Debug)]
pub struct Marks {
    rows: BTreeMap<(String, String), Value>,
    panes: Vec<Value>,
    pending: BTreeMap<(String, String), u64>,
    generation: u64,
}
pub fn adopt(payload: &Value) -> Marks {
    let mut marks = Marks::default();
    marks.adopt_poll(payload, 0);
    marks
}
pub fn indicator_display(_mark: &Value, state: &str) -> Value {
    let ai = core::ai_status(state);
    json!([format!("ai:{ai}"), null, core::ai_cycle(ai) > 0.])
}
impl Marks {
    pub fn generation(&self) -> u64 {
        self.generation
    }
    pub fn row(&self, scope: &str, key: &str) -> Value {
        self.rows
            .get(&(scope.into(), key.into()))
            .cloned()
            .unwrap_or_else(
                || json!({"scope":scope,"key":key,"mark":"none","favorite":false,"revision":0}),
            )
    }
    pub fn pane_key(&self, session: &str, pane: &str) -> Option<String> {
        core::pane_key_for(Some(&self.panes), &json!(session), &json!(pane))
            .and_then(|v| v.as_str().map(str::to_string))
    }
    pub fn adopt_row(&mut self, row: &Value) -> bool {
        let (Some(scope), Some(key)) = (
            row.get("scope").and_then(Value::as_str),
            row.get("key").and_then(Value::as_str),
        ) else {
            return false;
        };
        if !matches!(scope, "session" | "pane") || key.is_empty() {
            return false;
        }
        let identity = (scope.into(), key.into());
        if self.rows.get(&identity).is_some_and(|old| {
            old.get("revision").and_then(Value::as_u64).unwrap_or(0)
                > row.get("revision").and_then(Value::as_u64).unwrap_or(0)
        }) {
            return false;
        }
        self.rows.insert(identity, row.clone());
        true
    }
    pub fn adopt_poll(&mut self, payload: &Value, generation: u64) -> bool {
        if generation != self.generation {
            return false;
        }
        let previous = std::mem::take(&mut self.rows);
        self.rows = previous
            .iter()
            .filter(|(id, _)| self.pending.contains_key(*id))
            .map(|(id, row)| (id.clone(), row.clone()))
            .collect();
        self.panes = payload
            .get("panes")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter(|v| v.is_object())
            .cloned()
            .collect();
        for row in payload
            .get("marks")
            .or_else(|| payload.get("rows"))
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let identity = (
                row.get("scope")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string(),
                row.get("key")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string(),
            );
            if !self.pending.contains_key(&identity) {
                if let Some(old) = previous.get(&identity) {
                    self.rows.insert(identity, old.clone());
                }
                self.adopt_row(row);
            }
        }
        true
    }
    pub fn begin(&mut self, scope: &str, key: &str, value: &Value) -> Option<MarkRequest> {
        if key.is_empty()
            || !matches!(scope, "session" | "pane")
            || (scope == "session" && key == "local")
        {
            return None;
        }
        if scope == "pane"
            && !self
                .panes
                .iter()
                .any(|p| p.get("paneKey").and_then(Value::as_str) == Some(key))
        {
            return None;
        }
        if !(value.as_str().is_some_and(|s| core::MARKS.contains(&s))
            || value.get("favorite").is_some_and(Value::is_boolean))
        {
            return None;
        }
        let row = self.row(scope, key);
        self.generation = self.generation.saturating_add(1);
        self.pending
            .insert((scope.into(), key.into()), self.generation);
        Some(MarkRequest {
            body: json!({"scope":scope,"key":key,"value":value,"expectedRevision":row.get("revision").unwrap_or(&Value::Null)}),
            scope: scope.into(),
            key: key.into(),
            generation: self.generation,
        })
    }
    pub fn finish(&mut self, request: &MarkRequest, row: Option<&Value>) -> bool {
        let identity = (request.scope.clone(), request.key.clone());
        if self.pending.get(&identity) != Some(&request.generation) {
            return false;
        }
        self.pending.remove(&identity);
        self.generation = self.generation.saturating_add(1);
        if let Some(row) = row.filter(|r| {
            r.get("scope").and_then(Value::as_str) == Some(&request.scope)
                && r.get("key").and_then(Value::as_str) == Some(&request.key)
        }) {
            self.adopt_row(row);
        }
        true
    }
}

pub fn menu_item(mark: &str, label: &str, active: bool) -> gtk::MenuItem {
    use gtk::prelude::*;
    let item = gtk::MenuItem::new();
    item.style_context().add_class("wm-item");
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    let image = gtk::Image::new();
    if let Some(pb) = IndicatorCache::default().pixbuf(mark, None, 0, 1, None) {
        image.set_from_pixbuf(Some(&pb));
    }
    row.pack_start(&image, false, false, 0);
    let name = gtk::Label::new(Some(label));
    name.set_xalign(0.);
    name.set_hexpand(true);
    if active {
        name.style_context().add_class("wm-on");
    }
    row.pack_start(&name, true, true, 0);
    if active {
        let dot = gtk::Label::new(Some("●"));
        dot.style_context().add_class("wm-dot");
        row.pack_end(&dot, false, false, 0);
    }
    item.add(&row);
    item
}

#[derive(Default)]
pub struct IndicatorCache {
    scale: i32,
    entries: BTreeMap<(String, Option<String>, usize, Option<u32>), gdk_pixbuf::Pixbuf>,
}
impl IndicatorCache {
    pub fn len(&self) -> usize {
        self.entries.len()
    }
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
    pub fn pixbuf(
        &mut self,
        icon: &str,
        color: Option<&str>,
        frame: usize,
        scale: i32,
        pixels: Option<u32>,
    ) -> Option<gdk_pixbuf::Pixbuf> {
        use gdk_pixbuf::prelude::*;
        let scale = scale.max(1);
        if self.scale != scale {
            self.entries.clear();
            self.scale = scale;
        }
        let key = (icon.to_string(), color.map(str::to_string), frame, pixels);
        if let Some(pb) = self.entries.get(&key) {
            return Some(pb.clone());
        }
        let svg = if let Some(ai) = icon.strip_prefix("ai:") {
            let phase =
                (core::ai_cycle(ai) > 0.).then_some(frame as f64 / core::AI_PULSE_FRAMES as f64);
            core::ai_dot_svg(
                ai,
                pixels
                    .filter(|p| *p > 0)
                    .unwrap_or(core::AI_DOT_PIXELS)
                    .checked_mul(scale as u32)?,
                phase,
            )
        } else {
            let n = core::frame_count(icon);
            core::icon_svg(icon, color, 14, (n > 1).then_some(frame as f64 / n as f64))
        };
        let loader = gdk_pixbuf::PixbufLoader::with_type("svg").ok()?;
        loader.write(svg.as_bytes()).ok()?;
        loader.close().ok()?;
        let pb = loader.pixbuf()?;
        if self.entries.len() >= 48
            && let Some(old) = self.entries.keys().next().cloned()
        {
            self.entries.remove(&old);
        }
        self.entries.insert(key, pb.clone());
        Some(pb)
    }
}
