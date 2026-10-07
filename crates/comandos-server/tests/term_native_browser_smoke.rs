//! Explicit real Terminal payload, WebSocket and private tmux browser check.
mod support;

use comandos_server::dash::{
    self,
    term::{TermMode, attach::TmuxTarget},
    web::{assets::Manifest, native_term_page},
};
use std::{path::PathBuf, time::Duration};
use support::{FakeLegacy, TestHome, run_tmux, start_session};
use tokio::{net::TcpListener, sync::watch, time::timeout};

const SESSION: &str = "native-browser";
const OUTPUT: &str = "ROOT_NATIVE_TERM_BROWSER_OUTPUT";

#[tokio::test]
#[ignore = "explicit admitted Terminal payload and owned remote browser only"]
async fn serve_real_terminal_and_verify_browser_input() {
    let web_dir = PathBuf::from(
        std::env::var_os("COMANDOS_TERM_SMOKE_WEB_DIR")
            .expect("explicit admitted Terminal output directory"),
    );
    let stop_file = PathBuf::from(
        std::env::var_os("COMANDOS_TERM_SMOKE_STOP_FILE")
            .expect("explicit private browser receipt path"),
    );
    assert!(web_dir.is_absolute() && stop_file.is_absolute());
    assert!(!stop_file.exists(), "browser receipt must be fresh");
    let manifest = Manifest::load_terminal(&web_dir).expect("admitted Terminal manifest");
    native_term_page::bundle(&manifest, &web_dir).expect("real Terminal payload admission");
    assert!(support::tmux_available(), "private tmux is required");

    let home = TestHome::new_short("term-native-browser");
    start_session(&home, SESSION, "/bin/sh");
    std::fs::create_dir_all(home.tmux_dir().join("home")).unwrap();
    home.write("webterm-enabled", "");
    home.write("dash-token", "fixture-token");
    home.write(
        "comandos-web.json",
        r#"{"on":["term-main","term-tail"],"shadow":[]}"#,
    );
    let legacy = FakeLegacy::start().await;
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let mut config = dash::parse_args(&[], &home.root, Some(&legacy.port.to_string())).unwrap();
    config.port = address.port();
    config.token = b"fixture-token".to_vec();
    config.term = TermMode::Native;
    config.web_dir = web_dir;
    config.repo_root = Some(support::repo());
    let mut options = home.options();
    options.term_target = TmuxTarget::Private(home.tmux_dir());
    options.user_bin_dirs.clear();
    options.usage_effects = false;
    let (stop, shutdown) = watch::channel(false);
    let server = tokio::spawn(dash::serve_with(listener, config, Some(options), shutdown));
    println!(
        "NATIVE_TERM_SMOKE_URL=http://{address}/term/?web=native&auth=fixture-token&arg={SESSION}"
    );
    println!("NATIVE_TERM_SMOKE_RECEIPT={}", stop_file.display());

    let result = timeout(Duration::from_secs(420), async {
        while !stop_file.exists() {
            assert!(
                !server.is_finished(),
                "private server exited before browser check"
            );
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        let receipt: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&stop_file).unwrap()).unwrap();
        assert_eq!(receipt["ready"], true, "actual browser readiness");
        assert_eq!(receipt["console_errors"], 0, "actual browser console");
        assert_eq!(
            receipt["browser_closed"], true,
            "owned browser must close first"
        );
        let capture = run_tmux(&home, &["capture-pane", "-p", "-t", "=native-browser:0.0"]);
        assert!(
            capture.lines().any(|line| line.trim() == OUTPUT),
            "real browser input must execute in the private pane: {capture}"
        );
    })
    .await;
    stop.send(true).unwrap();
    timeout(Duration::from_secs(10), server)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert!(
        run_tmux(&home, &["list-clients", "-F", "#{client_pid}"])
            .trim()
            .is_empty(),
        "server shutdown must reap its private terminal client"
    );
    assert!(
        legacy.requests().is_empty(),
        "Terminal stays native"
    );
    result.expect("owned remote browser deadline");
}
