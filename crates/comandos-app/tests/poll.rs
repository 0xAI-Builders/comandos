#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]
use comandos_app::{
    config::RunMode,
    dash_client::DashClient,
    poll::{PollIntervals, PollUpdate, Poller, live_pref_snapshot},
};
use serde_json::json;
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};
use std::time::Duration;

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
