//! `comandos-term`: terminal de ComandOS sin ttyd ni xterm.js. Crate puro
//! (host y `wasm32-unknown-unknown`): no abre PTY, no habla con tmux.

pub mod clock;
pub mod engine;
