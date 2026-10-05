//! Fixture de `xtask web-build`: exporta `boot(k)` como lo hará `comandos-web`
//! y usa un módulo `inline_js` (snippet) como `comandos-web-dom`.
use wasm_bindgen::prelude::*;

#[wasm_bindgen(inline_js = "export function twice(n) { return n * 2; }")]
extern "C" {
    fn twice(n: usize) -> usize;
}

#[wasm_bindgen]
pub fn boot(k: &str) -> usize {
    twice(k.len())
}
