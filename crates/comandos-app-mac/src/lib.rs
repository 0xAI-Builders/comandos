//! Native desktop policy; AppKit is confined to the macOS ffi module.
pub mod app;
#[cfg(target_os = "macos")]
pub mod ffi;
pub mod jobs;
pub mod tmux;
