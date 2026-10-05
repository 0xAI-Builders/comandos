//! Biblioteca del arnés de paridad visual y de DOM (Fase 3, T2). `parity`,
//! `poll` y `rss` siguen en el binario (preflight S23). `web_build` compila los
//! WASM de la Fase 3 (T4).
pub mod fixtures;
pub mod mcp;
pub mod png_diff;
pub mod shots;
pub mod web_build;

/// DOM normalizado: vive en `comandos-domdiff` (preflight R12).
pub use comandos_domdiff as dom_diff;
