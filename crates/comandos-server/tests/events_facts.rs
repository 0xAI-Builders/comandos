//! Optional observations keep the real native worker, HTTP and SQLite effects.
mod support;

use comandos_server::events_routes::Facts;
use serde_json::Value;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use support::{TestHome, dead_port, front, get};

struct FixtureFacts(u64);
impl Facts for FixtureFacts {
    fn now_ms(&mut self) -> comandos_store::Result<u64> {
        Ok(support::NOW_MS as u64)
    }
    fn fresh_id(&mut self, prefix: &str) -> comandos_store::Result<String> {
        self.0 += 1;
        Ok(format!("{prefix}-factory-{}", self.0))
    }
    fn process_start(&mut self, pid: &str) -> Option<String> {
        comandos_server::events_routes::NativeFacts.process_start(pid)
    }
}

#[tokio::test]
async fn event_observations_are_created_once_and_reused_by_the_real_worker() {
    let home = TestHome::new("event-facts-cache");
    home.write(
        "events.jsonl",
        "{\"project\":\"old\",\"status\":\"done\",\"ts\":10}\n",
    );
    let mut options = home.options();
    assert!(
        options.events_facts.is_none(),
        "native observations are the default"
    );
    let created = Arc::new(AtomicUsize::new(0));
    let count = created.clone();
    options.events_facts = Some(Arc::new(move || {
        count.fetch_add(1, Ordering::SeqCst);
        Box::new(FixtureFacts(0))
    }));
    let native = front(&home, dead_port(), options).await;
    assert_eq!(
        created.load(Ordering::SeqCst),
        0,
        "no facts allocation at startup"
    );
    assert_eq!(get(native.port, "/work-marks").await.status, 200);
    assert_eq!(created.load(Ordering::SeqCst), 1);
    let first = get(native.port, "/events/v2").await;
    assert_eq!(first.status, 200);
    let body: Value = serde_json::from_slice(&first.body).unwrap();
    assert_eq!(body["events"][0]["eventId"], "event-factory-1");
    assert_eq!(body["events"][0]["receivedAtMs"], support::NOW_MS);
    let repeated = get(native.port, "/events/v2").await;
    assert_eq!((repeated.status, repeated.body), (first.status, first.body));
    assert_eq!(
        created.load(Ordering::SeqCst),
        1,
        "one instance across requests"
    );
    native.stop().await;
}
