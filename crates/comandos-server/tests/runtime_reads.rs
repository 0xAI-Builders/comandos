//! Runtime caches must not stall the single HTTP executor on a mode lease.
mod support;
use comandos_server::dash::{self, term::lifecycle::Control, web::WebState};
use comandos_store::{files::FileLock, unified};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use support::TestHome;

#[tokio::test]
async fn live_controls_keep_the_last_selection_while_authority_is_busy() {
    let home = TestHome::new("runtime-read-cache");
    home.write("webterm-enabled", "");
    home.write("comandos-web.json", r#"{"on":["live"],"shadow":[]}"#);
    let path = unified::unified_path(&home.root);
    let db = unified::open_unified(&path).unwrap();
    let cfg = dash::parse_args(&[], &home.root, None).unwrap();
    let web = Arc::new(WebState::new(&cfg));
    let control = Control::new_live(&home.root);
    assert!(control.enabled());
    assert!(web.selection().on.contains("live"));
    tokio::time::sleep(Duration::from_millis(1010)).await;
    let lock =
        FileLock::exclusive(&path.with_file_name("comandos.sqlite3.domain-modes.lock")).unwrap();
    let started = Instant::now();
    for _ in 0..100 {
        assert!(control.enabled());
        assert!(web.selection().on.contains("live"));
    }
    assert!(started.elapsed() < Duration::from_millis(200));
    web.refresh_selection().await;
    assert!(
        web.selection().on.contains("live"),
        "a failed refresh must retain the selection"
    );
    assert!(
        control.refresh().await,
        "busy authority must not disable active terminals"
    );
    drop(lock);
    std::fs::remove_file(home.hooks().join("webterm-enabled")).unwrap();
    assert!(!control.refresh().await);
    assert!(!control.enabled());
    home.write("webterm-enabled", "");
    assert!(control.refresh().await);
    assert!(control.enabled());
    drop(db);
}

#[tokio::test]
async fn native_document_reads_wait_for_a_brief_mode_lease_off_the_http_executor() {
    use comandos_server::dash::native::files::DomainDocument;
    let home = TestHome::new("runtime-read-contention");
    home.write("app-tabs.json", r#"{"local":"Local"}"#);
    let path = unified::unified_path(&home.root);
    let db = unified::open_unified(&path).unwrap();
    let lock =
        FileLock::exclusive(&path.with_file_name("comandos.sqlite3.domain-modes.lock")).unwrap();
    let doc = DomainDocument::new(&home.root, &home.hooks(), "app-tabs.json").unwrap();
    assert!(
        matches!(doc.read_bytes(), Err(comandos_store::Error::ModeBusy)),
        "HTTP-thread admission must remain nonblocking"
    );
    let worker = tokio::task::spawn_blocking(move || doc.read_bytes_in_worker());
    tokio::time::sleep(Duration::from_millis(30)).await;
    assert!(
        !worker.is_finished(),
        "brief contention must wait instead of declining to Python"
    );
    drop(lock);
    assert_eq!(
        worker.await.unwrap().unwrap().unwrap(),
        br#"{"local":"Local"}"#
    );

    let lock =
        FileLock::exclusive(&path.with_file_name("comandos.sqlite3.domain-modes.lock")).unwrap();
    let doc = DomainDocument::new(&home.root, &home.hooks(), "app-tabs.json").unwrap();
    let worker = tokio::task::spawn_blocking(move || doc.read_bytes_in_worker());
    assert!(matches!(
        tokio::time::timeout(Duration::from_secs(1), worker)
            .await
            .unwrap()
            .unwrap(),
        Err(comandos_store::Error::ModeBusy)
    ));
    drop(lock);
    drop(db);
}
