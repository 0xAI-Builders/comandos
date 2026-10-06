//! Node-only WASM proof of the browser JSON boundary; no DOM or network.
#![cfg(target_arch = "wasm32")]
use comandos_web_dom::port;
use serde_json::Value;
use wasm_bindgen::{JsValue, prelude::wasm_bindgen};
use wasm_bindgen_test::wasm_bindgen_test;

#[wasm_bindgen(inline_js = r#"
export function boundaryFixture(which) {
  switch (which) {
    case 0: return {"10":true,"2":false, z:[undefined,NaN,Infinity,-0,1e-7,1e21,5e-324], a:"e\u0301😀"};
    case 1: return {good:"yes", bad:"\ud800"};
    case 2: return {["\udfff"]:"key", good:"yes"};
    case 3: return {text:"\ue000\ud800\ue000\udfff😀", ["\ue000\ud800"]:"\ue000"};
    case 4: return undefined;
    case 5: return 1n;
    case 6: { const v={}; v.self=v; return v; }
    case 7: return {toJSON(){return {a:[true,"json hook",-0]};}};
    case 8: return {get x(){throw Error("getter fails");}};
    case 9: return Object.assign(Object.create(null), {"__proto__":"literal", constructor:"field"});
    default: { let v="leaf"; for(let i=0;i<which;i++)v=[v]; return v; }
  }
}
export function hookFixture(trace) { return {toJSON(){trace.push("toJSON");return {get value(){trace.push("getter");return "original";}};}}; }
"#)]
extern "C" {
    #[wasm_bindgen(js_name = boundaryFixture)]
    fn fixture(which: u32) -> JsValue;
    #[wasm_bindgen(js_name = hookFixture)]
    fn hook_fixture(trace: &js_sys::Array) -> JsValue;
}
fn previous(v: &JsValue, utf16: bool) -> Value {
    js_sys::JSON::stringify(v)
        .ok()
        .and_then(|s| s.as_string())
        .and_then(|s| {
            let source = if utf16 {
                comandos_web_view::utf16::json_to_unicode(&s)
            } else {
                s
            };
            serde_json::from_str(&source).ok()
        })
        .unwrap_or(Value::Null)
}
#[wasm_bindgen_test]
fn ordinary_and_lossless_values_match_previous_boundary() {
    for which in (0..10).chain([126, 127, 128, 129]) {
        for utf16 in [false, true] {
            let actual = if utf16 {
                port::to_utf16_json(&fixture(which))
            } else {
                port::to_json(&fixture(which))
            };
            assert_eq!(
                actual,
                previous(&fixture(which), utf16),
                "case={which} utf16={utf16}"
            );
        }
    }
}
#[wasm_bindgen_test]
fn json_hooks_run_once_with_original_order() {
    let trace = js_sys::Array::new();
    assert_eq!(
        port::to_json(&hook_fixture(&trace)),
        serde_json::json!({"value":"original"})
    );
    assert_eq!(trace.length(), 2);
    assert_eq!(trace.get(0), JsValue::from_str("toJSON"));
    assert_eq!(trace.get(1), JsValue::from_str("getter"));
}
#[wasm_bindgen_test]
fn serde_internal_names_remain_ordinary_js_fields() {
    for key in [
        "$serde_json::private::Number",
        "$serde_json::private::RawValue",
    ] {
        let obj = js_sys::Object::new();
        js_sys::Reflect::set(&obj, &key.into(), &"ordinary text".into()).unwrap();
        let actual = port::to_json(&obj);
        assert_eq!(actual[key], "ordinary text");
        assert_eq!(
            js_sys::JSON::stringify(&port::from_json(&actual).unwrap()).unwrap(),
            js_sys::JSON::stringify(&obj).unwrap()
        );
    }
}
