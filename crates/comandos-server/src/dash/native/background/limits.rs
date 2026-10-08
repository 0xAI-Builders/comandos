//! `_limits_snapshot_loop` (bin/cc-dash:1330): cada 300 s,
//! `usage_provider_limits()`, es decir `LimitsCache::get` de la 2e (lanza un
//! refresco si la caché venció y no hay otro en vuelo). Así la última foto de
//! cada ciclo semanal queda a ≤ 5 min de su reset aunque nadie abra el
//! tablero.
//!
//! La primera lectura no es de este bucle: la hace el refresco de arranque de
//! la 2e (`Native::start_background`, P52 del preflight); este bucle espera
//! su periodo antes de cada vuelta. Solo con `Background::front` (con
//! `legacy` el bucle es del Python, D3 de la 2e). Sin efectos de uso (la
//! sombra) `get` no refresca nada.
use super::Stop;
use crate::dash::native::Native;
use std::{
    sync::{Arc, Weak},
    time::Duration,
};

/// `time.sleep(300)`.
pub const PERIOD: Duration = Duration::from_secs(300);

/// Arranca el bucle como tarea registrada del frente.
pub fn start(native: &Arc<Native>, stop: Arc<Stop>) -> bool {
    start_with(native, stop, PERIOD)
}

/// `start` con el periodo inyectado (pruebas).
pub fn start_with(native: &Arc<Native>, stop: Arc<Stop>, period: Duration) -> bool {
    let weak = Arc::downgrade(native);
    native.tasks().spawn(run(weak, stop, period)).is_ok()
}

async fn run(weak: Weak<Native>, stop: Arc<Stop>, period: Duration) {
    loop {
        tokio::select! {
            () = tokio::time::sleep(period) => {}
            () = stop.wait() => return,
        }
        if stop.is_set() {
            return;
        }
        match weak.upgrade() {
            // `try: usage_provider_limits() except Exception: pass`: `get` no
            // falla; el refresco corre en su propia tarea.
            Some(native) if native.enabled() => {
                let _ = native.limits().get(&native.refresh_deps());
            }
            _ => return,
        }
    }
}
