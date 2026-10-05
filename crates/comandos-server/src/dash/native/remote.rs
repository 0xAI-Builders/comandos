//! Corte `services` (plan 2f-3): acceso remoto y terminal web. Vacío hasta
//! su tarea del sub-plan 2f-3.
use super::{Answer, Entry, Native};
use crate::Request;
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemoteRoute {}

pub const ROUTES: &[Entry] = &[];

pub async fn answer(_native: &Arc<Native>, route: RemoteRoute, _request: &Request) -> Answer {
    match route {}
}
