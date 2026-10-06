use comandos_web_dom::{bridge::global_get, port::invoke};
use wasm_bindgen::JsValue;
pub fn mount() -> Result<(), JsValue> {
    invoke(&global_get("__comandosMountSounds"), &[]).map(|_| ())
}
