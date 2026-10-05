//! `read_states_cached` (7191): TTL de 1,2 s y un solo cómputo en vuelo.
//! Los que llegan durante un cómputo reciben su resultado o su error (D4: si
//! el líder se abandona, repiten y uno pasa a líder). Los errores no se
//! cachean; el instante de la caché se toma al terminar, como el Python.
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

#[derive(Default)]
struct Inner {
    at_ms: i64,
    items: Option<Arc<States>>,
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
    Hit(Arc<States>),
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

    fn enter(&self, now_ms: &NowMs) -> Role {
        let mut inner = self.lock();
        if let Some(items) = &inner.items
            && now_ms() - inner.at_ms < TTL_MS
        {
            return Role::Hit(items.clone());
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
        loop {
            match self.enter(now_ms) {
                Role::Hit(items) => return Ok(items),
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
                        if let Ok(items) = &result {
                            inner.at_ms = now_ms();
                            inner.items = Some(items.clone());
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
}
