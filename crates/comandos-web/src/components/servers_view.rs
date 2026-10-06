use serde_json::{Value, json};
pub fn form_payload(form: &Value) -> Value {
    let mut v = json!({});
    for name in ["host", "hostname", "user", "port", "identity"] {
        if let Some(m) = v.as_object_mut() {
            m.insert(
                name.into(),
                json!(
                    form.get(name)
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .trim_matches(|c: char| c.is_whitespace() || c == '\u{feff}')
                ),
            );
        }
    }
    v
}
pub fn target(h: &Value) -> String {
    let s = |k: &str| match h.get(k) {
        Some(Value::String(v)) => v.clone(),
        Some(Value::Number(v)) => v.to_string(),
        _ => String::new(),
    };
    format!(
        "{}{}{}",
        if s("user").is_empty() {
            String::new()
        } else {
            format!("{}@", s("user"))
        },
        s("hostname"),
        if s("port").is_empty() || h.get("port").and_then(Value::as_f64) == Some(0.0) {
            String::new()
        } else {
            format!(":{}", s("port"))
        }
    )
}
#[derive(Debug, PartialEq, Eq)]
pub enum PanelPlan {
    Immediate,
    Hide,
    Show,
}
pub fn panel_plan(
    hidden: bool,
    animate: bool,
    exists: bool,
    can_animate: bool,
    reduced: bool,
    width: f64,
) -> PanelPlan {
    if !animate || !exists || !can_animate || reduced || (hidden && width == 0.0) {
        PanelPlan::Immediate
    } else if hidden {
        PanelPlan::Hide
    } else {
        PanelPlan::Show
    }
}
pub fn menu_left(left: f64, viewport: f64, width: f64) -> f64 {
    left.min(viewport - width - 8.0).max(8.0)
}
