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
//! - `fetch(path, opt)` y `r.json()` se evalúan en el motor con el mismo texto
//!   que el JS (un módulo JS de `inline_js`, que `wasm-bindgen` emite como
//!   archivo: nada de `eval` ni del constructor `Function`, así que una CSP sin
//!   `unsafe-eval` sigue funcionando mientras permita `'wasm-unsafe-eval'`, que
//!   necesita el propio WASM), de modo que `fetch` se resuelve en cada llamada (los dobles de prueba que
//!   sustituyen `window.fetch` siguen funcionando), la respuesta se lee por
//!   propiedades sin exigir un `Response` de verdad, y un error del motor trae
//!   su propio texto nativo.
//! - `opt` son objetos planos con las claves en el orden del JS; sin token y
//!   sin cuerpo se pasa `undefined` como segundo argumento, igual que el JS.
//! - Un cuerpo «falso» en JS (`null`, `false`, `0`, `""`) hace GET: `post`
//!   también (ver [`js_truthy`]).
//! - El cuerpo se envía con `JSON.stringify` del valor ya convertido a JS, así
//!   que los bytes son los del JS (orden de claves y texto de números).
//! - La respuesta la analiza el motor (`r.json()`) y [`from_tree`] la recorre
//!   tal cual, sin volver a texto ni a `serde_json::from_str` (que corta a 128
//!   niveles y rechaza sustitutos sueltos). Las claves quedan en el orden de JS
//!   (índices enteros primero; `preserve_order`), `undefined` y funciones se
//!   tratan como en `JSON.stringify`, y cada número conserva el texto de
//!   `String(n)` (`arbitrary_precision`): `value.to_string()` pinta lo mismo que
//!   el JS. Un fallo al recorrer (un getter que lanza) es un error, nunca
//!   `null`.
//! - **Divergencia conocida: sustitutos UTF-16 sueltos.** `r.json()` conserva
//!   un `"\ud800"` suelto en la cadena de JS; un `String` de Rust no puede
//!   guardarlo, así que aquí pasa a U+FFFD ([`utf16_lossy`]). Se **pinta**
//!   igual (el motor dibuja U+FFFD para el sustituto), pero el **dato**
//!   difiere: la igualdad con otra cadena, `JSON.stringify` (`"\ud800"` frente
//!   a `"�"`) y `encodeURIComponent` (que lanza `URIError` en JS y no aquí).
//!   El JSON válido del servidor (Rust, UTF-8) no los produce; un port que
//!   reenvíe un valor recibido como clave o id (p. ej. texto crudo de una
//!   transcripción) debe llevarlo por una ruta `JsValue`, no por `Value`.
//! - Un `r.json()` que falla da `{}` (como `.catch(()=>({}))`).
//! - Con `j === null` el JS lanza al leer `j.error` o `j.ok`: [`error_message`]
//!   devuelve [`Failure::ReadNull`] y la lectura se hace de verdad en el motor,
//!   así que el mensaje es el nativo de cada uno (Chromium:
//!   «Cannot read properties of null (reading 'error')»; WebKitGTK:
//!   «null is not an object (evaluating 'j.error')»).

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

/// Por qué falla una llamada según la regla de `api()`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Failure {
    /// `throw new Error(msg)`.
    Message(String),
    /// `j` es `null`/`undefined` y el JS lanza al leer `j.<campo>` (`"error"`
    /// u `"ok"`); el texto del TypeError lo pone el motor.
    ReadNull(&'static str),
}

/// Regla de error de `api()`. `None` = la llamada devuelve `j`. `status_text`
/// es `r.statusText`; `is_post` es la truthiness del cuerpo; `ok` es `r.ok`;
/// `Value::Null` representa un `j` nulo (`null` o `undefined`).
pub fn error_message(status_text: &str, j: &Value, is_post: bool, ok: bool) -> Option<Failure> {
    if ok {
        if !is_post {
            return None;
        }
        if j.is_null() {
            return Some(Failure::ReadNull("ok"));
        }
        if field(j, "ok") != Some(&Value::Bool(false)) {
            return None;
        }
    }
    if j.is_null() {
        return Some(Failure::ReadNull("error"));
    }
    let pick = |key: &str| field(j, key).filter(|v| js_truthy(v)).map(js_string_of);
    Some(Failure::Message(
        pick("error")
            .or_else(|| pick("message"))
            .or_else(|| (!status_text.is_empty()).then(|| status_text.to_string()))
            .unwrap_or_else(|| DEFAULT_ERROR.to_string()),
    ))
}

/// Un nodo de un valor de JS ya analizado, visto por [`from_tree`].
#[derive(Debug, Clone)]
pub enum Node<C> {
    Null,
    Bool(bool),
    /// Con el texto de `String(n)`.
    Number(serde_json::Number),
    Str(String),
    /// Elementos en orden; un hueco o `undefined` va como hijo [`Node::Skip`].
    Array(Vec<C>),
    /// Claves en el orden de `Object.keys`.
    Object(Vec<(String, C)>),
    /// `undefined`, funciones y símbolos: se omiten en objetos y son `null`
    /// en listas, como en `JSON.stringify`.
    Skip,
}

