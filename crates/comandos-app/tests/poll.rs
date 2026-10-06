#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]
use comandos_app::{
    config::RunMode,
    dash_client::DashClient,
    poll::{PollIntervals, PollUpdate, Poller, live_pref_snapshot},
};
use serde_json::json;
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU64, Ordering},
};
use std::time::{Duration, Instant};

#[test]
fn live_pref_snapshot_only_keeps_live_keys() {
    assert_eq!(
        live_pref_snapshot(&json!({
            "font_family": "JetBrains",
            "theme": "noche",
            "button_style": "arcade",
            "favorites": ["a"]
        })),
        json!({
            "font_family": "JetBrains",
            "font_size": null,
            "cursor_shape": null,
            "cursor_blink": null,
            "terminal_padding": null,
            "terminal_opacity": null,
            "theme": "noche",
            "tabs_layout": null
        })
    );
}

#[test]
fn disconnected_poller_stops_without_updates_or_sockets() {
    let client = DashClient::new(None, RunMode::Sandbox).unwrap();
    let generation = Arc::new(AtomicU64::new(42));
    let intervals = PollIntervals {
        state_prefs: Duration::from_millis(20),
        workspace: Duration::from_millis(20),
        marks: Duration::from_millis(20),
        notices_backoff: Duration::from_millis(20),
        notices_timeout: Duration::from_millis(20),
        ..PollIntervals::default()
    };
    let (poller, rx) = Poller::start_with_intervals(client, generation.clone(), intervals);
    std::thread::sleep(Duration::from_millis(40));
    poller.stop();
    assert!(rx.try_recv().is_err());
    assert_eq!(generation.load(Ordering::Acquire), 42);
}

#[test]
fn poll_update_derives_debug_and_partial_eq() {
    assert_eq!(
        PollUpdate::Notices {
            revision: "r".into(),
            badge: 3
        },
        PollUpdate::Notices {
            revision: "r".into(),
            badge: 3
        }
    );
}

#[test]
fn poller_coalesces_repeated_state_and_live_prefs() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let _server = std::thread::spawn(move || {
        for stream in listener.incoming().take(32) {
            let mut stream = stream.unwrap();
            let mut buf = [0u8; 1024];
            let n = std::io::Read::read(&mut stream, &mut buf).unwrap_or(0);
            let request = String::from_utf8_lossy(&buf[..n]);
            let body = if request.contains("/state") {
                r#"{"status":"same"}"#
            } else if request.contains("/prefs") {
                r#"{"theme":"noche","button_style":"ignored"}"#
            } else if request.contains("/notices/watch") {
                r#"{"rev":"r1","badge":7}"#
            } else {
                "null"
            };
            let response = format!(
                "HTTP/1.0 200 OK\r\nContent-Length: {}\r\n\r\n{}",
                body.len(),
                body
            );
            let _ = std::io::Write::write_all(&mut stream, response.as_bytes());
        }
    });
    let client = DashClient::new(Some(&format!("http://127.0.0.1:{port}")), RunMode::Live).unwrap();
    let intervals = PollIntervals {
        state_prefs: Duration::from_millis(20),
        workspace: Duration::from_millis(100),
        marks: Duration::from_millis(100),
        notices_backoff: Duration::from_millis(20),
        notices_timeout: Duration::from_millis(50),
        ..PollIntervals::default()
    };
    let (poller, rx) = Poller::start_with_intervals(client, Arc::new(AtomicU64::new(3)), intervals);
    std::thread::sleep(Duration::from_millis(90));
    poller.stop();
    let updates: Vec<_> = rx.try_iter().collect();
    let state_count = updates
        .iter()
        .filter(|u| matches!(u, PollUpdate::State(_)))
        .count();
    let prefs_count = updates
        .iter()
        .filter(|u| matches!(u, PollUpdate::Prefs { .. }))
        .count();
    assert_eq!(state_count, 1, "{updates:?}");
    assert_eq!(prefs_count, 1, "{updates:?}");
}

#[test]
fn poller_stop_cancels_blocked_notice_worker_under_500ms() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let accepted = Arc::new(AtomicBool::new(false));
    let done = Arc::new(AtomicBool::new(false));
    let accepted2 = accepted.clone();
    let done2 = done.clone();
    let server = std::thread::spawn(move || {
        for stream in listener.incoming().take(8) {
            let mut stream = stream.unwrap();
            let mut buf = [0u8; 1024];
            let n = std::io::Read::read(&mut stream, &mut buf).unwrap_or(0);
            let request = String::from_utf8_lossy(&buf[..n]);
            if request.contains("/notices/watch") {
                accepted2.store(true, Ordering::Release);
                let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
                let mut one = [0u8; 1];
                let _ = std::io::Read::read(&mut stream, &mut one);
                done2.store(true, Ordering::Release);
                break;
            }
            let body = if request.contains("/prefs") {
                "{}"
            } else {
                "null"
            };
            let response = format!(
                "HTTP/1.0 200 OK\r\nContent-Length: {}\r\n\r\n{}",
                body.len(),
                body
            );
            let _ = std::io::Write::write_all(&mut stream, response.as_bytes());
        }
    });
    let client = DashClient::new(Some(&format!("http://127.0.0.1:{port}")), RunMode::Live).unwrap();
    let intervals = PollIntervals {
        state_prefs: Duration::from_millis(50),
        workspace: Duration::from_millis(50),
        marks: Duration::from_millis(50),
        notices_wait_secs: 25,
        notices_timeout: Duration::from_secs(35),
        ..PollIntervals::default()
    };
    let (poller, _rx) =
        Poller::start_with_intervals(client, Arc::new(AtomicU64::new(0)), intervals);
    let wait_until = Instant::now() + Duration::from_secs(2);
    while !accepted.load(Ordering::Acquire) && Instant::now() < wait_until {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(accepted.load(Ordering::Acquire));
    let start = Instant::now();
    poller.stop();
    assert!(start.elapsed() < Duration::from_millis(500));
    assert!(done.load(Ordering::Acquire));
    server.join().unwrap();
}
