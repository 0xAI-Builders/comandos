use comandos_core::web_assets::MANIFEST_FILE;
use serde_json::Value;
use std::{collections::BTreeMap, fs, path::Path};

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Manifest {
    files: BTreeMap<String, String>,
}

impl Manifest {
    pub fn load(web_dir: &Path) -> Result<Self, String> {
        let text = fs::read_to_string(web_dir.join(MANIFEST_FILE)).map_err(|e| e.to_string())?;
        let value: Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
        let mut files = BTreeMap::new();
        if let Some(map) = value.get("files").and_then(Value::as_object) {
            for (k, v) in map {
                if let Some(path) = v.as_str().filter(|p| valid_relative(p)) {
                    files.insert(k.clone(), path.to_string());
                }
            }
        }
        Ok(Self { files })
    }

    pub fn path(&self, logical: &str) -> String {
        self.files
            .get(logical)
            .cloned()
            .unwrap_or_else(|| format!("0123456789ab/{logical}"))
    }

    pub fn test() -> Self {
        Self::default()
    }
}

pub fn valid_relative(path: &str) -> bool {
    !path.is_empty()
        && !path.contains(['\\', '\0'])
        && path
            .split('/')
            .all(|seg| !seg.is_empty() && seg != "." && seg != "..")
}
