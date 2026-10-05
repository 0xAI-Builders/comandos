//! Corte `tabs` (plan 2f-1): registro de pestañas y sus escritores. Vacío
//! hasta la Tarea 2f-1/T2.
use super::{Answer, Entry, Native};
use crate::Request;
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TabsRoute {}

pub const ROUTES: &[Entry] = &[];

pub async fn answer(_native: &Arc<Native>, route: TabsRoute, _request: &Request) -> Answer {
    match route {}
}
