//! `setInterval`/`setTimeout` con dueño: un [`Handle`] cancela al soltarse;
//! [`Handle::forget`] lo deja correr lo que la página. Se llaman como métodos
//! de `window`, así que un reloj falso que sustituya `window.setTimeout`
//! (los ganchos de prueba de B2) también los gobierna.

use wasm_bindgen::JsCast;
use wasm_bindgen::closure::Closure;

enum Kind {
    Interval(Closure<dyn FnMut()>),
    Timeout(Closure<dyn FnMut()>),
}

/// Un temporizador en marcha.
pub struct Handle {
    id: i32,
    kind: Option<Kind>,
}

impl Handle {
    /// Lo deja correr para siempre (la clausura no se libera).
    pub fn forget(mut self) {
        match self.kind.take() {
            Some(Kind::Interval(c) | Kind::Timeout(c)) => c.forget(),
            None => {}
        }
    }
}

impl Drop for Handle {
    fn drop(&mut self) {
        let Some(kind) = self.kind.take() else {
            return;
        };
        if let Some(w) = web_sys::window() {
            match kind {
                Kind::Interval(_) => w.clear_interval_with_handle(self.id),
                Kind::Timeout(_) => w.clear_timeout_with_handle(self.id),
            }
        }
    }
}

/// `setInterval(f, ms)`. Sin `window` (o si el navegador lo rechaza) no corre.
pub fn interval(ms: i32, f: impl FnMut() + 'static) -> Handle {
    let c = Closure::<dyn FnMut()>::new(f);
    let id = web_sys::window().and_then(|w| {
        w.set_interval_with_callback_and_timeout_and_arguments_0(c.as_ref().unchecked_ref(), ms)
            .ok()
    });
    Handle {
        id: id.unwrap_or(0),
        kind: id.map(|_| Kind::Interval(c)),
    }
}

/// `setTimeout(f, ms)`. Soltar el `Handle` antes de que venza lo cancela.
pub fn timeout(ms: i32, f: impl FnOnce() + 'static) -> Handle {
    let mut f = Some(f);
    let c = Closure::<dyn FnMut()>::new(move || {
        if let Some(f) = f.take() {
            f();
        }
    });
    let id = web_sys::window().and_then(|w| {
        w.set_timeout_with_callback_and_timeout_and_arguments_0(c.as_ref().unchecked_ref(), ms)
            .ok()
    });
    Handle {
        id: id.unwrap_or(0),
        kind: id.map(|_| Kind::Timeout(c)),
    }
}
