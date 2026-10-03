//! Shared MCP OAuth credentials. Model credentials are never imported or logged.
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use url::Url;

mod import;
mod timestamp;
pub use import::import_credentials;

const MAX_CONFIG: u64 = 16 * 1024 * 1024;
static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

static ACTIVE_WORKERS: AtomicUsize = AtomicUsize::new(0);
static WORKER_FINISHED: tokio::sync::Notify = tokio::sync::Notify::const_new();

struct Worker;
impl Drop for Worker {
    fn drop(&mut self) {
        if ACTIVE_WORKERS.fetch_sub(1, Ordering::SeqCst) == 1 {
            WORKER_FINISHED.notify_waiters();
        }
    }
}
struct AbortQueued(tokio::task::AbortHandle);
impl Drop for AbortQueued {
    fn drop(&mut self) {
        self.0.abort();
    }
}

/// Call after request tasks stop spawning auth work. A running token rotation
/// must persist before process exit; dropping a request only cancels queued work.
pub async fn drain_workers() {
    loop {
        let notified = WORKER_FINISHED.notified();
        tokio::pin!(notified);
        notified.as_mut().enable();
        if ACTIVE_WORKERS.load(Ordering::SeqCst) == 0 {
            return;
        }
        notified.await;
    }
}

async fn blocking_transaction<F>(action: F) -> Result<Option<String>, String>
where
    F: FnOnce() -> Result<Option<String>, String> + Send + 'static,
{
    ACTIVE_WORKERS.fetch_add(1, Ordering::SeqCst);
    let worker = Worker;
    let task = tokio::task::spawn_blocking(move || {
        let _worker = worker;
        action()
    });
    let _abort = AbortQueued(task.abort_handle());
    task.await.map_err(|_| failure())?
}
fn failure() -> String {
    "Credential storage unavailable".into()
}
fn invalid() -> String {
    "Invalid configuration".into()
}
fn string<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
}
fn number(value: &Value, key: &str) -> f64 {
    value.get(key).and_then(Value::as_f64).unwrap_or(0.0)
}
fn now() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64()
}

/// JSONC configuration parser shared by credentials and the serve catalog.
pub fn parse_config_bytes(raw: &[u8]) -> Result<Value, String> {
    let clean = clean_config_bytes(raw)?;
    parse_clean_config_bytes(&clean)
}

pub(crate) fn clean_config_bytes(raw: &[u8]) -> Result<Vec<u8>, String> {
    if raw.len() as u64 > MAX_CONFIG {
        return Err(invalid());
    }
    std::str::from_utf8(raw).map_err(|_| invalid())?;
    let mut clean = raw.to_vec();
    let (mut i, mut quoted, mut escaped) = (0, false, false);
    while i < clean.len() {
        let b = clean[i];
        if quoted {
            if escaped {
                escaped = false;
            } else if b == b'\\' {
                escaped = true;
            } else if b == b'"' {
                quoted = false;
            }
            i += 1;
            continue;
        }
        if b == b'"' {
            quoted = true;
            i += 1;
            continue;
        }
        if b == b'/' && clean.get(i + 1) == Some(&b'/') {
            while i < clean.len() && clean[i] != b'\n' {
                clean[i] = b' ';
                i += 1;
            }
            continue;
        }
        if b == b'/' && clean.get(i + 1) == Some(&b'*') {
            clean[i] = b' ';
            clean[i + 1] = b' ';
            i += 2;
            let mut closed = false;
            while i < clean.len() {
                if clean[i] == b'*' && clean.get(i + 1) == Some(&b'/') {
                    clean[i] = b' ';
                    clean[i + 1] = b' ';
                    i += 2;
                    closed = true;
                    break;
                }
                clean[i] = b' ';
                i += 1;
            }
            if !closed {
                return Err(invalid());
            }
            continue;
        }
        i += 1;
    }
    quoted = false;
    escaped = false;
    for i in 0..clean.len() {
        let b = clean[i];
        if quoted {
            if escaped {
                escaped = false;
            } else if b == b'\\' {
                escaped = true;
            } else if b == b'"' {
                quoted = false;
            }
        } else if b == b'"' {
            quoted = true;
        } else if b == b',' {
            let mut j = i + 1;
            while j < clean.len() && clean[j].is_ascii_whitespace() {
                j += 1;
            }
            if matches!(clean.get(j), Some(b'}' | b']')) {
                clean[i] = b' ';
            }
        }
    }
    Ok(clean)
}

