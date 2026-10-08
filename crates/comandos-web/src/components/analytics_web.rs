use comandos_web_dom::port::*;
use wasm_bindgen::JsValue;
pub fn mount_render() -> Result<(), JsValue> {
    comandos_web_dom::bridge::global_set("__comandosDeferredAnalyticsRender", &JsValue::TRUE)?;
    super::super::content_runtime::factory("AnalyticsRender", &["create"])?;
    Ok(())
}
pub fn mount() -> Result<(), JsValue> {
    super::super::content_runtime::declare("analytics")?;
    comandos_web_dom::bridge::global_set("__comandosDeferredAnalytics", &JsValue::TRUE)?;
    comandos_web_dom::bridge::global_set("__comandosAnalyticsDeferred", &JsValue::TRUE)?;
    let api = super::super::content_runtime::factory("Analytics", &["create"])?;
    set(
        &api,
        "TABS",
        &from_json(&serde_json::json!(["cuentas", "comparar", "pomodoro"]))?,
    )?;
    method(&api, "tabName", |args| {
        Ok(super::tab_name(&string(&args.get(0))).into())
    })
}
