//! Puente de globales entre el WASM y el JS que sigue vivo.
//!
//! Un componente portado publica en `window` los mismos nombres que exportaba
//! su JS (`export_fn0`, `export_fn1`, `export_fn2`, `export_closure`,
//! `export_obj`) para
//! que el JS que queda, `cc-app` (`run_javascript`), `cc-app-mac`
//! (`evaluateJavaScript`) y los iframes (`parent.X`) lo llamen sin enterarse
//! del cambio; y llama a lo que aún vive en JS con `call_global`, que da
//! `Err` (nunca pánico) si el nombre no existe o no es una función.
//!
//! Las clausuras exportadas se guardan por nombre ([`Exports`]) en un
//! `thread_local`. Contrato: **se exporta una vez por montaje**. Reexportar
//! el mismo nombre sustituye la entrada y suelta la clausura anterior; un JS
//! que hubiera guardado la función vieja recibe entonces un error visible
//! («closure invoked … after being dropped») en lugar de una fuga muda.
//!
//! ## Limitación: `let`/`const`/`class` del script en línea
//!
//! Solo las propiedades de `window` son alcanzables: `function f(){}` y
//! `var x` de nivel superior lo son; `let`, `const` y `class` de nivel
//! superior **no** (viven en el entorno léxico del script, no en `window`).
//! `global_get` devuelve `undefined` para ellos y `global_set` crea una
//! propiedad de `window` que el JS no ve. El inventario de B3
//! (`xtask/web/interop.json`, riesgos) marca los que se reasignan desde otra
//! unidad: `L`, `LABEL`, `PROVIDERS`, `favoriteReadAt`, `tabRowsHold`,
//! `tabsPollTs`, `termFallbackNotified` (y `openTerms`, que el iframe lee como
//! `parent.openTerms` y por eso siempre ve `undefined`). Un port que necesite
//! leer o escribir uno de ellos tiene dos caminos, en el mismo cambio:
//!
//! 1. portar también a su dueño (la unidad que lo declara), o
//! 2. añadir en el JS dueño un accesor de una línea, `window.__getX = () => X`
//!    y `window.__setX = v => { X = v; }`, y usarlo con `call_global`.
//!
//! ## Contrato padre → iframe
//!
//! El tablero lee y escribe propiedades propias en el `window` de los iframes
//! de terminal (`frame.contentWindow.X`). Sus nombres ([`FRAME_CONTRACT`]) los
//! debe conservar cualquier port, del tablero o del iframe (A9/A10):
//!
//! - `__comandosOwnsTouchGestures`: lo define `term.html` (`term:main`); el
//!   tablero (`region:app-combinada`) lo lee para no duplicar los gestos
//!   táctiles.
//! - `__comandosScrollWired` y `__comandosSwitchWired`: marcas que el tablero
//!   (`region:app-combinada`) escribe en el iframe tras cablear el desplazamiento
//!   y el cambio de pane, y que lee para no cablearlos dos veces.

/// Propiedades del `window` de un iframe que forman el contrato padre → iframe
/// (`@frame_contract` de `xtask/web/interop.json`).
pub const FRAME_CONTRACT: [&str; 3] = [
    "__comandosOwnsTouchGestures",
    "__comandosScrollWired",
    "__comandosSwitchWired",
];

/// Por qué no se pudo llamar a un global.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CallError<E> {
    /// `window[name]` es `undefined` o `null`.
    Missing,
    /// Existe pero no es una función.
    NotCallable,
    /// La función lanzó; lleva lo lanzado.
    Threw(E),
}

impl<E> CallError<E> {
    /// Texto del error (lo lanzado no se describe aquí: es del llamador). Sin
    /// `format!`: el formateador pesa en el WASM.
    pub fn describe(&self, name: &str) -> String {
        let tail = match self {
            CallError::Missing => " no existe (¿es un let/const del script en línea?)",
            CallError::NotCallable => " no es una función",
            CallError::Threw(_) => " lanzó una excepción",
        };
        ["window.", name, tail].concat()
    }
}

/// Lo mínimo de un ámbito global para resolver y llamar a una función por su
/// nombre. Lo implementa `window` en wasm; las pruebas de host usan un doble.
pub trait Scope {
    type Value;
    type Error;
    /// `None` si el nombre no existe (o vale `undefined`/`null`).
    fn lookup(&self, name: &str) -> Option<Self::Value>;
    fn is_callable(&self, v: &Self::Value) -> bool;
    fn invoke(
        &self,
        name: &str,
        f: &Self::Value,
        args: &[Self::Value],
    ) -> Result<Self::Value, Self::Error>;
}

/// Lo exportado a `window`, por nombre: reexportar sustituye y devuelve lo
/// anterior (que el llamador suelta).
#[derive(Debug)]
pub struct Exports<T> {
    by_name: std::collections::BTreeMap<String, T>,
}

impl<T> Default for Exports<T> {
    fn default() -> Self {
        Exports {
            by_name: std::collections::BTreeMap::new(),
        }
    }
}

impl<T> Exports<T> {
    /// Guarda `v` como `name`; devuelve lo que hubiera antes con ese nombre.
    pub fn keep(&mut self, name: &str, v: T) -> Option<T> {
        self.by_name.insert(name.to_string(), v)
    }

    pub fn len(&self) -> usize {
        self.by_name.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_name.is_empty()
    }
}

