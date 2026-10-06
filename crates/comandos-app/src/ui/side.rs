//! Sidebar wire state and geometry retain the Python contract; GTK owns its clients.
use serde_json::{Value, json};
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LayoutError(pub String);
impl std::fmt::Display for LayoutError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for LayoutError {}
#[derive(Debug, Clone, PartialEq)]
pub struct SideState {
    pub session: String,
    pub hidden: bool,
    pub collapsed: bool,
    pub left_hidden: bool,
    pub left_pos: i32,
    pub term_share: f64,
    pub cmds_closed: bool,
    pub sheet_open: bool,
    pub cmds_h: i32,
    pub tabs: Vec<Value>,
    persisted: Value,
}
impl Default for SideState {
    fn default() -> Self {
        Self {
            session: String::new(),
            hidden: false,
            collapsed: true,
            left_hidden: false,
            left_pos: 620,
            term_share: 0.42,
            cmds_closed: false,
            sheet_open: false,
            cmds_h: 0,
            tabs: vec![],
            persisted: json!({}),
        }
    }
}
impl SideState {
    pub fn from_json(value: &Value) -> Result<Self, LayoutError> {
        if !value.is_null() && !value.is_object() {
            return Err(LayoutError("layout must be an object".into()));
        }
        let mut state = Self {
            persisted: if value.is_object() {
                value.clone()
            } else {
                json!({})
            },
            ..Self::default()
        };
        state.left_hidden = value
            .get("leftHidden")
            .is_some_and(comandos_core::json::truthy);
        if let Some(pos) = value
            .get("leftPos")
            .filter(|v| comandos_core::json::truthy(v))
        {
            state.left_pos =
                super::app_commands::python_int(pos).map_err(|e| LayoutError(e.to_string()))?;
        }
        if let Some(share) = value
            .get("termShare")
            .filter(|v| comandos_core::json::truthy(v))
        {
            state.term_share = comandos_core::focus::float(share).map_err(LayoutError)?;
            if !state.term_share.is_finite() {
                return Err(LayoutError("invalid share".into()));
            }
        }
        Ok(state)
    }
    pub fn to_json(&self) -> Value {
        self.persisted.clone()
    }
    pub fn set_left(&mut self, hidden: bool, position: i32) {
        if hidden && position >= 200 {
            self.left_pos = position;
            if let Some(map) = self.persisted.as_object_mut() {
                map.insert("leftPos".into(), json!(position));
            }
        }
        self.left_hidden = hidden;
        if let Some(map) = self.persisted.as_object_mut() {
            map.insert("leftHidden".into(), json!(hidden));
        }
    }
    pub fn save_share(&mut self, total: i32, position: i32, visible: bool, pinned: bool) -> bool {
        if total <= 120 || !visible || self.collapsed || pinned {
            return false;
        }
        self.term_share =
            ((1.0 - f64::from(position) / f64::from(total)) * 1000.0).round_ties_even() / 1000.0;
        if let Some(map) = self.persisted.as_object_mut() {
            map.insert("termShare".into(), json!(self.term_share));
        }
        true
    }
    pub fn adopt_web(&mut self, data: &Value) {
        if data.get("hidden").is_some() {
            self.hidden = data.get("hidden").is_some_and(comandos_core::json::truthy);
        }
        if let Some(tabs) = data.get("tabs") {
            self.tabs = side_tabs(tabs);
        }
        if let Some(cmds) = data.get("cmds").filter(|v| v.is_object()) {
            self.sheet_open = cmds.get("open").is_some_and(comandos_core::json::truthy);
            self.cmds_closed = !self.sheet_open;
            self.cmds_h = cmds
                .get("h")
                .and_then(|v| super::app_commands::python_int(v).ok())
                .unwrap_or(0)
                .clamp(0, 4000);
        }
        self.session = data
            .get("session")
            .filter(|v| comandos_core::json::truthy(v))
            .map(comandos_core::pomodoro::python_str)
            .filter(|s| crate::tab_actions::valid_session(s))
            .unwrap_or_default();
    }
    pub fn pin_position(&self, visible: bool, zoom: f64, lo: i32, hi: i32) -> Option<i32> {
        if !self.cmds_closed || self.collapsed || !visible || self.cmds_h < 40 {
            return None;
        }
        let zoom = if zoom == 0.0 || !zoom.is_finite() {
            1.0
        } else {
            zoom
        };
        Some(((f64::from(self.cmds_h) * zoom).round_ties_even() as i32).clamp(lo, hi.max(lo)))
    }
    pub fn share_position(&self, total: i32, visible: bool, pin: Option<i32>) -> Option<i32> {
        pin.or_else(|| {
            (total > 120 && visible && !self.collapsed)
                .then(|| (f64::from(total) * (1.0 - self.term_share.clamp(0.15, 0.85))) as i32)
        })
    }
}
pub fn side_tabs(value: &Value) -> Vec<Value> {
    value.as_array().into_iter().flatten().take(40).filter_map(|tab|{
    if !tab.is_object(){return None;}
    let id=tab.get("id").filter(|v|comandos_core::json::truthy(v)).map(comandos_core::pomodoro::python_str).unwrap_or_default();if !crate::tab_actions::valid_session(&id){return None;}
    let text=|key,count|tab.get(key).filter(|v|comandos_core::json::truthy(v)).map(comandos_core::pomodoro::python_str).unwrap_or_else(||id.clone()).chars().take(count).collect::<String>();
    Some(json!({"id":id,"label":text("label",40),"title":text("title",120),"on":tab.get("on").is_some_and(comandos_core::json::truthy),"sel":tab.get("sel").is_some_and(comandos_core::json::truthy),"closing":tab.get("closing").is_some_and(comandos_core::json::truthy)}))
}).collect()
}
pub fn scroll_position(value: f64, step: f64, lower: f64, upper: f64, page: f64) -> f64 {
    lower.max((upper - page).min(value + step))
}
pub fn arrow_state(
    width: i32,
    value: f64,
    lower: f64,
    upper: f64,
    page: f64,
) -> (bool, bool, bool) {
    (
        !(1 < width && width < 250) && upper - page > 2.0,
        value > lower + 2.0,
        value < upper - page - 2.0,
    )
}
pub fn drag_position(start: (f64, i32), y: f64, lo: i32, hi: i32) -> i32 {
    ((f64::from(start.1) + y - start.0) as i32).clamp(lo, hi.max(lo))
}

