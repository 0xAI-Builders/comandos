//! Connection policies shared by the browser controller and host regressions.
#[derive(Default, Debug)]
pub struct Reconnect {
    pub attempt: u32,
}
impl Reconnect {
    pub fn next_delay_ms(&mut self) -> u32 {
        let delay = 250_u32.saturating_mul(1_u32 << self.attempt.min(4));
        self.attempt = self.attempt.saturating_add(1).min(5);
        delay
    }
    pub fn connected(&mut self) {
        self.attempt = 0;
    }
}
#[derive(Debug)]
pub struct Drained {
    pub items: Vec<Vec<u8>>,
    pub stale: bool,
}
#[derive(Debug)]
pub struct InputQueue {
    max_bytes: usize,
    max_age_ms: f64,
    bytes: usize,
    since: Option<f64>,
    items: Vec<Vec<u8>>,
}
impl InputQueue {
    pub fn new(max_bytes: usize, max_age_ms: f64) -> Self {
        Self {
            max_bytes,
            max_age_ms,
            bytes: 0,
            since: None,
            items: Vec::new(),
        }
    }
    pub fn push(&mut self, data: &[u8], now: f64) -> bool {
        if data.len() > self.max_bytes.saturating_sub(self.bytes) {
            return false;
        }
        if data.is_empty() {
            return true;
        }
        self.since.get_or_insert(now);
        self.bytes += data.len();
        self.items.push(data.to_vec());
        true
    }
    pub fn drain(&mut self, now: f64) -> Drained {
        let stale = self
            .since
            .take()
            .is_some_and(|since| now - since > self.max_age_ms);
        self.bytes = 0;
        Drained {
            items: std::mem::take(&mut self.items),
            stale,
        }
    }
}
/// IDs are returned to the caller, which resolves each pending composer promise.
/// A response from another socket can never clear a draft.
#[derive(Default, Debug)]
pub struct AckTracker {
    next: u64,
    pending: Vec<(u64, u64, f64)>,
}
impl AckTracker {
    pub fn wait(&mut self, socket: u64, now: f64, timeout: f64) -> u64 {
        self.next = self.next.wrapping_add(1);
        self.pending.push((self.next, socket, now + timeout));
        self.next
    }
    fn take(&mut self, predicate: impl Fn(&(u64, u64, f64)) -> bool) -> Vec<u64> {
        let mut ids = Vec::new();
        self.pending.retain(|item| {
            if predicate(item) {
                ids.push(item.0);
                false
            } else {
                true
            }
        });
        ids
    }
    pub fn cancel(&mut self, id: u64) -> Vec<u64> {
        self.take(|i| i.0 == id)
    }
    pub fn output(&mut self, socket: u64) -> Vec<u64> {
        self.take(|i| i.1 == socket)
    }
    pub fn closed(&mut self, socket: u64) -> Vec<u64> {
        self.take(|i| i.1 == socket)
    }
    pub fn expire(&mut self, now: f64) -> Vec<u64> {
        self.take(|i| now >= i.2)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn backoff_matches_term_html() {
        let mut r = Reconnect::default();
        assert_eq!(
            (0..7).map(|_| r.next_delay_ms()).collect::<Vec<_>>(),
            [250, 500, 1000, 2000, 4000, 4000, 4000]
        );
        r.connected();
        assert_eq!(r.next_delay_ms(), 250);
    }
    #[test]
    fn queue_preserves_binary_and_rejects_overflow() {
        let mut q = InputQueue::new(4096, 15000.0);
        assert!(q.push(&[0, 255, 128], 0.0));
        assert!(q.push(&[b'a'; 4093], 1.0));
        assert!(!q.push(b"b", 2.0));
        let d = q.drain(15000.0);
        assert!(!d.stale);
        assert_eq!(d.items.first(), Some(&vec![0, 255, 128]));
        assert!(q.push(b"c", 20000.0));
        let d = q.drain(35001.0);
        assert!(d.stale);
        assert_eq!(d.items, vec![vec![b'c']]);
        assert!(q.drain(35002.0).items.is_empty());
    }
    #[test]
    fn acks_are_bound_to_socket_and_deadline() {
        let mut a = AckTracker::default();
        let first = a.wait(1, 0.0, 100.0);
        let second = a.wait(2, 1.0, 100.0);
        assert!(a.output(3).is_empty());
        assert_eq!(a.output(1), vec![first]);
        assert!(a.expire(100.0).is_empty());
        assert_eq!(a.expire(101.0), vec![second]);
        let third = a.wait(2, 200.0, 100.0);
        assert_eq!(a.closed(2), vec![third]);
        assert!(a.output(2).is_empty());
    }
}
