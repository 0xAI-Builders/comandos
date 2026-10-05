//! Pruebas en navegador (`wasm-bindgen-test`). Aquí solo se compilan; las
//! corre A12/B4 en el Chrome del Mac, nunca un navegador local.
#![cfg(target_arch = "wasm32")]

use comandos_web_dom::{api, bridge, dom, events, log, storage};
use js_sys::{Function, Object, Reflect};
use serde_json::json;
use std::cell::Cell;
use std::rc::Rc;
use wasm_bindgen::JsValue;
use wasm_bindgen_test::{wasm_bindgen_test, wasm_bindgen_test_configure};

wasm_bindgen_test_configure!(run_in_browser);

fn get(o: &JsValue, k: &str) -> JsValue {
    Reflect::get(o, &k.into()).unwrap_or(JsValue::UNDEFINED)
}

/// Sustituye `window.fetch` por un doble que guarda `[path, opt]` en
/// `window.__fetchCalls` y responde `resp`.
fn fake_fetch(resp: &str) {
    let f = Function::new_with_args(
        "path, opt",
        &format!(
            "(window.__fetchCalls = window.__fetchCalls || []).push([path, opt]); return Promise.resolve({resp});"
        ),
    );
    let _ = bridge::global_set("__fetchCalls", &js_sys::Array::new());
    let _ = bridge::global_set("fetch", &f);
}

fn last_call() -> (JsValue, JsValue) {
    let calls: js_sys::Array = bridge::global_get("__fetchCalls").into();
    let last: js_sys::Array = calls.pop().into();
    (last.get(0), last.get(1))
}

#[wasm_bindgen_test]
async fn api_get_without_token_passes_undefined_and_returns_js_order() {
    let _ = storage::remove("cc_token");
    fake_fetch(
        r#"{ok: true, statusText: "OK", json: () => Promise.resolve({b: 1, 2: "x", a: 1.0})}"#,
    );
    let v = api::get("/state").await.unwrap();
    let (path, opt) = last_call();
    assert_eq!(path.as_string().as_deref(), Some("/state"));
    assert!(opt.is_undefined());
    assert_eq!(
        serde_json::to_string(&v).unwrap(),
        r#"{"2":"x","b":1,"a":1}"#
    );
}

