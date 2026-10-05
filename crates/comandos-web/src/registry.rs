//! Componentes portados, su montaje y su enganche al DOM.
//!
//! `boot` lo llama el cargador generado (`boot.js`) con el nonce de la
//! compuerta, en dos fases por componente:
//!
//! 1. **`mount`**, en el orden de `<meta name="comandos-web">`: publica en
//!    `window` los globales del componente (ver `comandos_web_dom::bridge`).
//!    Corre mientras el navegador aún analiza el `<head>` (la compuerta
//!    `gate.js` bloquea el análisis hasta `/web/ready`): `<body>` no existe, y
//!    el JS en línea llamará a esos globales al analizarse. No toca el DOM ni
//!    registra uso (`ui_log`).
//! 2. **`attach`** (opcional), en el mismo orden y solo para los que montaron:
//!    lo que necesita el DOM. El registro lo corre con `dom::on_ready`: al
//!    momento si `document.readyState` ya no es `"loading"` (el cargador es
//!    `async` y puede llegar tarde, p. ej. si la compuerta venció a los 8 s), o
//!    en `DOMContentLoaded`. Sus fallos van al registro de uso por
//!    `window.ulog("web-attach", id, error, 0)` y al final se marca
//!    `window.__comandosAttached = true`.
//!
//! Cada `mount` y cada `attach` se llama desde JS con `try/catch`
//! (`Function.prototype.call` de `js-sys`, que captura): un `Err`, una
//! excepción de JS o una trampa quedan en `failed` y no impiden los demás.
//! `boot` es idempotente: una segunda llamada en la misma página no hace nada.

use wasm_bindgen::JsValue;

pub use comandos_web_dom::dom_ready;

/// Una fase de un componente (`mount` o `attach`).
pub type Phase = fn() -> Result<(), JsValue>;

/// Un componente portado: su id (el de `components/<id>.json`), su montaje y
/// su enganche al DOM.
pub struct Component {
    pub id: &'static str,
    pub mount: Phase,
    pub attach: Option<Phase>,
}

/// Componentes que este WASM sabe montar. Crece con cada port (B4…B13).
pub const COMPONENTS: &[Component] = &[];

/// Error de un id que la página pide y este WASM no tiene.
pub const UNKNOWN: &str = "componente desconocido en este WASM";

pub fn find(id: &str) -> Option<&'static Component> {
    find_in(COMPONENTS, id)
}

pub fn find_in<'a>(components: &'a [Component], id: &str) -> Option<&'a Component> {
    components.iter().find(|c| c.id == id)
}

/// Ids de `<meta name="comandos-web" content="id1 id2 …">`, en orden y sin
/// repetidos (un id repetido se monta una vez).
pub fn meta_ids(content: &str) -> Vec<&str> {
    let mut out: Vec<&str> = Vec::new();
    for id in content.split_ascii_whitespace() {
        if !out.contains(&id) {
            out.push(id);
        }
    }
    out
}

/// Resultado del montaje, para `POST /web/ready`.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Report {
    pub mounted: Vec<String>,
    /// `(id, error)`.
    pub failed: Vec<(String, String)>,
}

/// Monta `ids` en orden. `run(id)` monta uno (`None` si este WASM no lo
/// conoce). Un componente que falla queda en `failed` y no impide montar los
/// demás: decide el servidor (B2).
pub fn mount_all<E>(
    ids: &[&str],
    mut run: impl FnMut(&str) -> Option<Result<(), E>>,
    describe: impl Fn(&E) -> String,
) -> Report {
    let mut r = Report::default();
    // `String::from` y no `to_string()`: sobre `&&str` este pasa por
    // `core::fmt` y mete el formateador en el arranque vacío.
    for &id in ids {
        match run(id) {
            Some(Ok(())) => r.mounted.push(String::from(id)),
            Some(Err(e)) => r.failed.push((String::from(id), describe(&e))),
            None => r.failed.push((String::from(id), String::from(UNKNOWN))),
        }
    }
    r
}

/// Los ids que pasan a `attach`: los montados que lo tienen, en su orden.
pub fn attach_order(r: &Report, has_attach: impl Fn(&str) -> bool) -> Vec<&str> {
    r.mounted
        .iter()
        .map(String::as_str)
        .filter(|id| has_attach(id))
        .collect()
}

/// `boot` solo corre si es la primera vez en esta instancia y la página no
/// estaba ya lista (`window.__comandosReady === true`).
pub fn first_boot(booted: bool, page_ready: bool) -> bool {
    !booted && !page_ready
}

#[cfg(target_arch = "wasm32")]
pub use web::{boot, boot_with};

#[cfg(target_arch = "wasm32")]
mod web {
    use super::{
        COMPONENTS, Component, Phase, Report, attach_order, find_in, first_boot, meta_ids,
        mount_all,
    };
    use comandos_web_dom::bridge::{call_global, global_get, global_set, js_text};
    use js_sys::{Array, Function, JSON, Object, Promise, Reflect};
    use std::cell::Cell;
    use wasm_bindgen::closure::Closure;
    use wasm_bindgen::prelude::wasm_bindgen;
    use wasm_bindgen::{JsCast, JsValue};

    thread_local! {
        static BOOTED: Cell<bool> = const { Cell::new(false) };
    }

