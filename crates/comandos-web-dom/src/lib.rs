//! `comandos-web-dom`: la capa web común del tablero en Rust (Fase 3b,
//! decisión T1: `web-sys` + `maud`, sin framework).
//!
//! Cada módulo reproduce un trozo del JS heredado con su misma semántica, para
//! que un componente portado se comporte igual que el JS que sustituye:
//!
//! - [`api`]: `api(path, body)` de `index.html` (región «red»).
//! - [`bridge`]: los globales de `window` que se comparten con el JS que queda,
//!   con `cc-app`/`cc-app-mac` y con los iframes.
//! - [`log`]: `ulog`/`ulogFlush` (región «registro LOCAL de uso»).
//! - [`i18n`]: `L` y `tf(es, en)` (región «i18n»).
//! - `dom`, `events`, `storage`, `timers`: envoltorios finos de `web-sys`.
//!
//! La lógica pura (reglas de error, truthiness, búfer del registro, idioma,
//! resolución de globales) compila y se prueba en host. Lo que toca el
//! navegador solo existe en `wasm32` y lo prueban las `wasm-bindgen-test` de
//! `tests/web.rs`, que corren en el Mac (A12/B4), nunca en un navegador local.

#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::unreachable,
    clippy::todo,
    clippy::unimplemented
)]

pub mod api;
pub mod bridge;
pub mod i18n;
pub mod log;

#[cfg(target_arch = "wasm32")]
pub mod dom;
#[cfg(target_arch = "wasm32")]
pub mod events;
#[cfg(target_arch = "wasm32")]
pub mod storage;
#[cfg(target_arch = "wasm32")]
pub mod timers;

/// `document.readyState !== "loading"`: el DOM ya está para tocarlo.
pub fn dom_ready(ready_state: &str) -> bool {
    ready_state != "loading"
}
