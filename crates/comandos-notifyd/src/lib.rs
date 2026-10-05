//! `comandos-notifyd`: popups propios de ComandOS (sustituye a `bin/cc-notifyd`).
//! La lógica sin GTK vive en módulos puros para probarla contra el Python sin
//! pantalla; `popup` es la capa fina de GTK3 y `main.rs` lo arranca.
pub mod dash;
pub mod http;
pub mod markup;
pub mod model;
pub mod notice;
pub mod popup;
pub mod position;
pub mod stack;
pub mod theme;
