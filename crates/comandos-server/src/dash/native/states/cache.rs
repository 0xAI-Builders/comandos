//! `read_states_cached` (7191): TTL de 1,2 s y un solo cómputo en vuelo.
//! Los que llegan durante un cómputo reciben su resultado o su error (D4: si
//! el líder se abandona, repiten y uno pasa a líder). El instante de la caché
//! se toma al terminar, como el Python.
//!
//! Un `Decline` también se guarda durante el mismo TTL (revisión final, I1):
//! con una incertidumbre persistente, cada sondeo repetiría el cómputo
//! entero (≈ 50–70 procesos de tmux contra el servidor de las sesiones vivas)
//! para acabar reenviando. Reenviar siempre es correcto, así que la salida no
//! cambia. `Failure` y `Timeout` no se guardan: el Python tampoco cachea sus
//! excepciones, y cada sondeo responde su 500/504 como él.
use super::{StateFault, States};
use std::{
    future::Future,
    sync::{Arc, Mutex, MutexGuard},
};
use tokio::sync::watch;

pub const TTL_MS: i64 = 1200;

/// Reloj en milisegundos que puede cruzar `.await` en un futuro `Send`.
pub type NowMs = dyn Fn() -> i64 + Send + Sync;

#[derive(Clone)]
enum Flight {
    Running,
    Done(Result<Arc<States>, StateFault>),
    Abandoned,
}

/// El último resultado que se puede repetir: un éxito o un `Decline`.
type Cached = Result<Arc<States>, StateFault>;

#[derive(Default)]
struct Inner {
    at_ms: i64,
    last: Option<Cached>,
    /// Vuelo en curso y su número (el `is flight` del Python).
    flight: Option<(u64, watch::Receiver<Flight>)>,
    next: u64,
}

#[derive(Default)]
pub struct StatesCache {
    inner: Mutex<Inner>,
}

/// Publica «abandonado» si el líder se suelta sin terminar.
struct Guard<'a> {
    cache: &'a StatesCache,
    id: u64,
    tx: Option<watch::Sender<Flight>>,
}

impl Drop for Guard<'_> {
    fn drop(&mut self) {
        if let Some(tx) = self.tx.take() {
            self.cache.clear_flight(self.id);
            let _ = tx.send(Flight::Abandoned);
        }
    }
}

/// Lo que decide la entrada bajo el candado.
enum Role {
    Hit(Cached),
    Follow(watch::Receiver<Flight>),
    Lead(u64, watch::Sender<Flight>),
}

impl StatesCache {
    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn clear_flight(&self, id: u64) {
        let mut inner = self.lock();
        if inner.flight.as_ref().is_some_and(|(f, _)| *f == id) {
            inner.flight = None;
        }
    }

    fn enter(&self, now_ms: &NowMs, fresh: bool) -> Role {
        let mut inner = self.lock();
        if !fresh
            && let Some(last) = &inner.last
            && now_ms() - inner.at_ms < TTL_MS
        {
            return Role::Hit(last.clone());
        }
        if let Some((_, rx)) = &inner.flight {
            return Role::Follow(rx.clone());
        }
        let (tx, rx) = watch::channel(Flight::Running);
        let id = inner.next;
        inner.next = inner.next.wrapping_add(1);
        inner.flight = Some((id, rx));
        Role::Lead(id, tx)
    }

    /// El estado vigente o el resultado del cómputo en vuelo (o de uno nuevo).
    pub async fn get<F, Fut>(&self, now_ms: &NowMs, compute: F) -> Result<Arc<States>, StateFault>
    where
        F: Fn() -> Fut,
        Fut: Future<Output = Result<States, StateFault>>,
    {
        self.get_with(now_ms, compute, false).await
    }

