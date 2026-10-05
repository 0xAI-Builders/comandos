//! Componentes portados y su montaje.
//!
//! `boot` lo llama el cargador generado (`boot.js`) con el nonce de la
//! compuerta. Corre mientras el navegador aún analiza el `<head>` (la compuerta
//! `gate.js` bloquea el análisis hasta `/web/ready`): `<body>` no existe
//! todavía. Un `mount` solo publica sus globales en `window` (ver
//! `comandos_web_dom::bridge`) y deja el trabajo sobre el DOM para
//! `DOMContentLoaded`, igual que el JS que sustituye.

use wasm_bindgen::JsValue;

/// Un componente portado: su id (el de `components/<id>.json`) y su montaje.
pub struct Component {
    pub id: &'static str,
    pub mount: fn() -> Result<(), JsValue>,
}

/// Componentes que este WASM sabe montar. Crece con cada port (B4…B13).
pub const COMPONENTS: &[Component] = &[];

/// Error de un id que la página pide y este WASM no tiene.
pub const UNKNOWN: &str = "componente desconocido en este WASM";

pub fn find(id: &str) -> Option<&'static Component> {
    COMPONENTS.iter().find(|c| c.id == id)
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

/// Monta `ids` en orden. Un componente que falla (o que este WASM no conoce)
/// queda en `failed` y no impide montar los demás: decide el servidor (B2).
pub fn mount_all<E>(
    ids: &[&str],
    find: impl Fn(&str) -> Option<fn() -> Result<(), E>>,
    describe: impl Fn(&E) -> String,
) -> Report {
    let mut r = Report::default();
    // `String::from` y no `to_string()`: sobre `&&str` este pasa por
    // `core::fmt` y mete el formateador en el arranque vacío.
    for &id in ids {
        match find(id).map(|mount| mount()) {
            Some(Ok(())) => r.mounted.push(String::from(id)),
            Some(Err(e)) => r.failed.push((String::from(id), describe(&e))),
            None => r.failed.push((String::from(id), String::from(UNKNOWN))),
        }
    }
    r
}

#[cfg(target_arch = "wasm32")]
pub use web::boot;

#[cfg(target_arch = "wasm32")]
mod web {
    use super::{Report, find, meta_ids, mount_all};
    use comandos_web_dom::bridge::{global_set, js_text};
    use js_sys::{Array, Function, JSON, Object, Promise, Reflect};
    use wasm_bindgen::closure::Closure;
    use wasm_bindgen::prelude::wasm_bindgen;
    use wasm_bindgen::{JsCast, JsValue};

    /// Lee `<meta name="comandos-web">`, monta en ese orden, marca
    /// `window.__comandosReady = true` y avisa `POST /web/ready
    /// {"k", "mounted", "failed": [{"id", "error"}]}`.
    #[wasm_bindgen]
    pub fn boot(k: &str) {
        let content = comandos_web_dom::dom::query(r#"meta[name="comandos-web"]"#)
            .and_then(|m| m.get_attribute("content"))
            .unwrap_or_default();
        let ids = meta_ids(&content);
        let report = mount_all(&ids, |id| find(id).map(|c| c.mount), js_text);
        let _ = global_set("__comandosReady", &JsValue::TRUE);
        let _ = post_ready(k, &report);
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
        let p = fetch.call2(&win, &"/web/ready".into(), &opt)?;
        // Rechazo atendido. `boot` corre una vez por página: esta clausura
        // queda viva sin acumularse.
        let ignore = Closure::<dyn FnMut(JsValue)>::new(|_| {});
        let _ = Promise::resolve(&p).catch(&ignore);
        ignore.forget();
        Ok(())
    }
}
