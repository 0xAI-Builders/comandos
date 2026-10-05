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
    for extra in [
        &["--no-native"][..],
        &["--state-db", "/tmp/x.sqlite3"][..],
        &["--usage-db", "/tmp/u.sqlite"][..],
    ] {
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

#[test]
fn pane_targets_read_ids_and_identities_from_the_list_reply() {
    let body = br#"{"ok": true, "panes": [{"id": "%0", "index": 0, "identity": {"pane_id": "%0"}}, {"id": "%3", "identity": {"pane_id": "%3"}}], "remoteFocus": null}"#;
    let targets = poll::pane_targets(body);
    assert_eq!(targets.len(), 2);
    assert_eq!(targets[0].0, "%0");
    assert_eq!(targets[1].1["pane_id"], "%3");
    // Un trozo chunked con su marco: se toma del primer `{` al último `}`.
    let chunked =
        b"4a\r\n{\"ok\": true, \"panes\": [{\"id\": \"%1\", \"identity\": {}}]}\r\n0\r\n\r\n";
    assert_eq!(poll::pane_targets(chunked).len(), 1);
    assert!(poll::pane_targets(br#"{"error": "sesion invalida"}"#).is_empty());
    assert!(poll::pane_targets(b"").is_empty());
}

#[test]
fn shadow_lists_a_real_private_session() {
    assert_eq!(
        poll::SHADOW_SESSIONS[0],
        "local",
        "la sesión que crea la pila"
    );
    let body: serde_json::Value = serde_json::from_str(&poll::panes_list("local")).unwrap();
    assert_eq!(
        body,
        serde_json::json!({"session": "local", "action": "list"})
    );
}

#[test]
fn shadow_agents_keep_the_two_defaults_and_alternate_after() {
    let two = poll::shadow_agents(2);
    assert_eq!(
        two,
        vec![
            ("poll-claude".to_string(), "claude"),
            ("poll-codex".to_string(), "codex")
        ]
    );
    let many = poll::shadow_agents(5);
    assert_eq!(many.len(), 5);
    assert_eq!(many[2], ("poll-claude-2".to_string(), "claude"));
    assert_eq!(many[3], ("poll-codex-3".to_string(), "codex"));
    let mut names: Vec<_> = many.iter().map(|(s, _)| s.clone()).collect();
    names.dedup();
    assert_eq!(names.len(), 5, "una sesión por agente");
    assert!(poll::shadow_agents(0).is_empty());
}

#[test]
fn load_options_parse_and_agents_require_shadow() {
    let args = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    let base = [
        "--base",
        "http://127.0.0.1:1",
        "--minutes",
        "1",
        "--pid",
        "1",
    ];
    for extra in [
        &["--pollers", "3"][..],
        &["--burst", "17"][..],
        &["--max-threads", "8", "--max-pss-mib", "64"][..],
        &["--max-static-p95-ms", "100"][..],
    ] {
        let mut v = args(&base);
        v.extend(args(extra));
        assert!(poll::parse(&v).is_ok(), "{extra:?}");
    }
    for extra in [
        &["--agents", "48"][..],
        &["--extra-panes", "190"][..],
        &["--slow-tmux-ms", "1500"][..],
        &["--glibc-tunables", ""][..],
    ] {
        let mut v = args(&base);
        v.extend(args(extra));
        let error = poll::parse(&v).err().unwrap_or_default();
        assert!(error.contains("requieren --shadow"), "{extra:?}: {error}");
    }
    let mut bad = args(&base);
    bad.extend(args(&["--pollers", "x"]));
    assert!(poll::parse(&bad).is_err());
}

#[test]
fn shadow_front_gets_the_production_malloc_tuning_unless_told_otherwise() {
    let args = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    let shadow = ["--shadow", "--hooks", "/tmp/h", "--minutes", "1"];
    let tuned = |extra: &[&str]| {
        let mut v = args(&shadow);
        v.extend(args(extra));
        poll::parse(&v)
            .unwrap()
            .front_glibc_tunables()
            .map(str::to_owned)
    };
    assert_eq!(
        tuned(&[]).as_deref(),
        Some(comandos_core::malloc_tuning::PRODUCTION_GLIBC_TUNABLES)
    );
    assert_eq!(tuned(&["--glibc-tunables", ""]), None);
    assert_eq!(
        tuned(&["--glibc-tunables", "glibc.malloc.arena_max=1"]).as_deref(),
        Some("glibc.malloc.arena_max=1")
    );
}

#[test]
fn bound_failures_flag_threads_and_pss_over_the_limits() {
    let samples = [
        (0, 17_000, Some(3)),
        (1, 45_000, Some(7)),
        (10, 46_000, None),
    ];
    assert!(poll::bound_failures(&samples, Some(8), Some(64)).is_empty());
    assert!(poll::bound_failures(&samples, None, None).is_empty());
    let over = poll::bound_failures(&samples, Some(6), Some(43));
    assert_eq!(over.len(), 3, "{over:?}");
    assert!(over[0].contains("minuto 1") && over[0].contains("7 hilos"));
    assert!(
        over.iter()
            .any(|f| f.contains("minuto 10") && f.contains("Pss"))
    );
}
