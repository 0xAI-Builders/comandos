//! Native desktop policy; AppKit is confined to the macOS ffi module.
pub mod app;
pub mod dialogs;
#[cfg(target_os = "macos")]
pub mod ffi;
pub mod files;
pub mod jobs;
pub mod strip;
pub mod tabs_ops;
pub mod tmux;

pub mod ipc;
