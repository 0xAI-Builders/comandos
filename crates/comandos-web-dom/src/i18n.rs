//! Idioma del tablero (región «i18n» de `index.html`).
//!
//! En el JS, `let L = "es"` y el arranque hace `if(c._lang === "en") L = "en"`
//! tras `api("/conf")`; `tf(es, en)` elige por `L`. `L` y `tf` son `let`/`const`
//! del script en línea: **no** son propiedades de `window` y el puente no los
//! alcanza (ver `bridge`). Por eso el idioma del lado Rust es su propio estado:
//! lo fija quien lea `/conf` con [`lang_from_conf`] y [`set_lang`] (el port de
//! la región «i18n» o del arranque); hasta entonces vale español, como `L`.
//! La tabla `T_EN` y `t(s)` se portan con la región «i18n».

use serde_json::Value;
use std::cell::Cell;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    Es,
    En,
}

impl Lang {
    /// El valor de `L` en el JS.
    pub fn code(self) -> &'static str {
        match self {
            Lang::Es => "es",
            Lang::En => "en",
        }
    }
}

thread_local! {
    static LANG: Cell<Lang> = const { Cell::new(Lang::Es) };
}

/// Idioma actual (por omisión español).
pub fn lang() -> Lang {
    LANG.with(Cell::get)
}

pub fn set_lang(l: Lang) {
    LANG.with(|c| c.set(l));
}

/// La regla del arranque: inglés solo si `c._lang === "en"`.
pub fn lang_from_conf(conf: &Value) -> Lang {
    match conf.get("_lang").and_then(Value::as_str) {
        Some("en") => Lang::En,
        _ => Lang::Es,
    }
}

/// `tf(es, en)`.
pub fn tf<'a>(es: &'a str, en: &'a str) -> &'a str {
    match lang() {
        Lang::Es => es,
        Lang::En => en,
    }
}
