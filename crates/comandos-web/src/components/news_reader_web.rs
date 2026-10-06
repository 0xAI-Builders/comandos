//! Delegates the unchanged native content component to its separate Rust artifact.
use comandos_web_dom::{bridge::global_get, port::invoke};
use wasm_bindgen::JsValue;
fn phase(key: &str) -> Result<(), JsValue> {
    invoke(&global_get(key), &[]).map(|_| ())
}
pub fn mount() -> Result<(), JsValue> {
    phase("__comandosMountNewsReader")
}
pub fn attach() -> Result<(), JsValue> {
    phase("__comandosAttachNewsReader")
}
pub fn retire_vendor() -> Result<(), JsValue> {
    phase("__comandosRetireNewsVendor")
}
