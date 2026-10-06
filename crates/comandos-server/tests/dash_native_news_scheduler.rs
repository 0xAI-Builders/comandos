//! Front ownership, one thread, completion/notice and interruptible shutdown.
mod support;
use comandos_runtime::news_editions;
use comandos_server::dash::native::{
    Background, Native,
    news::scheduler::{self, Inputs},
};
use serde_json::json;
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use support::TestHome;
#[tokio::test]
async fn scheduler_only_with_front() {
    let home = TestHome::new("news-scheduler-legacy");
    let mut opts = home.options();
    opts.background = Background::legacy();
    let native = Arc::new(Native::new(opts));
    assert!(
        scheduler::start_with(
            &native,
            Inputs::default(),
            Duration::ZERO,
            Duration::from_millis(1)
        )
        .is_none()
    );
    assert!(!home.state_db().exists());
}
#[tokio::test]
async fn scheduler_tick_builds_edition_and_one_notice() {
    let home = TestHome::new("news-scheduler-front");
    home.write(
        "news-editions.json",
        &json!({"enabled":true,"summarizer":{"kind":"acp","agent":"fixture","model":"test"}})
            .to_string(),
    );
    let mut opts = home.options();
    opts.background = Background::front();
    let now =
        news_editions::slot_times("2026-10-05", &news_editions::default_policy()).unwrap()[0].1 + 1;
    opts.clock = Arc::new(move || now);
    drop(comandos_runtime::open_state(&home.state_db(), 5000).unwrap());
    let native = Arc::new(Native::new(opts));
    let inputs = Inputs {
        fetch: Some(Arc::new(|_, _| {
            Ok(
                json!({"items":[{"url":"https://source.test/new","title":"New","fetchStatus":"ok","text":"Read me"}],"failures":[]}),
            )
        })),
        summarize: Some(Arc::new(|_| {
            Ok(json!({"model":"test","story":{"title":"Nuevo","sourceIds":["s1"]}}))
        })),
        ..Default::default()
    };
    let runner =
        scheduler::start_with(&native, inputs, Duration::ZERO, Duration::from_millis(10)).unwrap();
    assert!(scheduler::start(&native).is_none());
    let end = Instant::now() + Duration::from_secs(5);
    loop {
        let conn = comandos_runtime::open_state(&home.state_db(), 5000).unwrap();
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM events WHERE kind='news_edition'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        if count == 1 {
            let(status,source):(String,String)=conn.query_row("SELECT e.status,v.source_event_id FROM news_editions e JOIN events v ON v.source_event_id='news-edition:'||e.id WHERE e.status='published'",[],|r|Ok((r.get(0)?,r.get(1)?))).unwrap();
            assert_eq!(status, "published");
            assert_eq!(source, "news-edition:2026-10-05@09:00");
            break;
        }
        assert!(Instant::now() < end, "no edition notice");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    runner.stop();
    let end = Instant::now() + Duration::from_secs(2);
    while !runner.finished() {
        assert!(Instant::now() < end);
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}
#[tokio::test]
async fn waiting_scheduler_stops_without_waiting_two_minutes() {
    let home = TestHome::new("news-scheduler-stop");
    let mut opts = home.options();
    opts.background = Background::front();
    let native = Arc::new(Native::new(opts));
    let runner = scheduler::start(&native).unwrap();
    runner.stop();
    let end = Instant::now() + Duration::from_secs(2);
    while !runner.finished() {
        assert!(Instant::now() < end);
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}

#[tokio::test]
async fn scheduler_callback_panic_always_marks_thread_finished() {
    let home = TestHome::new("news-scheduler-panic");
    home.write(
        "news-editions.json",
        &json!({"enabled":true,"summarizer":{"kind":"acp","agent":"fixture","model":"test"}})
            .to_string(),
    );
    let mut opts = home.options();
    opts.background = Background::front();
    let now =
        news_editions::slot_times("2026-10-05", &news_editions::default_policy()).unwrap()[0].1 + 1;
    opts.clock = Arc::new(move || now);
    drop(comandos_runtime::open_state(&home.state_db(), 5000).unwrap());
    let native = Arc::new(Native::new(opts));
    let (entered, rx) = std::sync::mpsc::channel();
    let inputs = Inputs {
        fetch: Some(Arc::new(move |_, _| {
            let _ = entered.send(());
            panic!("fixture callback panic")
        })),
        summarize: Some(Arc::new(|_| panic!("must not summarize"))),
        ..Default::default()
    };
    let runner =
        scheduler::start_with(&native, inputs, Duration::ZERO, Duration::from_secs(60)).unwrap();
    let deadline = Instant::now() + Duration::from_secs(2);
    while rx.try_recv().is_err() {
        assert!(Instant::now() < deadline);
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    runner.stop();
    let deadline = Instant::now() + Duration::from_secs(2);
    while !runner.finished() {
        assert!(
            Instant::now() < deadline,
            "dead thread never published finished"
        );
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}
