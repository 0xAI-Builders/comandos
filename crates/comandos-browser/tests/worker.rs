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
    let worker = Worker::start(&cfg, cfg.state_dir.join("profile"), tx)
        .await
        .unwrap();
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
    let worker = Worker::start(&cfg, cfg.state_dir.join("profile"), tx)
        .await
        .unwrap();
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

fn alive(pid: i32) -> bool {
    std::fs::read_to_string(format!("/proc/{pid}/stat"))
        .ok()
        .is_some_and(|stat| {
            stat[stat.rfind(')').unwrap() + 2..]
                .split_whitespace()
                .next()
                != Some("Z")
        })
}

#[tokio::test(flavor = "current_thread")]
async fn cancelled_close_after_reaping_root_retains_descendant_generations() {
    let dir = temp_dir("retry-descendants");
    let child_file = dir.join("child.pid");
    let cfg=BrokerConfig::from_value(json!({"command":[env!("CARGO_BIN_EXE_fake-mcp-worker")],"env":{"FAKE_CHILD_PID_FILE":child_file},"state_dir":dir,"catalog":{"tools":[{"name":"navigate_page"}]},"stop_grace":0.1}), &dir).unwrap();
    let (tx, _rx) = mpsc::channel(1);
    let worker = Worker::start(&cfg, dir.join("profile"), tx).await.unwrap();
    let root = worker.pid().unwrap() as i32;
    let descendant = std::fs::read_to_string(child_file)
        .unwrap()
        .parse::<i32>()
        .unwrap();
    let close = {
        let worker = worker.clone();
        tokio::spawn(async move { worker.close(0.1).await })
    };
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        while comandos_browser::proc_scan::process_identity(std::path::Path::new("/proc"), root)
            .is_some()
        {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    close.abort();
    let _ = close.await;
    let was_alive = alive(descendant);
    let result = worker.clone().close(0.1).await;
    let survived = alive(descendant);
    let _ = nix::sys::signal::kill(
        nix::unistd::Pid::from_raw(descendant),
        nix::sys::signal::Signal::SIGKILL,
    );
    assert!(was_alive, "fixture must survive TERM and root reaping");
    assert!(result.is_ok());
    assert!(!survived);
}

#[tokio::test(flavor = "current_thread")]
async fn blocked_stdin_does_not_hide_root_or_descendant_ownership() {
    let dir = temp_dir("blocked-stdin");
    let child_file = dir.join("child.pid");
    let cfg=BrokerConfig::from_value(json!({"command":[env!("CARGO_BIN_EXE_fake-mcp-worker")],"env":{"FAKE_CHILD_PID_FILE":child_file,"FAKE_PAUSE_AFTER_INIT_MS":"5000"},"state_dir":dir,"catalog":{"tools":[{"name":"navigate_page"}]},"stop_grace":0.1}), &dir).unwrap();
    let (tx, _rx) = mpsc::channel(1);
    let worker = Worker::start(&cfg, dir.join("profile"), tx).await.unwrap();
    let descendant = std::fs::read_to_string(child_file)
        .unwrap()
        .parse::<i32>()
        .unwrap();
    let request = {
        let worker = worker.clone();
        tokio::spawn(async move {
            worker
                .request("tools/call", json!({"payload":"x".repeat(500_000)}))
                .await
        })
    };
    tokio::task::yield_now().await;
    let has_pid = worker.pid().is_some();
    let result = worker.clone().close(0.1).await;
    request.abort();
    let _ = request.await;
    let survived = alive(descendant);
    let _ = nix::sys::signal::kill(
        nix::unistd::Pid::from_raw(descendant),
        nix::sys::signal::Signal::SIGKILL,
    );
    assert!(has_pid);
    assert!(result.is_ok());
    assert!(!survived);
}

#[tokio::test(flavor = "current_thread")]
async fn dropping_a_worker_terminates_its_owned_descendants() {
    let dir = temp_dir("drop-descendants");
    let child_file = dir.join("child.pid");
    let cfg=BrokerConfig::from_value(json!({"command":[env!("CARGO_BIN_EXE_fake-mcp-worker")],"env":{"FAKE_CHILD_PID_FILE":child_file},"state_dir":dir,"catalog":{"tools":[{"name":"navigate_page"}]}}),&dir).unwrap();
    let (tx, _rx) = mpsc::channel(1);
    let worker = Worker::start(&cfg, dir.join("profile"), tx).await.unwrap();
    let descendant = std::fs::read_to_string(child_file)
        .unwrap()
        .parse::<i32>()
        .unwrap();
    drop(worker);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    while alive(descendant) && std::time::Instant::now() < deadline {
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    let survived = alive(descendant);
    let _ = nix::sys::signal::kill(
        nix::unistd::Pid::from_raw(descendant),
        nix::sys::signal::Signal::SIGKILL,
    );
    assert!(!survived);
}
