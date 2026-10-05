//! Biblioteca del arnés de paridad visual y de DOM (Fase 3, T2). `parity`,
//! `poll` y `rss` siguen en el binario (preflight S23). `web_build` compila los
//! WASM de la Fase 3 (T4). `web_inventory` y `web_port` inventarían la
//! interfaz y vigilan la deriva de lo portado (B3).
pub mod fixtures;
pub mod mcp;
pub mod png_diff;
pub mod shots;
pub mod web_build;
pub mod web_inventory;
pub mod web_port;

/// DOM normalizado: vive en `comandos-domdiff` (preflight R12).
pub use comandos_domdiff as dom_diff;
