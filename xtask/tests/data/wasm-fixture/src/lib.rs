//! Fixture de `xtask web-build`: exporta `boot(k)` como lo hará `comandos-web`.
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub fn boot(k: &str) -> usize {
    k.len()
}
