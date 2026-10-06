use gtk::prelude::*;
use serde_json::{Value, json};

pub const SCHEMA: u32 = 1;
pub const QUIET_MS: u64 = 300;
pub const DEFAULT_TIMEOUT_MS: u64 = 15_000;
pub fn enabled(value: Option<&str>) -> bool {
    value == Some("1")
}

#[derive(Debug, PartialEq)]
pub enum Decision {
    Pending,
    Ready(Value),
    Failed(Vec<String>),
}
pub struct Diagnostic {
    deadline: u64,
    candidate: Option<(u64, Value)>,
    done: bool,
}
impl Diagnostic {
    pub fn new(start_ms: u64, timeout_ms: u64) -> Self {
        Self {
            deadline: start_ms.saturating_add(timeout_ms),
            candidate: None,
            done: false,
        }
    }
    pub fn done(&self) -> bool {
        self.done
    }
    pub fn observe(
        &mut self,
        now_ms: u64,
        issues: &[String],
        snapshot: Option<&Value>,
    ) -> Decision {
        if self.done {
            return Decision::Pending;
        }
        if now_ms >= self.deadline {
            self.done = true;
            self.candidate = None;
            return Decision::Failed(if issues.is_empty() {
                vec![
                    if snapshot.is_some() {
                        "layout_not_quiet"
                    } else {
                        "snapshot_unavailable"
                    }
                    .into(),
                ]
            } else {
                issues.to_vec()
            });
        }
        if issues.is_empty()
            && let Some(snapshot) = snapshot
        {
            if let Some((since, previous)) = &self.candidate
                && previous == snapshot
            {
                if now_ms.saturating_sub(*since) >= QUIET_MS {
                    self.done = true;
                    self.candidate = None;
                    return Decision::Ready(snapshot.clone());
                }
            } else {
                self.candidate = Some((now_ms, snapshot.clone()));
            }
        } else {
            self.candidate = None;
        }
        Decision::Pending
    }
}