/// Resuelve `name` en `scope` y lo llama con `args`.
pub fn call_in<S: Scope>(
    scope: &S,
    name: &str,
    args: &[S::Value],
) -> Result<S::Value, CallError<S::Error>> {
    let f = scope.lookup(name).ok_or(CallError::Missing)?;
    if !scope.is_callable(&f) {
        return Err(CallError::NotCallable);
    }
    scope.invoke(name, &f, args).map_err(CallError::Threw)
}

#[cfg(target_arch = "wasm32")]
pub use web::{
    call_global, export_closure, export_fn0, export_fn1, export_fn2, export_obj, global_get,
    global_set, is_function, js_text,
};

#[cfg(target_arch = "wasm32")]
mod web {
    use super::{CallError, Exports, Scope, call_in};
    use js_sys::{Array, Function, Object, Reflect};
    use std::any::Any;
    use std::cell::RefCell;
    use wasm_bindgen::closure::Closure;
    use wasm_bindgen::{JsCast, JsValue};

    thread_local! {
        /// Clausuras publicadas en `window`, por nombre.
        static KEEP: RefCell<Exports<Box<dyn Any>>> = RefCell::new(Exports::default());
    }

    fn global() -> Object {
        js_sys::global()
    }

    /// `String(v)` de JS, con la función `String` del motor. Si lanza (un
    /// `toString` que lanza) da cadena vacía; un `Symbol` da `Symbol(…)`.
    pub fn js_text(v: &JsValue) -> String {
        if let Some(s) = v.as_string() {
            return s;
        }
        Reflect::get(&global(), &"String".into())
            .ok()
            .and_then(|f| f.dyn_into::<Function>().ok())
            .and_then(|f| f.call1(&JsValue::UNDEFINED, v).ok())
            .and_then(|s| s.as_string())
            .unwrap_or_default()
    }

    /// `window[name]` (o `undefined`).
    pub fn global_get(name: &str) -> JsValue {
        Reflect::get(&global(), &name.into()).unwrap_or(JsValue::UNDEFINED)
    }

    /// `window[name] = v`. No alcanza los `let`/`const` del script en línea.
    pub fn global_set(name: &str, v: &JsValue) -> Result<(), JsValue> {
        Reflect::set(&global(), &name.into(), v).map(|_| ())
    }

    /// `typeof window[name] === "function"`.
    pub fn is_function(name: &str) -> bool {
        global_get(name).is_function()
    }

    struct Window;

    impl Scope for Window {
        type Value = JsValue;
        type Error = JsValue;
        fn lookup(&self, name: &str) -> Option<JsValue> {
            let v = global_get(name);
            (!v.is_undefined() && !v.is_null()).then_some(v)
        }
        fn is_callable(&self, v: &JsValue) -> bool {
            v.is_function()
        }
        fn invoke(&self, _name: &str, f: &JsValue, args: &[JsValue]) -> Result<JsValue, JsValue> {
            let f: &Function = f.unchecked_ref();
            let list: Array = args.iter().collect();
            Reflect::apply(f, &global(), &list)
        }
    }

    /// Llama a `window[name](...args)` con `this = window`. `Err` si no existe,
    /// si no es una función (un `Error` de JS con el motivo) o con lo que
    /// lance la función.
    pub fn call_global(name: &str, args: &[JsValue]) -> Result<JsValue, JsValue> {
        call_in(&Window, name, args).map_err(|e| match e {
            CallError::Threw(v) => v,
            other => js_sys::Error::new(&other.describe(name)).into(),
        })
    }

    /// `window[name] = c` para una clausura de cualquier aridad (p. ej.
    /// `Closure<dyn Fn(JsValue, JsValue, JsValue, JsValue)>` para `ulog`). La
    /// clausura queda guardada por nombre; reexportar suelta la anterior.
    pub fn export_closure<T: ?Sized + 'static>(name: &str, c: Closure<T>) -> Result<(), JsValue> {
        global_set(name, c.as_ref())?;
        let boxed: Box<dyn Any> = Box::new(c);
        let kept = KEEP.with(|k| match k.try_borrow_mut() {
            Ok(mut k) => Ok(k.keep(name, boxed)),
            Err(_) => Err(boxed),
        });
        match kept {
            // La vieja se suelta fuera del préstamo.
            Ok(previous) => drop(previous),
            // Reentrada imposible en la práctica: antes fugar que invalidar
            // la función que `window` ya apunta.
            Err(boxed) => std::mem::forget(boxed),
        }
        Ok(())
    }

    /// `window[name] = () => f()`.
    pub fn export_fn0(name: &str, f: impl Fn() + 'static) -> Result<(), JsValue> {
        export_closure(name, Closure::<dyn Fn()>::new(f))
    }

    /// `window[name] = a => f(a)` (un argumento ausente llega como `undefined`).
    pub fn export_fn1(name: &str, f: impl Fn(JsValue) -> JsValue + 'static) -> Result<(), JsValue> {
        export_closure(name, Closure::<dyn Fn(JsValue) -> JsValue>::new(f))
    }

    /// `window[name] = (a, b) => f(a, b)`.
    pub fn export_fn2(
        name: &str,
        f: impl Fn(JsValue, JsValue) -> JsValue + 'static,
    ) -> Result<(), JsValue> {
        export_closure(name, Closure::<dyn Fn(JsValue, JsValue) -> JsValue>::new(f))
    }

    /// `window[name] = obj` (p. ej. un espacio de nombres `{adopt, render}`
    /// hecho con funciones ya exportadas o con `Closure::into_js_value`).
    pub fn export_obj(name: &str, obj: &Object) -> Result<(), JsValue> {
        global_set(name, obj)
    }
}
