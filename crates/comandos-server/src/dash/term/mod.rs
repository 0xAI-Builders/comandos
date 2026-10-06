//! Puente de terminal; la autenticación y el init pertenecen a la puerta HTTP.
pub mod attach;
pub mod bridge;
pub mod replay;

pub mod routes;
pub use routes::TermMode;
pub mod compat;