pub fn normalize(mut context: Value) -> Result<Value, String> {
    for field in [
        "viewport",
        "window",
        "dashboard",
        "theme",
        "font",
        "strip",
        "workspace",
    ] {
        if context.get(field).is_none_or(|value| !value.is_object()) {
            return Err(format!("missing object: {field}"));
        }
    }
    nullable_string(context.get("window").ok_or("window unavailable")?, "title")?;
    let dashboard = context.get("dashboard").ok_or("dashboard unavailable")?;
    nullable_string(dashboard, "uri")?;
    nullable_string(dashboard, "title")?;
    boolean(dashboard, "loading")?;
    if dashboard
        .get("hardware_acceleration_policy")
        .and_then(Value::as_str)
        .is_none_or(str::is_empty)
    {
        return Err("WebKit policy unavailable".into());
    }
    if let Some(readiness) = context.get("readiness")
        && (readiness.get("status").and_then(Value::as_str) != Some("ready")
            || readiness
                .get("issues")
                .and_then(Value::as_array)
                .is_none_or(|issues| !issues.is_empty()))
    {
        return Err("failed capture cannot be normalized into ready".into());
    }
    for field in ["tabs", "widgets", "terminals"] {
        if context.get(field).is_none_or(|value| !value.is_array()) {
            return Err(format!("missing array: {field}"));
        }
    }
    for field in ["width", "height", "dpr"] {
        if context
            .get("viewport")
            .and_then(|v| v.get(field))
            .and_then(Value::as_f64)
            .is_none_or(|n| !n.is_finite() || n <= 0.)
        {
            return Err(format!("invalid viewport: {field}"));
        }
    }
    if context
        .get("theme")
        .and_then(|v| v.get("name"))
        .and_then(Value::as_str)
        .is_none_or(str::is_empty)
    {
        return Err("applied theme unavailable".into());
    }
    validate_font(context.get("font").ok_or("font unavailable")?)?;
    for group in context
        .get("workspace")
        .and_then(|workspace| workspace.get("groups"))
        .and_then(Value::as_array)
        .ok_or("authoritative groups unavailable")?
    {
        validate_tree(group.get("tree").ok_or("authoritative tree unavailable")?)?;
    }
    for widget in context
        .get("widgets")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        if widget.get("label").is_some_and(Value::is_string) {
            validate_font(widget.get("font").ok_or("widget font unavailable")?)?;
        }
        if widget.get("mapped").and_then(Value::as_bool) == Some(true) {
            let geometry = widget
                .get("geometry")
                .ok_or("widget geometry unavailable")?;
            for field in ["x", "y", "width", "height"] {
                finite_number(geometry, field, matches!(field, "width" | "height"))?;
            }
        }
    }
    for terminal in context
        .get("terminals")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        validate_font(terminal.get("font").ok_or("terminal font unavailable")?)?;
        let cursor = terminal
            .get("cursor")
            .filter(|value| value.is_object())
            .ok_or("cursor unavailable")?;
        if cursor.get("row").and_then(Value::as_i64).is_none()
            || cursor.get("col").and_then(Value::as_u64).is_none()
            || cursor
                .get("shape")
                .and_then(Value::as_str)
                .is_none_or(str::is_empty)
        {
            return Err("invalid cursor observation".into());
        }
        for field in ["visible", "painted", "wide"] {
            boolean(cursor, field)?;
        }
        let metrics = terminal
            .get("cell_metrics")
            .ok_or("cell metrics unavailable")?;
        for field in ["width", "height", "dpr", "font_size_px"] {
            finite_number(metrics, field, true)?;
        }
        for field in ["origin_x", "origin_y"] {
            finite_number(metrics, field, false)?;
        }
        let grid = terminal.get("grid").ok_or("terminal grid unavailable")?;
        let cols = grid
            .get("cols")
            .and_then(Value::as_u64)
            .filter(|n| *n > 0)
            .ok_or("invalid grid columns")?;
        let rows = grid
            .get("rows")
            .and_then(Value::as_u64)
            .filter(|n| *n > 0)
            .ok_or("invalid grid rows")?;
        let cells = grid
            .get("cells")
            .and_then(Value::as_array)
            .ok_or("grid cells unavailable")?;
        if u64::try_from(cells.len()).ok() != Some(rows) {
            return Err("incomplete grid rows".into());
        }
        for row in cells {
            let row = row.as_array().ok_or("invalid grid row")?;
            if u64::try_from(row.len()).ok() != Some(cols) {
                return Err("incomplete grid columns".into());
            }
            for cell in row {
                if cell.get("text").and_then(Value::as_str).is_none() {
                    return Err("cell text unavailable".into());
                }
                if cell
                    .get("width")
                    .and_then(Value::as_u64)
                    .is_none_or(|width| width > 2)
                {
                    return Err("invalid cell width".into());
                }
                for field in ["fg", "bg"] {
                    rgb(cell.get(field).ok_or("cell color unavailable")?)?;
                }
                for field in [
                    "bold", "italic", "dim", "hidden", "inverse", "strike", "wrap",
                ] {
                    boolean(cell, field)?;
                }
                if !matches!(
                    cell.get("underline").and_then(Value::as_str),
                    Some("none" | "single" | "double" | "curly" | "dotted" | "dashed")
                ) {
                    return Err("cell underline unavailable".into());
                }
                let color = cell
                    .get("underline_color")
                    .ok_or("underline color unavailable")?;
                if !color.is_null() {
                    rgb(color)?;
                }
                nullable_string(cell, "hyperlink")?;
            }
        }
    }
    if let Some(object) = context.as_object_mut() {
        object.insert("schema".into(), json!(SCHEMA));
        object.insert("readiness".into(), json!({"status":"ready","issues":[]}));
    }
    Ok(context)
}

fn boolean(value: &Value, field: &str) -> Result<(), String> {
    if value.get(field).and_then(Value::as_bool).is_none() {
        return Err(format!("boolean observation unavailable: {field}"));
    }
    Ok(())
}
fn nullable_string(value: &Value, field: &str) -> Result<(), String> {
    if value
        .get(field)
        .is_none_or(|value| !value.is_null() && !value.is_string())
    {
        return Err(format!("string observation unavailable: {field}"));
    }
    Ok(())
}
fn rgb(value: &Value) -> Result<(), String> {
    if value.as_array().is_none_or(|channels| {
        channels.len() != 3
            || channels
                .iter()
                .any(|channel| channel.as_u64().is_none_or(|channel| channel > 255))
    }) {
        return Err("invalid RGB observation".into());
    }
    Ok(())
}

