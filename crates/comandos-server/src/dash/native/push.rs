//! Corte `services` (plan 2f-3): avisos push y POST `/pomodoro`
//! (`PushRoute::PomodoroPost`, P3 del preflight; la GET sigue en la base).
//! Vacío hasta su tarea del sub-plan 2f-3.
use super::{Answer, Entry, Native};
use crate::Request;
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PushRoute {}

pub const ROUTES: &[Entry] = &[];

pub async fn answer(_native: &Arc<Native>, route: PushRoute, _request: &Request) -> Answer {
    match route {}
}
