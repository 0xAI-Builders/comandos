//! Envoltorios finos del DOM. Nada aquí lanza: lo que falta da `None` o una
//! lista vacía.

use web_sys::{Document, Element, Window};

pub use crate::events::{Listener, on};

pub fn window() -> Option<Window> {
    web_sys::window()
}

pub fn document() -> Option<Document> {
    window().and_then(|w| w.document())
}

/// `document.getElementById(id)`.
pub fn by_id(id: &str) -> Option<Element> {
    document().and_then(|d| d.get_element_by_id(id))
}

/// `document.querySelector(sel)`; un selector inválido da `None`.
pub fn query(sel: &str) -> Option<Element> {
    document().and_then(|d| d.query_selector(sel).ok().flatten())
}

/// `[...document.querySelectorAll(sel)]`, en orden de documento.
pub fn query_all(sel: &str) -> Vec<Element> {
    let Some(list) = document().and_then(|d| d.query_selector_all(sel).ok()) else {
        return Vec::new();
    };
    (0..list.length())
        .filter_map(|i| list.get(i))
        .filter_map(|n| wasm_bindgen::JsCast::dyn_into::<Element>(n).ok())
        .collect()
}

/// `el.innerHTML = html`. Solo acepta `maud::Markup`: el HTML sale de
/// `html!` (que escapa) o de un `PreEscaped` explícito y localizable con
/// `grep` (regla de escape de `comandos-web-view`).
pub fn set_html(el: &Element, html: comandos_web_view::maud::Markup) {
    el.set_inner_html(&html.into_string());
}

/// Corre `f` cuando el DOM está listo: ya, si `document.readyState` no es
/// `"loading"` (un `boot` tardío, tras `DOMContentLoaded`); si no, en un
/// `DOMContentLoaded` de una sola vez. Los oyentes registrados así desde el
/// `<head>` corren **antes** que los `DOMContentLoaded` de los `<script>` del
/// cuerpo: un port en que ese orden importe lo documenta.
pub fn on_ready(f: impl FnOnce() + 'static) {
    let Some(doc) = document() else {
        return;
    };
    if crate::dom_ready(&doc.ready_state()) {
        f();
        return;
    }
    let mut f = Some(f);
    let opts = crate::events::Options {
        once: true,
        ..Default::default()
    };
    crate::events::on_with(&doc, "DOMContentLoaded", opts, move |_| {
        if let Some(f) = f.take() {
            f();
        }
    })
    .forget();
}

/// `document.createElement(tag)`.
pub fn create(tag: &str) -> Option<Element> {
    document().and_then(|d| d.create_element(tag).ok())
}

/// `inApp()` de `index.html` (región «app nativa»):
/// `!!(window.webkit && window.webkit.messageHandlers && window.webkit.messageHandlers.centro)`.
pub fn in_app() -> bool {
    let get = |o: &wasm_bindgen::JsValue, k: &str| {
        js_sys::Reflect::get(o, &k.into()).unwrap_or(wasm_bindgen::JsValue::UNDEFINED)
    };
    let webkit = get(&js_sys::global(), "webkit");
    if !webkit.is_truthy() {
        return false;
    }
    let handlers = get(&webkit, "messageHandlers");
    handlers.is_truthy() && get(&handlers, "centro").is_truthy()
}
