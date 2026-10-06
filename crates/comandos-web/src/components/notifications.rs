//! Notices: server authority, one long poll, explicit read/open/dismiss actions.
#[cfg(target_arch = "wasm32")]
#[path = "notifications_web.rs"]
mod web;
#[cfg(target_arch = "wasm32")]
pub use web::mount;
#[cfg(not(target_arch = "wasm32"))]
pub fn mount() -> Result<(), wasm_bindgen::JsValue> {
    Ok(())
}
