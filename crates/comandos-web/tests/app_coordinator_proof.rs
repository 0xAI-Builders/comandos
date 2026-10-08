//! Isolated compiled-WASM entry point for the source oracle. No registry edits.
#![cfg(target_arch = "wasm32")]
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
#[allow(dead_code)] // Shared boundary helpers unused by this isolated component.
#[path = "../src/components/web_support.rs"]
pub(crate) mod web_support;
mod components {
    pub(crate) use crate::web_support;
}
#[path = "../src/components/app_coordinator.rs"]
mod app_coordinator;

#[wasm_bindgen::prelude::wasm_bindgen]
pub fn proof_app_coordinator() -> Result<(), wasm_bindgen::JsValue> {
    let _combined: fn() -> Result<(), wasm_bindgen::JsValue> = app_coordinator::mount;
    for mount in [
        app_coordinator::mount_app,
        app_coordinator::mount_identity,
        app_coordinator::mount_sidebar,
        app_coordinator::mount_render,
    ] {
        mount()?;
    }
    Ok(())
}