#[wasm_bindgen_test]
async fn api_post_sends_token_then_content_type_and_js_body() {
    storage::set("cc_token", "tk").unwrap();
    fake_fetch(r#"{ok: true, json: () => Promise.resolve({ok: true})}"#);
    api::post("/x", &json!({"b": 1.50, "a": [1]}))
        .await
        .unwrap();
    let (_, opt) = last_call();
    assert_eq!(get(&opt, "method").as_string().as_deref(), Some("POST"));
    let keys = Object::keys(&get(&opt, "headers").into());
    assert_eq!(
        keys.join(",").as_string().as_deref(),
        Some("X-Comandos-Token,Content-Type")
    );
    assert_eq!(
        get(&opt, "body").as_string().as_deref(),
        Some(r#"{"b":1.5,"a":[1]}"#)
    );
    let _ = storage::remove("cc_token");
}

#[wasm_bindgen_test]
async fn api_post_ok_false_and_http_errors_follow_the_rules() {
    fake_fetch(r#"{ok: true, statusText: "", json: () => Promise.resolve({ok: false})}"#);
    let e = api::post("/x", &json!({"a": 1})).await.unwrap_err();
    assert_eq!(e.message, api::DEFAULT_ERROR);
    fake_fetch(
        r#"{ok: false, statusText: "Bad Gateway", json: () => Promise.reject(new Error("no json"))}"#,
    );
    let e = api::get("/x").await.unwrap_err();
    assert_eq!(e.message, "Bad Gateway");
}

#[wasm_bindgen_test]
async fn api_post_with_a_falsy_body_is_a_get() {
    fake_fetch(r#"{ok: true, json: () => Promise.resolve({})}"#);
    api::post("/x", &json!(null)).await.unwrap();
    let (_, opt) = last_call();
    assert!(opt.is_undefined());
}

#[wasm_bindgen_test]
async fn api_network_failure_passes_the_engine_message() {
    let f = Function::new_with_args(
        "p, o",
        "return Promise.reject(new TypeError('Failed to fetch'));",
    );
    bridge::global_set("fetch", &f).unwrap();
    assert_eq!(api::get("/x").await.unwrap_err().message, "Failed to fetch");
}

#[wasm_bindgen_test]
async fn api_http_error_with_empty_body_and_200_non_json() {
    // `r.json()` rechaza en los dos: `j = {}`.
    fake_fetch(r#"{ok: false, statusText: "", json: () => Promise.reject(new SyntaxError("x"))}"#);
    assert_eq!(
        api::get("/x").await.unwrap_err().message,
        api::DEFAULT_ERROR
    );
    fake_fetch(
        r#"{ok: false, statusText: "Not Found", json: () => Promise.reject(new SyntaxError("x"))}"#,
    );
    assert_eq!(api::get("/x").await.unwrap_err().message, "Not Found");
    fake_fetch(r#"{ok: true, statusText: "OK", json: () => Promise.reject(new SyntaxError("x"))}"#);
    assert_eq!(api::get("/x").await.unwrap(), json!({}));
}

#[wasm_bindgen_test]
async fn api_null_body_error_is_the_engine_type_error() {
    fake_fetch(r#"{ok: false, statusText: "Bad", json: () => Promise.resolve(null)}"#);
    let got = api::get("/x").await.unwrap_err().message;
    // El mismo texto que da el motor al leer `j.error` de `null`.
    let expected =
        Function::new_with_args("j", "try { return j.error } catch (e) { return e.message }")
            .call1(&JsValue::UNDEFINED, &JsValue::NULL)
            .unwrap()
            .as_string()
            .unwrap();
    assert_eq!(got, expected);
}

#[wasm_bindgen_test]
async fn api_keeps_lone_surrogates_and_deep_nesting() {
    fake_fetch(
        r#"{ok: true, json: () => Promise.resolve({t: "x\ud800y", d: JSON.parse("[".repeat(300) + "1" + "]".repeat(300))})}"#,
    );
    let v = api::get("/x").await.unwrap();
    assert_eq!(v["t"], "x\u{FFFD}y");
    let mut d = &v["d"];
    let mut n = 0;
    while let Some(a) = d.as_array() {
        d = &a[0];
        n += 1;
    }
    assert_eq!((n, d.to_string()), (300, "1".to_string()));
}

#[wasm_bindgen_test]
fn reexport_replaces_and_drops_the_old_closure() {
    bridge::export_fn0("__b1Again", || {}).unwrap();
    let old: Function = bridge::global_get("__b1Again").into();
    bridge::export_fn0("__b1Again", || {}).unwrap();
    assert!(old.call0(&JsValue::UNDEFINED).is_err(), "la vieja se soltó");
    assert!(bridge::call_global("__b1Again", &[]).is_ok());
}

#[wasm_bindgen_test]
fn bridge_exports_and_calls_globals() {
    bridge::export_fn1("__b1Twice", |v| {
        JsValue::from_f64(v.as_f64().unwrap_or(0.0) * 2.0)
    })
    .unwrap();
    let r = bridge::call_global("__b1Twice", &[JsValue::from_f64(21.0)]).unwrap();
    assert_eq!(r.as_f64(), Some(42.0));
    assert!(bridge::call_global("__b1NoExiste", &[]).is_err());
    bridge::global_set("__b1Num", &JsValue::from_f64(1.0)).unwrap();
    assert!(bridge::call_global("__b1Num", &[]).is_err());
}

#[wasm_bindgen_test]
fn dom_helpers_find_and_fill_elements() {
    let el = dom::create("div").unwrap();
    el.set_id("b1-probe");
    dom::document()
        .unwrap()
        .body()
        .unwrap()
        .append_child(&el)
        .unwrap();
    dom::set_html(&el, maud::html! { i.x { "a" } i.x { "b" } });
    assert!(dom::by_id("b1-probe").is_some());
    assert_eq!(dom::query_all("#b1-probe .x").len(), 2);
    assert!(dom::query("#b1-probe .nada").is_none());
    assert!(dom::query("[[inválido").is_none());
    el.remove();
}

#[wasm_bindgen_test]
fn listener_is_removed_on_drop() {
    let el = dom::create("button").unwrap();
    let hits = Rc::new(Cell::new(0));
    let h = hits.clone();
    let l = events::on(&el, "click", move |_| h.set(h.get() + 1));
    let click = || {
        let ev = web_sys::Event::new("click").unwrap();
        let _ = el.dispatch_event(&ev);
    };
    click();
    drop(l);
    click();
    assert_eq!(hits.get(), 1);
}

#[wasm_bindgen_test]
fn ui_log_delegates_to_the_inline_ulog_when_present() {
    let f = Function::new_with_args("k, n, c, d", "window.__ulogArgs = [k, n, c, d];");
    bridge::global_set("ulog", &f).unwrap();
    log::ui_log("click", "#btn", "header", 1.5);
    let args: js_sys::Array = bridge::global_get("__ulogArgs").into();
    assert_eq!(
        args.join("|").as_string().as_deref(),
        Some("click|#btn|header|1.5")
    );
    let _ = Reflect::delete_property(&js_sys::global(), &"ulog".into());
}