pub(crate) fn parse_clean_config_bytes(clean: &[u8]) -> Result<Value, String> {
    let text = std::str::from_utf8(clean).map_err(|_| invalid())?;
    let value = comandos_core::json::parse_unique_value(text).map_err(|_| invalid())?;
    if !value.is_object() {
        return Err(invalid());
    }
    Ok(value)
}

fn read_bytes(path: &Path) -> Result<Option<Vec<u8>>, String> {
    let file = match File::open(path) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(failure()),
    };
    let mut out = Vec::new();
    file.take(MAX_CONFIG + 1)
        .read_to_end(&mut out)
        .map_err(|_| failure())?;
    if out.len() as u64 > MAX_CONFIG {
        return Err(invalid());
    }
    Ok(Some(out))
}
fn read_config(path: &Path) -> Result<Value, String> {
    read_bytes(path)?.map_or_else(|| Ok(json!({})), |b| parse_config_bytes(&b))
}
fn private_dir(path: &Path) -> Result<(), String> {
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(path)
        .map_err(|_| failure())
}
fn private_write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let parent = path.parent().ok_or_else(failure)?;
    private_dir(parent)?;
    let (tmp, mut file) = loop {
        let tmp = parent.join(format!(
            ".comandos-{}-{}",
            std::process::id(),
            NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
        ));
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&tmp)
        {
            Ok(f) => break (tmp, f),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(_) => return Err(failure()),
        }
    };
    let result = (|| {
        file.write_all(bytes).map_err(|_| failure())?;
        file.sync_all().map_err(|_| failure())?;
        fs::rename(&tmp, path).map_err(|_| failure())?;
        File::open(parent)
            .and_then(|f| f.sync_all())
            .map_err(|_| failure())
    })();
    let _ = fs::remove_file(tmp);
    result
}
fn json_bytes(value: &Value) -> Result<Vec<u8>, String> {
    let mut b = serde_json::to_vec_pretty(value).map_err(|_| invalid())?;
    b.push(b'\n');
    Ok(b)
}

fn credential_lock(home: &Path) -> Result<File, String> {
    credential_lock_until(home, None)
}
fn credential_lock_until(home: &Path, deadline: Option<Instant>) -> Result<File, String> {
    let state = home.join(".local/state/comandos/extensions");
    private_dir(&state)?;
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .open(state.join("credentials.lock"))
        .map_err(|_| failure())?;
    // The same open-description lock guards both imports and live token rotation.
    let waiting = Instant::now();
    loop {
        if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
            return Err("Credential operation timed out".into());
        }
        match lock.try_lock() {
            Ok(()) => return Ok(lock),
            Err(fs::TryLockError::WouldBlock) if waiting.elapsed() < Duration::from_secs(20) => {
                std::thread::sleep(Duration::from_millis(25));
            }
            Err(_) => return Err("Credential lock unavailable".into()),
        }
    }
}

