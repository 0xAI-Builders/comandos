#![allow(clippy::unwrap_used, clippy::expect_used)]
use comandos_app::{config::RunMode, ui::presence::Presence};
#[path = "support/t13_oracle.rs"]
mod oracle;
use serde_json::json;
#[test]
fn heartbeat_does_not_consume_real_interaction_debounce() {
    let mut presence = Presence::default();
    assert_eq!(
        presence
            .payload(10000, false, true, "desktop-private", RunMode::Sandbox)
            .unwrap()["interaction"],
        false
    );
    assert_eq!(
        presence
            .payload(10001, true, true, "desktop-private", RunMode::Sandbox)
            .unwrap()["interaction"],
        true
    );
    assert!(
        presence
            .payload(15000, true, true, "desktop-private", RunMode::Sandbox)
            .is_none()
    );
    let body = presence
        .payload(15001, true, false, "desktop-private", RunMode::Sandbox)
        .unwrap();
    assert_eq!(body["visible"], false);
    assert_eq!(body["deviceId"], "desktop-private");
}
#[test]
fn shadow_never_posts_or_records_activity_or_focus() {
    let mut presence = Presence::default();
    for interaction in [false, true] {
        assert!(
            presence
                .payload(60000, interaction, true, "desktop-private", RunMode::Shadow)
                .is_none()
        );
    }
    assert_eq!(presence.last_interaction_ms(), 0);
    assert!(Presence::focus_payload("desktop-private", "safe", true, RunMode::Shadow).is_none());
    assert!(Presence::focus_payload("desktop-private", "safe", false, RunMode::Sandbox).is_none());
    assert_eq!(
        Presence::focus_payload("desktop-private", "safe", true, RunMode::Sandbox).unwrap()["activeTabId"],
        "safe"
    );
}
#[test]
fn original_ast_heartbeat_interaction_and_visibility_oracle() {
    let cases = [
        (1000, false, true),
        (4999, true, true),
        (5000, true, true),
        (5001, false, false),
        (9999, true, true),
        (10000, true, false),
        (30000, false, true),
        (30001, true, true),
    ];
    let expected = oracle::oracle("presence", &json!(cases));
    let mut presence = Presence::default();
    for ((now, interaction, visible), expected) in
        cases.into_iter().zip(expected.as_array().unwrap())
    {
        let body = presence.payload(
            now,
            interaction,
            visible,
            "desktop-private",
            RunMode::Sandbox,
        );
        let actual = body.map_or_else(
            || json!([]),
            |body| json!([["_dash_call",["/presence",body],{"timeout":3}]]),
        );
        assert_eq!(&actual, expected);
    }
}
#[test]
fn shadow_zero_requests_against_private_loopback_fixture() {
    use comandos_app::dash_client::DashClient;
    use std::{net::TcpListener, time::Duration};
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let client = DashClient::new(
        Some(&format!("http://{}", listener.local_addr().unwrap())),
        RunMode::Shadow,
    )
    .unwrap();
    let mut presence = Presence::default();
    for now in [10000, 15000, 30000, 60000] {
        for interaction in [false, true] {
            if let Some(body) =
                presence.payload(now, interaction, true, "desktop-private", RunMode::Shadow)
            {
                client
                    .post("/presence", &body, Duration::from_secs(1))
                    .unwrap();
            }
        }
    }
    if let Some(body) = Presence::focus_payload("desktop-private", "safe", true, RunMode::Shadow) {
        client
            .post("/workspace/client", &body, Duration::from_secs(1))
            .unwrap();
    }
    assert_eq!(
        listener.accept().unwrap_err().kind(),
        std::io::ErrorKind::WouldBlock
    );
}
