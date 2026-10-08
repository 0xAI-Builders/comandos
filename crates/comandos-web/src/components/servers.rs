//! SSH management and header controls use the existing live foundation globals.
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::string_slice,
    clippy::panic,
    clippy::unreachable,
    clippy::todo,
    clippy::unimplemented
)]
#[path = "servers_view.rs"]
pub mod view;
#[cfg(target_arch = "wasm32")]
#[path = "servers_web.rs"]
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
