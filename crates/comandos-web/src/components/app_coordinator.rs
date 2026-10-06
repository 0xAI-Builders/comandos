//! Combined dashboard/terminal coordinator. State is published before consumers mount.
//! The shared Maps and terminal objects retain their identity across polling and rendering.

/// A touch device splits only in landscape; keyboard viewport shrinkage cannot flip it.
pub fn should_split_layout(width: f64, height: f64, coarse: bool) -> bool {
    if coarse {
        width > height && width >= 748.0
    } else {
        width >= 900.0
    }
}
pub fn split_bounds(width: f64) -> (f64, f64) {
    (300.0, (width - 360.0).clamp(300.0, 760.0))
}
pub fn visible_height(heights: &[f64]) -> f64 {
    heights
        .iter()
        .copied()
        .filter(|h| *h > 0.0)
        .reduce(f64::min)
        .unwrap_or(0.0)
        .round()
}
pub fn row_key(session: &str, pane: &str) -> String {
    if pane.is_empty() {
        session.into()
    } else {
        format!("{session}|{pane}")
    }
}
pub fn tab_model_short(model: &str) -> String {
    let line_start = model
        .char_indices()
        .filter(|(_, c)| matches!(c, '\n' | '\r' | '\u{2028}' | '\u{2029}'))
        .map(|(i, c)| i + c.len_utf8())
        .next_back()
        .unwrap_or(0);
    let bracket = model
        .get(line_start..)
        .and_then(|tail| tail.find('['))
        .map(|offset| line_start + offset);
    let model = bracket
        .and_then(|end| model.get(..end))
        .unwrap_or(model)
        .replacen("claude-", "", 1);
    let suffix = model.rsplit_once('-');
    if let Some((stem, tail)) = suffix
        && (tail == "5"
            || tail.len() == 8
                && tail.starts_with("202")
                && tail.bytes().all(|b| b.is_ascii_digit()))
    {
        stem.into()
    } else {
        model
    }
}
#[cfg(not(target_arch = "wasm32"))]
pub fn mount() -> Result<(), wasm_bindgen::JsValue> {
    Ok(())
}
#[cfg(not(target_arch = "wasm32"))]
pub use mount as mount_app;
#[cfg(not(target_arch = "wasm32"))]
pub use mount as mount_identity;
#[cfg(not(target_arch = "wasm32"))]
pub use mount as mount_sidebar;
#[cfg(not(target_arch = "wasm32"))]
pub use mount as mount_render;
#[cfg(target_arch = "wasm32")]
pub use web::{mount, mount_app, mount_identity, mount_render, mount_sidebar};
#[cfg(target_arch = "wasm32")]
#[path = "app_coordinator_web.rs"]
mod web;
