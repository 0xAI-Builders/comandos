//! Oyentes con dueño: un [`Listener`] quita su oyente al soltarse. Para uno que
//! deba vivir lo que la página, [`Listener::forget`].

use wasm_bindgen::JsCast;
use wasm_bindgen::closure::Closure;
use web_sys::{AddEventListenerOptions, Event, EventListenerOptions, EventTarget};

/// Opciones de `addEventListener`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Options {
    pub capture: bool,
    pub passive: bool,
    pub once: bool,
}

struct Inner {
    target: EventTarget,
    kind: String,
    capture: bool,
    closure: Closure<dyn FnMut(Event)>,
}

/// Un oyente registrado. Al soltarse llama a `removeEventListener`.
pub struct Listener {
    inner: Option<Inner>,
}

impl Listener {
    /// Lo deja registrado para siempre (la clausura no se libera).
    pub fn forget(mut self) {
        if let Some(inner) = self.inner.take() {
            inner.closure.forget();
        }
    }
}

impl Drop for Listener {
    fn drop(&mut self) {
        if let Some(inner) = self.inner.take() {
            let opts = EventListenerOptions::new();
            opts.set_capture(inner.capture);
            let _ = inner
                .target
                .remove_event_listener_with_callback_and_event_listener_options(
                    &inner.kind,
                    inner.closure.as_ref().unchecked_ref(),
                    &opts,
                );
        }
    }
}

/// `target.addEventListener(kind, f)`.
pub fn on(target: &EventTarget, kind: &str, f: impl FnMut(Event) + 'static) -> Listener {
    on_with(target, kind, Options::default(), f)
}

/// `target.addEventListener(kind, f, {capture, passive, once})`. Si el
/// navegador rechaza el registro, el `Listener` no tiene efecto.
pub fn on_with(
    target: &EventTarget,
    kind: &str,
    opts: Options,
    f: impl FnMut(Event) + 'static,
) -> Listener {
    let closure = Closure::<dyn FnMut(Event)>::new(f);
    let o = AddEventListenerOptions::new();
    o.set_capture(opts.capture);
    o.set_passive(opts.passive);
    o.set_once(opts.once);
    let added = target.add_event_listener_with_callback_and_add_event_listener_options(
        kind,
        closure.as_ref().unchecked_ref(),
        &o,
    );
    let inner = added.ok().map(|_| Inner {
        target: target.clone(),
        kind: kind.to_string(),
        capture: opts.capture,
        closure,
    });
    Listener { inner }
}
