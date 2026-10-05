//! `comandos-web-view`: plantillas del tablero en Rust (Fase 3b, decisión T1:
//! `web-sys` + `maud`, sin framework). Crate puro: compila igual en host y en
//! `wasm32-unknown-unknown`, sin dependencias web.
//!
//! ## Regla de escape
//!
//! Todo texto que venga del usuario, del servidor o de otra sesión entra en el
//! HTML por `maud` (`(expr)` escapa `& < > "`) o por las funciones de
//! [`escape`], que copian byte a byte los escapes del JS heredado (`esc`,
//! `mdEsc`, `attrEsc`). `maud::PreEscaped` queda reservado para HTML que genera
//! el propio crate (iconos SVG, marcado fijo); nunca para texto ajeno.
//!
//! `maud` no escapa `'` y los `esc()` del JS sí (`&#39;`). Tras `innerHTML` el
//! DOM es el mismo (un nodo de texto con `'`) y `outerHTML` vuelve a serializar
//! igual, así que la paridad de DOM no cambia; si un port necesita la *cadena*
//! idéntica a la del JS (p. ej. para comparar antes de reescribir), usa
//! [`escape::text`].

pub mod escape;

/// La macro de plantillas, para que los componentes no fijen otra versión.
pub use maud;