/// The immutable desktop initialization accepts Python int conversions and round ties-even.
pub fn pane_initial_position(width: i32, document: &Value) -> Option<i32> {
    if width < 900 {
        return None;
    }
    let saved = document
        .get("position")
        .filter(|v| comandos_core::json::truthy(v))
        .and_then(|v| super::app_commands::python_int(v).ok())
        .unwrap_or(0);
    let position = if saved != 0 {
        saved
    } else {
        (f64::from(width) * 0.27).round_ties_even() as i32
    };
    Some(position.clamp(300, (width - 720).max(320)))
}

#[derive(Debug, Clone, PartialEq)]
pub struct LeftFrame {
    pub offset: f64,
    pub mask_start: Option<f64>,
    pub alpha: f64,
    pub edge: f64,
    pub shadow_alpha: Option<f64>,
}
pub fn left_frame(hiding: bool, progress: f64, x: f64, width: f64) -> LeftFrame {
    let k = if hiding { progress } else { 1.0 - progress };
    let offset = -width * k;
    let edge = x + width + offset;
    LeftFrame {
        offset,
        mask_start: (!hiding).then(|| x.max(edge)),
        alpha: 1.0 - 0.45 * k,
        edge,
        shadow_alpha: (k > 0.0 && k < 1.0 && edge > x).then_some(0.28 * (1.0 - k)),
    }
}

impl LeftFrame {
    pub fn to_json(&self) -> Value {
        json!({"offset":self.offset,"mask_start":self.mask_start,"alpha":self.alpha,"edge":self.edge,"shadow_alpha":self.shadow_alpha})
    }
}

pub fn wheel_delta(dx: f64, dy: f64, discrete: Option<f64>) -> f64 {
    (if let Some(direction) = discrete {
        direction
    } else if dy != 0. {
        dy
    } else {
        dx
    }) * 60.
}

/// A GTK frame clock starts a transition on its first frame, as the original _tween.
pub struct Tween {
    start: std::cell::Cell<Option<i64>>,
    from: f64,
    to: f64,
    millis: u64,
}
impl Tween {
    pub fn new(from: f64, to: f64, millis: u64) -> Self {
        Self {
            start: std::cell::Cell::new(None),
            from,
            to,
            millis,
        }
    }
    pub fn step(&self, now: i64) -> (f64, bool) {
        let start = self.start.get().unwrap_or(now);
        self.start.set(Some(start));
        let t = ((now.saturating_sub(start)) as f64 / (self.millis.max(1) as f64 * 1000.)).min(1.);
        let ease = 1. - (1. - t).powf(3.);
        (self.from + (self.to - self.from) * ease, t >= 1.)
    }
}
