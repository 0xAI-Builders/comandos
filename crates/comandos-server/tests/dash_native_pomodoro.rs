//! Dominio F2: GET /pomodoro con app-state, la cola de foco y la base de uso.
mod support;

use comandos_server::dash::native::wall_clock_ms;
use std::sync::Arc;
use support::{FakeLegacy, TestHome, dead_port, front, get, oracle::oracle, request_body};

/// `serverNowMs` difiere entre dos procesos: se iguala a 0 antes de comparar bytes.
fn masked(text: &str) -> String {
    let key = "\"serverNowMs\": ";
    let Some(at) = text.find(key) else {
        return text.to_owned();
    };
    let start = at + key.len();
    let end = start + text[start..].bytes().take_while(u8::is_ascii_digit).count();
    format!("{}0{}", &text[..start], &text[end..])
}

fn seed_usage(home: &TestHome, rows: &[(&str, &str)]) {
    let conn = comandos_store::usage::open_usage_db_at(&home.usage_db()).unwrap();
    comandos_store::usage::ensure_schema(&conn).unwrap();
    for (key, value) in rows {
        conn.execute(
            "insert into focus_settings(key,value) values(?,?)",
            rusqlite::params![key, value],
        )
        .unwrap();
    }
}

#[tokio::test]
async fn pomodoro_is_native_with_python_shape() {
    let home = TestHome::new("pomo-shape");
    seed_usage(
        &home,
        &[
            ("focusMinutes", "50"),
            ("style", "\"garden\""),
            ("roto", "{"),
        ],
    );
    home.write("focus-queue.jsonl", "{\"a\": 1}\nroto\n\n{\"b\": [2]}\n");
    let front = front(&home, dead_port(), home.options()).await;
    let wire = get(front.port, "/pomodoro").await;
    assert_eq!(wire.status, 200, "{}", wire.text());
    let body = wire.text();
    assert!(body.starts_with(r#"{"revision": "#), "{body}");
    assert!(
        body.contains(r#""serverNowMs": 1791115200000, "block": null, "queue": [{"a": 1}, {"b": [2]}], "settings": {"focusMinutes": 50, "style": "garden"}, "progress": {"#),
        "{body}"
    );
    // `default_prefs()`: el foco suena por omisión y, sin clientes, en el altavoz local.
    let device = comandos_server::dash::native::desktop_device();
    assert!(
        body.ends_with(&format!(
            r#""sound": {{"enabled": true, "device": "local-speaker", "desktopDevice": "{device}"}}}}"#
        )),
        "{body}"
    );
    front.stop().await;
}

#[tokio::test]
async fn pomodoro_matches_python_oracle() {
    let home = TestHome::new("pomo-oracle");
    seed_usage(&home, &[("cycles", "4"), ("autoBreak", "true")]);
    home.write("focus-queue.jsonl", "{\"title\": \"ñ\"}\n");
    let Some(py) = oracle(&home).await else {
        return;
    };
    let mut opts = home.options();
    opts.clock = Arc::new(wall_clock_ms);
    let front = front(&home, dead_port(), opts).await;
    let a = get(py.port, "/pomodoro").await;
    let b = get(front.port, "/pomodoro").await;
    assert_eq!((a.status, masked(&a.text())), (b.status, masked(&b.text())));
    // Con los avisos silenciados, `sound.enabled` cambia en los dos.
    let prefs = r#"{"muted": true}"#;
    assert_eq!(
        request_body(front.port, "POST", "/notices/prefs", "", prefs)
            .await
            .status,
        200
    );
    let a = get(py.port, "/pomodoro").await;
    let b = get(front.port, "/pomodoro").await;
    assert_eq!(masked(&a.text()), masked(&b.text()));
    assert!(
        b.text()
            .contains(r#""sound": {"enabled": false, "device": null"#),
        "{}",
        b.text()
    );
    front.stop().await;
}

#[tokio::test]
async fn usage_newer_schema_disables_only_usage_lane() {
    let home = TestHome::new("pomo-newer");
    seed_usage(&home, &[]);
    rusqlite::Connection::open(home.usage_db())
        .unwrap()
        .execute_batch("pragma user_version=12")
        .unwrap();
    home.write("snippets.json", "[]");
    let legacy = FakeLegacy::start().await;
    let front = front(&home, legacy.port, home.options()).await;
    for _ in 0..2 {
        assert_eq!(
            get(front.port, "/pomodoro").await.text(),
            r#"{"legacy": true}"#
        );
    }
    assert_eq!(
        get(front.port, "/snippets").await.text(),
        "[]",
        "las demás siguen nativas"
    );
    assert_eq!(get(front.port, "/notices/prefs").await.status, 200);
    let version: i64 = rusqlite::Connection::open(home.usage_db())
        .unwrap()
        .query_row("pragma user_version", [], |r| r.get(0))
        .unwrap();
    assert_eq!(version, 12, "nunca se baja la versión");
    assert_eq!(
        legacy.requests(),
        vec!["GET /pomodoro HTTP/1.1".to_owned(); 2]
    );
    front.stop().await;
}

#[tokio::test]
async fn undecodable_queue_declines() {
    let home = TestHome::new("pomo-queue");
    std::fs::write(
        home.hooks().join("focus-queue.jsonl"),
        b"{\"a\": 1}\n\xff\n",
    )
    .unwrap();
    let legacy = FakeLegacy::start().await;
    let front = front(&home, legacy.port, home.options()).await;
    assert_eq!(
        get(front.port, "/pomodoro").await.text(),
        r#"{"legacy": true}"#
    );
    front.stop().await;
}
