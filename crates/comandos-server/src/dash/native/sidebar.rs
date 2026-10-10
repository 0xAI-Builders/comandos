//! Bounded session explorer and account-wide OpenRouter credits.
use super::{Answer, Entry, Key, Native, NativeRoute, Verb, reply};
use crate::Request;
use http::{Method, StatusCode};
use serde_json::{Value, json};
use std::{
    fs,
    io::{Read, Write},
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::{Component, Path, PathBuf},
    sync::{Arc, OnceLock},
    time::{Duration, Instant},
};

pub const ROUTES: &[Entry] = &[
    Entry {
        verb: Verb::Post,
        key: Key::Path("/fs/explorer"),
        route: NativeRoute::Sidebar,
    },
    Entry {
        verb: Verb::Get,
        key: Key::Path("/credits/openrouter"),
        route: NativeRoute::Sidebar,
    },
    Entry {
        verb: Verb::Post,
        key: Key::Path("/credits/openrouter"),
        route: NativeRoute::Sidebar,
    },
];
#[derive(Default)]
pub struct CreditsCache(pub tokio::sync::Mutex<Option<(Instant, Value)>>);
fn gate() -> Arc<tokio::sync::Semaphore> {
    static GATE: OnceLock<Arc<tokio::sync::Semaphore>> = OnceLock::new();
    GATE.get_or_init(|| Arc::new(tokio::sync::Semaphore::new(2)))
        .clone()
}
fn error(status: StatusCode, message: &str) -> Answer {
    reply(status, &json!({"error":message}))
}
fn now() -> i64 {
    super::wall_clock_ms() / 1000
}
fn disconnected(message: &str) -> Value {
    json!({"connected":false,"updated_at":now(),"error":message})
}
fn select_pane(rows: &str, session: &str, pane: &str) -> Result<(String, String), &'static str> {
    if session.is_empty() || session.len() > 200 || session.chars().any(char::is_control) {
        return Err("Invalid session");
    }
    if !pane.is_empty()
        && (!pane.starts_with('%')
            || pane.len() < 2
            || !pane[1..].bytes().all(|c| c.is_ascii_digit()))
    {
        return Err("Invalid pane");
    }
    for row in rows.lines() {
        let cols: Vec<_> = row.splitn(5, '\t').collect();
        if cols.len() != 5 || cols[0] != session {
            continue;
        }
        if (!pane.is_empty() && cols[1] == pane)
            || (pane.is_empty() && cols[2] == "1" && cols[3] == "1")
        {
            if !Path::new(cols[4]).is_absolute() {
                return Err("Pane working directory unavailable");
            }
            return Ok((cols[1].into(), cols[4].into()));
        }
    }
    Err("Pane does not belong to the requested session")
}
fn listing(
    root: &Path,
    path: &str,
    expected: Option<&str>,
    pane: &str,
) -> Result<Value, &'static str> {
    let root = root
        .canonicalize()
        .map_err(|_| "Working directory unavailable")?;
    if expected.is_some_and(|v| !v.is_empty() && Path::new(v) != root) {
        return Err("Working directory changed; refresh explorer");
    }
    let relative = Path::new(path);
    if relative.is_absolute()
        || relative
            .components()
            .any(|c| !matches!(c, Component::Normal(_) | Component::CurDir))
    {
        return Err("Path must stay within the working directory");
    }
    let dir = root
        .join(relative)
        .canonicalize()
        .map_err(|_| "Directory unavailable")?;
    if !dir.starts_with(&root) || !dir.is_dir() {
        return Err("Directory is outside the working directory");
    }
    let mut entries = Vec::new();
    let mut scanned = 0;
    let mut truncated = false;
    for item in fs::read_dir(&dir).map_err(|_| "Directory cannot be read")? {
        if scanned == 2000 {
            truncated = true;
            break;
        }
        scanned += 1;
        let Ok(item) = item else {
            continue;
        };
        let path = item.path();
        let Ok(meta) = fs::symlink_metadata(&path) else {
            continue;
        };
        let symlink = meta.file_type().is_symlink();
        let Ok(target) = path.canonicalize() else {
            continue;
        };
        if !target.starts_with(&root) {
            continue;
        }
        let Ok(name) = item.file_name().into_string() else {
            continue;
        };
        let Ok(full) = path.strip_prefix(&root) else {
            continue;
        };
        let Some(absolute) = path.to_str() else {
            continue;
        };
        let Some(rel) = full.to_str() else {
            continue;
        };
        entries.push((target.is_dir(),name.clone(),json!({"name":name,"path":absolute,"relative":rel,"directory":target.is_dir(),"symlink":symlink})));
    }
    entries.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    if entries.len() > 300 {
        truncated = true;
        entries.truncate(300);
    }
    let rel = dir
        .strip_prefix(&root)
        .map_err(|_| "Invalid directory")?
        .to_str()
        .ok_or("Unsupported directory name")?;
    Ok(
        json!({"root":root.to_str().ok_or("Unsupported root name")?,"path":rel,"pane":pane,"entries":entries.into_iter().map(|(_,_,v)|v).collect::<Vec<_>>(),"truncated":truncated}),
    )
}
fn credit_data(value: &Value, at: i64) -> Result<Value, &'static str> {
    let credits = value["data"]["total_credits"]
        .as_f64()
        .filter(|n| n.is_finite() && *n >= 0.0)
        .ok_or("Invalid credits response")?;
    let usage = value["data"]["total_usage"]
        .as_f64()
        .filter(|n| n.is_finite() && *n >= 0.0)
        .ok_or("Invalid credits response")?;
    Ok(
        json!({"connected":true,"total_credits":credits,"total_usage":usage,"remaining":credits-usage,"updated_at":at}),
    )
}
const CREDITS_URL: &str = "https://openrouter.ai/api/v1/credits";
async fn fetch_credits(key: &str) -> Result<Value, &'static str> {
    if key.is_empty() || key.len() > 4096 || key.chars().any(char::is_whitespace) {
        return Err("Enter a valid OpenRouter management key");
    }
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(8))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|_| "Credits service unavailable")?;
    let mut response = client
        .get(CREDITS_URL)
        .bearer_auth(key)
        .send()
        .await
        .map_err(|_| "Unable to reach OpenRouter; try again later")?;
    if !response.status().is_success() {
        return Err(if matches!(response.status().as_u16(), 401 | 403) {
            "OpenRouter requires a valid management key"
        } else {
            "OpenRouter credits unavailable; try again later"
        });
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| "Unable to read OpenRouter response")?
    {
        if bytes.len() + chunk.len() > 16384 {
            return Err("OpenRouter response exceeds limit");
        }
        bytes.extend_from_slice(&chunk);
    }
    let value: Value = serde_json::from_slice(&bytes).map_err(|_| "Invalid OpenRouter response")?;
    credit_data(&value, now())
}
fn key_path(home: &Path) -> PathBuf {
    home.join(".config/comandos/openrouter-management-key")
}
fn read_key(path: &Path) -> Result<Option<String>, &'static str> {
    match fs::OpenOptions::new()
        .read(true)
        .custom_flags(nix::libc::O_NOFOLLOW)
        .open(path)
    {
        Ok(file) => {
            let meta = file
                .metadata()
                .map_err(|_| "Cannot read saved management key")?;
            if !meta.is_file() || meta.permissions().mode() & 0o077 != 0 {
                return Err("Saved management key must have private permissions");
            }
            let mut key = String::new();
            file.take(4097)
                .read_to_string(&mut key)
                .map_err(|_| "Cannot read saved management key")?;
            if key.len() > 4096 {
                return Err("Invalid saved management key");
            }
            Ok(Some(key.trim().into()))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err("Cannot read saved management key"),
    }
}
fn save_key(path: &Path, key: &str) -> Result<(), &'static str> {
    let parent = path.parent().ok_or("Cannot save management key")?;
    fs::create_dir_all(parent).map_err(|_| "Cannot save management key")?;
    let nonce = format!(
        ".openrouter-key-{}-{}",
        std::process::id(),
        super::wall_clock_ms()
    );
    let temp = parent.join(nonce);
    let result = (|| {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temp)
            .map_err(|_| "Cannot save management key")?;
        file.write_all(key.as_bytes())
            .and_then(|_| file.sync_all())
            .map_err(|_| "Cannot save management key")?;
        fs::rename(&temp, path).map_err(|_| "Cannot save management key")?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}
