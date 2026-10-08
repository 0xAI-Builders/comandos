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
