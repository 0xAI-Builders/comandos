use std::{
    collections::{HashMap, VecDeque},
    sync::{Mutex, OnceLock},
    time::{Duration, Instant},
};
use tokio::sync::watch;

const CAP: usize = 256;
const TTL: Duration = Duration::from_secs(10);

pub fn global() -> &'static Gate {
    static GATE: OnceLock<Gate> = OnceLock::new();
    GATE.get_or_init(Gate::default)
}

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
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        inner.sweep();
        if inner.nonces.len() >= CAP {
            inner.gate_full += 1;
            return Inserted::Full;
        }
        let k = nonce();
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
        self.inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .nonces
            .remove(k);
    }
}

impl Inner {
    fn sweep(&mut self) {
        let now = Instant::now();
        while let Some(k) = self.order.front() {
            let expired = self
                .nonces
                .get(k)
                .is_none_or(|(created, _)| now.duration_since(*created) > TTL);
            if !expired {
                break;
            }
            if let Some(k) = self.order.pop_front() {
                self.nonces.remove(&k);
            }
        }
    }
}

fn nonce() -> String {
    let mut bytes = [0u8; 12];
    if getrandom::fill(&mut bytes).is_err() {
        return format!("{:?}", Instant::now());
    }
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
