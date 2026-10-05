//! Pruebas en navegador del arranque. Aquí solo se compilan; las corre A12/B4
//! en el Chrome del Mac, nunca un navegador local.
#![cfg(target_arch = "wasm32")]

use comandos_web::registry::{Component, boot_with};
use comandos_web_dom::bridge;
use js_sys::{Function, Reflect};
use wasm_bindgen::JsValue;
use wasm_bindgen_test::{wasm_bindgen_test, wasm_bindgen_test_configure};

wasm_bindgen_test_configure!(run_in_browser);

fn meta(content: &str) -> web_sys::Element {
    let m = comandos_web_dom::dom::create("meta").unwrap();
    m.set_attribute("name", "comandos-web").unwrap();
    m.set_attribute("content", content).unwrap();
    comandos_web_dom::dom::query("head")
        .unwrap()
        .append_child(&m)
        .unwrap();
    m
}

fn capture_ready() {
    let f = Function::new_with_args(
        "path, opt",
        "(window.__ready = window.__ready || []).push([path, opt.body]); return Promise.resolve({ok: true});",
    );
    bridge::global_set("__ready", &js_sys::Array::new()).unwrap();
    bridge::global_set("fetch", &f).unwrap();
}

fn last_ready_body() -> Option<String> {
    let sent: js_sys::Array = bridge::global_get("__ready").into();
    let last: js_sys::Array = sent.pop().into();
    last.get(1).as_string()
}

fn ok() -> Result<(), JsValue> {
    bridge::global_set("__mountedOk", &JsValue::TRUE)
}
fn throws() -> Result<(), JsValue> {
    // Una excepción de JS que atraviesa el WASM sin `catch`.
    wasm_bindgen::throw_str("lanzó en mount")
}
fn fails() -> Result<(), JsValue> {
    Err(js_sys::Error::new("devolvió Err").into())
}
fn attach_ok() -> Result<(), JsValue> {
    bridge::global_set("__attachedOk", &JsValue::TRUE)
}

static TEST: &[Component] = &[
    Component {
        id: "t-throw",
        mount: throws,
        attach: None,
    },
    Component {
        id: "t-err",
        mount: fails,
        attach: None,
    },
    Component {
        id: "t-ok",
        mount: ok,
        attach: Some(attach_ok),
    },
];

#[wasm_bindgen_test]
fn a_throwing_mount_is_recorded_and_the_rest_still_mount() {
    let m = meta("t-throw t-err t-ok");
    capture_ready();
    boot_with("n2", TEST);
    assert_eq!(bridge::global_get("__mountedOk"), JsValue::TRUE);
    assert_eq!(
        last_ready_body().as_deref(),
        Some(
            r#"{"k":"n2","mounted":["t-ok"],"failed":[{"id":"t-throw","error":"lanzó en mount"},{"id":"t-err","error":"devolvió Err"}]}"#
        )
    );
    // El documento de la prueba ya cargó: `attach` corre en el acto.
    assert_eq!(bridge::global_get("__attachedOk"), JsValue::TRUE);
    assert_eq!(bridge::global_get("__comandosAttached"), JsValue::TRUE);
    m.remove();
}

#[wasm_bindgen_test]
fn boot_reports_unknown_ids_marks_ready_and_runs_once() {
    let m = meta("nada nada otra");
    capture_ready();
    let _ = Reflect::delete_property(&js_sys::global(), &"__comandosReady".into());
    comandos_web::boot("n1");
    assert_eq!(bridge::global_get("__comandosReady"), JsValue::TRUE);
    assert_eq!(
        last_ready_body().as_deref(),
        Some(
            r#"{"k":"n1","mounted":[],"failed":[{"id":"nada","error":"componente desconocido en este WASM"},{"id":"otra","error":"componente desconocido en este WASM"}]}"#
        )
    );
    comandos_web::boot("n1");
    let sent: js_sys::Array = bridge::global_get("__ready").into();
    assert_eq!(sent.length(), 0, "la segunda llamada no vuelve a avisar");
    m.remove();
}
