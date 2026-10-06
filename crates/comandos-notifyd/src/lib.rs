//! `comandos-notifyd`: popups propios de ComandOS (sustituye a `bin/cc-notifyd`).
//! La lógica sin GTK vive en módulos puros para probarla contra el Python sin
//! pantalla; `popup` es la capa fina de GTK3 y `entry` lo arranca.
pub mod actions;
pub mod dash;
pub mod entry;
pub mod http;
pub mod markup;
pub mod model;
pub mod notice;
pub mod popup;
pub mod position;
pub mod stack;
pub mod sweep;
pub mod theme;
