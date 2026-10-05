//! `api(path, body)` del tablero (región «red» de `index.html`), byte a byte:
//!
//! ```js
//! async function api(path, body){
//!   const headers = {};
//!   const tok = authToken();
//!   if(tok) headers["X-Comandos-Token"] = tok;
//!   const opt = body
//!     ? {method:"POST", headers:{...headers, "Content-Type":"application/json"}, body: JSON.stringify(body)}
//!     : (tok ? {headers} : undefined);
//!   const r = await fetch(path, opt);
//!   const j = await r.json().catch(()=>({}));
//!   if(!r.ok || (body && j.ok === false)) throw new Error(j.error || j.message || r.statusText || "La acción no se completó");
//!   return j;
//! }
//! ```
//!
//! Detalles que se conservan a propósito:
//!
//! - `fetch` se toma de `window` en cada llamada (`Reflect.get`), no del enlace
//!   estático, para que los dobles de prueba que sustituyen `window.fetch`
//!   sigan funcionando; y la respuesta se lee por propiedades (`ok`,
//!   `statusText`, `json()`), sin exigir un `Response` de verdad.
//! - `opt` son objetos planos con las claves en el orden del JS; sin token y
//!   sin cuerpo se pasa `undefined` como segundo argumento, igual que el JS.
//! - Un cuerpo «falso» en JS (`null`, `false`, `0`, `""`) hace GET: `post`
//!   también (ver [`js_truthy`]).
//! - El cuerpo se envía con `JSON.stringify` del valor ya convertido a JS, así
//!   que los bytes son los del JS (orden de claves y texto de números).
//! - La respuesta pasa por `r.json()` y vuelve a texto con `JSON.stringify`
//!   antes de llegar a `serde_json`: las claves quedan en el orden de JS (índices
//!   enteros primero; `preserve_order`) y cada número conserva el texto de
//!   `String(n)` (`arbitrary_precision`). Así `value.to_string()` de un número
//!   pinta lo mismo que el JS.
//! - Un `r.json()` que falla da `{}` (como `.catch(()=>({}))`).

use serde_json::Value;

/// Último recurso del mensaje de error, literal del JS.
pub const DEFAULT_ERROR: &str = "La acción no se completó";

/// Error de una llamada: el `message` del `Error` que lanzaría el JS.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApiError {
    pub message: String,
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for ApiError {}

/// Truthiness de JS sobre un valor JSON: falsos `null`, `false`, `0`, `-0` y
/// `""`; verdaderos el resto, incluidos `{}` y `[]`.
pub fn js_truthy(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => !json_number_is_zero(&n.to_string()),
        Value::String(s) => !s.is_empty(),
        Value::Array(_) | Value::Object(_) => true,
    }
}

/// Un número JSON vale cero si todas las cifras de su mantisa son `0`
/// (`0`, `-0`, `0.00`, `0e7`…). Se mira el texto, sin convertir a `f64`: así el
/// WASM no arrastra el analizador de coma flotante. Un subdesbordamiento
/// (`1e-400`, que JS lee como 0) cuenta como verdadero; ni el servidor ni
/// `JSON.stringify` lo producen.
fn json_number_is_zero(text: &str) -> bool {
    text.bytes()
        .take_while(|b| !matches!(b, b'e' | b'E'))
        .all(|b| matches!(b, b'0' | b'-' | b'+' | b'.'))
}

/// `String(v)` de JS para un valor JSON. Los números dan su texto, que con
/// `arbitrary_precision` es el que escribió `JSON.stringify` (= `String(n)`).
pub fn js_string_of(v: &Value) -> String {
    match v {
        Value::Null => "null".into(),
        Value::Bool(b) => if *b { "true" } else { "false" }.into(),
        Value::Number(n) => n.to_string(),
        Value::String(s) => s.clone(),
        Value::Object(_) => "[object Object]".into(),
        // `Array.prototype.join(",")`: `null`/`undefined` dan cadena vacía.
        Value::Array(items) => items
            .iter()
            .map(|x| {
                if x.is_null() {
                    String::new()
                } else {
                    js_string_of(x)
                }
            })
            .collect::<Vec<_>>()
            .join(","),
    }
}

/// `j[key]` de JS: un objeto da su campo (o `undefined` = `None`); un
/// primitivo o una lista, `undefined`. `null` lanza (lo trata el llamador).
fn field<'a>(j: &'a Value, key: &str) -> Option<&'a Value> {
    j.as_object().and_then(|m| m.get(key))
}

/// TypeError de leer una propiedad de `null` (texto de Chromium). Sin
/// `format!`: el formateador pesa en el WASM.
fn null_read(key: &str) -> String {
    ["Cannot read properties of null (reading '", key, "')"].concat()
}

/// Regla de error de `api()`. `None` = la llamada devuelve `j`; `Some(msg)` =
/// lanza `Error(msg)`. `status_text` es `r.statusText`; `is_post` es la
/// truthiness del cuerpo; `ok` es `r.ok`.
///
/// `j === null` (un servidor que responde `null`) hace que el JS lance un
/// TypeError al leer `j.error` o `j.ok`; se reproduce con el texto de Chromium
/// (WebKitGTK lo redacta distinto).
pub fn error_message(status_text: &str, j: &Value, is_post: bool, ok: bool) -> Option<String> {
    if ok {
        if !is_post {
            return None;
        }
        if j.is_null() {
            return Some(null_read("ok"));
        }
        if field(j, "ok") != Some(&Value::Bool(false)) {
            return None;
        }
    }
    if j.is_null() {
        return Some(null_read("error"));
    }
    let pick = |key: &str| field(j, key).filter(|v| js_truthy(v)).map(js_string_of);
    Some(
        pick("error")
            .or_else(|| pick("message"))
            .or_else(|| (!status_text.is_empty()).then(|| status_text.to_string()))
            .unwrap_or_else(|| DEFAULT_ERROR.to_string()),
    )
}

