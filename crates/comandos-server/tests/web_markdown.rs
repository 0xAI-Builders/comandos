use comandos_server::dash::web::markdown::{Profile, render};
#[test]
fn markdown_matches_live_reader_safety_and_presentation() {
    let h = render(
        "hola <script>alert(1)</script> [x](javascript:alert(1)) https://example.org",
        Profile::News,
    );
    assert!(
        !h.contains("<script") && !h.contains("href=\"javascript:"),
        "{h}"
    );
    assert!(
        h.contains("&lt;script&gt;") && h.contains("href=\"https://example.org\""),
        "{h}"
    );
    assert!(
        h.contains("target=\"_blank\"") && h.contains("rel=\"noopener noreferrer nofollow\""),
        "{h}"
    );
    assert!(
        render("[google.com](https://evil.example/login)", Profile::News)
            .contains("class=\"nr-host\"")
    );
    assert!(!render("[example.com](https://example.com/x)", Profile::News).contains("nr-host"));
    assert!(!render("![a](https://example.org/a.png)", Profile::News).contains("<img"));
    assert!(!render("user@example.org", Profile::News).contains("<a "));
    for start in [0, 1, 2, 3] {
        let list = render(&format!("{start}. item"), Profile::News);
        assert!(list.starts_with("<ol>"), "{list}");
        assert!(!list.contains("start="), "{list}");
    }
    for quote in ['«', '»', '“', '”', '‘', '’', '…'] {
        let linked = render(&format!("https://example.org/path{quote}"), Profile::News);
        assert!(
            linked.contains("href=\"https://example.org/path\""),
            "{linked}"
        );
        assert!(linked.contains(&format!("</a>{quote}")), "{linked}");
    }
    let table = render("| a | b |\n|---|---|\n| 1 | 2 |", Profile::News);
    assert!(
        table.contains("<div class=\"nr-table-wrap\"><table>"),
        "{table}"
    );
}

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
        Self::start_readonly(true).await
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
        std::fs::write(root.join("token"), b"fixture-token").unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let config = Config {
            token: b"fixture-token".to_vec(),
            token_file: Some(root.join("token")),
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

#[tokio::test]
async fn renderer_is_authenticated_bounded_and_pure_even_in_shadow_without_native() {
    let front = Front::start().await;
    let request = r#"{"text":"**hola** https://example.org","profile":"news"}"#;
    let token = "X-Comandos-Token: fixture-token\r\n";
    assert!(
        front
            .http("POST", "/web/markdown", request, "")
            .await
            .starts_with("HTTP/1.1 401")
    );
    assert!(
        front
            .http("POST", "/web/markdown?token=wrong", request, "")
            .await
            .starts_with("HTTP/1.1 401")
    );
    let valid = front.http("POST", "/web/markdown", request, token).await;
    assert!(valid.starts_with("HTTP/1.1 200"), "{valid}");
    assert!(
        valid.contains("<strong>hola</strong>") && !valid.contains("\"shadow\""),
        "{valid}"
    );
    assert!(
        front
            .http(
                "POST",
                "/web/markdown",
                request,
                &format!("{token}Origin: https://evil.example\r\n")
            )
            .await
            .starts_with("HTTP/1.1 403")
    );
    for body in [
        r#"{"profile":"other","text":"hi"}"#,
        r#"{"profile":"news","text":1}"#,
        r#"{"text":"hi"}"#,
        "{invalid",
    ] {
        assert!(
            front
                .http("POST", "/web/markdown", body, token)
                .await
                .starts_with("HTTP/1.1 400"),
            "{body}"
        );
    }
    let too_long =
        serde_json::json!({"profile":"news", "text":"x".repeat(256 * 1024 + 1)}).to_string();
    let over = front.http("POST", "/web/markdown", &too_long, token).await;
    assert!(
        over.starts_with("HTTP/1.1 413") && over.contains("texto demasiado largo"),
        "{over}"
    );
    // The transport rejects declared length before waiting for any body bytes.
    let mut socket = TcpStream::connect(front.address).await.unwrap();
    socket.write_all(format!("POST /web/markdown HTTP/1.1\r\nHost: {}\r\nContent-Length: 262145\r\n{token}Connection: close\r\n\r\n", front.address).as_bytes()).await.unwrap();
    let mut raw = Vec::new();
    tokio::time::timeout(Duration::from_secs(2), socket.read_to_end(&mut raw))
        .await
        .unwrap()
        .unwrap();
    assert!(String::from_utf8(raw).unwrap().starts_with("HTTP/1.1 413"));
    for literal in [
        r"high\ud800end",
        r"low\udc00end",
        r"marker\ue000\udb80\udc00end",
    ] {
        let body = format!(r#"{{"text":"{}","profile":"news"}}"#, literal);
        let reply = front.http("POST", "/web/markdown", &body, token).await;
        assert!(reply.starts_with("HTTP/1.1 200"), "{reply}");
        let response = reply.split_once("\r\n\r\n").unwrap().1;
        let expected = comandos_web_view::utf16::json_to_unicode(&body);
        let expected: serde_json::Value = serde_json::from_str(&expected).unwrap();
        let actual = comandos_web_view::utf16::json_to_unicode(response);
        let actual: serde_json::Value = serde_json::from_str(&actual).unwrap();
        assert_eq!(
            actual["html"].as_str().unwrap().trim(),
            format!("<p>{}</p>", expected["text"].as_str().unwrap())
        );
    }
    // The exemption cannot enable an unrelated write under the read-only shadow policy.
    let write = front
        .http(
            "POST",
            "/news/notes",
            r#"{"action":"add","text":"never stored"}"#,
            token,
        )
        .await;
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(write.split_once("\r\n\r\n").unwrap().1).unwrap()
            ["shadow"],
        true
    );
    assert!(!front.root.join(".claude/comandos.db").exists());
    assert!(!front.root.join(".claude/news").exists());
    std::fs::write(front.root.join("token"), b"rotated-token").unwrap();
    assert!(
        front
            .http("POST", "/web/markdown", request, token)
            .await
            .starts_with("HTTP/1.1 401")
    );
    let rotated = front
        .http(
            "POST",
            "/web/markdown",
            request,
            "X-Comandos-Token: rotated-token\r\n",
        )
        .await;
    assert!(rotated.starts_with("HTTP/1.1 200"), "{rotated}");
    front.stop().await;
}
