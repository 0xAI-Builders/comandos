//! Pruebas en navegador del arranque. Aquí solo se compilan; las corre A12/B4
//! en el Chrome del Mac, nunca un navegador local.
#![cfg(target_arch = "wasm32")]

use comandos_web_dom::bridge;
use js_sys::{Function, Reflect};
use wasm_bindgen::JsValue;
use wasm_bindgen_test::{wasm_bindgen_test, wasm_bindgen_test_configure};

wasm_bindgen_test_configure!(run_in_browser);

#[wasm_bindgen_test]
fn boot_reports_unknown_ids_and_marks_ready() {
    let meta = comandos_web_dom::dom::create("meta").unwrap();
    meta.set_attribute("name", "comandos-web").unwrap();
    meta.set_attribute("content", "nada nada otra").unwrap();
    comandos_web_dom::dom::query("head")
        .unwrap()
        .append_child(&meta)
        .unwrap();
    let f = Function::new_with_args(
        "path, opt",
        "window.__ready = [path, opt.body]; return Promise.resolve({ok: true});",
    );
    bridge::global_set("fetch", &f).unwrap();
    comandos_web::boot("n1");
    assert_eq!(bridge::global_get("__comandosReady"), JsValue::TRUE);
    let sent: js_sys::Array = bridge::global_get("__ready").into();
    assert_eq!(sent.get(0).as_string().as_deref(), Some("/web/ready"));
    assert_eq!(
        sent.get(1).as_string().as_deref(),
        Some(
            r#"{"k":"n1","mounted":[],"failed":[{"id":"nada","error":"componente desconocido en este WASM"},{"id":"otra","error":"componente desconocido en este WASM"}]}"#
        )
    );
    let _ = Reflect::delete_property(&js_sys::global(), &"__comandosReady".into());
    meta.remove();
}
