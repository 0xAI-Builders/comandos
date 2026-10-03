//! Loss-checked private configuration IO. The sync lock coordinates our writers;
//! comparisons cannot provide an atomic CAS against unrelated external writers.
pub mod toml;
use crate::Result;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    os::unix::fs::{DirBuilderExt, OpenOptionsExt},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};
static NEXT: AtomicU64 = AtomicU64::new(0);
pub fn state_dir(home: &Path) -> PathBuf {
    home.join(".local/state/comandos/extensions")
}
pub fn catalog_path(home: &Path) -> PathBuf {
    home.join(".config/comandos/extensions/catalog.json")
}
pub fn err(path: &Path) -> String {
    format!("Invalid configuration: {}", path.display())
}
pub fn private_dir(path: &Path) -> Result<()> {
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(path)
        .map_err(|_| err(path))
}
pub fn temporary(parent: &Path, prefix: &str) -> PathBuf {
    parent.join(format!(
        ".{prefix}-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ))
}
pub struct SyncLock(File);
impl SyncLock {
    pub fn new(home: &Path) -> Result<Self> {
        let root = state_dir(home);
        private_dir(&root)?;
        let p = root.join("sync.lock");
        let f = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .custom_flags(nix::libc::O_NOFOLLOW)
            .open(&p)
            .map_err(|_| err(&p))?;
        f.lock().map_err(|_| err(&p))?;
        Ok(Self(f))
    }
}
impl Drop for SyncLock {
    fn drop(&mut self) {
        let _ = self.0.unlock();
    }
}
pub fn read_bytes(path: &Path) -> Result<Option<Vec<u8>>> {
    let f = match File::open(path) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(err(path)),
    };
    let mut b = Vec::new();
    f.take(16 * 1024 * 1024 + 1)
        .read_to_end(&mut b)
        .map_err(|_| err(path))?;
    if b.len() > 16 * 1024 * 1024 {
        return Err(err(path));
    }
    Ok(Some(b))
}
pub fn json_bytes(value: &Value) -> Result<Vec<u8>> {
    let mut b = serde_json::to_vec_pretty(value).map_err(|_| "Invalid configuration value")?;
    b.push(b'\n');
    Ok(b)
}
pub fn private_write(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path.parent().ok_or_else(|| err(path))?;
    private_dir(parent)?;
    let (tmp, mut file) = loop {
        let tmp = temporary(parent, "comandos");
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&tmp)
        {
            Ok(f) => break (tmp, f),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(_) => return Err(err(path)),
        }
    };
    let result = (|| {
        file.write_all(bytes)
            .and_then(|_| file.sync_all())
            .map_err(|_| err(path))?;
        fs::rename(&tmp, path).map_err(|_| err(path))?;
        File::open(parent)
            .and_then(|f| f.sync_all())
            .map_err(|_| err(path))
    })();
    let _ = fs::remove_file(tmp);
    result
}
pub fn save_json(path: &Path, value: &Value) -> Result<()> {
    private_write(path, &json_bytes(value)?)
}
pub fn replace_config(
    home: &Path,
    path: &Path,
    before: Option<&[u8]>,
    after: &[u8],
) -> Result<bool> {
    if path.is_symlink() {
        return Err(format!(
            "Configuration symlink requires an explicit target: {}",
            path.display()
        ));
    }
    if read_bytes(path)?.as_deref() != before {
        return Err(format!(
            "Configuration changed concurrently: {}",
            path.display()
        ));
    }
    if before == Some(after) {
        return Ok(false);
    }
    if let Some(before) = before {
        let root = state_dir(home).join("backups");
        private_dir(&root)?;
        let name = path
            .strip_prefix(home)
            .map_err(|_| err(path))?
            .to_string_lossy()
            .replace('/', "__");
        let backup = root.join(format!(
            "{name}.{}",
            &format!("{:x}", Sha256::digest(before))[..16]
        ));
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&backup)
        {
            Ok(mut f) => {
                f.write_all(before)
                    .and_then(|_| f.sync_all())
                    .map_err(|_| err(&backup))?;
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                if backup.is_symlink()
                    || !backup.is_file()
                    || read_bytes(&backup)?.as_deref() != Some(before)
                {
                    return Err(format!("Existing backup is invalid: {}", backup.display()));
                }
            }
            Err(_) => return Err(err(&backup)),
        }
    }
    if path.is_symlink() || read_bytes(path)?.as_deref() != before {
        return Err(format!(
            "Configuration changed during backup: {}",
            path.display()
        ));
    }
    private_write(path, after)?;
    Ok(true)
}
pub struct Document {
    pub data: Value,
    native: Option<toml::Document>,
}
impl Document {
    pub fn parse(path: &Path, raw: Option<&[u8]>) -> Result<Self> {
        let missing = raw.is_none();
        let raw = raw.unwrap_or(b"");
        if path.extension().is_some_and(|s| s == "toml") {
            let native = toml::Document::parse(std::str::from_utf8(raw).map_err(|_| err(path))?)
                .map_err(|_| err(path))?;
            let data = native.value()?;
            Ok(Self {
                data,
                native: Some(native),
            })
        } else {
            let data = if missing {
                json!({})
            } else {
                parse_json(raw).map_err(|_| err(path))?
            };
            Ok(Self { data, native: None })
        }
    }
    pub fn render(&self) -> Result<Vec<u8>> {
        match &self.native {
            Some(doc) => Ok(doc.render(&self.data)?.into_bytes()),
            None => json_bytes(&self.data),
        }
    }
    pub fn native_fingerprints(&self, key: &str) -> Result<Value> {
        let data = match &self.native {
            Some(doc) => doc.fingerprint_value(&self.data)?,
            None => self.data.clone(),
        };
        let mut result = json!({});
        if let Some(items) = data.get(key).and_then(Value::as_object) {
            for (n, s) in items {
                let mut s = s.clone();
                if data["disabled_mcp_servers"]
                    .as_array()
                    .is_some_and(|a| a.contains(&json!(n)))
                {
                    s["enabled"] = json!(false);
                }
                result[n] = json!(fingerprint(&s)?);
            }
        }
        Ok(result)
    }
    pub fn check_normalized(&self, path: &[&str], spec: &Value) -> Result<()> {
        if let Some(doc) = &self.native {
            let truth = |v: &&Value| match v {
                Value::Null => false,
                Value::Bool(v) => *v,
                Value::String(s) => !s.is_empty(),
                Value::Array(v) => !v.is_empty(),
                Value::Object(v) => !v.is_empty(),
                Value::Number(n) => n.as_f64() != Some(0.0),
            };
            let mut fields = Vec::new();
            if spec.get("command").filter(truth).is_some() {
                fields.extend(["command", "args", "cwd", "env_vars"]);
                fields.push(if spec.get("environment").is_some() {
                    "environment"
                } else {
                    "env"
                });
            } else {
                if let Some(key) = ["url", "serverUrl", "httpUrl"]
                    .into_iter()
                    .find(|key| spec.get(*key).filter(truth).is_some())
                {
                    fields.push(key);
                }
                fields.push(if spec.get("headers").filter(truth).is_some() {
                    "headers"
                } else {
                    "http_headers"
                });
                fields.extend(["bearer_token_env_var", "env_http_headers"]);
            }
            fields.extend(["disabled_tools", "enabled_tools"]);
            doc.check_fields(path, &fields)?;
        }
        Ok(())
    }
    pub fn check_policy(&self, key: &str, name: &str, field: &str) -> Result<()> {
        if let Some(doc) = &self.native {
            doc.check_policy(key, name, field)?;
        }
        Ok(())
    }
}
pub fn read_config(path: &Path) -> Result<Value> {
    let raw = read_bytes(path)?;
    Ok(Document::parse(path, raw.as_deref())?.data)
}
pub fn fingerprint(value: &Value) -> Result<String> {
    Ok(format!(
        "{:x}",
        Sha256::digest(crate::python_json::dumps(value, true, false)?.as_bytes())
    ))
}

