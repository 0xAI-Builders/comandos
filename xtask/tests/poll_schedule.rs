//! Calendario de `poll`: las frecuencias exactas del inventario §1.11, por cliente.
#![allow(dead_code)]

#[path = "../src/parity.rs"]
mod parity;
#[path = "../src/poll.rs"]
mod poll;
#[path = "../src/rss.rs"]
mod rss;

use poll::{Client, min_max_from, schedule, slope_kib_per_hour};

fn count(s: &[(u64, &'static str, String)], method: &str, path: &str) -> usize {
    s.iter()
        .filter(|(_, m, p)| *m == method && p == path)
        .count()
}

#[test]
fn dashboard_two_minutes_match_the_table() {
    let s = schedule(2, Client::Dashboard);
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
    assert_eq!(count(&s, "GET", "/workspace"), 0, "/workspace es de cc-app");
}

#[test]
fn app_two_minutes_match_the_cc_app_rows() {
    let s = schedule(2, Client::App);
    assert_eq!(count(&s, "GET", "/state"), 40);
    assert_eq!(count(&s, "GET", "/prefs"), 40);
    assert_eq!(count(&s, "GET", "/workspace"), 60);
    assert_eq!(count(&s, "GET", "/work-marks"), 24);
    assert_eq!(count(&s, "POST", "/presence"), 4);
    assert_eq!(s.len(), 40 + 40 + 60 + 24 + 4);
}

#[test]
fn schedule_is_sorted_and_bounded_and_skips_the_long_poll() {
    for c in [Client::Dashboard, Client::App] {
        let s = schedule(2, c);
        assert!(s.windows(2).all(|w| w[0].0 <= w[1].0));
        assert!(s.iter().all(|(t, _, _)| *t < 120_000));
        assert!(s.iter().all(|(_, _, p)| !p.starts_with("/notices/watch")));
        assert_eq!(s.first().map(|e| e.0), Some(0));
    }
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

#[test]
fn min_max_excludes_the_warmup() {
    let pts: Vec<(u64, u64)> = (0..=9)
        .map(|m| (m, if m < 5 { 9999 } else { 1000 + (m - 5) * 100 }))
        .collect();
    assert_eq!(min_max_from(&pts, 5), Some((1000, 1400)));
    assert_eq!(min_max_from(&pts, 0), Some((1000, 9999)));
    assert_eq!(min_max_from(&pts[..3], 5), None);
}

#[test]
fn shadow_only_options_require_shadow() {
    let args = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    let base = [
        "--base",
        "http://127.0.0.1:1",
        "--minutes",
        "1",
        "--pid",
        "1",
    ];
    assert!(poll::parse(&args(&base)).is_ok());
    for extra in [&["--no-native"][..], &["--state-db", "/tmp/x.sqlite3"][..]] {
        let mut v = args(&base);
        v.extend(args(extra));
        let error = poll::parse(&v).err().unwrap_or_default();
        assert!(error.contains("requieren --shadow"), "{extra:?}: {error}");
    }
    let shadow = args(&[
        "--shadow",
        "--hooks",
        "/tmp/h",
        "--minutes",
        "1",
        "--no-native",
        "--state-db",
        "/tmp/x",
    ]);
    assert!(poll::parse(&shadow).is_ok());
}