#[cfg(target_arch = "wasm32")]
pub use web::{auth_token, get, post};

#[cfg(target_arch = "wasm32")]
mod web {
    use super::{ApiError, error_message};
    use js_sys::{Function, JSON, Object, Promise, Reflect};
    use serde_json::Value;
    use wasm_bindgen::{JsCast, JsValue};
    use wasm_bindgen_futures::JsFuture;

    /// `authToken()`: `localStorage.getItem("cc_token") || ""`, y `""` si el
    /// almacenamiento no está disponible o lanza.
    pub fn auth_token() -> String {
        crate::storage::get("cc_token").unwrap_or_default()
    }

    /// `api(path)`: GET.
    pub async fn get(path: &str) -> Result<Value, ApiError> {
        request(path, &JsValue::UNDEFINED).await
    }

    /// `api(path, body)`: POST si `body` es verdadero en JS; si no, GET.
    pub async fn post(path: &str, body: &Value) -> Result<Value, ApiError> {
        // Serializar un `Value` no falla; si fallara, `JSON.parse("")` da el
        // error. Así no entra el `Display` de los errores de `serde_json`.
        let text = serde_json::to_string(body).unwrap_or_default();
        let js = JSON::parse(&text).map_err(|e| from_js(&e))?;
        request(path, &js).await
    }

    /// `message` de un error de JS (o `String(e)` si no lo tiene).
    pub(crate) fn from_js(e: &JsValue) -> ApiError {
        let message = Reflect::get(e, &"message".into())
            .ok()
            .and_then(|m| m.as_string())
            .unwrap_or_else(|| crate::bridge::js_text(e));
        ApiError { message }
    }

    fn set(obj: &Object, key: &str, v: &JsValue) -> Result<(), ApiError> {
        Reflect::set(obj, &key.into(), v)
            .map(|_| ())
            .map_err(|e| from_js(&e))
    }

    async fn request(path: &str, body: &JsValue) -> Result<Value, ApiError> {
        let win = web_sys::window().ok_or_else(|| ApiError {
            message: "sin window".into(),
        })?;
        let headers = Object::new();
        let tok = auth_token();
        if !tok.is_empty() {
            set(&headers, "X-Comandos-Token", &tok.as_str().into())?;
        }
        let is_post = body.is_truthy();
        let opt: JsValue = if is_post {
            let opt = Object::new();
            set(&opt, "method", &"POST".into())?;
            let h = Object::assign(&Object::new(), &headers);
            set(&h, "Content-Type", &"application/json".into())?;
            set(&opt, "headers", &h)?;
            let text = JSON::stringify(body).map_err(|e| from_js(&e))?;
            set(&opt, "body", &text)?;
            opt.into()
        } else if !tok.is_empty() {
            let opt = Object::new();
            set(&opt, "headers", &headers)?;
            opt.into()
        } else {
            JsValue::UNDEFINED
        };
        let fetch: Function = Reflect::get(&win, &"fetch".into())
            .map_err(|e| from_js(&e))?
            .dyn_into()
            .map_err(|_| ApiError {
                message: "fetch is not a function".into(),
            })?;
        let r = JsFuture::from(Promise::resolve(
            &fetch
                .call2(&win, &path.into(), &opt)
                .map_err(|e| from_js(&e))?,
        ))
        .await
        .map_err(|e| from_js(&e))?;
        // Mismo orden de lecturas que el JS: `r.json()`, luego `r.ok` y
        // `r.statusText`. `r.json().catch(()=>({}))`: una excepción síncrona
        // sí sale (el JS también lanza antes de llegar al `.catch`).
        let json: Function = Reflect::get(&r, &"json".into())
            .map_err(|e| from_js(&e))?
            .dyn_into()
            .map_err(|_| ApiError {
                message: "r.json is not a function".into(),
            })?;
        let pending = json.call0(&r).map_err(|e| from_js(&e))?;
        let j = match JsFuture::from(Promise::resolve(&pending)).await {
            Ok(j) => j,
            Err(_) => Object::new().into(),
        };
        let ok = Reflect::get(&r, &"ok".into())
            .map_err(|e| from_js(&e))?
            .is_truthy();
        let status = Reflect::get(&r, &"statusText".into()).map_err(|e| from_js(&e))?;
        let status = if status.is_truthy() {
            crate::bridge::js_text(&status)
        } else {
            String::new()
        };
        let value = to_value(&j);
        match error_message(&status, &value, is_post, ok) {
            Some(message) => Err(ApiError { message }),
            None => Ok(value),
        }
    }

    /// `j` de JS a `serde_json` por su texto JSON (orden y números de JS).
    /// `undefined` (solo lo da un doble de prueba) cuenta como `null`.
    fn to_value(j: &JsValue) -> Value {
        JSON::stringify(j)
            .ok()
            .and_then(|s| s.as_string())
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or(Value::Null)
    }
}
