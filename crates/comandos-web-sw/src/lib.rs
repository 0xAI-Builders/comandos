//! Classic service-worker runtime. JS glue only instantiates this dedicated WASM.
pub mod policy;
use wasm_bindgen::prelude::*;
#[cfg(target_arch = "wasm32")]
mod worker;
#[wasm_bindgen]
pub fn boot(native: bool, precache: JsValue) -> Result<(), JsValue> {
    #[cfg(target_arch = "wasm32")]
    return worker::boot(native, precache);
    #[cfg(not(target_arch = "wasm32"))]
    {
        let _ = (native, precache);
        Ok(())
    }
}
