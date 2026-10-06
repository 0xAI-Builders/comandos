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
        Ok(Self {
            files: manifest.files,
        })
    }

    pub fn path(&self, logical: &str) -> String {
        self.files.get(logical).cloned().unwrap_or_default()
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