#[derive(Clone)]
pub struct Auth {
    home: PathBuf,
    name: String,
    endpoint: String,
}
impl Auth {
    pub fn new(home: PathBuf, name: String, endpoint: String) -> Result<Option<Self>, String> {
        let auth = Self {
            home,
            name,
            endpoint,
        };
        let data = read_config(&auth.credentials())?;
        match data.get(&auth.name) {
            None | Some(Value::Null) => Ok(None),
            Some(item) => {
                auth.check_endpoint(item)?;
                Ok(Some(auth))
            }
        }
    }
    fn credentials(&self) -> PathBuf {
        self.home
            .join(".config/comandos/extensions/credentials.json")
    }
    fn state(&self) -> PathBuf {
        self.home.join(".local/state/comandos/extensions")
    }
    fn check_endpoint(&self, item: &Value) -> Result<(), String> {
        if string(item, "url") != Some(&self.endpoint) {
            Err("Credential endpoint mismatch".into())
        } else {
            Ok(())
        }
    }
    pub fn same_origin(&self, address: &str) -> bool {
        match (Url::parse(&self.endpoint), Url::parse(address)) {
            (Ok(a), Ok(b)) => {
                a.scheme() == b.scheme()
                    && a.host_str().is_some()
                    && a.host_str() == b.host_str()
                    && a.port_or_known_default() == b.port_or_known_default()
            }
            _ => false,
        }
    }
    /// Checks share one deadline across lock wait, discovery and refresh. Once a
    /// replacement token is returned, the transaction must still persist it.
    pub async fn access_token_until(
        &self,
        rejected: Option<String>,
        deadline: Option<Instant>,
    ) -> Result<Option<String>, String> {
        let auth = self.clone();
        blocking_transaction(move || {
            auth.access_token_with_clock_until(
                rejected.as_deref(),
                now,
                |item| refresh_oauth_until(item, deadline),
                deadline,
            )
        })
        .await
    }
    pub async fn access_token(&self, rejected: Option<String>) -> Result<Option<String>, String> {
        self.access_token_until(rejected, None).await
    }
    /// Fixed clock convenience for deterministic rotation tests.
    pub fn access_token_with<F>(
        &self,
        rejected: Option<&str>,
        at: f64,
        refresh: F,
    ) -> Result<Option<String>, String>
    where
        F: FnOnce(&Value) -> Result<Value, String>,
    {
        self.access_token_with_clock(rejected, || at, refresh)
    }
    /// Injected refresh and clock keep rotation/concurrency tests offline.
    pub fn access_token_with_clock<F, C>(
        &self,
        rejected: Option<&str>,
        clock: C,
        refresh: F,
    ) -> Result<Option<String>, String>
    where
        F: FnOnce(&Value) -> Result<Value, String>,
        C: FnMut() -> f64,
    {
        self.access_token_with_clock_until(rejected, clock, refresh, None)
    }
    fn access_token_with_clock_until<F, C>(
        &self,
        rejected: Option<&str>,
        mut clock: C,
        refresh: F,
        deadline: Option<Instant>,
    ) -> Result<Option<String>, String>
    where
        F: FnOnce(&Value) -> Result<Value, String>,
        C: FnMut() -> f64,
    {
        let _lock = credential_lock_until(&self.home, deadline)?;
        let mut data = read_config(&self.credentials())?;
        let mut item = match data.get(&self.name) {
            None | Some(Value::Null) => return Ok(None),
            Some(item) => item.clone(),
        };
        self.check_endpoint(&item)?;
        if let Some(latest) = newer_source(&item)? {
            item = latest;
            data[&self.name] = item.clone();
            private_write(&self.credentials(), &json_bytes(&data)?)?;
        }
        let token = string(&item, "access_token").map(str::to_owned);
        let expiry = number(&item, "expires_at");
        // Lock contention and source reads can consume the expiry margin.
        let at = clock();
        if !at.is_finite() {
            return Err(failure());
        }
        let was_rejected = rejected.is_some_and(|r| !r.is_empty() && token.as_deref() == Some(r));
        if !was_rejected && (expiry == 0.0 || expiry > at + 60.0) {
            return Ok(token);
        }
        if string(&item, "refresh_token").is_none() {
            return if was_rejected {
                Err("Credential rejected".into())
            } else {
                Ok(token)
            };
        }
        let refresh_error = || "Refresh failed".to_string();
        let replacement = refresh(&item).map_err(|_| refresh_error())?;
        let access = string(&replacement, "access_token")
            .ok_or_else(refresh_error)?
            .to_owned();
        let seconds = match replacement.get("expires_in") {
            None => 3600.0,
            Some(v) => v.as_f64().ok_or_else(refresh_error)?,
        };
        // expires_in starts when refresh completes, including native timestamps.
        let at = clock();
        let expiry = at + seconds;
        if !expiry.is_finite() {
            return Err(refresh_error());
        }
        item["access_token"] = json!(access);
        item["expires_at"] = json!(expiry);
        if let Some(r) = string(&replacement, "refresh_token") {
            item["refresh_token"] = json!(r);
        }
        if let Some(e) = string(&replacement, "_token_endpoint") {
            item["token_endpoint"] = json!(e);
        }
        data[&self.name] = item.clone();
        private_write(&self.credentials(), &json_bytes(&data)?)?;
        sync_source(self, &item, token.as_deref(), at)?;
        Ok(Some(access))
    }
}

