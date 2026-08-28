use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};

use crate::auth::{AuthStorage, FileAuthStore};
use crate::paths;

use super::login::{CANONICAL_ISSUER, CLIENT_ID};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StoredAuth {
    pub access: String,
    pub refresh: String,
    pub expires_at_ms: u64,
    pub issuer: String,
    pub client_id: String,
}

pub struct GrokTokenStore<S: AuthStorage<StoredAuth>> {
    store: S,
}

impl<S: AuthStorage<StoredAuth>> GrokTokenStore<S> {
    pub fn new(store: S) -> Self {
        Self { store }
    }
    pub fn load_auth(&self) -> anyhow::Result<Option<StoredAuth>> {
        self.store.load()
    }
    pub fn save_auth(&self, auth: StoredAuth) -> anyhow::Result<()> {
        self.store.save(auth)
    }
    pub fn clear_auth(&self) -> anyhow::Result<()> {
        self.store.clear()
    }
    pub fn auth_path(&self) -> String {
        self.store.path()
    }
}

/// Uses the official Grok Build OAuth session as the authority while keeping
/// the proxy-format file as a compatibility fallback. Refreshes are mirrored
/// back to the official record so both clients keep one subscription session.
pub struct OfficialGrokAuthStore {
    proxy: FileAuthStore<StoredAuth>,
    official: PathBuf,
}

impl OfficialGrokAuthStore {
    pub fn new(proxy: FileAuthStore<StoredAuth>, official: PathBuf) -> Self {
        Self { proxy, official }
    }

    fn load_official(&self) -> anyhow::Result<Option<StoredAuth>> {
        let Ok(raw) = fs::read_to_string(&self.official) else {
            return Ok(None);
        };
        let data: Value = serde_json::from_str(&raw)?;
        let Some(root) = data.as_object() else {
            return Ok(None);
        };
        for (name, value) in root {
            let Some(record) = value.as_object() else {
                continue;
            };
            let client = record
                .get("oidc_client_id")
                .and_then(Value::as_str)
                .unwrap_or("");
            if client != CLIENT_ID && !name.contains(CLIENT_ID) {
                continue;
            }
            let access = record.get("key").and_then(Value::as_str).unwrap_or("");
            let refresh = record
                .get("refresh_token")
                .and_then(Value::as_str)
                .unwrap_or("");
            if access.is_empty() || refresh.is_empty() {
                continue;
            }
            let issuer = record
                .get("oidc_issuer")
                .and_then(Value::as_str)
                .unwrap_or(CANONICAL_ISSUER);
            let expires_at_ms = record
                .get("expires_at")
                .and_then(Value::as_str)
                .and_then(|stamp| {
                    time::OffsetDateTime::parse(
                        stamp,
                        &time::format_description::well_known::Rfc3339,
                    )
                    .ok()
                })
                .map(|value| value.unix_timestamp_nanos().max(0) as u64 / 1_000_000)
                .unwrap_or(0);
            return Ok(Some(StoredAuth {
                access: access.into(),
                refresh: refresh.into(),
                expires_at_ms,
                issuer: issuer.into(),
                client_id: CLIENT_ID.into(),
            }));
        }
        Ok(None)
    }

    fn save_official(&self, auth: &StoredAuth) -> anyhow::Result<()> {
        if !self.official.is_file() {
            return Ok(());
        }
        let mut data: Value = serde_json::from_str(&fs::read_to_string(&self.official)?)?;
        let Some(root) = data.as_object_mut() else {
            return Ok(());
        };
        let Some((_name, value)) = root.iter_mut().find(|(name, value)| {
            value.get("oidc_client_id").and_then(Value::as_str) == Some(CLIENT_ID)
                || name.contains(CLIENT_ID)
        }) else {
            return Ok(());
        };
        let Some(record) = value.as_object_mut() else {
            return Ok(());
        };
        record.insert("key".into(), Value::String(auth.access.clone()));
        record.insert("refresh_token".into(), Value::String(auth.refresh.clone()));
        record.insert("oidc_issuer".into(), Value::String(auth.issuer.clone()));
        record.insert(
            "oidc_client_id".into(),
            Value::String(auth.client_id.clone()),
        );
        if let Ok(value) = time::OffsetDateTime::from_unix_timestamp_nanos(
            (auth.expires_at_ms as i128) * 1_000_000,
        ) {
            if let Ok(stamp) = value.format(&time::format_description::well_known::Rfc3339) {
                record.insert("expires_at".into(), Value::String(stamp));
            }
        }
        let tmp = self.official.with_extension("json.tmp");
        fs::write(&tmp, serde_json::to_vec_pretty(&data)?)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&tmp, fs::Permissions::from_mode(0o600))?;
        }
        fs::rename(tmp, &self.official)?;
        Ok(())
    }
}

