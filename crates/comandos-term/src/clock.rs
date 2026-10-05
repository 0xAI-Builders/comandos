//! Reloj de las actualizaciones sincronizadas (CSI ? 2026). vte llama a
//! `Instant::now()` en su temporizador por defecto (`StdSyncHandler`), que en
//! `wasm32-unknown-unknown` entra en pánico; aquí el tiempo lo pone quien
//! llama (`Engine::advance`/`Engine::tick`) en milisegundos de su reloj.
//!
//! El rasgo `Timeout` de vte no recibe el instante actual, así que el valor
//! viaja por un `thread_local`: el motor lo fija justo antes de cada llamada al
//! analizador, que es síncrona y no cambia de hilo.
use alacritty_terminal::vte::ansi::Timeout;
use std::{cell::Cell, time::Duration};

thread_local! {
    static NOW_MS: Cell<f64> = const { Cell::new(0.0) };
}

/// Fija el «ahora» que verán los temporizadores de este hilo.
pub(crate) fn set_now(now_ms: f64) {
    NOW_MS.with(|n| n.set(now_ms));
}

fn now() -> f64 {
    NOW_MS.with(Cell::get)
}

/// Plazo de la actualización sincronizada en curso, en el reloj del llamador.
#[derive(Debug, Default)]
pub struct ClockSync {
    deadline_ms: Option<f64>,
}

impl ClockSync {
    /// Instante (ms) en que vence la actualización sincronizada abierta.
    pub fn deadline_ms(&self) -> Option<f64> {
        self.deadline_ms
    }
}

impl Timeout for ClockSync {
    fn set_timeout(&mut self, duration: Duration) {
        self.deadline_ms = Some(now() + duration.as_secs_f64() * 1000.0);
    }

    fn clear_timeout(&mut self) {
        self.deadline_ms = None;
    }

    fn pending_timeout(&self) -> bool {
        self.deadline_ms.is_some_and(|d| now() < d)
    }
}
