//! Registro LOCAL de uso (región «registro LOCAL de uso» de `index.html`):
//! mismo endpoint (`POST /ui-log`), misma forma y mismos tiempos que el JS:
//!
//! ```js
//! function ulog(k, n, c, d){
//!   if(!n) return;
//!   const sel = (typeof activePaneTarget === "function" && activePaneTarget()?.session) || "";
//!   ULOG.buf.push({ts: Date.now() / 1000, k, n: String(n).slice(0, 80), c: String(c || "").slice(0, 80), d: Math.round(d || 0), s: sel});
//!   if(ULOG.buf.length >= 40) ulogFlush();
//! }
//! function ulogFlush(){
//!   if(!ULOG.buf.length) return;
//!   const events = ULOG.buf.splice(0, 200);
//!   const body = JSON.stringify({events});
//!   try{
//!     if(navigator.sendBeacon && !inApp()) navigator.sendBeacon("/ui-log", new Blob([body], {type: "application/json"}));
//!     else fetch("/ui-log", {method: "POST", headers: {"Content-Type": "application/json"}, body, keepalive: true}).catch(()=>{});
//!   }catch(e){}
//! }
//! setInterval(ulogFlush, 5000);
//! ```
//!
//! `ui_log` es `ulog(k, n, c, d)`. Mientras la región siga en JS, delega en
//! `window.ulog` (un solo búfer y un solo temporizador para toda la página);
//! sin ella usa el búfer propio de este módulo con la misma semántica:
//! `slice(0, 80)` por unidades UTF-16 (`String.prototype.slice`, no por bytes),
//! `Math.round`, `JSON.stringify` del objeto en el orden `ts, k, n, c, d, s`, y
//! el `fetch` de `/ui-log` **sin** cabecera de token, como el JS. El
//! temporizador de 5 s y el vaciado en `pagehide` se instalan con el primer
//! evento propio (vaciar un búfer vacío no hace nada, así que es equivalente).
//!
//! El plan de B1 pedía `ui_log(event, data)`; el JS vivo es `ulog(k, n, c, d)` y
//! manda el JS vivo.

/// Con tantos eventos en el búfer se vacía sin esperar al temporizador.
pub const FLUSH_AT: usize = 40;
/// Eventos por envío (`splice(0, 200)`).
pub const BATCH: usize = 200;
/// Periodo del vaciado (`setInterval(ulogFlush, 5000)`).
pub const FLUSH_EVERY_MS: i32 = 5000;
/// Longitud máxima de `n` y `c`, en unidades UTF-16.
pub const TEXT_MAX: u32 = 80;

/// Búfer de eventos con las reglas de `ULOG.buf`.
#[derive(Debug)]
pub struct Buffer<T> {
    items: Vec<T>,
}

impl<T> Default for Buffer<T> {
    fn default() -> Self {
        Buffer { items: Vec::new() }
    }
}

impl<T> Buffer<T> {
    /// Añade un evento; `true` si ya toca vaciar (`length >= 40`).
    pub fn push(&mut self, item: T) -> bool {
        self.items.push(item);
        self.items.len() >= FLUSH_AT
    }

