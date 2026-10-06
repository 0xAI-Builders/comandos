use comandos_browser::{
    config::BrokerConfig,
    pool::Pool,
    session::{ERR_BUSY, ERR_EXPIRED, ERR_TIMEOUT, Registry, SessionId, ToolBackend},
};
use serde_json::{Value, json};
use std::{path::PathBuf, sync::Arc, time::Duration};

fn temp_dir(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "cb-{label}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn cfg(dir: PathBuf) -> BrokerConfig {
    BrokerConfig::from_value(
        json!({
            "command":[env!("CARGO_BIN_EXE_fake-mcp-worker")],
            "state_dir":dir,
            "catalog":{"tools":[{"name":"navigate_page"}]},
            "tool_timeout":1.0,
            "idle_seconds":300.0,
            "stop_grace":0.2
        }),
        &std::env::temp_dir(),
    )
    .unwrap()
}

async fn call(pool: &Arc<Pool>, id: &str, arguments: Value) -> Value {
    pool.call(
        SessionId(id.to_owned()),
        json!({"name":"navigate_page","arguments":arguments}),
    )
    .await
}

#[tokio::test(flavor = "current_thread")]
async fn third_session_is_busy_with_python_text() {
    let dir = temp_dir("busy");
    let mut c = cfg(dir.clone());
    c.max_workers = 2;
    let pool = Pool::new(c, Arc::new(Registry::new()));
    assert_eq!(
        call(&pool, "a", json!({})).await["content"][0]["text"],
        "ok navigate_page"
    );
    assert_eq!(
        call(&pool, "b", json!({})).await["content"][0]["text"],
        "ok navigate_page"
    );
    let out = call(&pool, "c", json!({})).await;
    assert_eq!(out["content"][0]["text"], ERR_BUSY);
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test(flavor = "current_thread")]
async fn python_workers_count_against_cap() {
    let dir = temp_dir("python-status");
    let status = dir.join("python-status.json");
    std::fs::write(&status, r#"{"workers":1}"#).unwrap();
    let mut c = cfg(dir.clone());
    c.python_status = Some(status);
    let pool = Pool::new(c, Arc::new(Registry::new()));
    assert_eq!(
        call(&pool, "a", json!({})).await["content"][0]["text"],
        "ok navigate_page"
    );
    let out = call(&pool, "b", json!({})).await;
    assert_eq!(out["content"][0]["text"], ERR_BUSY);
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test(flavor = "current_thread")]
async fn idle_session_expires_then_reports_expired_once() {
    let dir = temp_dir("expired");
    let mut c = cfg(dir.clone());
    c.idle_seconds = 0.01;
    let pool = Pool::new(c, Arc::new(Registry::new()));
    assert_eq!(
        call(&pool, "a", json!({})).await["content"][0]["text"],
        "ok navigate_page"
    );
    tokio::time::sleep(Duration::from_millis(30)).await;
    pool.reap_idle().await;
    let out = call(&pool, "a", json!({})).await;
    assert_eq!(out["content"][0]["text"], ERR_EXPIRED);
    let out = call(&pool, "a", json!({})).await;
    assert_eq!(out["content"][0]["text"], "ok navigate_page");
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test(flavor = "current_thread")]
async fn tool_timeout_releases_after_worker_exit() {
    let dir = temp_dir("timeout");
    let mut c = cfg(dir.clone());
    c.tool_timeout = 0.05;
    let pool = Pool::new(c, Arc::new(Registry::new()));
    let out = call(&pool, "a", json!({"sleep_ms":5000})).await;
    assert_eq!(out["content"][0]["text"], ERR_TIMEOUT);
    assert_eq!(pool.status().await["workers"], 0);
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test(flavor = "current_thread")]
async fn housekeeping_survives_close_failure_and_status_keeps_stuck() {
    let dir = temp_dir("stuck");
    let mut c = cfg(dir.clone());
    c.idle_seconds = 0.01;
    let pool = Pool::new(c, Arc::new(Registry::new()));
    assert_eq!(
        call(&pool, "a", json!({})).await["content"][0]["text"],
        "ok navigate_page"
    );
    pool.fail_next_close();
    tokio::time::sleep(Duration::from_millis(30)).await;
    pool.reap_idle().await;
    let st = pool.status().await;
    assert_eq!(st["stuck"], 1);
    comandos_browser::broker::write_status(&dir, &st).unwrap();
    let first = std::fs::metadata(dir.join("status.json"))
        .unwrap()
        .modified()
        .unwrap();
    tokio::time::sleep(Duration::from_millis(10)).await;
    comandos_browser::broker::write_status(&dir, &pool.status().await).unwrap();
    assert!(
        std::fs::metadata(dir.join("status.json"))
            .unwrap()
            .modified()
            .unwrap()
            >= first
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test(flavor = "current_thread")]
async fn worker_notifications_reach_owner_session() {
    let dir = temp_dir("notify");
    let registry = Arc::new(Registry::new());
    let (session, mut notes) = registry.admit().await.unwrap();
    let pool = Pool::new(cfg(dir.clone()), registry);
    let out = pool
        .call(
            session.clone(),
            json!({"name":"navigate_page","arguments":{"notify":true}}),
        )
        .await;
    assert_eq!(out["content"][0]["text"], "ok navigate_page");
    let note = tokio::time::timeout(Duration::from_secs(1), notes.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(note["method"], "notifications/message");
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test(flavor = "current_thread")]
async fn status_keys_order_matches_python_with_stuck_at_end() {
    let dir = temp_dir("status-order");
    let pool = Pool::new(cfg(dir.clone()), Arc::new(Registry::new()));
    let text = comandos_browser::wire::encode_status(&pool.status().await).unwrap();
    assert!(text.starts_with(
        r#"{"connections": 0, "workers": 0, "maxWorkers": 2, "queued": 0, "sessions": []"#
    ));
    assert!(text.ends_with(r#", "stuck": 0}"#));
    let _ = std::fs::remove_dir_all(dir);
}
