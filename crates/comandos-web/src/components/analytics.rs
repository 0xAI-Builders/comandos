//! Analytics model renderer and UI controller are an atomic script pair.
pub fn tab_name(name: &str) -> &'static str {
    match name.to_lowercase().as_str() {
        "comparar" | "proyectos" | "proveedores" => "comparar",
        "pomodoro" => "pomodoro",
        "recursos" => "recursos",
        _ => "cuentas",
    }
}
#[cfg(not(target_arch = "wasm32"))]
pub fn mount_render() -> Result<(), wasm_bindgen::JsValue> {
    Ok(())
}
#[cfg(not(target_arch = "wasm32"))]
pub fn mount() -> Result<(), wasm_bindgen::JsValue> {
    Ok(())
}
#[cfg(target_arch = "wasm32")]
#[path = "analytics_web.rs"]
mod web;
#[cfg(target_arch = "wasm32")]
pub use web::{mount, mount_render};
