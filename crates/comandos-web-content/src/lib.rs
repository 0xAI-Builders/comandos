//! Existing news and analytics components in a separate Rust/WASM artifact.
//! The core registry owns selection; public APIs stay synchronous after transport initialization.
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
use comandos_web_dom::bridge::global_set;
use comandos_web_dom::port::utf16_string as string;
use comandos_web_dom::port::{to_utf16_json as to_json, *};
use comandos_web_view::analytics::Renderer;
use std::{cell::RefCell, rc::Rc};
use wasm_bindgen::{JsValue, prelude::wasm_bindgen};
fn mount_render() -> Result<(), JsValue> {
    let api = object();
    method(&api, "create", |a| {
        let model = to_json(&a.get(0));
        let view = to_json(&a.get(1));
        let r = Rc::new(RefCell::new(Renderer::new(model, view)));
        let out = object();
        let state = r.clone();
        method(&out, "html", move |a| {
            let mut r = state
                .try_borrow_mut()
                .map_err(|_| js_sys::Error::new("Analytics renderer busy"))?;
            Ok(utf16_value(&r.html(&string(&a.get(0)))))
        })?;
        method(&out, "phoneDays", move |_| {
            Ok((r.try_borrow().map(|r| r.phone_days()).unwrap_or(0) as f64).into())
        })?;
        Ok(out)
    })?;
    global_set("AnalyticsRender", &api)
}

mod analytics_ui;
mod news_reader;
#[allow(dead_code)]
#[path = "../../comandos-web/src/components/web_support.rs"]
mod web_support;
/// Registers private native transport hooks; component globals still mount in registry order.
#[wasm_bindgen]
pub fn register_content() -> Result<(), JsValue> {
    for (key, mount) in [
        (
            "__comandosMountAnalytics",
            analytics_ui::mount as fn() -> Result<(), JsValue>,
        ),
        (
            "__comandosMountAnalyticsRenderer",
            mount_render as fn() -> Result<(), JsValue>,
        ),
        (
            "__comandosMountNewsReader",
            news_reader::mount as fn() -> Result<(), JsValue>,
        ),
        (
            "__comandosAttachNewsReader",
            news_reader::attach as fn() -> Result<(), JsValue>,
        ),
        (
            "__comandosRetireNewsVendor",
            news_reader::retire_vendor as fn() -> Result<(), JsValue>,
        ),
    ] {
        global_set(
            key,
            &function(move |_| {
                mount()?;
                Ok(JsValue::UNDEFINED)
            }),
        )?;
    }
    Ok(())
}

fn tab_name(name: &str) -> &'static str {
    match name.to_lowercase().as_str() {
        "comparar" | "proyectos" | "proveedores" => "comparar",
        "pomodoro" => "pomodoro",
        _ => "cuentas",
    }
}
