//! Base única y operaciones de bytes; todavía sin cambio de modo en producción.
mod collections;
mod documents;
mod logs;
mod open;
pub use collections::*;
pub use documents::*;
pub use logs::*;
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