enum Frame<C> {
    Array(Vec<Value>, std::vec::IntoIter<C>),
    Object(
        serde_json::Map<String, Value>,
        std::vec::IntoIter<(String, C)>,
        String,
    ),
}

/// Convierte un valor de JS en `serde_json::Value` sin pasar por texto.
/// Iterativo: no tiene límite de anidamiento ni consume pila por nivel. Un
/// error de `inspect` se devuelve tal cual.
pub fn from_tree<C, E>(
    root: C,
    mut inspect: impl FnMut(&C) -> Result<Node<C>, E>,
) -> Result<Value, E> {
    walk_tree(root, |value, _| inspect(value))
}

// Frames already track the current container depth. Share the traversal with
// bounded browser conversions without rebuilding every child as (value, depth).
fn walk_tree<C, E>(
    root: C,
    mut inspect: impl FnMut(&C, usize) -> Result<Node<C>, E>,
) -> Result<Value, E> {
    let mut stack: Vec<Frame<C>> = Vec::new();
    let mut next = Some(root);
    loop {
        // Visita: un valor hecho (`Some`, con `None` = omitido) o un marco nuevo.
        let mut done: Option<Option<Value>> = match next.take() {
            None => None,
            Some(c) => match inspect(&c, stack.len())? {
                Node::Null => Some(Some(Value::Null)),
                Node::Bool(b) => Some(Some(Value::Bool(b))),
                Node::Number(n) => Some(Some(Value::Number(n))),
                Node::Str(s) => Some(Some(Value::String(s))),
                Node::Skip => Some(None),
                Node::Array(items) => {
                    stack.push(Frame::Array(
                        Vec::with_capacity(items.len()),
                        items.into_iter(),
                    ));
                    None
                }
                Node::Object(fields) => {
                    stack.push(Frame::Object(
                        serde_json::Map::new(),
                        fields.into_iter(),
                        String::new(),
                    ));
                    None
                }
            },
        };
        loop {
            if let Some(v) = done.take() {
                match stack.last_mut() {
                    None => return Ok(v.unwrap_or(Value::Null)),
                    Some(Frame::Array(out, _)) => out.push(v.unwrap_or(Value::Null)),
                    Some(Frame::Object(map, _, key)) => {
                        if let Some(v) = v {
                            map.insert(std::mem::take(key), v);
                        }
                    }
                }
            }
            match stack.last_mut() {
                None => return Ok(Value::Null),
                Some(Frame::Array(_, rest)) => {
                    if let Some(c) = rest.next() {
                        next = Some(c);
                        break;
                    }
                }
                Some(Frame::Object(_, rest, key)) => {
                    if let Some((k, c)) = rest.next() {
                        *key = k;
                        next = Some(c);
                        break;
                    }
                }
            }
            // Marco agotado: es un valor hecho para su padre.
            done = match stack.pop() {
                Some(Frame::Array(out, _)) => Some(Some(Value::Array(out))),
                Some(Frame::Object(map, _, _)) => Some(Some(Value::Object(map))),
                None => return Ok(Value::Null),
            };
        }
    }
}

/// UTF-16 a `String`; un sustituto suelto pasa a U+FFFD.
pub fn utf16_lossy(units: &[u16]) -> String {
    String::from_utf16_lossy(units)
}

#[cfg(target_arch = "wasm32")]
pub use web::{auth_token, get, post, serialized_value};

#[cfg(target_arch = "wasm32")]
mod web {
    use super::{ApiError, Failure, Node, error_message, utf16_lossy, walk_tree};
    use js_sys::{Array, JSON, JsString, Object, Promise, Reflect};
    use serde_json::Value;
    use wasm_bindgen::prelude::wasm_bindgen;
    use wasm_bindgen::{JsCast, JsValue};
    use wasm_bindgen_futures::JsFuture;