impl AuthStorage<StoredAuth> for OfficialGrokAuthStore {
    fn load(&self) -> anyhow::Result<Option<StoredAuth>> {
        self.load_official()
            .or_else(|_| self.proxy.load())
            .and_then(|official| {
                if official.is_some() {
                    Ok(official)
                } else {
                    self.proxy.load()
                }
            })
    }
    fn save(&self, value: StoredAuth) -> anyhow::Result<()> {
        self.proxy.save(value.clone())?;
        self.save_official(&value)
    }
    fn clear(&self) -> anyhow::Result<()> {
        self.proxy.clear()
    }
    fn path(&self) -> String {
        self.official.to_string_lossy().into_owned()
    }
}

fn official_auth_file() -> PathBuf {
    if let Ok(path) = std::env::var("CCP_GROK_AUTH_FILE") {
        return PathBuf::from(path);
    }
    let home = std::env::var("GROK_HOME")
        .ok()
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var("HOME")
                .ok()
                .map(|v| Path::new(&v).join(".grok"))
        })
        .unwrap_or_else(|| PathBuf::from(".grok"));
    home.join("auth.json")
}

pub fn file_store() -> GrokTokenStore<OfficialGrokAuthStore> {
    let primary = paths::provider_auth_file("grok");
    let legacy = paths::provider_legacy_auth_file("grok");
    let proxy = FileAuthStore::new(
        primary.to_string_lossy().into_owned(),
        legacy.to_string_lossy().into_owned(),
    );
    GrokTokenStore::new(OfficialGrokAuthStore::new(proxy, official_auth_file()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::AuthStorage;

    #[test]
    fn official_grok_session_is_loaded_and_refreshed_in_place() {
        let dir = tempfile::tempdir().unwrap();
        let official = dir.path().join("official.json");
        let proxy = dir.path().join("proxy.json");
        fs::write(
            &official,
            serde_json::to_vec_pretty(&serde_json::json!({
                format!("{}::{}", CANONICAL_ISSUER, CLIENT_ID): {
                    "key":"official-access",
                    "refresh_token":"official-refresh",
                    "expires_at":"2030-01-01T00:00:00Z",
                    "oidc_issuer":CANONICAL_ISSUER,
                    "oidc_client_id":CLIENT_ID,
                    "first_name":"preserved"
                }
            }))
            .unwrap(),
        )
        .unwrap();
        let store = OfficialGrokAuthStore::new(
            FileAuthStore::new(
                proxy.to_string_lossy().into_owned(),
                proxy.to_string_lossy().into_owned(),
            ),
            official.clone(),
        );
        let loaded = store.load().unwrap().unwrap();
        assert_eq!(loaded.access, "official-access");
        assert_eq!(loaded.refresh, "official-refresh");
        store
            .save(StoredAuth {
                access: "rotated-access".into(),
                refresh: "rotated-refresh".into(),
                expires_at_ms: 1_900_000_000_000,
                issuer: CANONICAL_ISSUER.into(),
                client_id: CLIENT_ID.into(),
            })
            .unwrap();
        let value: Value = serde_json::from_slice(&fs::read(official).unwrap()).unwrap();
        let record = value.as_object().unwrap().values().next().unwrap();
        assert_eq!(record["key"], "rotated-access");
        assert_eq!(record["refresh_token"], "rotated-refresh");
        assert_eq!(record["first_name"], "preserved");
        assert!(proxy.is_file());
    }
}
