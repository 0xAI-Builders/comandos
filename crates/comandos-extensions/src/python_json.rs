//! Python-compatible sorted JSON bytes for persisted metadata identities.
use serde_json::Value;
use sha2::{Digest, Sha256};

pub use comandos_core::json::dumps;

pub fn digest(value: &Value) -> Result<String, String> {
    Ok(format!(
        "{:x}",
        Sha256::digest(dumps(value, true, true)?.as_bytes())
    ))
}

/// Object ordering cannot change length; ASCII escaping makes bytes == chars.
pub fn default_len(value: &Value) -> Result<usize, String> {
    Ok(dumps(value, true, false)?.len())
}
