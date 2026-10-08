use comandos_app::ui::presence::FocusRetry;
use std::time::{Duration, Instant};

#[test]
fn failed_focus_is_not_posted_at_every_80ms_tick() {
    let start = Instant::now();
    let mut retry = FocusRetry::default();
    let mut posts = 0;
    for tick in 0..125 {
        let now = start + Duration::from_millis(tick * 80);
        if retry.allows("local", now) {
            posts += 1;
            retry.failed("local", now);
        }
    }
    assert_eq!(posts, 2, "HTTP failure must not become a POST storm");
}

#[test]
fn changing_focus_is_immediate_and_success_clears_failed_key() {
    let start = Instant::now();
    let mut retry = FocusRetry::default();
    retry.failed("local", start);
    assert!(retry.allows("term-other", start));
    assert!(!retry.allows("local", start + Duration::from_secs(1)));
    retry.succeeded("term-other");
    assert!(!retry.allows("local", start + Duration::from_secs(1)));
    retry.succeeded("local");
    assert!(retry.allows("local", start + Duration::from_secs(1)));
}

#[test]
fn retry_deadline_is_monotonic_and_includes_its_boundary() {
    let start = Instant::now();
    let mut retry = FocusRetry::default();
    retry.failed("local", start);
    assert!(!retry.allows("local", start + Duration::from_millis(4999)));
    assert!(retry.allows("local", start + Duration::from_millis(5000)));
    retry.failed("local", start + Duration::from_millis(5000));
    assert!(!retry.allows("local", start + Duration::from_millis(9999)));
    assert!(retry.allows("local", start + Duration::from_millis(10000)));
}
