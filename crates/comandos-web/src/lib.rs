//! `comandos-web`: el WASM del tablero (Fase 3b). Lo compila
//! `cargo run -p xtask -- web-build --crate comandos-web`; el manifiesto
//! (`comandos_core::web_assets::Manifest`) lo nombra `comandos_web.js`,
//! `comandos_web_bg.wasm` y `comandos_web_boot.js` (el cargador que inyecta el
//! frente, B2).
//!
//! ## Registro de componentes (preflight R11)
//!
//! Cada componente portado tiene:
//!
//! - su `mount` en [`registry::COMPONENTS`], y
//! - su entrada en `crates/comandos-web/components/<id>.json` (un archivo por
//!   componente, sin lista común): `id`, `kind` (`script`, `region` o `page`),
//!   `source`, `marker_start`/`marker_end`/`script_index` (regiones), `sha256`
//!   del origen portado, `exports` y `deps`. La crea y la refresca
//!   `cargo run -p xtask -- web-port pin <id>`; el frente (B2) junta los
//!   archivos por ruta (sin depender de este crate, que arrastraría `web-sys`).
//!
//! `tests/registry.rs` exige que ambos lados coincidan y que `exports` declare
//! todos los globales del componente que consume algo de fuera
//! (otra unidad, `cc-app`, `cc-app-mac` o un iframe).
//!
//! ## `interop.json`
//!
//! No hay copia en este crate: la fuente única es `xtask/web/interop.json`
//! (y `inventory.json`), que genera `xtask web-inventory` (B3). Las pruebas la
//! leen de su sitio con `include_str!`, así que regenerar el inventario
//! revalida los `exports` sin duplicar nada a mano.

pub mod registry;

#[cfg(target_arch = "wasm32")]
pub use registry::boot;