    /// Los primeros 200 eventos (o menos), que salen del búfer.
    pub fn take_batch(&mut self) -> Vec<T> {
        let n = self.items.len().min(BATCH);
        self.items.drain(..n).collect()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

/// `Math.round(d || 0)`: el entero más cercano con los empates hacia +∞;
/// `NaN` cuenta como 0.
pub fn js_round(d: f64) -> f64 {
    if d.is_nan() {
        return 0.0;
    }
    let f = d.floor();
    if d - f >= 0.5 { f + 1.0 } else { f }
}

#[cfg(target_arch = "wasm32")]
pub use web::{ui_log, ui_log_flush, ui_log_native};

#[cfg(target_arch = "wasm32")]
mod web {
    use super::{Buffer, FLUSH_EVERY_MS, TEXT_MAX, js_round};
    use crate::bridge::call_global;
    use js_sys::{Array, Date, Function, JSON, JsString, Object, Promise, Reflect};
    use std::cell::{Cell, RefCell};
    use wasm_bindgen::closure::Closure;
    use wasm_bindgen::{JsCast, JsValue};

    thread_local! {
        static BUF: RefCell<Buffer<JsValue>> = RefCell::new(Buffer::default());
        static STARTED: Cell<bool> = const { Cell::new(false) };
        /// El `()=>{}` del `.catch` del `fetch`: uno solo para todos los envíos.
        static IGNORE: Closure<dyn FnMut(JsValue)> = Closure::new(|_| {});
    }

    /// `ulog(k, n, c, d)`: delega en `window.ulog` si existe; si no, el búfer
    /// propio.
    pub fn ui_log(k: &str, n: &str, c: &str, d: f64) {
        let args = [k.into(), n.into(), c.into(), JsValue::from_f64(d)];
        match call_global("ulog", &args) {
            Ok(_) => {}
            // Ausente o no llamable: el registro es nuestro.
            Err(_) if !crate::bridge::is_function("ulog") => ui_log_native(k, n, c, d),
            // `ulog` lanzó (p. ej. `activePaneTarget` lanzó): el JS pierde el
            // evento igual.
            Err(_) => {}
        }
    }

    /// `String(s).slice(0, 80)` con la semántica de JS (unidades UTF-16).
    fn clip(s: &str) -> JsValue {
        JsString::from(s).slice(0, TEXT_MAX).into()
    }

    /// `(typeof activePaneTarget === "function" && activePaneTarget()?.session) || ""`.
    /// `None` si `activePaneTarget` lanzó (el JS pierde el evento).
    fn selected_session() -> Option<JsValue> {
        if !crate::bridge::is_function("activePaneTarget") {
            return Some("".into());
        }
        let t = call_global("activePaneTarget", &[]).ok()?;
        let s = if t.is_undefined() || t.is_null() {
            JsValue::UNDEFINED
        } else {
            Reflect::get(&t, &"session".into()).ok()?
        };
        Some(if s.is_truthy() { s } else { "".into() })
    }

    /// El `ulog` del JS con el búfer de este módulo.
    pub fn ui_log_native(k: &str, n: &str, c: &str, d: f64) {
        if n.is_empty() {
            return;
        }
        let Some(sel) = selected_session() else {
            return;
        };
        let ev = Object::new();
        let fields: [(&str, JsValue); 6] = [
            ("ts", JsValue::from_f64(Date::now() / 1000.0)),
            ("k", k.into()),
            ("n", clip(n)),
            ("c", clip(c)),
            ("d", JsValue::from_f64(js_round(d))),
            ("s", sel),
        ];
        for (key, v) in fields {
            if Reflect::set(&ev, &key.into(), &v).is_err() {
                return;
            }
        }
        start();
        let full = BUF.with(|b| b.borrow_mut().push(ev.into()));
        if full {
            ui_log_flush();
        }
    }

    /// Instala `setInterval(ulogFlush, 5000)` y el vaciado en `pagehide` una vez.
    fn start() {
        if STARTED.with(|s| s.replace(true)) {
            return;
        }
        let Some(win) = web_sys::window() else {
            return;
        };
        // Una sola clausura para el intervalo y `pagehide`; vive lo que la página.
        let flush = Closure::<dyn Fn()>::new(ui_log_flush);
        let f: &Function = flush.as_ref().unchecked_ref();
        let _ = win.set_interval_with_callback_and_timeout_and_arguments_0(f, FLUSH_EVERY_MS);
        let _ = win.add_event_listener_with_callback("pagehide", f);
        flush.forget();
    }

    /// `ulogFlush()` del búfer propio.
    pub fn ui_log_flush() {
        let events = BUF.with(|b| b.borrow_mut().take_batch());
        if events.is_empty() {
            return;
        }
        let list: Array = events.iter().collect();
        let wrap = Object::new();
        if Reflect::set(&wrap, &"events".into(), &list).is_err() {
            return;
        }
        let Some(body) = JSON::stringify(&wrap).ok() else {
            return;
        };
        let _ = send(&body.into());
    }

    /// El `try{…}catch(e){}` de `ulogFlush`: cualquier fallo se calla.
    fn send(body: &JsValue) -> Result<(), JsValue> {
        let win: JsValue = web_sys::window().ok_or(JsValue::NULL)?.into();
        let nav = Reflect::get(&win, &"navigator".into())?;
        let beacon = Reflect::get(&nav, &"sendBeacon".into())?;
        if beacon.is_truthy() && !crate::dom::in_app() {
            let opts = Object::new();
            Reflect::set(&opts, &"type".into(), &"application/json".into())?;
            let blob_ctor: Function =
                Reflect::get(&js_sys::global(), &"Blob".into())?.dyn_into()?;
            let parts = Array::of1(body);
            let blob = Reflect::construct(&blob_ctor, &Array::of2(&parts, &opts))?;
            let beacon: Function = beacon.dyn_into()?;
            beacon.call2(&nav, &"/ui-log".into(), &blob)?;
            return Ok(());
        }
        let opt = Object::new();
        Reflect::set(&opt, &"method".into(), &"POST".into())?;
        let headers = Object::new();
        Reflect::set(&headers, &"Content-Type".into(), &"application/json".into())?;
        Reflect::set(&opt, &"headers".into(), &headers)?;
        Reflect::set(&opt, &"body".into(), body)?;
        Reflect::set(&opt, &"keepalive".into(), &JsValue::TRUE)?;
        let fetch: Function = Reflect::get(&win, &"fetch".into())?.dyn_into()?;
        let p = fetch.call2(&win, &"/ui-log".into(), &opt)?;
        // `.catch(()=>{})`: la promesa se descarta, con su rechazo atendido.
        IGNORE.with(|ignore| {
            let _ = Promise::resolve(&p).catch(ignore);
        });
        Ok(())
    }
}
