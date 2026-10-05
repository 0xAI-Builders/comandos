//! Pruebas en navegador (`wasm-bindgen-test`). Aquí solo se compilan; las
//! corre A12/B4 en el Chrome del Mac, nunca un navegador local.
#![cfg(target_arch = "wasm32")]

use comandos_web_dom::{api, bridge, dom, events, log, storage};
use js_sys::{Function, JSON, Object, Reflect};
use serde_json::json;
use std::cell::Cell;
use std::rc::Rc;
use wasm_bindgen::JsValue;
use wasm_bindgen::prelude::wasm_bindgen;
use wasm_bindgen_test::{wasm_bindgen_test, wasm_bindgen_test_configure};

wasm_bindgen_test_configure!(run_in_browser);

fn get(o: &JsValue, k: &str) -> JsValue {
    Reflect::get(o, &k.into()).unwrap_or(JsValue::UNDEFINED)
}

// Dobles de prueba como módulo (`inline_js`): sin `eval` ni constructor
// `Function`, como el propio crate.
#[wasm_bindgen(inline_js = r#"
export function install_fetch(ok, statusText, body, fails) {
  window.__fetchCalls = [];
  window.fetch = (path, opt) => {
    window.__fetchCalls.push([path, opt]);
    return Promise.resolve({ok, statusText, json: () => fails ? Promise.reject(new SyntaxError("no json")) : Promise.resolve(body)});
  };
}
export function fetch_rejects(msg) { window.fetch = () => Promise.reject(new TypeError(msg)); }
export function engine_message(j) { try { return j.error; } catch (e) { return e.message; } }
export function record_ulog() { window.ulog = (k, n, c, d) => { window.__ulogArgs = [k, n, c, d]; }; }
"#)]
extern "C" {
    fn install_fetch(ok: bool, status_text: &str, body: &JsValue, fails: bool);
    fn fetch_rejects(msg: &str);
    fn engine_message(j: &JsValue) -> String;
    fn record_ulog();
}

/// Sustituye `window.fetch` por un doble que guarda `[path, opt]` en
/// `window.__fetchCalls` y responde `{ok, statusText, json}` con `body` (texto
/// JSON que analiza el motor) o con un `r.json()` que rechaza (`None`).
fn fake_fetch(ok: bool, status_text: &str, body: Option<&str>) {
    let parsed = body.map_or(JsValue::UNDEFINED, |b| JSON::parse(b).unwrap());
    install_fetch(ok, status_text, &parsed, body.is_none());
}

fn last_call() -> (JsValue, JsValue) {
    let calls: js_sys::Array = bridge::global_get("__fetchCalls").into();
    let last: js_sys::Array = calls.pop().into();
    (last.get(0), last.get(1))
}

#[wasm_bindgen_test]
async fn api_get_without_token_passes_undefined_and_returns_js_order() {
    let _ = storage::remove("cc_token");
    fake_fetch(true, "OK", Some(r#"{"b": 1, "2": "x", "a": 1.0}"#));
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
    fake_fetch(true, "", Some(r#"{"ok": true}"#));
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
    fake_fetch(true, "", Some(r#"{"ok": false}"#));
    let e = api::post("/x", &json!({"a": 1})).await.unwrap_err();
    assert_eq!(e.message, api::DEFAULT_ERROR);
    fake_fetch(false, "Bad Gateway", None);
    let e = api::get("/x").await.unwrap_err();
    assert_eq!(e.message, "Bad Gateway");
}

#[wasm_bindgen_test]
async fn api_post_with_a_falsy_body_is_a_get() {
    fake_fetch(true, "", Some("{}"));
    api::post("/x", &json!(null)).await.unwrap();
    let (_, opt) = last_call();
    assert!(opt.is_undefined());
}

#[wasm_bindgen_test]
async fn api_network_failure_passes_the_engine_message() {
    fetch_rejects("Failed to fetch");
    assert_eq!(api::get("/x").await.unwrap_err().message, "Failed to fetch");
}

#[wasm_bindgen_test]
async fn api_http_error_with_empty_body_and_200_non_json() {
    // `r.json()` rechaza en los dos: `j = {}`.
    fake_fetch(false, "", None);
    assert_eq!(
        api::get("/x").await.unwrap_err().message,
        api::DEFAULT_ERROR
    );
    fake_fetch(false, "Not Found", None);
    assert_eq!(api::get("/x").await.unwrap_err().message, "Not Found");
    fake_fetch(true, "OK", None);
    assert_eq!(api::get("/x").await.unwrap(), json!({}));
}

#[wasm_bindgen_test]
async fn api_null_body_error_is_the_engine_type_error() {
    fake_fetch(false, "Bad", Some("null"));
    let got = api::get("/x").await.unwrap_err().message;
    // El mismo texto que da el motor al leer `j.error` de `null`.
    let expected = engine_message(&JsValue::NULL);
    assert_eq!(got, expected);
}

#[wasm_bindgen_test]
async fn api_turns_lone_surrogates_into_u_fffd_and_keeps_deep_nesting() {
    let body = format!(
        r#"{{"t": "x\ud800y", "d": {}1{}}}"#,
        "[".repeat(300),
        "]".repeat(300)
    );
    fake_fetch(true, "", Some(&body));
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
    record_ulog();
    log::ui_log("click", "#btn", "header", 1.5);
    let args: js_sys::Array = bridge::global_get("__ulogArgs").into();
    assert_eq!(
        args.join("|").as_string().as_deref(),
        Some("click|#btn|header|1.5")
    );
    let _ = Reflect::delete_property(&js_sys::global(), &"ulog".into());
}
