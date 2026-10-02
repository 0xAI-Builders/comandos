//! Portable ComandOS domain rules. No I/O, clocks, environment or subprocesses.
pub mod event;
pub mod hook;
pub mod turn;

mod json;
/// JSON truthiness at the existing Python compatibility boundary.
pub use json::truthy as legacy_truthy;

pub mod notifications;
