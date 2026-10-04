//! Import only endpoint-matched MCP records, without network or login activity.
use super::{
    canonical, credential_lock, failure, invalid, json_bytes, private_write, read_config,
    received_timestamp,
};
use comandos_core::json::truthy;
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
    time::UNIX_EPOCH,
};

fn numeric(value: &Value) -> Result<f64, String> {
    match value {
        Value::Bool(value) => Ok(u8::from(*value) as f64),
        Value::Number(value) => value.as_f64().filter(|n| n.is_finite()).ok_or_else(invalid),
        _ => Err(invalid()),
    }
}

// Path.glob preserves directory iteration order. Keep it for equal-expiry ties.
fn nested_paths(root: &Path, filename: &str) -> Result<Vec<PathBuf>, String> {
    let entries = match fs::read_dir(root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(_) => return Err(failure()),
    };
    let mut paths = Vec::new();
    for entry in entries {
        let path = entry.map_err(|_| failure())?.path().join(filename);
        if path.exists() {
            paths.push(path);
        }
    }
    Ok(paths)
}

fn claude_candidates(home: &Path, candidates: &mut Vec<(String, Value)>) -> Result<(), String> {
    let mut paths = vec![home.join(".claude/.credentials.json")];
    paths.extend(nested_paths(
        &home.join(".claude-accounts"),
        ".credentials.json",
    )?);
    for path in paths {
        let data = read_config(&path)?;
        let Some(records) = data.get("mcpOAuth") else {
            continue;
        };
        for item in records.as_object().ok_or_else(invalid)?.values() {
            if !item.is_object() || !truthy(&item["accessToken"]) {
                continue;
            }
            let discovery = item
                .get("discoveryState")
                .map(|v| v.as_object().ok_or_else(invalid))
                .transpose()?;
            let expiry = if truthy(&item["expiresAt"]) {
                numeric(&item["expiresAt"])? / 1000.0
            } else {
                0.0
            };
            let Some(name) = item["serverName"].as_str() else {
                continue;
            };
            candidates.push((canonical(name).to_owned(), json!({
                "url":item["serverUrl"], "access_token":item["accessToken"],
                "refresh_token":item["refreshToken"], "client_id":item["clientId"],
                "expires_at":expiry,
                "issuer":discovery.and_then(|v|v.get("authorizationServerUrl")).cloned().unwrap_or(Value::Null),
                "source":path.to_str().ok_or_else(invalid)?
            })));
        }
    }
    Ok(())
}

fn grok_candidates(home: &Path, candidates: &mut Vec<(String, Value)>) -> Result<(), String> {
    let path = home.join(".grok/mcp_credentials.json");
    let data = read_config(&path)?;
    for (key, item) in data.as_object().ok_or_else(invalid)? {
        if !item.is_object() {
            continue;
        }
        let Some(token) = item.get("token_response") else {
            continue;
        };
        if !token.is_object() {
            return Err(invalid());
        }
        if !truthy(&token["access_token"]) {
            continue;
        }
        let (name, url) = key.split_once(':').ok_or_else(invalid)?;
        let raw_received = &item["token_received_at"];
        let received = if let Some(object) = raw_received.as_object() {
            object
                .get("secs_since_epoch")
                .map(numeric)
                .transpose()?
                .unwrap_or(0.0)
        } else {
            received_timestamp(raw_received)
        };
        let duration = token
            .get("expires_in")
            .map(numeric)
            .transpose()?
            .unwrap_or(0.0);
        let expiry = received + duration;
        if !expiry.is_finite() {
            return Err(invalid());
        }
        candidates.push((canonical(name).to_owned(), json!({
            "url":url,"access_token":token["access_token"],"refresh_token":token["refresh_token"],
            "client_id":item["client_id"],"issuer":item["issuer"],"expires_at":expiry,"source":path.to_str().ok_or_else(invalid)?
        })));
    }
    Ok(())
}

fn remote_candidates(
    home: &Path,
    catalog: &Value,
    candidates: &mut Vec<(String, Value)>,
) -> Result<(), String> {
    for (name, spec) in catalog["servers"].as_object().ok_or_else(invalid)? {
        if !spec.is_object() {
            return Err(invalid());
        }
        if !truthy(&spec["url"]) {
            continue;
        }
        let endpoint = spec["url"].as_str().ok_or_else(invalid)?;
        // Existing mcp-remote cache naming mandates MD5; this is not token security.
        let key = format!("{:x}", md5::compute(endpoint.as_bytes()));
        for path in nested_paths(&home.join(".mcp-auth"), &format!("{key}_tokens.json"))? {
            let token = read_config(&path)?;
            let client = read_config(&path.with_file_name(format!("{key}_client_info.json")))?;
            if !truthy(&token["access_token"]) {
                continue;
            }
            let modified = fs::metadata(&path)
                .and_then(|m| m.modified())
                .map_err(|_| failure())?;
            let received = modified
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs_f64())
                .unwrap_or_else(|e| -e.duration().as_secs_f64());
            let expiry = received
                + token
                    .get("expires_in")
                    .map(numeric)
                    .transpose()?
                    .unwrap_or(0.0);
            if !expiry.is_finite() {
                return Err(invalid());
            }
            candidates.push((name.clone(), json!({
                "url":endpoint,"access_token":token["access_token"],"refresh_token":token["refresh_token"],
                "client_id":client["client_id"],"client_secret":client["client_secret"],
                "token_endpoint_auth_method":client["token_endpoint_auth_method"],"expires_at":expiry,
                "issuer":endpoint.split("/v1/").next().unwrap_or(endpoint),"source":path.to_str().ok_or_else(invalid)?
            })));
        }
    }
    Ok(())
}

pub fn import_credentials(home: &Path, catalog: &Value) -> Result<Vec<String>, String> {
    let _lock = credential_lock(home)?;
    let servers = catalog
        .get("servers")
        .and_then(Value::as_object)
        .ok_or_else(invalid)?;
    let mut candidates = Vec::new();
    claude_candidates(home, &mut candidates)?;
    grok_candidates(home, &mut candidates)?;
    remote_candidates(home, catalog, &mut candidates)?;
    candidates.sort_by(|a, b| {
        b.1["expires_at"]
            .as_f64()
            .unwrap_or(0.0)
            .total_cmp(&a.1["expires_at"].as_f64().unwrap_or(0.0))
    });
    let path = home.join(".config/comandos/extensions/credentials.json");
    let mut data = read_config(&path)?;
    for (name, item) in candidates {
        let Some(spec) = servers.get(&name) else {
            continue;
        };
        if !spec.is_object() {
            return Err(invalid());
        }
        if let Some(existing) = data.get(&name) {
            if !existing.is_object() {
                return Err(invalid());
            }
            if existing["url"] == spec["url"] {
                continue;
            }
        }
        if spec["url"] == item["url"] {
            data[name] = item;
        }
    }
    if read_config(&path)? != data {
        private_write(&path, &json_bytes(&data)?)?;
    }
    let mut names: Vec<_> = data
        .as_object()
        .ok_or_else(invalid)?
        .keys()
        .cloned()
        .collect();
    names.sort();
    Ok(names)
}
