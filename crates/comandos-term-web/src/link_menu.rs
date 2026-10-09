//! One owned link menu per document. No timers, polling, or retained listeners after closing.
use comandos_web_dom::events;
use std::cell::RefCell;
use wasm_bindgen::JsCast;
use web_sys::{Document, Element, HtmlElement, HtmlTextAreaElement, Window};
thread_local! { static MENU: RefCell<Option<Menu>> = const { RefCell::new(None) }; }
struct Menu {
    root: Element,
    _listeners: Vec<events::Listener>,
}
impl Drop for Menu {
    fn drop(&mut self) {
        self.root.remove();
    }
}
pub(crate) fn close() {
    MENU.with(|slot| {
        slot.borrow_mut().take();
    });
}
pub(crate) fn show(
    window: &Window,
    document: &Document,
    textarea: &HtmlTextAreaElement,
    url: &str,
    x: f64,
    y: f64,
) {
    if !crate::links::is_web_url(url) {
        return;
    }
    close();
    let build = || -> Result<Menu, wasm_bindgen::JsValue> {
        let body = document.body().ok_or("missing body")?;
        let root = document.create_element("div")?;
        root.set_id("comandos-link-menu");
        root.set_attribute(
            "style",
            "position:fixed;inset:0;z-index:2147483647;pointer-events:auto;background:transparent",
        )?;
        let menu = document.create_element("section")?;
        menu.set_attribute("role", "dialog")?;
        menu.set_attribute("aria-label", "Opciones del enlace")?;
        let width = window.inner_width()?.as_f64().unwrap_or(360.);
        let height = window.inner_height()?.as_f64().unwrap_or(600.);
        let w = (width - 24.).min(360.).max(240.);
        let left = x.max(12.).min((width - w - 12.).max(12.));
        let top = (y + 12.).max(12.).min((height - 235.).max(12.));
        menu.set_attribute("style", &format!("position:absolute;left:{left}px;top:{top}px;width:{w}px;max-width:calc(100vw - 24px);box-sizing:border-box;padding:12px;border:1px solid var(--accent,#79def4);border-radius:10px;background:var(--bg,#111b2c);color:var(--text,#e8effa);box-shadow:0 12px 36px #0007;font:13px system-ui;display:flex;flex-direction:column;gap:9px"))?;
        let label = document.create_element("div")?;
        label.set_text_content(Some(url));
        label.set_attribute("style", "font:11px/1.5 monospace;overflow-wrap:anywhere;max-height:52px;overflow:auto;padding:2px 4px 6px;opacity:.8")?;
        menu.append_child(&label)?;
        let style = "display:flex;align-items:center;justify-content:flex-start;min-height:44px;box-sizing:border-box;width:100%;padding:10px 12px;color:inherit;background:var(--surface,#23334c);border:1px solid var(--border,#405777);border-radius:6px;text-decoration:none;font:inherit;cursor:pointer";
        let open = document.create_element("a")?;
        open.set_attribute("href", url)?;
        open.set_attribute("target", "_blank")?;
        open.set_attribute("rel", "noopener noreferrer")?;
        open.set_attribute("style", style)?;
        open.set_text_content(Some("Abrir enlace ↗"));
        let copy = document.create_element("button")?;
        copy.set_attribute("type", "button")?;
        copy.set_attribute("style", style)?;
        copy.set_text_content(Some("Copiar enlace"));
        let dismiss = document.create_element("button")?;
        dismiss.set_attribute("type", "button")?;
        dismiss.set_attribute("style", "min-height:32px;border:0;background:transparent;color:inherit;font:inherit;cursor:pointer")?;
        dismiss.set_attribute("aria-label", "Cerrar menú de enlace")?;
        dismiss.set_text_content(Some("Cerrar"));
        menu.append_child(&open)?;
        menu.append_child(&copy)?;
        menu.append_child(&dismiss)?;
        root.append_child(&menu)?;
        let mut listeners = Vec::new();
        let win = window.clone();
        let doc = document.clone();
        let input = textarea.clone();
        let text = url.to_owned();
        listeners.push(events::on(copy.as_ref(), "click", move |event| {
            event.prevent_default();
            event.stop_propagation();
            crate::write_clipboard(&win, &doc, &input, text.clone());
            close();
        }));
        listeners.push(events::on(dismiss.as_ref(), "click", move |event| {
            event.prevent_default();
            close();
        }));
        // Closing on pointerdown outside avoids consuming the mouseup/click which opened the menu.
        let backdrop = root.clone();
        listeners.push(events::on(root.as_ref(), "pointerdown", move |event| {
            if event
                .target()
                .is_some_and(|target| target == *backdrop.unchecked_ref::<web_sys::EventTarget>())
            {
                close();
            }
        }));
        listeners.push(events::on(document.as_ref(), "keydown", move |event| {
            if event
                .dyn_ref::<web_sys::KeyboardEvent>()
                .is_some_and(|e| e.key() == "Escape")
            {
                event.prevent_default();
                close();
            }
        }));
        body.append_child(&root)?;
        if let Some(el) = open.dyn_ref::<HtmlElement>() {
            let _ = el.focus();
        }
        Ok(Menu {
            root,
            _listeners: listeners,
        })
    };
    if let Ok(menu) = build() {
        MENU.with(|slot| *slot.borrow_mut() = Some(menu));
    }
}
