use comandos_browser::{config::BrokerConfig, worker::Worker};
use serde_json::json;
use std::path::PathBuf;
use tokio::sync::mpsc;

fn temp_dir(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "cb-worker-{label}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[tokio::test(flavor = "current_thread")]
async fn worker_initializes_calls_and_forwards_notifications() {
    let dir = temp_dir("basic");
    let cfg = BrokerConfig::from_value(
        json!({
            "command":[env!("CARGO_BIN_EXE_fake-mcp-worker")],
            "state_dir":dir,
            "catalog":{"tools":[{"name":"navigate_page"}]},
            "stop_grace":0.2
        }),
        &std::env::temp_dir(),
    )
    .unwrap();
    let (tx, mut rx) = mpsc::channel(4);
    let worker = std::sync::Arc::new(
        Worker::start(&cfg, cfg.state_dir.join("profile"), tx)
            .await
            .unwrap(),
    );
    let out = worker
        .request(
            "tools/call",
            json!({"name":"navigate_page","arguments":{"notify":true}}),
        )
        .await
        .unwrap();
    assert_eq!(out["result"]["content"][0]["text"], "ok navigate_page");
    let note = rx.recv().await.unwrap();
    assert_eq!(note["method"], "notifications/message");
    worker.close(0.2).await.unwrap();
    let _ = std::fs::remove_dir_all(cfg.state_dir);
}

#[tokio::test(flavor = "current_thread")]
async fn notifications_before_result_do_not_block_response_progress() {
    let dir = temp_dir("notification-backpressure");
    let cfg = BrokerConfig::from_value(
        json!({
            "command":[env!("CARGO_BIN_EXE_fake-mcp-worker")],
            "state_dir":dir,
            "catalog":{"tools":[{"name":"navigate_page"}]},
            "stop_grace":0.2
        }),
        &std::env::temp_dir(),
    )
    .unwrap();
    let (tx, _rx) = mpsc::channel(1);
    let worker = std::sync::Arc::new(
        Worker::start(&cfg, cfg.state_dir.join("profile"), tx)
            .await
            .unwrap(),
    );
    let out = tokio::time::timeout(
        std::time::Duration::from_millis(500),
        worker.request(
            "tools/call",
            json!({"name":"navigate_page","arguments":{"notify_count":32}}),
        ),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(out["result"]["content"][0]["text"], "ok navigate_page");
    worker.close(0.2).await.unwrap();
    let _ = std::fs::remove_dir_all(cfg.state_dir);
}

#[tokio::test(flavor = "current_thread")]
async fn failed_initialization_cleans_up_child_process() {
    let dir = temp_dir("init-fail-cleanup");
    let cfg = BrokerConfig::from_value(
        json!({
            "command":[env!("CARGO_BIN_EXE_fake-mcp-worker")],
            "env":{"FAKE_INIT_FAIL":"1"},
            "state_dir":dir,
            "catalog":{"tools":[{"name":"navigate_page"}]},
            "stop_grace":0.2
        }),
        &std::env::temp_dir(),
    )
    .unwrap();
    let (tx, _rx) = mpsc::channel(1);
    assert!(
        Worker::start(&cfg, cfg.state_dir.join("profile"), tx)
            .await
            .is_err()
    );
    assert!(!cfg.state_dir.join("profile").exists());
    let _ = std::fs::remove_dir_all(cfg.state_dir);
}
