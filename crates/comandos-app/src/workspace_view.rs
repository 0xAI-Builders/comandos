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
        Some("tab") => tree.get("tabId").cloned().unwrap_or(Value::Null),
        Some("split") => json!([
            tree.get("axis").unwrap_or(&Value::Null),
            shape(&tree["first"]),
            shape(&tree["second"])
        ]),
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
    if layout.get("area").is_some() {
        return match dock_hit(layout, x, y, moved)? {
            DockHit::Dock { target, .. } => Some(target),
            DockHit::Bar { .. } => None,
        };
    }
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
    if (min - left).abs() < f64::EPSILON {
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
    if !node.is_object() {
        return false;
    }
    if node.get("type").and_then(Value::as_str) == Some("tab") {
        return node.get("tabId").and_then(Value::as_str) == Some(key);
    }
    node.get("groups")
        .and_then(Value::as_array)
        .is_some_and(|groups| groups.iter().any(|g| contains_tab(&g["tree"], key)))
        || contains_tab(&node["first"], key)
        || contains_tab(&node["second"], key)
}

#[derive(Debug, Clone, PartialEq)]
pub enum DockHit {
    Bar { index: usize },
    Dock { target: DockTarget, rect: [f64; 4] },
}
impl DockHit {
    pub fn to_json(&self) -> Value {
        match self {
            Self::Bar { index } => json!({"kind":"bar","index":index}),
            Self::Dock { target, rect } => {
                json!({"kind":"dock","target":target.tab,"edge":match target.edge{DockEdge::Left=>"left",DockEdge::Right=>"right",DockEdge::Top=>"top",DockEdge::Bottom=>"bottom",DockEdge::Center=>"center"},"rect":rect})
            }
        }
    }
}
fn rect(value: &Value) -> Option<[f64; 4]> {
    let v = value.as_array()?;
    let result = [
        v.first()?.as_f64()?,
        v.get(1)?.as_f64()?,
        v.get(2)?.as_f64()?,
        v.get(3)?.as_f64()?,
    ];
    result.iter().all(|v| v.is_finite()).then_some(result)
}
fn inside([rx, ry, rw, rh]: [f64; 4], x: f64, y: f64) -> bool {
    rx <= x && x <= rx + rw && ry <= y && y <= ry + rh
}
fn nearest(values: &[(DockEdge, f64)]) -> Option<DockEdge> {
    values
        .iter()
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(edge, _)| *edge)
}
fn half([x, y, w, h]: [f64; 4], edge: DockEdge) -> [f64; 4] {
    match edge {
        DockEdge::Left => [x, y, w / 2., h],
        DockEdge::Right => [x + w / 2., y, w / 2., h],
        DockEdge::Top => [x, y, w, h / 2.],
        _ => [x, y + h / 2., w, h / 2.],
    }
}
/// Measured strip, viewport and leaves use root coordinates, matching GTK/Python.
pub fn dock_hit(layout: &Value, x: f64, y: f64, moved: &BTreeSet<String>) -> Option<DockHit> {
    if !x.is_finite() || !y.is_finite() {
        return None;
    }
    let strip = rect(layout.get("strip")?)?;
    if inside(strip, x, y) {
        let entries = layout.get("entries")?.as_array()?;
        let index = entries
            .iter()
            .position(|entry| {
                entry
                    .get(1)
                    .and_then(rect)
                    .is_some_and(|[ex, _, ew, _]| x < ex + ew / 2.)
            })
            .unwrap_or(entries.len());
        return Some(DockHit::Bar { index });
    }
    let area = rect(layout.get("area")?)?;
    if !inside(area, x, y) {
        return None;
    }
    let [ax, ay, aw, ah] = area;
    let distances = [
        (DockEdge::Left, x - ax),
        (DockEdge::Right, ax + aw - x),
        (DockEdge::Top, y - ay),
        (DockEdge::Bottom, ay + ah - y),
    ];
    let edge = nearest(&distances)?;
    let distance = distances.iter().find(|(e, _)| *e == edge)?.1;
    let group = layout.get("active").and_then(Value::as_str).unwrap_or("");
    let active_tabs = layout
        .get("activeTabs")
        .and_then(Value::as_array)
        .map(|v| v.iter().filter_map(Value::as_str).collect::<BTreeSet<_>>())
        .unwrap_or_default();
    if distance < 16. && !group.is_empty() && !active_tabs.iter().all(|key| moved.contains(*key)) {
        return Some(DockHit::Dock {
            target: DockTarget {
                group: group.into(),
                tab: format!("group:{group}"),
                edge,
            },
            rect: half(area, edge),
        });
    }
    for (tab, value) in layout.get("leaves")?.as_object()? {
        let rect = rect(value)?;
        if moved.contains(tab) || !inside(rect, x, y) {
            continue;
        }
        let [rx, ry, rw, rh] = rect;
        if rw <= 0. || rh <= 0. {
            continue;
        }
        let edge = nearest(&[
            (DockEdge::Left, (x - rx) / rw),
            (DockEdge::Right, (rx + rw - x) / rw),
            (DockEdge::Top, (y - ry) / rh),
            (DockEdge::Bottom, (ry + rh - y) / rh),
        ])?;
        return Some(DockHit::Dock {
            target: DockTarget {
                group: group.into(),
                tab: tab.clone(),
                edge,
            },
            rect: half(rect, edge),
        });
    }
    None
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum DragPhase {
    #[default]
    Idle,
    Pressed,
    Lifted,
    Docking,
    Cancelled,
}
#[derive(Debug, Default, Clone)]
pub struct DragGesture {
    pub phase: DragPhase,
    pub source: Option<String>,
    pub press_root: (f64, f64),
    pub pointer: (f64, f64),
    pub press_page: Option<u32>,
}
impl DragGesture {
    pub fn press(&mut self, source: String, root: (f64, f64), page: Option<u32>) {
        *self = Self {
            phase: DragPhase::Pressed,
            source: Some(source),
            press_root: root,
            pointer: root,
            press_page: page,
        };
    }
    pub fn motion(&mut self, root: (f64, f64), point: (f64, f64)) -> bool {
        if self.phase == DragPhase::Pressed {
            let dx = root.0 - self.press_root.0;
            let dy = root.1 - self.press_root.1;
            if dx * dx + dy * dy < 49. {
                return false;
            }
            self.phase = DragPhase::Lifted;
        }
        if matches!(self.phase, DragPhase::Lifted | DragPhase::Docking) {
            self.pointer = point;
            true
        } else {
            false
        }
    }
    pub fn docking(&mut self, present: bool) {
        if self.source.is_some() && self.phase != DragPhase::Pressed {
            self.phase = if present {
                DragPhase::Docking
            } else {
                DragPhase::Lifted
            };
        }
    }
    pub fn cancel(&mut self) -> Option<u32> {
        let page = self.press_page;
        *self = Self {
            phase: DragPhase::Cancelled,
            ..Self::default()
        };
        page
    }
    pub fn finish(&mut self) -> Option<String> {
        let source = self.source.take();
        *self = Self::default();
        source
    }
}
pub fn root_to_layer(point: (f64, f64), window: (f64, f64), offset: (f64, f64)) -> (f64, f64) {
    (point.0 - window.0 - offset.0, point.1 - window.1 - offset.1)
}
pub fn strip_edge_step(x: f64, width: f64) -> i32 {
    if x < 56. {
        -1
    } else if x > width - 56. {
        1
    } else {
        0
    }
}
pub fn tray_rects(width: f64, height: f64) -> Vec<(String, [f64; 4])> {
    let x = ((width - (4. * 112. + 3. * 12.)) / 2.).max(0.);
    let y = (height - 80. - 70.).max(0.);
    ["frozen", "awaiting_reply", "resolved", "none"]
        .iter()
        .enumerate()
        .map(|(i, mark)| ((*mark).into(), [x + i as f64 * 124., y, 112., 70.]))
        .collect()
}
pub fn tray_at(trays: &[(String, [f64; 4])], x: f64, y: f64, strip_bottom: f64) -> Option<String> {
    if y < strip_bottom + 30. {
        return None;
    }
    trays
        .iter()
        .find(|(_, [rx, ry, rw, rh])| {
            *rx - 8. <= x && x <= *rx + *rw + 8. && *ry - 8. <= y && y <= *ry + *rh + 8.
        })
        .map(|(mark, _)| mark.clone())
}
pub fn wrap_rows(width: i32, widths: &[i32]) -> Vec<Vec<usize>> {
    let mut rows: Vec<Vec<usize>> = Vec::new();
    let mut used = 0;
    for (i, w) in widths.iter().enumerate() {
        if rows.is_empty() || (used > 0 && used + 4 + w > width) {
            rows.push(Vec::new());
            used = 0;
        }
        if let Some(row) = rows.last_mut() {
            row.push(i);
        }
        used += (if used > 0 { 4 } else { 0 }) + w;
    }
    rows
}
