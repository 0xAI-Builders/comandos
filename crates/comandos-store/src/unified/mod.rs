//! Base única y operaciones de bytes; todavía sin cambio de modo en producción.
mod collections;
mod documents;
mod logs;
pub(crate) mod modes;
mod open;
mod preflight;
pub use collections::*;
pub use documents::*;
pub use logs::*;
pub use modes::{Mode, mode_of, seal_guard_path, set_mode};
pub use open::{MOVED_SENTINEL, open_unified, unified_path};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    Import,
    Mirror,
    Unified,
}
impl Origin {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Import => "import",
            Self::Mirror => "mirror",
            Self::Unified => "unified",
        }
    }
}