// CPython accepts these non-finite constants in JSON; keep them as numeric
// atoms without feeding serde's private map tokens through generic Value serde.
pub fn parse_json(raw: &[u8]) -> Result<Value> {
    if let Ok(value) = crate::auth::parse_config_bytes(raw) {
        return Ok(value);
    }
    let clean = crate::auth::clean_config_bytes(raw)?;
    let text = std::str::from_utf8(&clean).map_err(|_| "Invalid JSON")?;
    let tokens =
        regex::Regex::new(r#""(?:\\.|[^"\\])*"|-?Infinity|NaN"#).map_err(|_| "Invalid JSON")?;
    let strings = tokens
        .find_iter(text)
        .filter_map(|m| serde_json::from_str::<String>(m.as_str()).ok())
        .collect::<Vec<_>>();
    let mut markers = std::collections::BTreeMap::new();
    let replaced = tokens.replace_all(text, |caps: &regex::Captures<'_>| {
        let token = &caps[0];
        if token.starts_with('"') {
            return token.to_owned();
        }
        let mut marker = format!("__comandos_nonfinite_{}__", markers.len());
        while strings.contains(&marker) || markers.contains_key(&marker) {
            marker.push('_');
        }
        markers.insert(marker.clone(), token.to_owned());
        format!("\"{marker}\"")
    });
    let mut value = crate::auth::parse_clean_config_bytes(replaced.as_bytes())?;
    fn restore(
        value: &mut Value,
        markers: &std::collections::BTreeMap<String, String>,
    ) -> Result<()> {
        match value {
            Value::String(s) => {
                if let Some(n) = markers.get(s) {
                    *value = Value::Number(serde_json::Number::from_string_unchecked(n.clone()));
                }
            }
            Value::Array(a) => {
                for v in a {
                    restore(v, markers)?;
                }
            }
            Value::Object(o) => {
                for (k, v) in o {
                    if markers.contains_key(k) {
                        return Err("Invalid JSON key".into());
                    }
                    restore(v, markers)?;
                }
            }
            _ => {}
        }
        Ok(())
    }
    restore(&mut value, &markers)?;
    Ok(value)
}
