use comandos_web_dom::{bridge::global_get, port::invoke};
use wasm_bindgen::JsValue;
pub fn mount_render() -> Result<(), JsValue> {
    invoke(&global_get("__comandosMountAnalyticsRenderer"), &[]).map(|_| ())
}
pub fn mount() -> Result<(), JsValue> {
    invoke(&global_get("__comandosMountAnalytics"), &[]).map(|_| ())
}