pub async fn answer(native: &Arc<Native>, request: &Request) -> Answer {
    if request.target.split('?').next() == Some("/fs/explorer") {
        let Some(body) = request.data.as_ref().and_then(Value::as_object) else {
            return error(StatusCode::BAD_REQUEST, "Expected explorer request");
        };
        let Some(session) = body.get("session").and_then(Value::as_str) else {
            return error(StatusCode::BAD_REQUEST, "Session required");
        };
        let Some(pane) = body.get("pane").and_then(Value::as_str) else {
            return error(StatusCode::BAD_REQUEST, "Pane required");
        };
        let path = match body.get("path") {
            None => "",
            Some(Value::String(v)) => v,
            _ => return error(StatusCode::BAD_REQUEST, "Invalid path"),
        };
        let expected = match body.get("root") {
            None => None,
            Some(Value::String(v)) => Some(v.clone()),
            _ => return error(StatusCode::BAD_REQUEST, "Invalid root"),
        };
        if path.len() > 4096 {
            return error(StatusCode::BAD_REQUEST, "Path too long");
        }
        // All sessions are listed so tmux's fuzzy target matching cannot select another session.
        let _permit = gate()
            .acquire_owned()
            .await
            .map_err(|_| super::Fault::Error(crate::HandlerError::Failure))?;
        let rows=match native.options().tmux.run(&["list-panes","-a","-F","#{session_name}\t#{pane_id}\t#{window_active}\t#{pane_active}\t#{pane_current_path}"]).await{Ok(v) if v.ok=>v.stdout,_=>return error(StatusCode::BAD_REQUEST,"Unable to resolve session pane")};
        let (pane, cwd) = match select_pane(&rows, session, pane) {
            Ok(v) => v,
            Err(e) => return error(StatusCode::BAD_REQUEST, e),
        };
        let path = path.to_owned();
        let result = tokio::task::spawn_blocking(move || {
            let _permit = _permit;
            listing(Path::new(&cwd), &path, expected.as_deref(), &pane)
        })
        .await
        .map_err(|_| super::Fault::Error(crate::HandlerError::Failure))?;
        return match result {
            Ok(v) => reply(StatusCode::OK, &v),
            Err(e) => error(
                if e.starts_with("Working directory changed") {
                    StatusCode::CONFLICT
                } else {
                    StatusCode::BAD_REQUEST
                },
                e,
            ),
        };
    }
    let mut cache = native.credits.0.lock().await;
    if request.method == Method::GET
        && let Some((at, value)) = &*cache
        && at.elapsed() < Duration::from_secs(60)
    {
        return reply(StatusCode::OK, value);
    }
    let path = key_path(&native.options().home);
    let posted = request.method == Method::POST;
    let key = if posted {
        match request
            .data
            .as_ref()
            .and_then(|v| v.get("key"))
            .and_then(Value::as_str)
        {
            Some(v) => v.trim().to_owned(),
            None => return error(StatusCode::BAD_REQUEST, "Management key required"),
        }
    } else if let Ok(v) = std::env::var("OPENROUTER_MANAGEMENT_KEY") {
        v.trim().to_owned()
    } else {
        let path = path.clone();
        match tokio::task::spawn_blocking(move || read_key(&path)).await {
            Ok(Ok(Some(v))) => v,
            Ok(Ok(None)) => {
                let v =
                    disconnected("Connect an OpenRouter management key to view account credits");
                *cache = Some((Instant::now(), v.clone()));
                return reply(StatusCode::OK, &v);
            }
            _ => {
                let v = disconnected("Cannot read private OpenRouter management key");
                *cache = Some((Instant::now(), v.clone()));
                return reply(StatusCode::OK, &v);
            }
        }
    };
    let value = match fetch_credits(&key).await {
        Ok(v) => v,
        Err(e) => {
            let v = disconnected(e);
            if !posted {
                *cache = Some((Instant::now(), v.clone()));
            }
            return reply(
                if posted {
                    StatusCode::BAD_REQUEST
                } else {
                    StatusCode::OK
                },
                &v,
            );
        }
    };
    if posted {
        let result = tokio::task::spawn_blocking(move || save_key(&path, &key)).await;
        if !matches!(result, Ok(Ok(()))) {
            return error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Validated management key could not be saved",
            );
        }
    }
    *cache = Some((Instant::now(), value.clone()));
    reply(StatusCode::OK, &value)
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, os::unix::fs::symlink};
    fn temp() -> std::path::PathBuf {
        let p = std::env::temp_dir().join(format!(
            "comandos-sidebar-{}-{}-{}",
            std::process::id(),
            super::super::wall_clock_ms(),
            {
                static NEXT: std::sync::atomic::AtomicUsize =
                    std::sync::atomic::AtomicUsize::new(0);
                NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            }
        ));
        fs::create_dir_all(&p).unwrap();
        p
    }
    #[test]
    fn sidebar_confines_and_sorts() {
        let root = temp();
        fs::create_dir(root.join("z-dir")).unwrap();
        fs::write(root.join("a-file"), "").unwrap();
        symlink("z-dir", root.join("b-link")).unwrap();
        symlink("/", root.join("escape")).unwrap();
        let v = listing(&root, "", None, "%7").unwrap();
        assert_eq!(v["entries"][0]["name"], "b-link");
        assert_eq!(v["entries"][0]["symlink"], true);
        assert_eq!(v["entries"][1]["name"], "z-dir");
        assert_eq!(v["entries"][2]["name"], "a-file");
        assert_eq!(v["entries"][0]["relative"], "b-link");
        assert_eq!(
            listing(&root, "b-link", None, "%7").unwrap()["path"],
            "z-dir"
        );
        assert_eq!(v["entries"].as_array().unwrap().len(), 3);
        assert!(listing(&root, "../", None, "%7").is_err());
        assert!(listing(&root, "escape", None, "%7").is_err());
        assert!(listing(&root, "", Some("/stale"), "%7").is_err());
        assert!(listing(&root, "", Some(""), "%7").is_ok());
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn sidebar_limits() {
        let root = temp();
        for i in 0..2005 {
            fs::write(root.join(format!("{i:04}")), "").unwrap();
        }
        let v = listing(&root, "", None, "%1").unwrap();
        assert_eq!(v["entries"].as_array().unwrap().len(), 300);
        assert_eq!(v["truncated"], true);
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn sidebar_exact_session_and_pane() {
        let rows = "alpha\t%1\t1\t1\t/tmp/a\nbeta\t%2\t1\t1\t/tmp/b\nalpha\t%3\t0\t0\t/tmp/c\n";
        assert_eq!(
            select_pane(rows, "alpha", "%3").unwrap(),
            ("%3".into(), "/tmp/c".into())
        );
        assert_eq!(select_pane(rows, "alpha", "").unwrap().0, "%1");
        assert!(select_pane(rows, "alpha", "%2").is_err());
        assert!(select_pane(rows, "alp", "").is_err());
    }
    #[test]
    fn sidebar_private_key_storage_and_no_fake_balances() {
        let root = temp();
        let path = key_path(&root);
        assert!(read_key(&path).unwrap().is_none());
        save_key(&path, "test-management-key").unwrap();
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(
            read_key(&path).unwrap().as_deref(),
            Some("test-management-key")
        );
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
        assert!(read_key(&path).is_err());
        fs::remove_file(&path).unwrap();
        symlink("/etc/passwd", &path).unwrap();
        assert!(read_key(&path).is_err());
        let v = disconnected("Connect management key");
        assert_eq!(v["connected"], false);
        assert!(v.get("remaining").is_none());
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn sidebar_routes_are_exact_and_registered() {
        for method in [Method::GET, Method::POST] {
            assert_eq!(
                super::super::route(&method, "/credits/openrouter"),
                Some(NativeRoute::Sidebar)
            );
        }
        assert_eq!(
            super::super::route(&Method::POST, "/fs/explorer"),
            Some(NativeRoute::Sidebar)
        );
        assert_ne!(
            super::super::route(&Method::POST, "/fs/explorer/escape"),
            Some(NativeRoute::Sidebar)
        );
    }
    #[test]
    fn sidebar_credits_require_real_numbers() {
        let v = credit_data(
            &json!({"data":{"total_credits":20.0,"total_usage":7.5}}),
            10,
        )
        .unwrap();
        assert_eq!(v["remaining"], 12.5);
        assert_eq!(v["updated_at"], 10);
        assert!(credit_data(&json!({"data":{"total_credits":20}}), 10).is_err());
        assert!(credit_data(&json!({"data":{"total_credits":-1,"total_usage":0}}), 10).is_err());
    }
}
