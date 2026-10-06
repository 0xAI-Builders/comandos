//! Escritorio GTK de ComandOS: tablero WebKit, pestañas de terminal sobre tmux y popups.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]
#![cfg_attr(
    test,
    allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)
)]
pub mod config;
pub mod dash_client;
pub mod guard;
pub mod ipc;
pub mod jobs;
pub mod poll;
pub mod proc;
pub mod state_files;
pub mod tabs;
pub mod term;
pub mod theme;
pub mod tmux;
pub mod ui;
