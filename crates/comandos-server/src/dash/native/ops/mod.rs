//! Corte `ops` (plan 2f-2): operaciones de sesión (cuenta, modelo, motor,
//! extensiones, perfiles). Vacío hasta su tarea del sub-plan 2f-2.
pub mod configure;
pub mod results;

use super::{Answer, Entry, Native};
use crate::Request;
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpsRoute {}

pub const ROUTES: &[Entry] = &[];

pub async fn answer(_native: &Arc<Native>, route: OpsRoute, _request: &Request) -> Answer {
    match route {}
}