    /// Lee `<meta name="comandos-web">`, monta en ese orden, marca
    /// `window.__comandosReady = true`, avisa `POST /web/ready
    /// {"k", "mounted", "failed": [{"id", "error"}]}` y deja `attach` para
    /// cuando el DOM esté listo. Una segunda llamada no hace nada.
    #[wasm_bindgen]
    pub fn boot(k: &str) {
        let page_ready = global_get("__comandosReady") == JsValue::TRUE;
        // `try_with` y no `with`: este último trae la ruta de pánico de los
        // `thread_local` (y su formateador) al arranque vacío.
        let booted = BOOTED.try_with(|b| b.replace(true)).unwrap_or(true);
        if !first_boot(booted, page_ready) {
            return;
        }
        boot_with(k, COMPONENTS);
    }

    /// `boot` sobre una lista dada y sin la guarda de idempotencia (pruebas).
    pub fn boot_with(k: &str, components: &'static [Component]) {
        let content = comandos_web_dom::dom::query(r#"meta[name="comandos-web"]"#)
            .and_then(|m| m.get_attribute("content"))
            .unwrap_or_default();
        let ids = meta_ids(&content);
        let report = mount_all(
            &ids,
            |id| find_in(components, id).map(|c| guarded(c.mount)),
            describe,
        );
        let _ = global_set("__comandosReady", &JsValue::TRUE);
        let _ = post_ready(k, &report);
        let attach: Vec<(String, Phase)> = attach_order(&report, |id| {
            find_in(components, id).is_some_and(|c| c.attach.is_some())
        })
        .into_iter()
        .filter_map(|id| {
            find_in(components, id)
                .and_then(|c| c.attach)
                .map(|f| (String::from(id), f))
        })
        .collect();
        comandos_web_dom::dom::on_ready(move || attach_all(&attach));
    }

    fn attach_all(list: &[(String, Phase)]) {
        for (id, f) in list {
            if let Err(e) = guarded(*f) {
                // `window.ulog` ya existe en `DOMContentLoaded` (la región en JS
                // o la exportación de su port); llamarlo por el puente no mete
                // el registro nativo en el arranque.
                let args = [
                    "web-attach".into(),
                    id.as_str().into(),
                    describe(&e).into(),
                    JsValue::from_f64(0.0),
                ];
                let _ = call_global("ulog", &args);
            }
        }
        let _ = global_set("__comandosAttached", &JsValue::TRUE);
    }

    /// Texto de un fallo: `message` de un `Error`, o `String(e)`.
    fn describe(e: &JsValue) -> String {
        Reflect::get(e, &"message".into())
            .ok()
            .and_then(|m| m.as_string())
            .unwrap_or_else(|| js_text(e))
    }

    /// Llama a `f` desde JS con `try/catch`: un `Err`, una excepción de JS que
    /// atraviese el WASM o una trampa vuelven como `Err`.
    fn guarded(f: Phase) -> Result<(), JsValue> {
        let c = Closure::<dyn FnMut() -> Result<(), JsValue>>::new(f);
        let fun: &Function = c.as_ref().unchecked_ref();
        fun.call0(&JsValue::UNDEFINED).map(|_| ())
    }

    fn set(o: &Object, key: &str, v: &JsValue) -> Result<(), JsValue> {
        Reflect::set(o, &key.into(), v).map(|_| ())
    }

    /// Cuerpo de `/web/ready`, con las claves en el orden del plan.
    fn body(k: &str, r: &Report) -> Result<JsValue, JsValue> {
        let o = Object::new();
        set(&o, "k", &k.into())?;
        let mounted: Array = r
            .mounted
            .iter()
            .map(|s| JsValue::from(s.as_str()))
            .collect();
        set(&o, "mounted", &mounted)?;
        let failed = Array::new();
        for (id, error) in &r.failed {
            let f = Object::new();
            set(&f, "id", &id.as_str().into())?;
            set(&f, "error", &error.as_str().into())?;
            failed.push(&f);
        }
        set(&o, "failed", &failed)?;
        Ok(JSON::stringify(&o)?.into())
    }

    /// `fetch("/web/ready", …)` sin esperar respuesta: si no llega, la
    /// compuerta revierte sola a los 8 s (`?web=off`).
    fn post_ready(k: &str, r: &Report) -> Result<(), JsValue> {
        let win: JsValue = web_sys::window().ok_or(JsValue::NULL)?.into();
        let opt = Object::new();
        set(&opt, "method", &"POST".into())?;
        let headers = Object::new();
        set(&headers, "Content-Type", &"application/json".into())?;
        set(&opt, "headers", &headers)?;
        set(&opt, "body", &body(k, r)?)?;
        let fetch: Function = Reflect::get(&win, &"fetch".into())?.dyn_into()?;
        let p = Promise::resolve(&fetch.call2(&win, &"/web/ready".into(), &opt)?);
        // Rechazo atendido sin clausura: `p.catch(Boolean)` con el `Boolean`
        // del motor.
        let catch: Function = Reflect::get(&p, &"catch".into())?.dyn_into()?;
        let boolean = Reflect::get(&js_sys::global(), &"Boolean".into())?;
        catch.call1(&p, &boolean)?;
        Ok(())
    }
}
