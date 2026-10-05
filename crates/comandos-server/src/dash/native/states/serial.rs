//! Un hilo propio para los saltos de bloqueo de GET `/state`.
//!
//! Un cómputo de `/state` hace cientos de saltos de bloqueo seguidos (el
//! escaneo, y por pane con agente la cuenta, Grok y la evidencia de `/proc`).
//! En el pool de tokio cada salto cae en el hilo ocioso que despierta primero
//! (orden FIFO), así que la recolección se reparte entre todos los hilos del
//! pool y cada uno engorda su arena de glibc con lo mismo. Aquí van todos al
//! mismo hilo, en orden: una sola arena carga con la recolección y el pool
//! queda para el trabajo esporádico (estáticos, terminal, tecleo), cuyos hilos
//! se retiran solos al quedar ociosos.
use super::StateFault;
use crate::blocking::{BackendCaller, BackendWorker};
use std::{sync::Mutex, time::Duration};

/// Trabajos en cola como mucho: el vuelo único de `/state` hace uno a la vez.
const CAPACITY: usize = 8;

/// Plazo de un salto (cola incluida). Vencido, el cómputo declina y la
/// petición la responde el heredado: falla cerrado en vez de colgar hasta el
/// plazo de 120 s del manejador. Un trabajo que ya corría sigue hasta
/// terminar (no se aborta un hilo); el que seguía en cola se descarta.
pub const HOP_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Default)]
pub struct Serial {
    worker: Mutex<Option<BackendWorker<()>>>,
}

impl Serial {
    /// Corre `job` en el hilo de `/state`. Un pánico es la excepción sin
    /// capturar (500), como con `spawn_blocking`: el hilo que lo sufrió se
    /// retira y el siguiente trabajo arranca uno nuevo. Más de `HOP_TIMEOUT`
    /// es `Decline`.
    pub async fn run<T, F>(&self, job: F) -> Result<T, StateFault>
    where
        F: FnOnce() -> Result<T, StateFault> + Send + 'static,
        T: Send + 'static,
    {
        self.run_within(HOP_TIMEOUT, job).await
    }

    async fn run_within<T, F>(&self, limit: Duration, job: F) -> Result<T, StateFault>
    where
        F: FnOnce() -> Result<T, StateFault> + Send + 'static,
        T: Send + 'static,
    {
        let caller = self.caller()?;
        match tokio::time::timeout(limit, caller.call(move |_: &mut ()| job())).await {
            Ok(called) => called.map_err(|_| StateFault::Failure)?,
            Err(_) => Err(StateFault::Decline),
        }
    }

    /// El que llama al hilo vivo; arranca uno si no hay o si se retiró.
    fn caller(&self) -> Result<BackendCaller<()>, StateFault> {
        let mut slot = self.worker.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(worker) = slot.as_ref() {
            let caller = worker.caller();
            if !caller.stopped() {
                return Ok(caller);
            }
        }
        let fresh = BackendWorker::start(CAPACITY, ()).map_err(|_| StateFault::Failure)?;
        let caller = fresh.caller();
        if let Some(retired) = slot.replace(fresh) {
            // Ya terminó su bucle (por eso se retiró): esperarlo es inmediato,
            // pero se hace fuera de la petición y sin bloquear el runtime.
            tokio::spawn(async move {
                let _ = retired.shutdown().await;
            });
        }
        Ok(caller)
    }

    /// Para el hilo (si arrancó) y espera su trabajo en curso.
    pub async fn shutdown(&self) {
        let worker = self.worker.lock().unwrap_or_else(|p| p.into_inner()).take();
        if let Some(worker) = worker {
            let _ = worker.shutdown().await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn every_job_runs_on_one_thread_in_order() {
        let serial = Serial::default();
        let mut seen = Vec::new();
        for i in 0..20 {
            let id = serial
                .run(move || Ok((i, std::thread::current().id())))
                .await;
            seen.push(id);
        }
        let first = seen
            .first()
            .and_then(|r| r.as_ref().ok())
            .map(|(_, id)| *id);
        for (i, result) in seen.iter().enumerate() {
            let (n, id) = result.as_ref().map_err(|_| "falló").copied().unwrap();
            assert_eq!(n, i);
            assert_eq!(Some(id), first, "todos en el mismo hilo");
        }
        assert_ne!(first, Some(std::thread::current().id()));
        serial.shutdown().await;
    }

    #[tokio::test]
    async fn a_panic_is_a_failure_and_the_next_job_gets_a_fresh_thread() {
        let serial = Serial::default();
        let before = serial.run(|| Ok(std::thread::current().id())).await;
        let panicked: Result<(), StateFault> = serial.run(|| panic!("prueba")).await;
        assert_eq!(panicked, Err(StateFault::Failure));
        let after = serial.run(|| Ok(std::thread::current().id())).await;
        assert!(after.is_ok(), "el siguiente trabajo corre");
        assert_ne!(before.ok(), after.ok(), "en un hilo nuevo");
        let declined: Result<(), StateFault> = serial.run(|| Err(StateFault::Decline)).await;
        assert_eq!(
            declined,
            Err(StateFault::Decline),
            "los errores pasan tal cual"
        );
        serial.shutdown().await;
    }

    #[tokio::test]
    async fn a_slow_hop_declines_after_the_limit_and_the_thread_recovers() {
        let serial = Serial::default();
        let started = std::time::Instant::now();
        let slow: Result<(), StateFault> = serial
            .run_within(Duration::from_millis(50), || {
                std::thread::sleep(Duration::from_millis(300));
                Ok(())
            })
            .await;
        assert_eq!(slow, Err(StateFault::Decline), "falla cerrado al heredado");
        assert!(started.elapsed() < Duration::from_millis(250));
        // El salto lento termina y el hilo vuelve a responder.
        let next = serial.run(|| Ok(7)).await;
        assert_eq!(next, Ok(7));
        serial.shutdown().await;
    }
}
