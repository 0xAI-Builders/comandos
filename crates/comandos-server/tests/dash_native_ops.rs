//! Rutas ops: HOME temporales y tmux privados; ninguna CLI real.
mod support;
use comandos_server::dash::native::{Cut, Native, NativeRoute, Outcome};
use support::{TestHome, twin::Twin};

#[tokio::test]
async fn routes_and_missing_status_match_python() {
    let Some(t) = Twin::start("ops-routes", |h| {
        h.write(
            "motor-results.json",
            r#"{"audit|%0": {"ok": false, "detail": "x", "ts": 1.0}}"#,
        );
    })
    .await
    else {
        return;
    };
    for (path, body) in [
        ("/session/configure", r#"{"session":"audit","pane":"0"}"#),
        (
            "/session/configure",
            r#"{"session":"audit","pane":"%0","requestId":"x"}"#,
        ),
        ("/account/switch", r#"{"session":"audit","alias":"work"}"#),
        ("/model/switch", r#"{"session":"audit","pane":"0"}"#),
    ] {
        t.post(path, body).await.assert_same();
    }
    for path in [
        "/model/status?operationKey=audit%7C%250",
        "/model/status?operationKey=nadie%7C%250",
    ] {
        t.get(path).await.assert_same();
    }
    assert!(t.tmux_mutations_a().is_empty());
}

#[tokio::test]
async fn cut_off_preserves_missing_status_decline() {
    let home = TestHome::new("ops-cut-status");
    let mut opts = home.options();
    opts.cuts_off.insert(Cut::Ops);
    let native = std::sync::Arc::new(Native::new(opts));
    let req = comandos_server::Request {
        method: http::Method::GET,
        target: "/model/status?operationKey=audit".into(),
        peer: "127.0.0.1:1234".parse().unwrap(),
        headers: vec![],
        data: None,
        body: bytes::Bytes::new(),
        internal_producer: false,
    };
    assert!(matches!(
        native
            .dispatch(NativeRoute::ModelStatus, &req)
            .await
            .unwrap(),
        Outcome::Decline
    ));
}

#[tokio::test]
async fn uncertain_motor_results_decline_before_recovery() {
    let home = TestHome::new("ops-uncertain-status");
    std::fs::write(home.hooks().join("motor-results.json"), b"{\"x\":\"\xff\"}").unwrap();
    let conn = comandos_runtime::session_operations::open_journal(&home.journal_db()).unwrap();
    conn.execute("INSERT INTO session_operations VALUES ('dead','audit','f','{}','applying',2147483647,NULL,NULL,1)",[]).unwrap();
    let native = std::sync::Arc::new(Native::new(home.options()));
    let req = comandos_server::Request {
        method: http::Method::GET,
        target: "/model/status?operationKey=audit".into(),
        peer: "127.0.0.1:1234".parse().unwrap(),
        headers: vec![],
        data: None,
        body: bytes::Bytes::new(),
        internal_producer: false,
    };
    assert!(matches!(
        native
            .dispatch(NativeRoute::ModelStatus, &req)
            .await
            .unwrap(),
        Outcome::Decline
    ));
    let state: String = conn
        .query_row(
            "SELECT state FROM session_operations WHERE id='dead'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(state, "applying");
}

#[tokio::test]
async fn configure_changes_fake_codex_in_private_pane() {
    use support::twin::TwinOpts;
    let Some(t) = Twin::start_with(
        "ops-change",
        support::ops::seed_fake_codex,
        TwinOpts {
            fakebin_extra: vec![(
                "codex".into(),
                "#!/bin/sh\nexec \"$HOME/bin/codex\" \"$@\"\n".into(),
            )],
            ..Default::default()
        },
    )
    .await
    else {
        return;
    };
    for (path, body, state) in [
        (
            "/model/switch",
            r#"{"session":"audit","pane":"%0","model":"gpt-5.5","effort":"low","requestId":"req-model-000001"}"#,
            "confirmed",
        ),
        (
            "/account/switch",
            r#"{"session":"audit","pane":"%0","alias":"work","model":"invalid-removed","effort":"invalid-removed","motor":"invalid-removed","toHarness":"invalid-removed","requestId":"req-account-00001"}"#,
            "confirmed",
        ),
        (
            "/session/configure",
            r#"{"session":"audit","pane":"%0","model":"gpt-5.6-luna","requestId":"req-failure-00001"}"#,
            "rolled_back",
        ),
    ] {
        t.post(path, body).await.assert_same();
        support::ops::wait_operations_idle(&t).await;
        if path == "/model/switch" {
            for home in [&t.a, &t.b] {
                let conn = rusqlite::Connection::open(home.journal_db()).unwrap();
                conn.execute("UPDATE session_operations SET state='awaiting_confirmation' WHERE id='req-model-000001'",[]).unwrap();
            }
            // Esta GET recorre la rama refresh asíncrona y reconfirma el destino.
        }
        let status = t.get("/model/status?operationKey=audit%7C%250").await;
        for wire in [&status.front, &status.oracle] {
            let value: serde_json::Value = serde_json::from_str(&wire.text()).unwrap();
            assert_eq!(value["state"], state, "{path}: {value}");
        }
    }
    assert_eq!(
        support::ops::journal_summary(&t.a),
        support::ops::journal_summary(&t.b)
    );
}

#[tokio::test]
async fn concurrent_configure_claims_only_once() {
    use support::twin::TwinOpts;
    let Some(t) = Twin::start_with(
        "ops-concurrent",
        support::ops::seed_fake_codex,
        TwinOpts {
            fakebin_extra: vec![(
                "codex".into(),
                r#"#!/bin/sh
exec "$HOME/bin/codex" "$@"
"#
                .into(),
            )],
            ..Default::default()
        },
    )
    .await
    else {
        return;
    };
    let (a, b) = tokio::join!(
        t.post_front(
            "/session/configure",
            r#"{"session":"audit","pane":"%0","effort":"low","requestId":"req-concurrent-01"}"#
        ),
        t.post_front(
            "/session/configure",
            r#"{"session":"audit","pane":"%0","effort":"medium","requestId":"req-concurrent-02"}"#
        )
    );
    let values: [serde_json::Value; 2] = [
        serde_json::from_str(&a.text()).unwrap(),
        serde_json::from_str(&b.text()).unwrap(),
    ];
    assert_eq!(
        values.iter().filter(|v| v["queued"] == true).count(),
        1,
        "{values:?}"
    );
    assert_eq!(
        [a.status, b.status].iter().filter(|s| **s == 409).count(),
        1
    );
    // Solo el frente recibe solicitudes: sondear su journal antes de destruir el HOME.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    loop {
        let rows = support::ops::journal_summary(&t.a);
        if rows.iter().all(|r| {
            ["confirmed", "failed", "rolled_back", "recovery_required"]
                .contains(&r["state"].as_str().unwrap_or(""))
        }) {
            break;
        }
        assert!(std::time::Instant::now() < deadline);
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
}

#[tokio::test]
async fn resume_queue_only_when_front_owns_ops() {
    use comandos_server::dash::native::{Background, ops};
    for (tag, background, off, expected) in [
        ("ops-start-legacy", Background::legacy(), false, false),
        ("ops-start-cut", Background::front(), true, false),
        ("ops-start-front", Background::front(), false, true),
    ] {
        let home = TestHome::new(tag);
        home.write("motor-queue.json", r#"{"audit|%0":{}}"#);
        let mut opts = home.options();
        opts.background = background;
        if off {
            opts.cuts_off.insert(Cut::Ops);
        }
        let native = Native::new(opts);
        ops::start(&native);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while !native.tasks().is_empty() {
            assert!(std::time::Instant::now() < deadline);
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        assert_eq!(home.hooks().join("motor-results.json").exists(), expected);
        if expected {
            let results = ops::results::MotorResults::shared(native.options()).all();
            assert_eq!(results["audit|%0"]["ok"], false);
            assert!(
                results["audit|%0"]["detail"]
                    .as_str()
                    .unwrap()
                    .contains("sin snapshot")
            );
        }
    }
}
