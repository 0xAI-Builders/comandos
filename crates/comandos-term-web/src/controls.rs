//! Terminal page controls share the existing Rust transport and renderer.
#[cfg(target_arch = "wasm32")]
mod web;
#[cfg(target_arch = "wasm32")]
pub use web::TerminalControls;