fn received_timestamp(v: &Value) -> f64 {
    if let Some(s) = v.as_str() {
        timestamp::parse(s).unwrap_or(0.0)
    } else if v.is_object() {
        let seconds = &v["secs_since_epoch"];
        seconds
            .as_bool()
            .map(|v| f64::from(u8::from(v)))
            .or_else(|| seconds.as_f64())
            .unwrap_or(0.0)
    } else {
        v.as_bool()
            .map(|v| f64::from(u8::from(v)))
            .or_else(|| v.as_f64())
            .unwrap_or(0.0)
    }
}
fn with_native(item: &Value, token: &Value, expiry: f64, camel: bool) -> Value {
    let mut out = item.clone();
    out["access_token"] = token[if camel { "accessToken" } else { "access_token" }].clone();
    out["refresh_token"] = token[if camel {
        "refreshToken"
    } else {
        "refresh_token"
    }]
    .clone();
    out["expires_at"] = json!(expiry);
    if camel && let Some(client) = token.get("clientId") {
        out["client_id"] = client.clone();
    }
    out
}
fn newer_source(item: &Value) -> Result<Option<Value>, String> {
    let Some(source) = string(item, "source") else {
        return Ok(None);
    };
    let path = Path::new(source);
    let data = read_config(path)?;
    if let Some(records) = data.get("mcpOAuth").and_then(Value::as_object) {
        for v in records.values() {
            let expiry = number(v, "expiresAt") / 1000.0;
            if v.get("serverUrl") == item.get("url")
                && string(v, "accessToken").is_some()
                && expiry > number(item, "expires_at")
            {
                return Ok(Some(with_native(item, v, expiry, true)));
            }
        }
    }
    let filename = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    if filename.ends_with("_tokens.json") && string(&data, "access_token").is_some() {
        let modified = fs::metadata(path)
            .and_then(|m| m.modified())
            .map_err(|_| failure())?
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs_f64();
        let expiry = modified + number(&data, "expires_in");
        if data.get("access_token") != item.get("access_token")
            && expiry > number(item, "expires_at")
        {
            return Ok(Some(with_native(item, &data, expiry, false)));
        }
    }
    if filename == "mcp_credentials.json" {
        let suffix = format!(":{}", string(item, "url").unwrap_or(""));
        for (k, v) in data.as_object().ok_or_else(invalid)? {
            let token = &v["token_response"];
            let expiry = received_timestamp(&v["token_received_at"]) + number(token, "expires_in");
            if k.ends_with(&suffix)
                && string(token, "access_token").is_some()
                && token.get("access_token") != item.get("access_token")
                && expiry > number(item, "expires_at")
            {
                return Ok(Some(with_native(item, token, expiry, false)));
            }
        }
    }
    Ok(None)
}
fn canonical(name: &str) -> &str {
    match name {
        "x_suite" => "x-suite",
        "linear-server" => "linear",
        "chrome-devtools" | "chrome-devtools-current" | "chrome-current" => "chrome-bg",
        _ => name,
    }
}
fn sync_source(auth: &Auth, item: &Value, previous: Option<&str>, at: f64) -> Result<(), String> {
    let Some(source) = string(item, "source") else {
        return Ok(());
    };
    let path = Path::new(source);
    let Some(before) = read_bytes(path)? else {
        return Ok(());
    };
    let mut data = parse_config_bytes(&before)?;
    let mut changed = false;
    if let Some(records) = data.get_mut("mcpOAuth").and_then(Value::as_object_mut) {
        for v in records.values_mut() {
            if string(v, "serverUrl") != string(item, "url")
                || canonical(string(v, "serverName").unwrap_or("")) != auth.name
            {
                continue;
            }
            if previous.is_some() && string(v, "accessToken") != previous {
                continue;
            }
            if previous.is_none() && number(v, "expiresAt") / 1000.0 >= number(item, "expires_at") {
                continue;
            }
            v["accessToken"] = item["access_token"].clone();
            v["expiresAt"] = json!((number(item, "expires_at") * 1000.0) as i64);
            if string(item, "refresh_token").is_some() {
                v["refreshToken"] = item["refresh_token"].clone();
            }
            changed = true;
        }
    }
    let filename = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    let expires = ((number(item, "expires_at") - at).max(0.0)) as i64;
    let update = |v: &mut Value| {
        v["access_token"] = item["access_token"].clone();
        v["refresh_token"] = item["refresh_token"].clone();
        v["expires_in"] = json!(expires);
    };
    if filename == "mcp_credentials.json"
        && let Some(v) = data.get_mut(format!(
            "{}:{}",
            auth.name,
            string(item, "url").unwrap_or("")
        ))
        && v.is_object()
        && v["token_response"].is_object()
        && (previous.is_none() || string(&v["token_response"], "access_token") == previous)
    {
        update(&mut v["token_response"]);
        v["token_received_at"] = json!(at as i64);
        changed = true;
    }
    if filename.ends_with("_tokens.json")
        && (previous.is_none() || string(&data, "access_token") == previous)
    {
        update(&mut data);
        changed = true;
    }
    if changed {
        if fs::symlink_metadata(path)
            .map_err(|_| failure())?
            .file_type()
            .is_symlink()
        {
            return Err("Configuration symlink requires explicit target".into());
        }
        if read_bytes(path)?.as_deref() != Some(&before) {
            return Err("Configuration changed concurrently".into());
        }
        let relative = path.strip_prefix(&auth.home).map_err(|_| failure())?;
        let name = relative.to_string_lossy().replace('/', "__");
        let digest = format!("{:x}", Sha256::digest(&before));
        let backup = auth
            .state()
            .join("backups")
            .join(format!("{name}.{}", &digest[..16]));
        if !backup.exists() {
            private_write(&backup, &before)?;
        }
        if read_bytes(path)?.as_deref() != Some(&before) {
            return Err("Configuration changed during backup".into());
        }
        private_write(path, &json_bytes(&data)?)?;
    }
    Ok(())
}
fn https(address: &str) -> Result<Url, String> {
    let url = Url::parse(address).map_err(|_| failure())?;
    if url.scheme() != "https" || url.host_str().is_none() {
        return Err(failure());
    }
    Ok(url)
}
fn response_json(response: reqwest::blocking::Response) -> Result<Value, String> {
    if response.status() != reqwest::StatusCode::OK {
        return Err("OAuth request rejected".into());
    }
    let mut bytes = Vec::new();
    response
        .take(MAX_CONFIG + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| failure())?;
    parse_config_bytes(&bytes)
}
fn remaining(deadline: Instant) -> Result<Duration, String> {
    deadline
        .checked_duration_since(Instant::now())
        .filter(|d| !d.is_zero())
        .map(|d| d.min(Duration::from_secs(20)))
        .ok_or_else(|| "Credential operation timed out".into())
}
fn refresh_oauth_until(item: &Value, deadline: Option<Instant>) -> Result<Value, String> {
    let issuer = string(item, "issuer").ok_or_else(failure)?;
    let url = https(issuer)?;
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(20))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|_| failure())?;
    let mut endpoint = string(item, "token_endpoint").map(str::to_owned);
    if endpoint.is_none() {
        let base = &url[..url::Position::BeforePath];
        let candidates = [
            format!(
                "{base}/.well-known/oauth-authorization-server{}",
                url.path().trim_end_matches('/')
            ),
            format!(
                "{}/.well-known/openid-configuration",
                issuer.trim_end_matches('/')
            ),
            format!("{base}/.well-known/oauth-authorization-server"),
        ];
        for (i, address) in candidates.iter().enumerate() {
            if candidates[..i].contains(address) {
                continue;
            }
            let mut request = client.get(address);
            if let Some(deadline) = deadline {
                request = request.timeout(remaining(deadline)?);
            }
            let response = request.send().map_err(|_| failure())?;
            if response.status() == reqwest::StatusCode::OK {
                let data = response_json(response)?;
                if let Some(e) = string(&data, "token_endpoint") {
                    endpoint = Some(e.to_owned());
                    break;
                }
            }
        }
    }
    let endpoint = endpoint.ok_or_else(failure)?;
    https(&endpoint)?;
    let mut form = vec![
        ("grant_type", "refresh_token"),
        (
            "refresh_token",
            string(item, "refresh_token").ok_or_else(failure)?,
        ),
    ];
    if let Some(id) = string(item, "client_id") {
        form.push(("client_id", id));
    }
    let mut request = client.post(&endpoint);
    if let Some(deadline) = deadline {
        request = request.timeout(remaining(deadline)?);
    }
    if let Some(secret) = string(item, "client_secret") {
        if string(item, "token_endpoint_auth_method") == Some("client_secret_basic") {
            request =
                request.basic_auth(string(item, "client_id").ok_or_else(failure)?, Some(secret));
        } else {
            form.push(("client_secret", secret));
        }
    }
    let mut result = response_json(request.form(&form).send().map_err(|_| failure())?)?;
    if string(&result, "access_token").is_none() {
        return Err(failure());
    }
    result["_token_endpoint"] = json!(endpoint);
    Ok(result)
}