fn finite_number(value: &Value, field: &str, positive: bool) -> Result<(), String> {
    if value
        .get(field)
        .and_then(Value::as_f64)
        .is_none_or(|n| !n.is_finite() || (positive && n <= 0.))
    {
        return Err(format!("invalid measurement: {field}"));
    }
    Ok(())
}
fn validate_tree(tree: &Value) -> Result<(), String> {
    match tree.get("type").and_then(Value::as_str) {
        Some("tab")
            if tree
                .get("tabId")
                .and_then(Value::as_str)
                .is_some_and(|id| !id.is_empty()) =>
        {
            Ok(())
        }
        Some("split") => {
            finite_number(tree, "ratio", true)?;
            if tree
                .get("ratio")
                .and_then(Value::as_f64)
                .is_some_and(|ratio| ratio >= 1.)
                || !matches!(tree.get("axis").and_then(Value::as_str), Some("x" | "y"))
            {
                return Err("invalid authoritative split".into());
            }
            validate_tree(tree.get("first").ok_or("split first unavailable")?)?;
            validate_tree(tree.get("second").ok_or("split second unavailable")?)
        }
        _ => Err("invalid authoritative tree".into()),
    }
}
fn validate_font(font: &Value) -> Result<(), String> {
    if font
        .get("family")
        .and_then(Value::as_str)
        .is_none_or(str::is_empty)
        || !matches!(
            font.get("size_unit").and_then(Value::as_str),
            Some("pt" | "px")
        )
    {
        return Err("measured font unavailable".into());
    }
    finite_number(font, "size", true)
}

pub fn publish(
    guard: &crate::guard::WriteGuard,
    path: &std::path::Path,
    value: &Value,
) -> Result<(), String> {
    let bytes = serde_json::to_vec_pretty(value).map_err(|error| error.to_string())?;
    guard
        .write_atomic(path, &bytes, "layout.")
        .map_err(|error| format!("{error:?}"))
}

pub fn measured_font(
    widget: &gtk::Widget,
    requested: &pango::FontDescription,
) -> Result<Value, String> {
    use pango::prelude::*;
    let resolved = widget
        .pango_context()
        .load_font(requested)
        .ok_or("Pango font unavailable")?
        .describe();
    let family = resolved.family().ok_or("Pango family unavailable")?;
    Ok(
        json!({"family":family.to_string(),"size":f64::from(resolved.size()) / f64::from(pango::SCALE),"size_unit":if resolved.is_size_absolute(){"px"}else{"pt"},"weight":format!("{:?}",resolved.weight()).to_lowercase(),"style":format!("{:?}",resolved.style()).to_lowercase()}),
    )
}

pub fn owned_widget(
    window: &gtk::Window,
    id: &str,
    role: &str,
    widget: &gtk::Widget,
    label: Option<&str>,
) -> Value {
    let observed = widget
        .downcast_ref::<gtk::Label>()
        .map(|label| label.text().to_string())
        .or_else(|| {
            widget
                .downcast_ref::<gtk::Button>()
                .and_then(|button| button.label())
                .map(|s| s.to_string())
        });
    let label = observed.as_deref().or(label);
    let font = label
        .map(|_| measured_font(widget, &widget.style_context().font(widget.state_flags())))
        .transpose();
    let geometry = widget.is_mapped().then(|| widget.translate_coordinates(window, 0, 0).map(|(x,y)| json!({"x":x,"y":y,"width":widget.allocated_width(),"height":widget.allocated_height()}))).flatten();
    json!({"id":id,"role":role,"label":label,"font":font.ok().flatten(),"visible":widget.is_visible(),"mapped":widget.is_mapped(),"focused":window.focused_widget().is_some_and(|focus| focus == *widget || focus.is_ancestor(widget)),"geometry":geometry})
}

pub fn capture(window: &gtk::Window) -> Value {
    let (width, height) = window.size();
    json!({
        "widget": window.widget_name().to_string(),
        "title": window.title().map(|s| s.to_string()),
        "geometry": {"width": width, "height": height},
        "children": children(window.upcast_ref::<gtk::Widget>()),
    })
}

fn children(widget: &gtk::Widget) -> Vec<Value> {
    let Some(container) = widget.dynamic_cast_ref::<gtk::Container>() else {
        return Vec::new();
    };
    container
        .children()
        .into_iter()
        .map(|child| {
            let alloc = child.allocation();
            json!({
                "widget": child.widget_name().to_string(),
                "type": child.type_().name(),
                "geometry": {
                    "x": alloc.x(),
                    "y": alloc.y(),
                    "width": alloc.width(),
                    "height": alloc.height(),
                },
                "children": children(&child),
            })
        })
        .collect()
}
