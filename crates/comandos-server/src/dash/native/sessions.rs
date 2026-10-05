//! Corte `tabs` (plan 2f-1): crear, cerrar y enfocar sesiones. Vacío hasta
//! su tarea del sub-plan 2f-1.
use super::{Answer, Entry, Native};
use crate::Request;
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionsRoute {}

pub const ROUTES: &[Entry] = &[];

pub async fn answer(_native: &Arc<Native>, route: SessionsRoute, _request: &Request) -> Answer {
    match route {}
}
