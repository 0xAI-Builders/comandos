use comandos_browser::{
    config::BrokerConfig,
    pool::Pool,
    session::{ERR_BUSY, ERR_EXPIRED, ERR_TIMEOUT, Registry, SessionId, ToolBackend},
    worker::CloseError,
};
use serde_json::{Value, json};
use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

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

fn cfg_with_env(dir: PathBuf, env: serde_json::Value) -> BrokerConfig {
    BrokerConfig::from_value(
        json!({
            "command":[env!("CARGO_BIN_EXE_fake-mcp-worker")],
            "env":env,
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

#[tokio::test(flavor = "current_thread")]
async fn concurrent_slow_starts_reserve_capacity_before_spawn() {
    let dir = temp_dir("slow-start-capacity");
    let mut c = cfg_with_env(dir.clone(), json!({"FAKE_INIT_DELAY_MS":"200"}));
    c.max_workers = 2;
    c.queue_timeout = 0.05;
    let pool = Pool::new(c, Arc::new(Registry::new()));
    let (a, b, c) = tokio::join!(
        call(&pool, "a", json!({})),
        call(&pool, "b", json!({})),
        call(&pool, "c", json!({}))
    );
    let texts = [a, b, c]
        .into_iter()
        .map(|value| value["content"][0]["text"].as_str().unwrap().to_owned())
        .collect::<Vec<_>>();
    assert_eq!(
        texts
            .iter()
            .filter(|text| text.as_str() == "ok navigate_page")
            .count(),
        2
    );
    assert_eq!(
        texts
            .iter()
            .filter(|text| text.as_str() == ERR_BUSY)
            .count(),
        1
    );
    assert!(pool.status().await["workers"].as_u64().unwrap() <= 2);
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test(flavor = "current_thread")]
async fn closing_worker_keeps_capacity_until_close_finishes() {
    let dir = temp_dir("closing-capacity");
    let mut c = cfg(dir.clone());
    c.max_workers = 1;
    c.queue_timeout = 0.05;
    c.idle_seconds = 0.01;
    let pool = Pool::new(c, Arc::new(Registry::new()));
    assert_eq!(
        call(&pool, "a", json!({})).await["content"][0]["text"],
        "ok navigate_page"
    );
    let (started_tx, started_rx) = tokio::sync::oneshot::channel();
    let (release_tx, release_rx) = tokio::sync::oneshot::channel();
    let started = Arc::new(tokio::sync::Mutex::new(Some(started_tx)));
    let release = Arc::new(tokio::sync::Mutex::new(Some(release_rx)));
    pool.with_close_hook(Arc::new({
        let started = started.clone();
        let release = release.clone();
        move || {
            let started = started.clone();
            let release = release.clone();
            Box::pin(async move {
                if let Some(tx) = started.lock().await.take() {
                    let _ = tx.send(());
                }
                if let Some(rx) = release.lock().await.take() {
                    let _ = rx.await;
                }
                Ok(())
            })
        }
    }))
    .await;
    tokio::time::sleep(Duration::from_millis(30)).await;
    let reaper = {
        let pool = pool.clone();
        tokio::spawn(async move {
            pool.reap_idle().await;
        })
    };
    started_rx.await.unwrap();
    let out = call(&pool, "b", json!({})).await;
    assert_eq!(out["content"][0]["text"], ERR_BUSY);
    let _ = release_tx.send(());
    reaper.await.unwrap();
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test(flavor = "current_thread")]
async fn queued_acquire_waits_until_deadline_and_wakes_on_release() {
    let dir = temp_dir("queue-deadline");
    let mut c = cfg(dir.clone());
    c.max_workers = 1;
    c.queue_timeout = 0.5;
    let pool = Pool::new(c, Arc::new(Registry::new()));
    assert_eq!(
        call(&pool, "a", json!({})).await["content"][0]["text"],
        "ok navigate_page"
    );
    let started = Instant::now();
    let release = {
        let pool = pool.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(150)).await;
            pool.disconnect(SessionId("a".to_owned())).await;
        })
    };
    let b = call(&pool, "b", json!({})).await;
    assert_eq!(b["content"][0]["text"], "ok navigate_page");
    assert!(started.elapsed() >= Duration::from_millis(140));
    release.await.unwrap();
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test(flavor = "current_thread")]
async fn retrying_stuck_worker_keeps_capacity_reserved() {
    let dir = temp_dir("stuck-capacity");
    let mut c = cfg(dir.clone());
    c.max_workers = 1;
    c.queue_timeout = 0.05;
    c.idle_seconds = 0.01;
    let pool = Pool::new(c, Arc::new(Registry::new()));
    assert_eq!(
        call(&pool, "a", json!({})).await["content"][0]["text"],
        "ok navigate_page"
    );
    pool.fail_next_close();
    tokio::time::sleep(Duration::from_millis(30)).await;
    pool.reap_idle().await;
    assert_eq!(pool.status().await["stuck"], 1);
    let attempts = Arc::new(AtomicUsize::new(0));
    let (started_tx, started_rx) = tokio::sync::oneshot::channel();
    let (release_tx, release_rx) = tokio::sync::oneshot::channel();
    let started = Arc::new(tokio::sync::Mutex::new(Some(started_tx)));
    let release = Arc::new(tokio::sync::Mutex::new(Some(release_rx)));
    pool.with_close_hook(Arc::new({
        let attempts = attempts.clone();
        let started = started.clone();
        let release = release.clone();
        move || {
            let attempts = attempts.clone();
            let started = started.clone();
            let release = release.clone();
            Box::pin(async move {
                attempts.fetch_add(1, Ordering::SeqCst);
                if let Some(tx) = started.lock().await.take() {
                    let _ = tx.send(());
                }
                if let Some(rx) = release.lock().await.take() {
                    let _ = rx.await;
                }
                Err(CloseError::DescendantsAlive)
            })
        }
    }))
    .await;
    let retry = {
        let pool = pool.clone();
        tokio::spawn(async move {
            pool.retry_stuck().await;
        })
    };
    started_rx.await.unwrap();
    let out = call(&pool, "b", json!({})).await;
    assert_eq!(out["content"][0]["text"], ERR_BUSY);
    let _ = release_tx.send(());
    retry.await.unwrap();
    assert_eq!(attempts.load(Ordering::SeqCst), 1);
    assert_eq!(pool.status().await["stuck"], 1);
    let _ = std::fs::remove_dir_all(dir);
}
