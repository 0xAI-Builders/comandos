use comandos_core::web_assets::MANIFEST_FILE;
use std::{collections::BTreeMap, fs, path::Path};

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Manifest {
    files: BTreeMap<String, String>,
}

impl Manifest {
    pub fn load(web_dir: &Path) -> Result<Self, String> {
        let text = fs::read_to_string(web_dir.join(MANIFEST_FILE)).map_err(|e| e.to_string())?;
        let manifest: comandos_core::web_assets::Manifest =
            serde_json::from_str(&text).map_err(|e| e.to_string())?;
        manifest.check_paths()?;
        let loaded = Self {
            files: manifest.files,
        };
        loaded.check_files(web_dir)?;
        Ok(loaded)
    }

    pub fn check_files(&self, web_dir: &Path) -> Result<(), String> {
        for required in [
            "comandos_web_boot.js",
            "comandos_web.js",
            "comandos_web_bg.wasm",
        ] {
            if !self.files.contains_key(required) {
                return Err(format!("artefacto requerido ausente: {required}"));
            }
        }
        for (logical, rel) in &self.files {
            if !web_dir.join(rel).is_file() {
                return Err(format!("artefacto ausente: {logical} → {rel}"));
            }
        }
        Ok(())
    }

    pub fn path(&self, logical: &str) -> String {
        self.files.get(logical).cloned().unwrap_or_default()
    }

    pub(super) fn paths(&self) -> impl Iterator<Item = &str> {
        self.files.values().map(String::as_str)
    }

    pub fn is_versioned(&self, relative: &str) -> bool {
        relative.split_once('/').is_some_and(|(hash, _)| {
            hash.len() == 12 && hash.bytes().all(|b| b.is_ascii_hexdigit())
        }) && self.files.values().any(|path| path == relative)
    }

    pub fn test() -> Self {
        Self {
            files: BTreeMap::from([("comandos_web_boot.js".into(), "0123456789ab/boot.js".into())]),
        }
    }
}

pub fn valid_relative(path: &str) -> bool {
    !path.is_empty()
        && !path.contains(['\\', '\0'])
        && path
            .split('/')
            .all(|seg| !seg.is_empty() && seg != "." && seg != "..")
}