    /// `read_states()` sin caché (POST `/export`) dentro del mismo vuelo único:
    /// nunca sirve el resultado guardado, pero se une al cómputo que esté en
    /// vuelo (que empezó o empieza mientras se atiende la petición) o lidera uno
    /// nuevo. Así nunca hay dos cómputos a la vez —cada uno copia el rastreador
    /// de configuración y el último que termina pisaría al otro— y una ráfaga
    /// de exportaciones hace un solo cómputo. Su resultado queda en la caché
    /// como el de cualquier líder.
    pub async fn get_fresh<F, Fut>(
        &self,
        now_ms: &NowMs,
        compute: F,
    ) -> Result<Arc<States>, StateFault>
    where
        F: Fn() -> Fut,
        Fut: Future<Output = Result<States, StateFault>>,
    {
        self.get_with(now_ms, compute, true).await
    }

    async fn get_with<F, Fut>(
        &self,
        now_ms: &NowMs,
        compute: F,
        fresh: bool,
    ) -> Result<Arc<States>, StateFault>
    where
        F: Fn() -> Fut,
        Fut: Future<Output = Result<States, StateFault>>,
    {
        loop {
            match self.enter(now_ms, fresh) {
                Role::Hit(last) => return last,
                Role::Follow(mut rx) => {
                    let seen = rx
                        .wait_for(|f| !matches!(f, Flight::Running))
                        .await
                        .map(|f| f.clone());
                    match seen {
                        Ok(Flight::Done(result)) => return result,
                        // Abandonado o emisor soltado: repetir (uno pasa a líder).
                        Ok(Flight::Abandoned | Flight::Running) | Err(_) => continue,
                    }
                }
                Role::Lead(id, tx) => {
                    let mut guard = Guard {
                        cache: self,
                        id,
                        tx: Some(tx),
                    };
                    let result = compute().await.map(Arc::new);
                    let tx = guard.tx.take();
                    {
                        let mut inner = self.lock();
                        if matches!(result, Ok(_) | Err(StateFault::Decline)) {
                            inner.at_ms = now_ms();
                            inner.last = Some(result.clone());
                        }
                        if inner.flight.as_ref().is_some_and(|(f, _)| *f == id) {
                            inner.flight = None;
                        }
                    }
                    if let Some(tx) = tx {
                        let _ = tx.send(Flight::Done(result.clone()));
                    }
                    return result;
                }
            }
        }
    }
}

/// Como mucho una línea por minuto en el journal con la causa de un `Decline`
/// de GET `/state` (revisión final, I1): sin ella, un reenvío persistente no
/// deja rastro. La causa es la fase del cómputo, nunca datos de los panes.
const DECLINE_LOG_MS: i64 = 60_000;

#[derive(Default)]
pub struct DeclineLog {
    /// Instante de la última línea y `Decline`s callados desde entonces.
    inner: Mutex<Option<(i64, u64)>>,
}

impl DeclineLog {
    /// La línea que toca escribir por un `Decline` en `now_ms`, o `None` si ya
    /// se escribió una en el último minuto (y entonces se cuenta).
    pub fn line(&self, now_ms: i64, cause: &str) -> Option<String> {
        let mut inner = self.inner.lock().unwrap_or_else(|p| p.into_inner());
        match inner.as_mut() {
            Some((at, quiet)) if (*at..at.saturating_add(DECLINE_LOG_MS)).contains(&now_ms) => {
                *quiet = quiet.saturating_add(1);
                None
            }
            last => {
                let quiet = last.map_or(0, |(_, quiet)| *quiet);
                *inner = Some((now_ms, 0));
                Some(format!(
                    "comandos dash: GET /state declina y se reenvía al heredado (fase: {cause}; \
                     {quiet} más callados desde la línea anterior; como mucho una línea por minuto)"
                ))
            }
        }
    }

