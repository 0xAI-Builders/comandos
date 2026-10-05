//! Corte `tabs` (plan 2f-1): registro de pestañas y sus escritores. Sin rutas
//! hasta la Tarea 2f-1/T2; el registro (`tab_registry`) es de la Tarea 1.
use super::{Answer, Entry, Native};
use crate::Request;
use std::sync::Arc;

/// El registro de pestañas (Tarea 1): declarado aquí para no tocar `native/mod.rs`.
#[path = "tab_registry.rs"]
pub mod tab_registry;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TabsRoute {}

pub const ROUTES: &[Entry] = &[];

pub async fn answer(_native: &Arc<Native>, route: TabsRoute, _request: &Request) -> Answer {
    match route {}
}
