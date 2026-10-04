//! Portable ComandOS domain rules. No I/O, clocks, environment or subprocesses.
pub mod event;
pub mod hook;
pub mod turn;

pub mod json;
/// JSON truthiness at the existing Python compatibility boundary.
pub use json::truthy as legacy_truthy;

pub mod notifications;

pub mod workspace;

pub mod pomodoro;
pub mod focus;

pub mod mcp_descriptions;

pub mod allocation;
pub mod analytics_week;

pub mod work_marks;

pub mod dashboard_access;