#[cfg(test)]
mod cancellation_tests {
    use super::*;
    #[tokio::test(flavor = "current_thread")]
    async fn scoped_deadline_bounds_lock_wait_and_preserves_existing_rotation() {
        let home = std::env::temp_dir().join(format!(
            "auth-deadline-{}-{}",
            std::process::id(),
            NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
        ));
        private_write(
            &home.join(".config/comandos/extensions/credentials.json"),
            br#"{"demo":{"url":"https://example.test","access_token":"saved"}}"#,
        )
        .unwrap();
        let auth = Auth::new(home.clone(), "demo".into(), "https://example.test".into())
            .unwrap()
            .unwrap();
        let held = credential_lock(&home).unwrap();
        let task = tokio::spawn(async move {
            auth.access_token_until(None, Some(Instant::now() + Duration::from_millis(60)))
                .await
        });
        tokio::time::sleep(Duration::from_millis(200)).await;
        let ended = task.is_finished();
        drop(held);
        let result = task.await.unwrap();
        std::fs::remove_dir_all(home).unwrap();
        assert!(ended, "check auth worker outlived its scoped lock deadline");
        assert!(result.is_err());
    }
    #[test]
    fn cancelled_queued_refresh_never_starts_and_drain_finishes() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .max_blocking_threads(1)
            .build()
            .unwrap();
        runtime.block_on(async {
            let (started, ready) = tokio::sync::oneshot::channel();
            let (release, continue_work) = tokio::sync::oneshot::channel();
            let first = tokio::spawn(blocking_transaction(move || {
                started.send(()).unwrap();
                continue_work.blocking_recv().unwrap();
                Ok(None)
            }));
            ready.await.unwrap();
            let invoked = std::sync::Arc::new(AtomicUsize::new(0));
            let calls = invoked.clone();
            let queued = tokio::spawn(blocking_transaction(move || {
                calls.fetch_add(1, Ordering::SeqCst);
                Ok(None)
            }));
            tokio::task::yield_now().await;
            queued.abort();
            assert!(queued.await.unwrap_err().is_cancelled());
            release.send(()).unwrap();
            first.await.unwrap().unwrap();
            tokio::time::timeout(Duration::from_secs(2), drain_workers())
                .await
                .unwrap();
            assert_eq!(invoked.load(Ordering::SeqCst), 0);
        });
    }
    #[tokio::test(flavor = "current_thread")]
    async fn cancelled_request_drains_rotated_token_before_shutdown() {
        let home = std::env::temp_dir().join(format!(
            "auth-drain-{}-{}",
            std::process::id(),
            NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
        ));
        let auth = Auth {
            home: home.clone(),
            name: "demo".into(),
            endpoint: "https://example.test/mcp".into(),
        };
        private_write(&auth.credentials(),&json_bytes(&json!({"demo":{"url":auth.endpoint,"access_token":"old","refresh_token":"refresh","expires_at":1}})).unwrap()).unwrap();
        let (started, ready) = tokio::sync::oneshot::channel();
        let (release, continue_work) = tokio::sync::oneshot::channel();
        let path = auth.credentials();
        let request = tokio::spawn(async move {
            blocking_transaction(move || {
                auth.access_token_with(None, 1000., |_| {
                    started.send(()).unwrap();
                    continue_work.blocking_recv().unwrap();
                    Ok(json!({"access_token":"rotated","refresh_token":"replacement"}))
                })
            })
            .await
        });
        ready.await.unwrap();
        request.abort();
        assert!(request.await.unwrap_err().is_cancelled());
        let draining = tokio::spawn(drain_workers());
        tokio::time::sleep(Duration::from_millis(20)).await;
        // Always release worker even when assertion fails, to keep the test finite.
        let completed_early = draining.is_finished();
        release.send(()).unwrap();
        draining.await.unwrap();
        assert!(
            !completed_early,
            "shutdown returned while refresh/save still running"
        );
        let saved = read_config(&path).unwrap();
        assert_eq!(saved["demo"]["access_token"], "rotated");
        assert_eq!(saved["demo"]["refresh_token"], "replacement");
        fs::remove_dir_all(home).unwrap();
    }
}
