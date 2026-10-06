//! TF: owned producers, per-producer cuts and prompt Stop, all in fixture HOME.
mod support;
use comandos_server::dash::native::{Background, Cut, Native, background, news::scheduler, remote};
use std::{sync::Arc, time::Duration};
use support::TestHome;

#[tokio::test]
async fn start_is_owned_idempotent_and_shutdown_wakes_waiters() {
    for (mode, initial_count, count) in [(Background::legacy(), 1, 1), (Background::front(), 5, 4)]
    {
        let home = TestHome::new("background-owner");
        let mut opts = home.options();
        opts.background = mode;
        opts.usage_state_native = false;
        let native = Arc::new(Native::new(opts));
        assert!(native.ready().await);
        native.start_background();
        assert_eq!(native.tasks().len(), initial_count);
        native.start_background();
        assert_eq!(native.tasks().len(), initial_count);
        // The one-shot operations recovery finishes; the four loop owners stay.
        tokio::time::timeout(Duration::from_secs(2), async {
            while native.tasks().len() != count {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap();
        tokio::time::sleep(Duration::from_millis(30)).await;
        assert_eq!(native.tasks().len(), count, "news Runner must stay alive");
        native.shutdown().await;
        tokio::time::timeout(Duration::from_secs(2), async {
            while !native.tasks().is_empty() {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap();
        native.start_background();
        assert!(native.tasks().is_empty());
    }
}

#[tokio::test]
async fn disabled_producers_cannot_boot_or_write_but_usage_loop_remains() {
    let home = TestHome::new("background-cuts");
    home.write("webterm-enabled", "1");
    let mut opts = home.options();
    opts.background = Background::front();
    opts.usage_state_native = false;
    opts.cuts_off.extend([Cut::Services, Cut::News]);
    let native = Arc::new(Native::new(opts));
    let runner = background::start(&native);
    assert!(!runner.pomodoro());
    assert!(!runner.models());
    assert!(runner.limits());
    assert!(!remote::start(&native));
    assert!(scheduler::start(&native).is_none());
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(
        !home.state_db().exists(),
        "news recovery must not open/create the DB"
    );
    assert!(
        !native.options().usage_db.exists(),
        "pomodoro must not write"
    );
    assert!(!home.hooks().join("model-watch.json").exists());
    assert!(!home.root.join("cli.log").exists());
    assert_eq!(native.tasks().len(), 1);
    runner.stop();
    native.shutdown().await;
}

#[tokio::test]
async fn drop_stops_waiting_producers_without_an_arc_cycle() {
    let home = TestHome::new("background-drop");
    let mut opts = home.options();
    opts.background = Background::front();
    opts.usage_state_native = false;
    let native = Arc::new(Native::new(opts));
    assert!(native.ready().await);
    native.start_background();
    let tasks = native.tasks().clone();
    let weak = Arc::downgrade(&native);
    drop(native);
    tokio::time::timeout(Duration::from_secs(2), async {
        while !tasks.is_empty() || weak.upgrade().is_some() {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn shutdown_does_not_join_unbounded_registered_operations() {
    let home = TestHome::new("background-long-task");
    let native = Arc::new(Native::new(home.options()));
    assert!(native.ready().await);
    let task = native
        .tasks()
        .spawn_handle(std::future::pending::<()>())
        .unwrap();
    tokio::time::timeout(Duration::from_secs(2), native.shutdown())
        .await
        .unwrap();
    assert_eq!(
        native.tasks().len(),
        1,
        "D12 leaves long operations to process teardown"
    );
    task.abort();
    let _ = task.await;
}

#[tokio::test]
async fn native_start_recovers_ops_queue_only_for_its_enabled_front_owner() {
    for (mode, off, expected) in [
        (Background::legacy(), false, false),
        (Background::front(), true, false),
        (Background::front(), false, true),
    ] {
        let home = TestHome::new("background-ops-owner");
        home.write("motor-queue.json", r#"{"audit|%0":{}}"#);
        let mut opts = home.options();
        opts.background = mode;
        opts.usage_state_native = false;
        opts.cuts_off.extend([Cut::Services, Cut::News]);
        if off {
            opts.cuts_off.insert(Cut::Ops);
        }
        let native = Arc::new(Native::new(opts));
        native.start_background();
        if expected {
            tokio::time::timeout(Duration::from_secs(2), async {
                while !home.hooks().join("motor-results.json").exists() {
                    tokio::time::sleep(Duration::from_millis(5)).await;
                }
            })
            .await
            .unwrap();
            let results =
                comandos_server::dash::native::ops::results::MotorResults::shared(native.options())
                    .all();
            assert_eq!(results["audit|%0"]["ok"], false);
            assert!(
                results["audit|%0"]["detail"]
                    .as_str()
                    .unwrap()
                    .contains("sin snapshot")
            );
        } else {
            tokio::time::sleep(Duration::from_millis(50)).await;
            assert!(!home.hooks().join("motor-results.json").exists());
            assert!(!home.journal_db().exists());
        }
        native.shutdown().await;
    }
}
