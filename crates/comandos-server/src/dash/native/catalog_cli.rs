//! Corte `services` (plan 2f-3): catálogo de CLIs. Vacío hasta su tarea del
//! sub-plan 2f-3.
use super::{Answer, Entry, Native};
use crate::Request;
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CliRoute {}

pub const ROUTES: &[Entry] = &[];

pub async fn answer(_native: &Arc<Native>, route: CliRoute, _request: &Request) -> Answer {
    match route {}
}
