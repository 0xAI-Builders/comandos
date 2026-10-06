//! `localStorage` y `sessionStorage` con las guardas del JS heredado: leer
//! nunca lanza (como `try{ return localStorage.getItem(k) }catch(e){ return null }`);
//! escribir y borrar devuelven el error (cuota llena, almacenamiento vetado)
//! para que cada port decida si lo calla, como hace su JS.

use wasm_bindgen::JsValue;
use web_sys::Storage;

fn local() -> Result<Storage, JsValue> {
    web_sys::window()
        .ok_or(JsValue::NULL)?
        .local_storage()?
        .ok_or(JsValue::NULL)
}

fn session() -> Result<Storage, JsValue> {
    web_sys::window()
        .ok_or(JsValue::NULL)?
        .session_storage()?
        .ok_or(JsValue::NULL)
}

/// `localStorage.getItem(key)`; `None` si falta o si el almacenamiento no
/// está disponible.
pub fn get(key: &str) -> Option<String> {
    local().ok()?.get_item(key).ok().flatten()
}

/// `localStorage.setItem(key, value)`.
pub fn set(key: &str, value: &str) -> Result<(), JsValue> {
    local()?.set_item(key, value)
}

/// `localStorage.removeItem(key)`.
pub fn remove(key: &str) -> Result<(), JsValue> {
    local()?.remove_item(key)
}

/// `sessionStorage.getItem(key)`.
pub fn session_get(key: &str) -> Option<String> {
    session().ok()?.get_item(key).ok().flatten()
}

/// `sessionStorage.setItem(key, value)`.
pub fn session_set(key: &str, value: &str) -> Result<(), JsValue> {
    session()?.set_item(key, value)
}

/// `sessionStorage.removeItem(key)`.
pub fn session_remove(key: &str) -> Result<(), JsValue> {
    session()?.remove_item(key)
}
