//! News reader: owned async rendering and browser events, with server-only Markdown.
#[cfg(target_arch = "wasm32")]
#[path = "news_reader_web.rs"]
mod web;
#[cfg(target_arch = "wasm32")]
pub use web::{attach, mount, remove_cold_listener, retire_vendor};
#[cfg(not(target_arch = "wasm32"))]
pub fn mount() -> Result<(), wasm_bindgen::JsValue> {
    Ok(())
}
#[cfg(not(target_arch = "wasm32"))]
pub fn attach() -> Result<(), wasm_bindgen::JsValue> {
    Ok(())
}

#[cfg(not(target_arch = "wasm32"))]
pub fn retire_vendor() -> Result<(), wasm_bindgen::JsValue> {
    Ok(())
}
