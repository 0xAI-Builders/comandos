//! Corte `news` (plan 2f-4): noticias. Vacío hasta su tarea del sub-plan
//! 2f-4.
use super::{Answer, Entry, Native};
use crate::Request;
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NewsRoute {}

pub const ROUTES: &[Entry] = &[];

pub async fn answer(_native: &Arc<Native>, route: NewsRoute, _request: &Request) -> Answer {
    match route {}
}
