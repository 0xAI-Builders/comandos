use comandos_extensions::auth::import_credentials;
use serde_json::{Value, json};
use std::{
    fs::{self, File, FileTimes},
    os::unix::fs::PermissionsExt,
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
    time::{Duration, UNIX_EPOCH},
};
static NEXT: AtomicUsize = AtomicUsize::new(0);
struct Home(PathBuf);
impl Home {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "import-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
    fn write(&self, path: &str, value: &Value) -> PathBuf {
        let path = self.0.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, serde_json::to_vec(value).unwrap()).unwrap();
        path
    }
    fn credentials(&self) -> PathBuf {
        self.0.join(".config/comandos/extensions/credentials.json")
    }
    fn read(&self) -> Value {
        comandos_core::json::parse_slice(&fs::read(self.credentials()).unwrap()).unwrap()
    }
}
impl Drop for Home {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn catalog() -> Value {
    json!({"servers":{"demo":{"url":"https://example.test/mcp"},"linear":{"url":"https://linear.test/mcp"}}})
}
fn claude(token: &str, expiry: u64) -> Value {
    json!({"serverName":"demo","serverUrl":"https://example.test/mcp","accessToken":token,"refreshToken":"fixture-refresh","clientId":"fixture-client","expiresAt":expiry,"discoveryState":{"authorizationServerUrl":"https://issuer.test"}})
}

#[test]
fn imports_freshest_matching_mcp_only_and_preserves_existing_entries() {
    let home = Home::new();
    let primary = home.write(".claude/.credentials.json", &json!({
        "claudeAiOauth":{"accessToken":"model-only-sentinel"},
        "mcpOAuth":{"first":claude("older",100000),
            "alias":{"serverName":"linear-server","serverUrl":"https://linear.test/mcp","accessToken":"alias-token"},
            "wrong":{"serverName":"demo","serverUrl":"https://wrong.test","accessToken":"wrong-token","expiresAt":999999999}}
    }));
    let account = home.write(
        ".claude-accounts/second/.credentials.json",
        &json!({"mcpOAuth":{"second":claude("fresh",200000)}}),
    );
    home.write(
        ".config/comandos/extensions/credentials.json",
        &json!({"unrelated":{"keep":true}}),
    );
    let before = fs::read(&primary).unwrap();
    assert_eq!(
        import_credentials(&home.0, &catalog()).unwrap(),
        ["demo", "linear", "unrelated"]
    );
    let result = home.read();
    assert_eq!(result["demo"]["access_token"], "fresh");
    assert_eq!(result["demo"]["expires_at"], 200.0);
    assert_eq!(result["demo"]["source"], account.to_str().unwrap());
    assert_eq!(result["linear"]["access_token"], "alias-token");
    assert!(result["linear"]["refresh_token"].is_null());
    assert_eq!(result["unrelated"], json!({"keep":true}));
    assert!(!result.to_string().contains("model-only-sentinel"));
    assert!(!result.to_string().contains("wrong-token"));
    assert_eq!(fs::read(primary).unwrap(), before);
    assert_eq!(
        fs::metadata(home.credentials())
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
}

#[test]
fn matching_existing_endpoint_is_unchanged_even_if_source_is_fresher() {
    let home = Home::new();
    home.write(
        ".claude/.credentials.json",
        &json!({"mcpOAuth":{"first":claude("incoming",999999999)}}),
    );
    let path = home.write(".config/comandos/extensions/credentials.json", &json!({"demo":{"url":"https://example.test/mcp","access_token":"current","opaque":{"keep":true}}}));
    let before = fs::read(&path).unwrap();
    let modified = fs::metadata(&path).unwrap().modified().unwrap();
    assert_eq!(import_credentials(&home.0, &catalog()).unwrap(), ["demo"]);
    assert_eq!(fs::read(&path).unwrap(), before);
    assert_eq!(fs::metadata(path).unwrap().modified().unwrap(), modified);
}

#[test]
fn equal_expiry_preserves_original_record_discovery_order() {
    let home = Home::new();
    let path = home.write(".claude/.credentials.json", &json!({}));
    fs::write(
        path,
        format!(
            r#"{{"mcpOAuth":{{"z-first":{},"a-second":{}}}}}"#,
            claude("first", 100000),
            claude("second", 100000)
        ),
    )
    .unwrap();
    home.write(
        ".claude-accounts/third/.credentials.json",
        &json!({"mcpOAuth":{"third":claude("third",100000)}}),
    );
    import_credentials(&home.0, &catalog()).unwrap();
    assert_eq!(home.read()["demo"]["access_token"], "first");
}

#[test]
fn changed_endpoint_imports_a_new_match_without_crossing_endpoints() {
    let home = Home::new();
    home.write(
        ".claude/.credentials.json",
        &json!({"mcpOAuth":{"first":claude("new-endpoint",100000)}}),
    );
    home.write(
        ".config/comandos/extensions/credentials.json",
        &json!({"demo":{"url":"https://old.test/mcp","access_token":"old-endpoint"}}),
    );
    import_credentials(&home.0, &catalog()).unwrap();
    assert_eq!(home.read()["demo"]["access_token"], "new-endpoint");
}

#[test]
fn grok_timestamp_shapes_and_mcp_remote_cache_fields_are_imported() {
    for (received, expiry) in [
        (json!("2099-01-01T00:00:00Z"), 4070909400.),
        (json!({"secs_since_epoch":4070908800u64}), 4070909400.),
        (json!(4070908800u64), 4070909400.),
        (json!("2099-01-01T00:00:00.123456Z"), 4070909400.123456),
    ] {
        let home = Home::new();
        home.write(".grok/mcp_credentials.json", &json!({"linear-server:https://linear.test/mcp":{"token_received_at":received,"token_response":{"access_token":"grok-token","refresh_token":"grok-refresh","expires_in":600},"client_id":"grok-client","issuer":"https://grok-issuer.test"}}));
        let endpoint = "https://example.test/v1/mcp";
        let key = format!("{:x}", md5::compute(endpoint.as_bytes()));
        let token = home.write(&format!(".mcp-auth/pkg/{key}_tokens.json"), &json!({"access_token":"remote-token","refresh_token":"remote-refresh","expires_in":600}));
        File::options()
            .write(true)
            .open(&token)
            .unwrap()
            .set_times(FileTimes::new().set_modified(UNIX_EPOCH + Duration::from_secs_f64(1234.75)))
            .unwrap();
        home.write(&format!(".mcp-auth/pkg/{key}_client_info.json"), &json!({"client_id":"remote-client","client_secret":"fixture-secret","token_endpoint_auth_method":"client_secret_basic"}));
        let mut catalog = catalog();
        catalog["servers"]["demo"]["url"] = json!(endpoint);
        import_credentials(&home.0, &catalog).unwrap();
        let result = home.read();
        assert_eq!(result["linear"]["expires_at"].as_f64(), Some(expiry));
        assert_eq!(result["demo"]["expires_at"].as_f64(), Some(1834.75));
        assert_eq!(result["demo"]["client_secret"], "fixture-secret");
        assert_eq!(
            result["demo"]["token_endpoint_auth_method"],
            "client_secret_basic"
        );
        assert_eq!(result["demo"]["issuer"], "https://example.test");
        assert_eq!(result["demo"]["source"], token.to_str().unwrap());
    }
}

#[test]
fn malformed_source_does_not_overwrite_existing_credentials_or_print_contents() {
    let home = Home::new();
    let source = home.write(".claude/.credentials.json", &json!({}));
    fs::write(source, r#"{"mcpOAuth":{},"mcpOAuth":"private-sentinel"}"#).unwrap();
    let path = home.write(
        ".config/comandos/extensions/credentials.json",
        &json!({"keep":{"access_token":"saved"}}),
    );
    let before = fs::read(&path).unwrap();
    let error = import_credentials(&home.0, &catalog()).unwrap_err();
    assert!(!error.contains("private-sentinel"));
    assert_eq!(fs::read(path).unwrap(), before);
}

#[test]
fn import_waits_for_the_shared_credentials_lock_and_reloads_after_acquiring_it() {
    let home = Home::new();
    home.write(
        ".claude/.credentials.json",
        &json!({"mcpOAuth":{"first":claude("source",100000)}}),
    );
    let directory = home.0.join(".local/state/comandos/extensions");
    fs::create_dir_all(&directory).unwrap();
    let lock = File::create(directory.join("credentials.lock")).unwrap();
    lock.lock().unwrap();
    let (send, receive) = std::sync::mpsc::channel();
    std::thread::scope(|scope| {
        scope.spawn(|| {
            send.send(import_credentials(&home.0, &catalog())).unwrap();
        });
        assert!(receive.recv_timeout(Duration::from_millis(60)).is_err());
        home.write(
            ".config/comandos/extensions/credentials.json",
            &json!({"demo":{"url":"https://example.test/mcp","access_token":"rotated-by-holder"}}),
        );
        drop(lock);
        assert_eq!(
            receive
                .recv_timeout(Duration::from_secs(2))
                .unwrap()
                .unwrap(),
            ["demo"]
        );
    });
    assert_eq!(home.read()["demo"]["access_token"], "rotated-by-holder");
}
