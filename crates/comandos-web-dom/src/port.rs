//! Browser boundary helpers. All decisions and state belong to Rust components.
use js_sys::{Array, Function, Object, Promise, Reflect};
use wasm_bindgen::{JsCast, JsValue, closure::Closure, prelude::wasm_bindgen};
#[wasm_bindgen(inline_js = "export function spread(f) { return (...args) => f(args); }")]
extern "C" {
    fn spread(f: &JsValue) -> Function;
}
pub fn function(f: impl Fn(Array) -> Result<JsValue, JsValue> + 'static) -> JsValue {
    let c = Closure::<dyn Fn(Array) -> Result<JsValue, JsValue>>::new(f);
    let owned = c.into_js_value();
    let wrapped = spread(&owned);
    wrapped.into()
}
pub fn set(o: &JsValue, k: &str, v: &JsValue) -> Result<(), JsValue> {
    Reflect::set(o, &k.into(), v).map(|_| ())
}
pub fn get(o: &JsValue, k: &str) -> JsValue {
    Reflect::get(o, &k.into()).unwrap_or(JsValue::UNDEFINED)
}
pub fn call(o: &JsValue, k: &str, args: &[JsValue]) -> Result<JsValue, JsValue> {
    let f = get(o, k)
        .dyn_into::<Function>()
        .map_err(|_| js_sys::Error::new(&format!("{k} is not callable")))?;
    f.apply(o, &args.iter().cloned().collect::<Array>())
}
pub fn invoke(f: &JsValue, args: &[JsValue]) -> Result<JsValue, JsValue> {
    f.dyn_ref::<Function>()
        .ok_or_else(|| JsValue::from(js_sys::Error::new("callback is not callable")))?
        .apply(
            &JsValue::UNDEFINED,
            &args.iter().cloned().collect::<Array>(),
        )
}
pub async fn wait(v: Result<JsValue, JsValue>) -> Result<JsValue, JsValue> {
    wasm_bindgen_futures::JsFuture::from(Promise::resolve(&v?)).await
}
pub fn object() -> JsValue {
    Object::new().into()
}
pub fn from_json(v: &serde_json::Value) -> Result<JsValue, JsValue> {
    js_sys::JSON::parse(&v.to_string())
}
pub fn to_json(v: &JsValue) -> serde_json::Value {
    js_sys::JSON::stringify(v)
        .ok()
        .and_then(|s| s.as_string())
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or(serde_json::Value::Null)
}
pub fn string(v: &JsValue) -> String {
    invoke(&get(&js_sys::global(), "String"), std::slice::from_ref(v))
        .ok()
        .and_then(|v| v.as_string())
        .unwrap_or_default()
}
pub fn truthy(v: &JsValue) -> bool {
    v.is_truthy()
}
pub fn method(
    o: &JsValue,
    k: &str,
    f: impl Fn(Array) -> Result<JsValue, JsValue> + 'static,
) -> Result<(), JsValue> {
    set(o, k, &function(f))
}
pub fn getter(o: &JsValue, k: &str, f: impl Fn() -> JsValue + 'static) -> Result<(), JsValue> {
    let desc = object();
    set(&desc, "get", &function(move |_| Ok(f())))?;
    Object::define_property(&Object::from(o.clone()), &k.into(), &Object::from(desc));
    Ok(())
}

pub fn number(v: &JsValue) -> f64 {
    invoke(&get(&js_sys::global(), "Number"), std::slice::from_ref(v))
        .ok()
        .and_then(|v| v.as_f64())
        .unwrap_or(f64::NAN)
}
