//! Calendario de `poll`: las frecuencias exactas del inventario §1.11.
#![allow(dead_code)]

#[path = "../src/poll.rs"]
mod poll;
#[path = "../src/rss.rs"]
mod rss;

use poll::{schedule, slope_kib_per_hour};

fn count(s: &[(u64, &'static str, String)], method: &str, path: &str) -> usize {
    s.iter()
        .filter(|(_, m, p)| *m == method && p == path)
        .count()
}

#[test]
fn two_minutes_match_the_table_frequencies() {
    let s = schedule(2);
    assert_eq!(count(&s, "GET", "/state"), 60);
    assert_eq!(count(&s, "GET", "/active-tab"), 120);
    assert_eq!(count(&s, "GET", "/analytics/week"), 2);
    assert_eq!(count(&s, "GET", "/usage/state"), 12);
    assert_eq!(count(&s, "GET", "/prefs"), 24);
    assert_eq!(count(&s, "GET", "/notices"), 40);
    assert_eq!(count(&s, "GET", "/work-marks"), 24);
    assert_eq!(count(&s, "GET", "/pomodoro"), 8);
    assert_eq!(count(&s, "POST", "/presence"), 4);
    assert_eq!(count(&s, "POST", "/terminal-panes"), 60);
    assert_eq!(count(&s, "GET", "/tab-models?session=poll"), 60);
    assert_eq!(count(&s, "GET", "/workspace"), 60);
}

#[test]
fn schedule_is_sorted_and_bounded_and_skips_the_long_poll() {
    let s = schedule(2);
    assert!(s.windows(2).all(|w| w[0].0 <= w[1].0));
    assert!(s.iter().all(|(t, _, _)| *t < 120_000));
    assert!(s.iter().all(|(_, _, p)| !p.starts_with("/notices/watch")));
    assert_eq!(s.first().map(|e| e.0), Some(0));
}

#[test]
fn slope_uses_minute_five_onwards() {
    // 100 KiB/min a partir del minuto 5 = 6000 KiB/h; el arranque previo no cuenta.
    let pts: Vec<(u64, u64)> = (0..=9)
        .map(|m| (m, if m < 5 { 9999 } else { 1000 + (m - 5) * 100 }))
        .collect();
    let s = slope_kib_per_hour(&pts).unwrap();
    assert!((s - 6000.0).abs() < 1e-6, "{s}");
    assert!(slope_kib_per_hour(&pts[..6]).is_none());
}
