//! Modal policy shared by native panels and mode-profiled web views.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModalKind {
    Url,
    Chains,
    Analytics,
    Help,
    Snippets,
}
pub fn modal_size(kind: ModalKind, width: i32, height: i32) -> (i32, i32) {
    match kind {
        ModalKind::Chains => (
            (width - 120).clamp(640, 1040),
            (height - 90).clamp(420, 700),
        ),
        ModalKind::Analytics => (
            (width - 120).clamp(640, 1180),
            (height - 90).clamp(480, 940),
        ),
        _ => ((width - 120).max(640), (height - 90).max(420)),
    }
}
pub fn close_allowed(dismissable: bool, escape: bool, backdrop: bool) -> bool {
    dismissable && (escape || backdrop)
}
pub fn show_modal_panel(
    app: &std::rc::Rc<super::app::App>,
    panel: &gtk::Widget,
    dismissable: bool,
    on_close: std::rc::Rc<dyn Fn()>,
) {
    app.mount_custom_panel(panel, dismissable, on_close);
}
pub fn chain_refresh(message: &serde_json::Value) -> Option<String> {
    let what = message
        .get("chainModal")
        .and_then(serde_json::Value::as_str)?;
    if !matches!(what, "saved" | "run") {
        return None;
    }
    let slug = message
        .get("slug")
        .filter(|v| comandos_core::json::truthy(v))
        .map(comandos_core::pomodoro::python_str)
        .unwrap_or_default()
        .chars()
        .filter(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == '-')
        .take(80)
        .collect::<String>();
    let tail = if what == "run" && !slug.is_empty() {
        format!(
            ".then(()=>window.commandSidebar.startChain({}))",
            serde_json::json!(slug)
        )
    } else {
        String::new()
    };
    Some(format!(
        "window.commandSidebar&&window.commandSidebar.refresh(){tail}"
    ))
}
