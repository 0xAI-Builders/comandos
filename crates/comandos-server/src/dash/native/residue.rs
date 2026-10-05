//! Corte `residue` (plan 2f-3/T7): residuo del despachador (prefijos y
//! `HeadFallback`). Su tabla va la última de `TABLES`. Vacío hasta 2f-3/T7.
use super::{Answer, Entry, Native};
use crate::Request;
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResidueRoute {}

pub const ROUTES: &[Entry] = &[];

pub async fn answer(_native: &Arc<Native>, route: ResidueRoute, _request: &Request) -> Answer {
    match route {}
}