    /// Escribe la línea en stderr si toca.
    pub fn note(&self, now_ms: i64, cause: &str) {
        if let Some(line) = self.line(now_ms, cause) {
            eprintln!("{line}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        sync::atomic::{AtomicUsize, Ordering},
        time::Duration,
    };

    fn states(tag: &'static str) -> States {
        States {
            items: Arc::new(Vec::new()),
            body: bytes::Bytes::from_static(tag.as_bytes()),
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn followers_retry_when_leader_is_abandoned() {
        // Review Focus 1 / B10: el líder se suelta a mitad mientras dos
        // seguidores esperan; reciben resultado y solo hay dos cómputos.
        let cache = StatesCache::default();
        let calls = AtomicUsize::new(0);
        let now = || 1_000;
        let compute = || async {
            if calls.fetch_add(1, Ordering::SeqCst) == 0 {
                std::future::pending::<()>().await;
            }
            Ok(states("segundo"))
        };
        let leader = tokio::time::timeout(Duration::from_millis(50), cache.get(&now, compute));
        let follower = || async {
            tokio::time::sleep(Duration::from_millis(10)).await;
            cache.get(&now, compute).await
        };
        let (lead, a, b) = tokio::join!(leader, follower(), follower());
        assert!(lead.is_err(), "el líder debía abandonarse");
        assert_eq!(a.unwrap().body, "segundo");
        assert_eq!(b.unwrap().body, "segundo");
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn errors_are_shared_and_not_cached() {
        let cache = StatesCache::default();
        let calls = AtomicUsize::new(0);
        let now = || 1_000;
        let compute = || async {
            let n = calls.fetch_add(1, Ordering::SeqCst);
            tokio::time::sleep(Duration::from_millis(20)).await;
            if n == 0 {
                Err(StateFault::Timeout)
            } else {
                Ok(states("bien"))
            }
        };
        let (a, b) = tokio::join!(cache.get(&now, compute), cache.get(&now, compute));
        assert!(matches!(a, Err(StateFault::Timeout)));
        assert!(matches!(b, Err(StateFault::Timeout)));
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(cache.get(&now, compute).await.unwrap().body, "bien");
        // Vigente: no recalcula.
        assert_eq!(cache.get(&now, compute).await.unwrap().body, "bien");
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn declines_are_cached_for_the_ttl() {
        // Revisión final, I1: dos sondeos dentro de 1,2 s tras un `Decline`
        // hacen un solo cómputo; vencido el TTL, se recalcula.
        let cache = StatesCache::default();
        let calls = AtomicUsize::new(0);
        let clock = Arc::new(std::sync::atomic::AtomicI64::new(1_000));
        let shared = clock.clone();
        let now = move || shared.load(Ordering::SeqCst);
        let compute = || async {
            calls.fetch_add(1, Ordering::SeqCst);
            Err(StateFault::Decline)
        };
        assert!(matches!(
            cache.get(&now, compute).await,
            Err(StateFault::Decline)
        ));
        clock.fetch_add(TTL_MS - 1, Ordering::SeqCst);
        assert!(matches!(
            cache.get(&now, compute).await,
            Err(StateFault::Decline)
        ));
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        clock.fetch_add(1, Ordering::SeqCst);
        assert!(matches!(
            cache.get(&now, compute).await,
            Err(StateFault::Decline)
        ));
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn failures_are_not_cached() {
        // Un 500/504 se recalcula en cada sondeo, como el Python.
        let cache = StatesCache::default();
        let calls = AtomicUsize::new(0);
        let now = || 1_000;
        let compute = || async {
            calls.fetch_add(1, Ordering::SeqCst);
            Err(StateFault::Failure)
        };
        assert!(cache.get(&now, compute).await.is_err());
        assert!(cache.get(&now, compute).await.is_err());
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    /// Cómputo falso que cuenta cuántos hubo y cuántos llegaron a solaparse;
    /// espera a `gate` para que todos los llamadores entren antes de terminar.
    struct Probe {
        calls: AtomicUsize,
        active: AtomicUsize,
        max_active: AtomicUsize,
        gate: tokio::sync::Notify,
    }

    impl Probe {
        fn new() -> Self {
            Self {
                calls: AtomicUsize::new(0),
                active: AtomicUsize::new(0),
                max_active: AtomicUsize::new(0),
                gate: tokio::sync::Notify::new(),
            }
        }

        async fn compute(&self) -> Result<States, StateFault> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            let now = self.active.fetch_add(1, Ordering::SeqCst) + 1;
            self.max_active.fetch_max(now, Ordering::SeqCst);
            self.gate.notified().await;
            self.active.fetch_sub(1, Ordering::SeqCst);
            Ok(states("fresco"))
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn fresh_burst_shares_one_computation_with_polls() {
        // Ronda 1 de la T6 (2f-1): una ráfaga de `/export` mezclada con
        // sondeos de `/state` hace un solo cómputo y nunca dos a la vez.
        let cache = StatesCache::default();
        let probe = Probe::new();
        let now = || 1_000;
        let compute = || probe.compute();
        let release = async {
            for _ in 0..5 {
                tokio::task::yield_now().await;
            }
            probe.gate.notify_waiters();
        };
        let (a, b, c, d, ()) = tokio::join!(
            cache.get_fresh(&now, compute),
            cache.get(&now, compute),
            cache.get_fresh(&now, compute),
            cache.get_fresh(&now, compute),
            release,
        );
        for result in [a, b, c, d] {
            assert_eq!(result.unwrap().body, "fresco");
        }
        assert_eq!(probe.calls.load(Ordering::SeqCst), 1);
        assert_eq!(probe.max_active.load(Ordering::SeqCst), 1);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn fresh_skips_the_cached_result_but_polls_reuse_it() {
        let cache = StatesCache::default();
        let calls = AtomicUsize::new(0);
        let now = || 1_000;
        let compute = || async {
            let n = calls.fetch_add(1, Ordering::SeqCst);
            Ok(states(if n == 0 { "uno" } else { "dos" }))
        };
        assert_eq!(cache.get(&now, compute).await.unwrap().body, "uno");
        // Vigente para `/state`, pero `/export` relee.
        assert_eq!(cache.get_fresh(&now, compute).await.unwrap().body, "dos");
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        // Lo releído queda en la caché para el siguiente sondeo.
        assert_eq!(cache.get(&now, compute).await.unwrap().body, "dos");
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn fresh_after_a_running_poll_never_overlaps() {
        // Un sondeo ya en vuelo y una exportación que llega después: la
        // exportación sigue al líder en vez de lanzar un segundo cómputo.
        let cache = StatesCache::default();
        let probe = Probe::new();
        let now = || 1_000;
        let compute = || probe.compute();
        let late = async {
            tokio::task::yield_now().await;
            cache.get_fresh(&now, compute).await
        };
        let release = async {
            for _ in 0..5 {
                tokio::task::yield_now().await;
            }
            probe.gate.notify_waiters();
        };
        let (poll, export, ()) = tokio::join!(cache.get(&now, compute), late, release);
        assert_eq!(poll.unwrap().body, "fresco");
        assert_eq!(export.unwrap().body, "fresco");
        assert_eq!(probe.calls.load(Ordering::SeqCst), 1);
        assert_eq!(probe.max_active.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn decline_log_is_throttled_to_one_line_per_minute() {
        let log = DeclineLog::default();
        let first = log.line(1_000, "tarjetas").unwrap();
        assert!(first.contains("fase: tarjetas"), "{first}");
        assert!(first.contains("0 más callados"), "{first}");
        assert_eq!(log.line(1_001, "tarjetas"), None);
        assert_eq!(log.line(1_000 + DECLINE_LOG_MS - 1, "escaneo"), None);
        let next = log.line(1_000 + DECLINE_LOG_MS, "escaneo").unwrap();
        assert!(next.contains("fase: escaneo"), "{next}");
        assert!(next.contains("2 más callados"), "{next}");
        assert_eq!(log.line(1_000 + DECLINE_LOG_MS + 1, "escaneo"), None);
    }
}
