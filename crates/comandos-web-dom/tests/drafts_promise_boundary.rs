//! Unchanged original device-draft assertions against both implementations.
#![cfg(target_arch = "wasm32")]
use wasm_bindgen::{JsValue, prelude::wasm_bindgen};
use wasm_bindgen_futures::JsFuture;
use wasm_bindgen_test::wasm_bindgen_test;
#[wasm_bindgen(inline_js = r#"
export async function draftCheckSuite(original, checks, native) {
  const {createRequire} = await import('node:module');
  const require = createRequire(import.meta.url);
  const assert = require('node:assert/strict');
  const module = {exports:{}};
  new Function('module','exports',original)(module,module.exports);
  const importLine = "const D = require(path.join(__dirname, '..', 'dash', 'device-drafts.js'));";
  assert(checks.includes(importLine), 'original check import seam changed');
  const body = checks.replace(importLine, '').replace('(async () => {','return (async () => {');
  for (const D of [module.exports,native]) {
    const process = {exitCode:0};
    const logs=[];
    const console = {log:s=>logs.push(s),error:(...args)=>logs.push(args)};
    await new Function('require','__dirname','process','console','D',body)(require,'.',process,console,D);
    assert.equal(process.exitCode,0,JSON.stringify(logs));
    assert.deepEqual(logs,['6/6 device draft checks passed']);
  }
}
"#)]
extern "C" {
    #[wasm_bindgen(js_name = draftCheckSuite)]
    fn check_suite(original: &str, checks: &str, native: &JsValue) -> js_sys::Promise;
}
#[wasm_bindgen_test]
async fn unchanged_original_checks_match_the_shared_promise_frontier() {
    comandos_web_dom::drafts::web::export().unwrap();
    let native = comandos_web_dom::bridge::global_get("ComandosDeviceDrafts");
    JsFuture::from(check_suite(
        include_str!("../../../dash/device-drafts.js"),
        include_str!("../../../tests/device_drafts_checks.cjs"),
        &native,
    ))
    .await
    .unwrap();
}
