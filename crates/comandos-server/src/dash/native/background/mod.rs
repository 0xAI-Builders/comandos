//! Hilos de fondo de `cc-dash` como tareas del frente (2f-3/T5–T6). Sin rutas.
//!
//! `start` arranca lo que corresponde según `NativeOptions::background` (D6),
//! registra cada tarea en el `TaskTracker` del frente (D12) y devuelve un
//! `BackgroundRunner` que las para. Cada bucle guarda un `Weak<Native>` y mira
//! `Native::enabled()` en cada vuelta: con el frente apagado o soltado termina
//! solo, sin que nadie lo espere.
//!
//! Dueño (D6): el planificador de Pomodoro corre con `legacy` y con `front`
//! (sus `settle_due` son transacciones: con el del Python vivo, cada bloque se
//! cierra una vez). La migración de arranque (`pomodoro_adopt_legacy` e
//! `import_legacy_history`) solo la hace el dueño: con `legacy`, el Python;
//! con `front`, el frente. `_notices_push_loop` (1038) no se arranca nunca:
//! con `web_push.available() == False` no hace nada (D10).
pub mod pomodoro;

use super::{Background, Native};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use tokio::sync::Notify;

/// Señal de parada de los bucles de un `BackgroundRunner`.
#[derive(Default)]
pub struct Stop {
    stopped: AtomicBool,
    notify: Notify,
}

impl Stop {
    pub fn is_set(&self) -> bool {
        self.stopped.load(Ordering::Acquire)
    }

    fn set(&self) {
        self.stopped.store(true, Ordering::Release);
        self.notify.notify_waiters();
    }

    /// Vuelve cuando se pide la parada (o enseguida si ya se pidió).
    pub async fn wait(&self) {
        let notified = self.notify.notified();
        tokio::pin!(notified);
        notified.as_mut().enable();
        if self.is_set() {
            return;
        }
        notified.await;
    }
}

/// Los bucles que arrancó `start`.
pub struct BackgroundRunner {
    stop: Arc<Stop>,
    pomodoro: bool,
}

impl BackgroundRunner {
    /// ¿Arrancó el planificador de Pomodoro?
    pub fn pomodoro(&self) -> bool {
        self.pomodoro
    }

    /// Para los bucles: el que espera despierta y termina; el que trabaja
    /// termina al acabar su vuelta. Soltar el `BackgroundRunner` no los para.
    pub fn stop(&self) {
        self.stop.set();
    }
}

/// ¿Es el frente el dueño único de lo de fondo (2g)? Solo con `front`: decide
/// la migración de arranque del planificador y los bucles que no son
/// idempotentes con el Python.
pub fn front_owns(background: &Background) -> bool {
    *background == Background::front()
}

/// Arranca los bucles de fondo de este sub-plan según `background`.
pub fn start(native: &Arc<Native>) -> BackgroundRunner {
    let stop = Arc::new(Stop::default());
    let background = native.options().background;
    let pomodoro = background.pomodoro
        && native.enabled()
        && pomodoro::spawn(native, Arc::clone(&stop), front_owns(&background));
    BackgroundRunner { stop, pomodoro }
}
