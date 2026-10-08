//! Real HTTP against disposable files, without browser or tmux processes.
use comandos_server::{
    Config,
    dash::{
        self, DashState,
        web::{
            WebState,
            registry::{Entry, Resolved, sha256_hex},
        },
    },
};
use std::{path::PathBuf, sync::Arc, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::watch,
};

const PAGE: &str = "<meta charset=\"utf-8\">\n<script src=\"/quick-terminal.js\"></script>\n";

struct Front {
    root: PathBuf,
    address: std::net::SocketAddr,
    stop: watch::Sender<bool>,
    task: tokio::task::JoinHandle<std::io::Result<()>>,
}
impl Front {
    async fn start() -> Self {
        Self::start_readonly(false).await
    }
    async fn start_readonly(readonly: bool) -> Self {
        let mut nonce = [0u8; 8];
        getrandom::fill(&mut nonce).unwrap();
        let root = std::env::temp_dir().join(format!(
            "comandos-web-http-{}-{:x}",
            std::process::id(),
            u64::from_le_bytes(nonce)
        ));
        std::fs::create_dir_all(root.join(".claude/hooks/dash")).unwrap();
        std::fs::create_dir_all(root.join("web/fedcba987654")).unwrap();
        std::fs::write(root.join(".claude/hooks/dash/index.html"), PAGE).unwrap();
        std::fs::write(
            root.join(".claude/hooks/comandos-web.json"),
            r#"{"on":["quick-terminal"],"shadow":[]}"#,
        )
        .unwrap();
        std::fs::write(root.join("quick-terminal.js"), b"original-script").unwrap();
        std::fs::write(root.join("web/manifest.json"), r#"{"files":{"comandos_web_boot.js":"fedcba987654/boot.js","comandos_web.js":"fedcba987654/comandos_web.js","comandos_web_bg.wasm":"fedcba987654/comandos_web_bg.wasm"}}"#).unwrap();
        for name in ["boot.js", "comandos_web.js", "comandos_web_bg.wasm"] {
            std::fs::write(root.join("web/fedcba987654").join(name), b"fixture").unwrap();
        }
        let mut cfg = dash::parse_args(&[], &root, None).unwrap();
        cfg.native = false;
        cfg.shadow_readonly = readonly;
        cfg.token = b"fixture-token".to_vec();
        cfg.repo_root = Some(root.clone());
        cfg.web_dir = root.join("web");
        let mut web = WebState::new(&cfg);
        web.registry = Resolved::from_repo(
            vec![Entry::script(
                "quick-terminal",
                "quick-terminal.js",
                &sha256_hex(b"original-script"),
                &[],
                &[],
            )],
            &root,
        );
        let asset_exists = dash::asset_exists(&cfg.dash_dir);
        let state = Arc::new(DashState {
            web,
            config: cfg,
            asset_exists: asset_exists.clone(),
            native: None,
            term_target: dash::term::attach::TmuxTarget::Private(root.clone()),
            term_control: Arc::new(dash::term::lifecycle::Control::new(&root)),
        });
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let config = Config {
            token: b"fixture-token".to_vec(),
            token_file: None,
            asset_exists,
            handler: dash::handler(state),
            websocket: None,
            limits: dash::limits(),
        };
        let (stop, rx) = watch::channel(false);
        let task = tokio::spawn(comandos_server::serve(listener, config, rx));
        Self {
            root,
            address,
            stop,
            task,
        }
    }
    async fn http(&self, method: &str, path: &str, body: &str, headers: &str) -> String {
        let mut stream = TcpStream::connect(self.address).await.unwrap();
        stream.write_all(format!("{method} {path} HTTP/1.1\r\nHost: {}\r\nContent-Length: {}\r\nConnection: close\r\n{headers}\r\n{body}", self.address, body.len()).as_bytes()).await.unwrap();
        let mut raw = Vec::new();
        tokio::time::timeout(Duration::from_secs(12), stream.read_to_end(&mut raw))
            .await
            .unwrap()
            .unwrap();
        String::from_utf8(raw).unwrap()
    }
    async fn page(&self) -> String {
        self.http("GET", "/", "", "").await
    }
    async fn stop(self) {
        self.stop.send(true).unwrap();
        tokio::time::timeout(Duration::from_secs(3), self.task)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        std::fs::remove_dir_all(self.root).unwrap();
    }
}
fn nonce(page: &str) -> &str {
    page.split_once("data-k=\"")
        .unwrap()
        .1
        .split_once('"')
        .unwrap()
        .0
}

#[tokio::test]
async fn early_ready_releases_real_http_gate_and_web_off_is_byte_exact() {
    let front = Front::start().await;
    let page = front.page().await;
    assert!(page.starts_with("HTTP/1.1 200"));
    let k = nonce(&page);
    let reply = front
        .http(
            "POST",
            "/web/ready",
            &format!(r#"{{"k":"{k}","mounted":["quick-terminal"],"failed":[]}}"#),
            "",
        )
        .await;
    assert!(reply.starts_with("HTTP/1.1 200"), "{reply}");
    let gate = front
        .http("GET", &format!("/web/gate.js?k={k}"), "", "")
        .await;
    assert!(gate.ends_with("\r\n\r\n"), "ready returns empty JS: {gate}");
    assert!(
        front
            .http("POST", "/web/ready", &format!(r#"{{"k":"{k}"}}"#), "")
            .await
            .starts_with("HTTP/1.1 400")
    );
    assert!(
        front
            .http("GET", "/?web=off&app=1", "", "")
            .await
            .ends_with(PAGE)
    );
    let asset = front.http("GET", "/web/fedcba987654/boot.js", "", "").await;
    assert!(asset.contains("cache-control: public, max-age=31536000, immutable"));
    let manifest = front.http("GET", "/web/manifest.json", "", "").await;
    assert!(
        manifest.contains("cache-control: no-store"),
        "unversioned manifest must not become immutable: {manifest}"
    );
    front.stop().await;
}

#[tokio::test]
async fn source_and_artifact_changes_fail_closed_without_restart() {
    let front = Front::start().await;
    assert!(front.page().await.contains("data-k=\""));
    std::fs::write(
        front.root.join("quick-terminal.js"),
        b"different-new-script",
    )
    .unwrap();
    assert!(front.page().await.ends_with(PAGE));
    std::fs::write(front.root.join("quick-terminal.js"), b"original-script").unwrap();
    assert!(front.page().await.contains("data-k=\""));
    std::fs::remove_file(front.root.join("web/fedcba987654/comandos_web_bg.wasm")).unwrap();
    assert!(front.page().await.ends_with(PAGE));
    std::fs::write(
        front.root.join("web/fedcba987654/comandos_web_bg.wasm"),
        b"fixture",
    )
    .unwrap();
    assert!(front.page().await.contains("data-k=\""));
    front.stop().await;
}

#[tokio::test]
async fn readonly_front_releases_control_gate_without_enabling_mutations() {
    let front = Front::start_readonly(true).await;
    let page = front.page().await;
    let k = nonce(&page);
    assert!(
        front
            .http("POST", "/web/ready", &format!(r#"{{"k":"{k}"}}"#), "")
            .await
            .starts_with("HTTP/1.1 200")
    );
    let gate = front
        .http("GET", &format!("/web/gate.js?k={k}"), "", "")
        .await;
    assert!(
        gate.ends_with("\r\n\r\n"),
        "ready in shadow must not timeout: {gate}"
    );
    let mutation = front
        .http("POST", "/prefs", r#"{"theme":"changed"}"#, "")
        .await;
    assert!(mutation.contains(r#""shadow": true"#));
    front.stop().await;
}

#[tokio::test]
async fn selection_changes_are_observed_without_reconstructing_server_state() {
    let front = Front::start().await;
    assert!(front.page().await.contains("data-k=\""));
    std::fs::write(
        front.root.join(".claude/hooks/comandos-web.json"),
        r#"{"on":[],"shadow":["quick-terminal"]}"#,
    )
    .unwrap();
    tokio::time::sleep(Duration::from_millis(1100)).await;
    assert!(front.page().await.ends_with(PAGE));
    let shadow = front.http("GET", "/?web=shadow", "", "").await;
    assert!(shadow.contains("data-k=\""));
    assert!(shadow.contains("set-cookie: cc_web=shadow; Path=/; SameSite=Strict; HttpOnly"));
    let inherited = front
        .http("GET", "/", "", "Cookie: cc_web=shadow\r\n")
        .await;
    assert!(inherited.contains("data-k=\""));
    front.stop().await;
}

#[tokio::test]
async fn nonce_capacity_is_per_server_and_unknown_ready_is_rejected() {
    let front = Front::start().await;
    let other = Front::start().await;
    assert!(
        front
            .http("POST", "/web/ready", r#"{"k":"invented"}"#, "")
            .await
            .starts_with("HTTP/1.1 400")
    );
    for _ in 0..256 {
        assert!(front.page().await.contains("data-k=\""));
    }
    assert!(front.page().await.ends_with(PAGE));
    assert!(
        other.page().await.contains("data-k=\""),
        "capacity must not leak across server instances"
    );
    let status = front.http("GET", "/web/status", "", "").await;
    let value: serde_json::Value =
        serde_json::from_str(status.split_once("\r\n\r\n").unwrap().1).unwrap();
    assert_eq!(value["nonces"]["in_flight"], 256);
    assert_eq!(value["nonces"]["gate_full"], 1);
    other.stop().await;
    front.stop().await;
}

#[tokio::test]
async fn timeout_returns_one_shot_fallback_and_releases_capacity() {
    let front = Front::start().await;
    let page = front.page().await;
    let gate = front
        .http("GET", &format!("/web/gate.js?k={}", nonce(&page)), "", "")
        .await;
    assert!(gate.contains("sessionStorage.cc_web_fallback"));
    assert!(gate.contains("web-gate-timeout"));
    let status = front.http("GET", "/web/status", "", "").await;
    let value: serde_json::Value =
        serde_json::from_str(status.split_once("\r\n\r\n").unwrap().1).unwrap();
    assert_eq!(value["nonces"]["in_flight"], 0);
    front.stop().await;
}

#[tokio::test]
async fn remote_status_requires_dashboard_access_even_though_gate_is_public() {
    let front = Front::start().await;
    let denied = front
        .http("GET", "/web/status", "", "X-Forwarded-For: 203.0.113.2\r\n")
        .await;
    assert!(denied.starts_with("HTTP/1.1 401"), "{denied}");
    let allowed = front
        .http(
            "GET",
            "/web/status",
            "",
            "X-Forwarded-For: 203.0.113.2\r\nX-Comandos-Token: fixture-token\r\n",
        )
        .await;
    assert!(allowed.starts_with("HTTP/1.1 200"), "{allowed}");
    front.stop().await;
}
