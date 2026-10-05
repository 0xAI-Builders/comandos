//! Corte `services` (plan 2f-3): ajustes del tablero; también
//! `/models/latest` (`SettingsRoute::ModelsLatest`, P2 del preflight). Vacío
//! hasta su tarea del sub-plan 2f-3.
use super::{Answer, Entry, Native};
use crate::Request;
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsRoute {}

pub const ROUTES: &[Entry] = &[];

pub async fn answer(_native: &Arc<Native>, route: SettingsRoute, _request: &Request) -> Answer {
    match route {}
}