    // Las expresiones del JS vivo, evaluadas por el motor con su mismo texto:
    // `fetch` se resuelve en cada llamada y un error trae el mensaje nativo
    // (`j.error` sobre `null`: Chromium «Cannot read properties of null
    // (reading 'error')», WebKitGTK «null is not an object (evaluating
    // 'j.error')»). Es un módulo que `wasm-bindgen` escribe como archivo y
    // `xtask web-build` deja junto al cargador; no usa `eval`.
    #[wasm_bindgen(inline_js = "
export function api_fetch(path, opt) { return fetch(path, opt); }
export function api_json(r) { return r.json(); }
export function api_read_error(j) { return j.error; }
export function api_read_ok(j) { return j.ok; }
")]
    extern "C" {
        #[wasm_bindgen(catch)]
        fn api_fetch(path: &str, opt: &JsValue) -> Result<JsValue, JsValue>;
        #[wasm_bindgen(catch)]
        fn api_json(r: &JsValue) -> Result<JsValue, JsValue>;
        #[wasm_bindgen(catch)]
        fn api_read_error(j: &JsValue) -> Result<JsValue, JsValue>;
        #[wasm_bindgen(catch)]
        fn api_read_ok(j: &JsValue) -> Result<JsValue, JsValue>;
    }

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
        let pending = api_fetch(path, &opt).map_err(|e| from_js(&e))?;
        let r = JsFuture::from(Promise::resolve(&pending))
            .await
            .map_err(|e| from_js(&e))?;
        // Mismo orden de lecturas que el JS: `r.json()`, luego `r.ok` y
        // `r.statusText`. `r.json().catch(()=>({}))`: una excepción síncrona
        // sí sale (el JS también lanza antes de llegar al `.catch`).
        let pending = api_json(&r).map_err(|e| from_js(&e))?;
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
        let value = to_value(&j)?;
        match error_message(&status, &value, is_post, ok) {
            None => Ok(value),
            Some(Failure::Message(message)) => Err(ApiError { message }),
            Some(Failure::ReadNull(key)) => Err(read_in_engine(&j, key)),
        }
    }

    /// Hace en el motor la lectura que lanza en el JS (`j.error` o `j.ok` con
    /// `j` nulo), para que el TypeError lleve el texto nativo del motor.
    fn read_in_engine(j: &JsValue, key: &str) -> ApiError {
        let read = if key == "ok" {
            api_read_ok(j)
        } else {
            api_read_error(j)
        };
        match read {
            Err(e) => from_js(&e),
            Ok(_) => ApiError {
                message: super::DEFAULT_ERROR.to_string(),
            },
        }
    }

    /// Un valor de JS como lo vería `JSON.stringify` (ver [`super::from_tree`]).
    fn json_string(v: &JsValue, policy: Option<bool>) -> Result<String, ApiError> {
        let s: &JsString = v.unchecked_ref();
        if policy == Some(true) {
            return Ok(comandos_web_view::utf16::encode(s.iter()));
        }
        if s.is_valid_utf16() {
            return Ok(String::from(s));
        }
        if policy == Some(false) {
            return Err(ApiError {
                message: "JSON contiene un sustituto UTF-16 suelto".into(),
            });
        }
        Ok(utf16_lossy(&s.iter().collect::<Vec<_>>()))
    }

    fn inspect_with_strings(v: &JsValue, policy: Option<bool>) -> Result<Node<JsValue>, ApiError> {
        if v.is_null() {
            return Ok(Node::Null);
        }
        if let Some(b) = v.as_bool() {
            return Ok(Node::Bool(b));
        }
        if let Some(x) = v.as_f64() {
            if !x.is_finite() {
                return Ok(Node::Null);
            }
            let text = crate::bridge::js_text(v);
            return text.parse().map(Node::Number).map_err(|_| ApiError {
                message: ["número de JS sin forma JSON: ", &text].concat(),
            });
        }
        if v.dyn_ref::<JsString>().is_some() {
            return json_string(v, policy).map(Node::Str);
        }
        if Array::is_array(v) {
            let a: &Array = v.unchecked_ref();
            return Ok(Node::Array((0..a.length()).map(|i| a.get(i)).collect()));
        }
        if !v.is_object() {
            // `undefined`, funciones, símbolos y BigInt.
            return Ok(Node::Skip);
        }
        if v.is_function() {
            return Ok(Node::Skip);
        }
        let obj: &Object = v.unchecked_ref();
        let mut fields = Vec::new();
        for key in Object::keys(obj).iter() {
            let val = Reflect::get(obj, &key).map_err(|e| from_js(&e))?;
            fields.push((json_string(&key, policy)?, val));
        }
        Ok(Node::Object(fields))
    }

    /// Convert a JSON.stringify result using the engine and the shared tree
    /// walker. Unlike API responses, this keeps serde_json's 127-container
    /// limit and either rejects lone surrogates or uses the lossless encoding.
    pub fn serialized_value(text: &str, lossless_utf16: bool) -> Option<Value> {
        browser_value(JSON::parse(text).ok()?, Some(lossless_utf16), 127).ok()
    }

    fn browser_value(
        root: JsValue,
        policy: Option<bool>,
        max_depth: usize,
    ) -> Result<Value, ApiError> {
        walk_tree(root, |value, depth| {
            let node = inspect_with_strings(value, policy)?;
            if depth >= max_depth && matches!(node, Node::Array(_) | Node::Object(_)) {
                return Err(ApiError {
                    message: "JSON demasiado anidado".into(),
                });
            }
            Ok(node)
        })
    }

    fn to_value(j: &JsValue) -> Result<Value, ApiError> {
        browser_value(j.clone(), None, usize::MAX)
    }
}
