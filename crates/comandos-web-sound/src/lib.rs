//! Eager native audio preserves trusted gesture activation and synchronous playback handles.
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
mod ui_sounds;
use comandos_web_dom::{bridge::global_set, port::function};
use wasm_bindgen::{JsValue, prelude::wasm_bindgen};
#[wasm_bindgen]
pub fn register_sound() -> Result<(), JsValue> {
    global_set(
        "__comandosMountSounds",
        &function(|_| {
            ui_sounds::mount()?;
            Ok(JsValue::UNDEFINED)
        }),
    )
}
