use comandos_extensions::auth::Auth;
use serde_json::{Value, json};
use std::fs;
use std::path::PathBuf;
use std::sync::{
    Arc, Barrier,
    atomic::{AtomicUsize, Ordering},
};

static NEXT: AtomicUsize = AtomicUsize::new(0);
struct Home(PathBuf);
impl Home {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!(
            "auth-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&p).unwrap();
        Self(p)
    }
    fn write(&self, relative: &str, value: &Value) -> PathBuf {
        let p = self.0.join(relative);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(&p, serde_json::to_vec(value).unwrap()).unwrap();
        p
    }
    fn credentials(&self, item: Value) -> PathBuf {
        self.write(
            ".config/comandos/extensions/credentials.json",
            &json!({"demo":item,"unrelated":{"keep":"yes"}}),
        )
    }
    fn auth(&self) -> Auth {
        Auth::new(
            self.0.clone(),
            "demo".into(),
            "https://example.test/mcp".into(),
        )
        .unwrap()
        .unwrap()
    }
    fn read(&self) -> Value {
        serde_json::from_slice(
            &fs::read(self.0.join(".config/comandos/extensions/credentials.json")).unwrap(),
        )
        .unwrap()
    }
}
impl Drop for Home {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn old() -> Value {
    json!({"url":"https://example.test/mcp","access_token":"old","refresh_token":"refresh","expires_at":1,"opaque":{"keep":true}})
}
fn never(_: &Value) -> Result<Value, String> {
    panic!("refresh was not expected")
}

#[test]
fn live_clock_checks_margin_after_waiting_for_credential_lock() {
    let home = Home::new();
    let mut item = old();
    item["expires_at"] = json!(1070.);
    home.credentials(item);
    let auth = home.auth();
    let path = home
        .0
        .join(".local/state/comandos/extensions/credentials.lock");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let lock = fs::File::create(&path).unwrap();
    lock.lock().unwrap();
    let clock = AtomicUsize::new(1000);
    std::thread::scope(|scope| {
        let worker = scope.spawn(|| {
            auth.access_token_with_clock(
                None,
                || clock.load(Ordering::SeqCst) as f64,
                |_| Ok(json!({"access_token":"new","expires_in":3600})),
            )
        });
        // Observe the worker's separate open description while our lock is held.
        // Advancing fixture time here deterministically models a 15-second wait,
        // without sleeping for expiry or racing the worker's initial clock read.
        let started = std::time::Instant::now();
        let waiting = loop {
            let opened = fs::read_dir("/proc/self/fd")
                .unwrap()
                .filter_map(Result::ok)
                .filter(|entry| fs::read_link(entry.path()).ok().as_ref() == Some(&path))
                .count();
            if opened >= 2 {
                break true;
            }
            if started.elapsed() > std::time::Duration::from_secs(2) {
                break false;
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        };
        clock.store(1015, Ordering::SeqCst);
        lock.unlock().unwrap();
        let token = worker.join().unwrap().unwrap();
        assert!(
            waiting,
            "auth worker did not reach the held credential lock"
        );
        assert_eq!(token.as_deref(), Some("new"));
    });
    assert_eq!(home.read()["demo"]["expires_at"], 4615.);
}

#[test]
fn live_clock_stamps_expiry_and_native_source_after_delayed_refresh() {
    let home = Home::new();
    let source = home.write(
        ".grok/mcp_credentials.json",
        &json!({
            "demo:https://example.test/mcp": {"token_received_at":0,
                "token_response":{"access_token":"old","refresh_token":"refresh","expires_in":1}}
        }),
    );
    let mut item = old();
    item["source"] = json!(source);
    home.credentials(item);
    let auth = home.auth();
    let clock = AtomicUsize::new(1000);
    assert_eq!(
        auth.access_token_with_clock(
            None,
            || clock.load(Ordering::SeqCst) as f64,
            |_| {
                clock.store(1010, Ordering::SeqCst);
                Ok(json!({"access_token":"new","expires_in":65}))
            }
        )
        .unwrap()
        .as_deref(),
        Some("new")
    );
    assert_eq!(home.read()["demo"]["expires_at"], 1075.);
    let native: Value = serde_json::from_slice(&fs::read(&source).unwrap()).unwrap();
    assert_eq!(
        native["demo:https://example.test/mcp"]["token_received_at"],
        1010
    );
    assert_eq!(
        native["demo:https://example.test/mcp"]["token_response"]["expires_in"],
        65
    );
    assert_eq!(
        auth.access_token_with_clock(None, || clock.load(Ordering::SeqCst) as f64, never)
            .unwrap()
            .as_deref(),
        Some("new")
    );
}

#[test]
fn missing_store_is_absent_and_endpoint_mismatch_never_leaks() {
    let home = Home::new();
    assert!(
        Auth::new(
            home.0.clone(),
            "demo".into(),
            "https://example.test/mcp".into()
        )
        .unwrap()
        .is_none()
    );
    home.credentials(old());
    let other = Auth::new(
        home.0.clone(),
        "demo".into(),
        "https://other.test/mcp".into(),
    );
    let error = match other {
        Err(e) => e,
        Ok(Some(auth)) => auth.access_token_with(None, 1000., never).unwrap_err(),
        Ok(None) => panic!("endpoint mismatch hidden"),
    };
    assert!(!error.contains("refresh") && !error.contains("https://"));
}

#[test]
fn rotation_is_once_across_concurrent_connections_and_preserves_unknown_fields() {
    let home = Home::new();
    home.credentials(old());
    let auth = home.auth();
    let barrier = Arc::new(Barrier::new(8));
    let calls = Arc::new(AtomicUsize::new(0));
    std::thread::scope(|scope| {
        let mut handles = vec![];
        for _ in 0..8 {
            let (auth, barrier, calls) = (auth.clone(), barrier.clone(), calls.clone());
            handles.push(scope.spawn(move || {barrier.wait();auth.access_token_with(None,1000.,|_|{
                calls.fetch_add(1,Ordering::SeqCst); std::thread::sleep(std::time::Duration::from_millis(10));
                Ok(json!({"access_token":"new","refresh_token":"rotated","expires_in":3600,"_token_endpoint":"https://example.test/token"}))
            }).unwrap()}));
        }
        for handle in handles {
            assert_eq!(handle.join().unwrap().as_deref(), Some("new"));
        }
    });
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let data = home.read();
    assert_eq!(data["demo"]["refresh_token"], "rotated");
    assert_eq!(data["demo"]["opaque"]["keep"], true);
    assert_eq!(data["demo"]["expires_at"], 4600.);
    assert_eq!(data["unrelated"]["keep"], "yes");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(home.0.join(".config/comandos/extensions/credentials.json"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }
}

#[test]
fn rejected_token_refreshes_once_and_old_401_reuses_rotated_token() {
    let home = Home::new();
    home.credentials(json!({"url":"https://example.test/mcp","access_token":"old","refresh_token":"refresh","expires_at":99999}));
    let auth = home.auth();
    assert_eq!(
        auth.access_token_with(Some("old"), 1000., |_| Ok(json!({"access_token":"new"})))
            .unwrap()
            .as_deref(),
        Some("new")
    );
    assert_eq!(
        auth.access_token_with(Some("old"), 1000., never)
            .unwrap()
            .as_deref(),
        Some("new")
    );
    assert_eq!(home.read()["demo"]["refresh_token"], "refresh");
}

#[test]
fn refresh_failure_preserves_original_and_suppresses_upstream_secrets() {
    let home = Home::new();
    let p = home.credentials(old());
    let before = fs::read(&p).unwrap();
    let err = home
        .auth()
        .access_token_with(None, 1000., |_| Err("secret response token=PRIVATE".into()))
        .unwrap_err();
    assert!(!err.contains("PRIVATE") && !err.contains("secret response"));
    assert_eq!(fs::read(&p).unwrap(), before);
    let err = home
        .auth()
        .access_token_with(None, 1000., |_| Ok(json!({"access_token":""})))
        .unwrap_err();
    assert!(err.contains("Refresh failed"));
}

#[test]
fn stale_expiry_without_refresh_is_tried_but_rejected_token_is_not_replayed() {
    let home = Home::new();
    home.credentials(json!({"url":"https://example.test/mcp","access_token":"old","expires_at":1}));
    let auth = home.auth();
    assert_eq!(
        auth.access_token_with(None, 1000., never)
            .unwrap()
            .as_deref(),
        Some("old")
    );
    assert!(auth.access_token_with(Some("old"), 1000., never).is_err());
    assert!(auth.same_origin("https://example.test/messages?session=fixture"));
    assert!(!auth.same_origin("https://foreign.test/messages"));
    assert!(!auth.same_origin("http://example.test/messages"));
    assert!(!auth.same_origin("https://example.test:444/messages"));
}

#[test]
fn native_refresh_is_reused_then_rotation_updates_only_exact_mcp_record() {
    let home = Home::new();
    let source=home.write(".claude/.credentials.json",&json!({"claudeAiOauth":{"accessToken":"MODEL-PRIVATE"},"mcpOAuth":{"demo|hash":{"serverName":"demo","serverUrl":"https://example.test/mcp","accessToken":"native","refreshToken":"native-refresh","expiresAt":99999000}}}));
    let mut item = old();
    item["source"] = json!(source);
    home.credentials(item);
    let auth = home.auth();
    assert_eq!(
        auth.access_token_with(None, 1000., never)
            .unwrap()
            .as_deref(),
        Some("native")
    );
    assert_eq!(
        auth.access_token_with(Some("native"), 1000., |_| Ok(
            json!({"access_token":"next","refresh_token":"next-refresh","expires_in":3600})
        ))
        .unwrap()
        .as_deref(),
        Some("next")
    );
    let data: Value = serde_json::from_slice(&fs::read(&source).unwrap()).unwrap();
    assert_eq!(data["claudeAiOauth"]["accessToken"], "MODEL-PRIVATE");
    assert_eq!(data["mcpOAuth"]["demo|hash"]["accessToken"], "next");
    assert!(
        !fs::read_to_string(home.0.join(".config/comandos/extensions/credentials.json"))
            .unwrap()
            .contains("MODEL-PRIVATE")
    );
    assert!(
        home.0
            .join(".local/state/comandos/extensions/backups")
            .read_dir()
            .unwrap()
            .next()
            .is_some()
    );
}

#[test]
fn refresh_does_not_overwrite_a_native_token_changed_during_network_request() {
    let home = Home::new();
    let source=home.write(".claude/.credentials.json",&json!({"mcpOAuth":{"demo|hash":{"serverName":"demo","serverUrl":"https://example.test/mcp","accessToken":"old","expiresAt":1000}}}));
    let mut item = old();
    item["source"] = json!(source);
    home.credentials(item);
    home.auth().access_token_with(None,1000.,|_|{fs::write(&source,serde_json::to_vec(&json!({"mcpOAuth":{"demo|hash":{"serverName":"demo","serverUrl":"https://example.test/mcp","accessToken":"native-concurrent","expiresAt":90000000}}})).unwrap()).unwrap();Ok(json!({"access_token":"shared-new"}))}).unwrap();
    let data: Value = serde_json::from_slice(&fs::read(source).unwrap()).unwrap();
    assert_eq!(
        data["mcpOAuth"]["demo|hash"]["accessToken"],
        "native-concurrent"
    );
}

#[test]
fn grok_native_timestamp_formats_are_read_without_refresh() {
    for stamp in [
        json!("2099-01-01T00:00:00Z"),
        json!({"secs_since_epoch":4070908800_u64}),
        json!(4070908800_u64),
    ] {
        let home = Home::new();
        let source=home.write(".grok/mcp_credentials.json",&json!({"demo:https://example.test/mcp":{"token_received_at":stamp,"token_response":{"access_token":"new","refresh_token":"new-refresh","expires_in":3600}}}));
        let mut item = old();
        item["source"] = json!(source);
        home.credentials(item);
        assert_eq!(
            home.auth()
                .access_token_with(None, 1000., never)
                .unwrap()
                .as_deref(),
            Some("new")
        );
    }
}

#[test]
fn jsonc_preserves_quoted_urls_and_rejects_duplicate_keys_at_any_depth() {
    use comandos_extensions::auth::parse_config_bytes;
    let data=parse_config_bytes(br#"{/*fixture*/ "url":"https://test/path//?a=/*literal*/", "nested": {"tools":["a",],}, // trailing comment
    }"#).unwrap();
    assert_eq!(data["url"], "https://test/path//?a=/*literal*/");
    assert_eq!(data["nested"]["tools"], json!(["a"]));
    for raw in [
        r#"{"PRIVATE":1,"PRIVATE":2}"#,
        r#"{"nested":[{"secret":1,"secret":2}]}"#,
        r#"{"x":1} PRIVATE"#,
        r#"["PRIVATE"]"#,
        r#"{/*PRIVATE"#,
    ] {
        let error = parse_config_bytes(raw.as_bytes()).unwrap_err();
        assert_eq!(error, "Invalid configuration");
    }
    assert!(parse_config_bytes(&[0xff]).is_err());
}

#[test]
fn native_source_symlink_is_rejected_before_write_and_original_is_preserved() {
    use std::os::unix::fs::symlink;
    let home = Home::new();
    let source=home.write(".claude/real.json",&json!({"mcpOAuth":{"demo":{"serverName":"demo","serverUrl":"https://example.test/mcp","accessToken":"old","expiresAt":1000}}}));
    let link = source.with_file_name(".credentials.json");
    symlink(&source, &link).unwrap();
    let before = fs::read(&source).unwrap();
    let mut item = old();
    item["source"] = json!(link);
    home.credentials(item);
    let error = home
        .auth()
        .access_token_with(None, 1000., |_| Ok(json!({"access_token":"new"})))
        .unwrap_err();
    assert!(error.contains("symlink"));
    assert_eq!(fs::read(&source).unwrap(), before);
    // The rotated shared refresh result is retained even if native synchronization fails.
    assert_eq!(home.read()["demo"]["access_token"], "new");
}

#[test]
fn legacy_native_token_cache_updates_only_matching_previous_token() {
    let home = Home::new();
    let source = home.write(
        ".mcp-auth/v1/fixture_tokens.json",
        &json!({"access_token":"old","refresh_token":"refresh","expires_in":0,"scope":"keep"}),
    );
    let mut item = old();
    item["source"] = json!(source);
    home.credentials(item);
    home.auth()
        .access_token_with(None, 1000., |_| {
            Ok(json!({"access_token":"new","expires_in":300}))
        })
        .unwrap();
    let data: Value = serde_json::from_slice(&fs::read(source).unwrap()).unwrap();
    assert_eq!(data["access_token"], "new");
    assert_eq!(data["scope"], "keep");
    assert_eq!(data["expires_in"], 300);
}

#[test]
fn malformed_native_store_is_not_overwritten_or_exposed() {
    let home = Home::new();
    let source = home.write(".claude/.credentials.json", &json!({}));
    fs::write(&source, b"PRIVATE malformed").unwrap();
    let mut item = old();
    item["source"] = json!(source);
    let shared = home.credentials(item);
    let before = fs::read(&shared).unwrap();
    assert_eq!(
        home.auth()
            .access_token_with(None, 1000., never)
            .unwrap_err(),
        "Invalid configuration"
    );
    assert_eq!(fs::read(&shared).unwrap(), before);
    assert_eq!(fs::read(&source).unwrap(), b"PRIVATE malformed");
}

#[tokio::test(flavor = "current_thread")]
async fn oauth_refresh_rejects_insecure_issuer_without_network_or_secret_output() {
    let home = Home::new();
    let mut item = old();
    item["issuer"] = json!("http://localhost:1/PRIVATE");
    home.credentials(item);
    let error = home.auth().access_token(None).await.unwrap_err();
    assert_eq!(error, "Refresh failed");
}

#[test]
fn jsonc_retains_opaque_number_fields_and_arbitrary_integer_values() {
    use comandos_extensions::auth::parse_config_bytes;
    let raw=br#"{/* config */ "large":340282366920938463463374607431768211456,"opaque":{"$serde_json::private::Number":"opaque-key","$serde_json::private::RawValue":"[1]"},"float":1.0,}"#;
    let data = parse_config_bytes(raw).unwrap();
    assert_eq!(
        data["large"].to_string(),
        "340282366920938463463374607431768211456"
    );
    assert_eq!(data["opaque"]["$serde_json::private::Number"], "opaque-key");
    assert_eq!(data["opaque"]["$serde_json::private::RawValue"], "[1]");
    assert_eq!(data["float"].to_string(), "1.0");
}
