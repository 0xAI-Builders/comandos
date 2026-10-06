//! Escritorio GTK de ComandOS: tablero WebKit, pestañas de terminal sobre tmux y popups.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]
#![cfg_attr(
    test,
    allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)
)]
pub mod config;
pub mod guard;
pub mod jobs;
pub mod proc;
pub mod term;
pub mod tmux;
