use std::{
    collections::{HashMap, VecDeque},
    sync::Mutex,
    time::{Duration, Instant},
};
use tokio::sync::watch;

const CAP: usize = 256;
const TTL: Duration = Duration::from_secs(10);
// Native readiness includes asynchronous application startup, without a blocking gate.
const NATIVE_TTL: Duration = Duration::from_secs(120);

#[derive(Default)]
pub struct Gate {
    inner: Mutex<Inner>,
}

#[derive(Default)]
struct Inner {
    nonces: HashMap<String, (Instant, watch::Sender<bool>)>,
    order: VecDeque<String>,
    gate_full: u64,
}

pub enum Inserted {
    Nonce(String),
    Full,
}

impl Gate {
    pub fn insert(&self) -> Inserted {
        self.insert_mode("")
    }
    pub fn insert_native(&self) -> Inserted {
        self.insert_mode("native-")
    }
    pub fn insert_native_term(&self) -> Inserted {
        self.insert_mode("native-term-")
    }
    pub fn insert_native_extensions(&self) -> Inserted {
        self.insert_mode("native-extensions-")
    }
    fn insert_mode(&self, prefix: &str) -> Inserted {
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        inner.sweep();
        if inner.nonces.len() >= CAP {
            inner.gate_full += 1;
            return Inserted::Full;
        }
        let Some(k) = nonce() else {
            inner.gate_full += 1;
            return Inserted::Full;
        };
        let k = format!("{prefix}{k}");
        let (tx, _) = watch::channel(false);
        inner.nonces.insert(k.clone(), (Instant::now(), tx));
        inner.order.push_back(k.clone());
        Inserted::Nonce(k)
    }

    pub fn mark_ready(&self, k: &str) -> bool {
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        inner.sweep();
        let Some((_, tx)) = inner.nonces.get(k) else {
            return false;
        };
        if *tx.borrow() {
            return false;
        }
        // `send` pierde el valor si todavía no hay receptor. El módulo async
        // puede terminar antes de que el navegador pida el script bloqueante.
        tx.send_replace(true);
        true
    }

    pub fn subscribe(&self, k: &str) -> Option<watch::Receiver<bool>> {
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        inner.sweep();
        inner.nonces.get(k).map(|(_, tx)| tx.subscribe())
    }

    pub fn snapshot(&self) -> (usize, u64) {
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        inner.sweep();
        (inner.nonces.len(), inner.gate_full)
    }

    pub fn finish(&self, k: &str) {
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        inner.nonces.remove(k);
        inner.order.retain(|key| key != k);
    }
}

impl Inner {
    fn sweep(&mut self) {
        let now = Instant::now();
        self.nonces.retain(|k, (created, _)| {
            now.duration_since(*created)
                <= if k.starts_with("native-") {
                    NATIVE_TTL
                } else {
                    TTL
                }
        });
        self.order.retain(|k| self.nonces.contains_key(k));
    }
}

fn nonce() -> Option<String> {
    let mut bytes = [0u8; 12];
    getrandom::fill(&mut bytes).ok()?;
    Some(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expired_nonce_is_rejected_and_releases_its_capacity() {
        let gate = Gate::default();
        let Inserted::Nonce(k) = gate.insert() else {
            panic!("nonce")
        };
        {
            let mut inner = gate.inner.lock().unwrap();
            inner.nonces.get_mut(&k).unwrap().0 = Instant::now() - TTL - Duration::from_millis(1);
        }
        assert!(!gate.mark_ready(&k));
        assert!(gate.subscribe(&k).is_none());
        assert_eq!(gate.snapshot(), (0, 0));
        assert!(matches!(gate.insert(), Inserted::Nonce(_)));
    }

    #[test]
    fn native_startup_ticket_does_not_hold_expired_gradual_tickets() {
        let gate = Gate::default();
        let Inserted::Nonce(native) = gate.insert_native() else {
            panic!("native nonce")
        };
        let Inserted::Nonce(gradual) = gate.insert() else {
            panic!("gradual nonce")
        };
        {
            let mut inner = gate.inner.lock().unwrap();
            for key in [&native, &gradual] {
                inner.nonces.get_mut(key).unwrap().0 =
                    Instant::now() - TTL - Duration::from_millis(1);
            }
        }
        assert!(gate.mark_ready(&native));
        assert!(!gate.mark_ready(&gradual));
        assert_eq!(gate.snapshot().0, 1);
    }
    #[test]
    fn finishing_drops_sender_and_removes_order_bookkeeping() {
        let gate = Gate::default();
        let Inserted::Nonce(k) = gate.insert() else {
            panic!("nonce")
        };
        let receiver = gate.subscribe(&k).unwrap();
        gate.finish(&k);
        assert!(receiver.has_changed().is_err());
        assert_eq!(gate.snapshot(), (0, 0));
        assert!(gate.inner.lock().unwrap().order.is_empty());
    }
}
