//! Corte `tabs` (plan 2f-1): teclas y texto hacia los panes. Vacío hasta su
//! tarea del sub-plan 2f-1.
use super::{Answer, Entry, Native};
use crate::Request;
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputRoute {}

pub const ROUTES: &[Entry] = &[];

pub async fn answer(_native: &Arc<Native>, route: InputRoute, _request: &Request) -> Answer {
    match route {}
}
