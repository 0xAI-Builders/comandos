//! Artefactos web de `xtask web-build` (Fase 3, T4): formato de `manifest.json` y
//! dónde se escriben. Un solo tipo para `xtask` (escribe) y el servidor (lee,
//! B2/B15). Solo datos y reglas puras: sin E/S ni entorno.
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Nombre del manifiesto dentro del directorio de salida.
pub const MANIFEST_FILE: &str = "manifest.json";

/// Nombre lógico → ruta con hash relativa al directorio de salida, p. ej.
/// `"comandos_web_bg.wasm" → "a1b2c3d4e5f6/comandos_web_bg.wasm"`.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    pub files: BTreeMap<String, String>,
}

impl Manifest {
    /// Ruta con hash de un nombre lógico.
    pub fn path(&self, logical: &str) -> Option<&str> {
        self.files.get(logical).map(String::as_str)
    }

    /// Cada ruta es relativa y llana: segmentos no vacíos separados por `/`, sin
    /// `.`, `..`, barra invertida ni NUL. Así el servidor y `install --stage` pueden unirla a su
    /// directorio sin salir de él. El error nombra la entrada.
    pub fn check_paths(&self) -> Result<(), String> {
        for (logical, path) in &self.files {
            let bad_name = logical.is_empty() || logical.contains(['/', '\\', '\0']);
            let bad_path = path.is_empty()
                || path.contains(['\\', '\0'])
                || path
                    .split('/')
                    .any(|seg| seg.is_empty() || seg == "." || seg == "..");
            if bad_name || bad_path {
                return Err(format!(
                    "entrada inválida en el manifiesto: {logical:?} → {path:?}"
                ));
            }
        }
        Ok(())
    }
}

/// Directorio de compilación con la misma regla que cargo: `CARGO_TARGET_DIR` si
/// está y no es vacío (relativo al directorio desde el que se invoca cargo), si no
/// `<workspace>/target`. No lee `build.target-dir` de `.cargo/config.toml`: el repo
/// no lo fija, y la prueba de `xtask` contra `cargo metadata` lo vigila.
pub fn target_dir_from(cargo_target_dir: Option<&Path>, cwd: &Path, workspace: &Path) -> PathBuf {
    match cargo_target_dir.filter(|p| !p.as_os_str().is_empty()) {
        Some(p) if p.is_absolute() => p.to_path_buf(),
        Some(p) => cwd.join(p),
        None => workspace.join("target"),
    }
}

/// `<directorio de compilación>/web`: salida de `xtask web-build` (preflight R6).
/// `install --stage` no la deduce: recibe el origen explícito (`--web`).
pub fn out_dir_from(cargo_target_dir: Option<&Path>, cwd: &Path, workspace: &Path) -> PathBuf {
    target_dir_from(cargo_target_dir, cwd, workspace).join("web")
}

/// Opt-in compiled dashboard bundle, emitted together with its WASM artifacts.
pub const NATIVE_PAGE_FILE: &str = "comandos_native_page.json";
pub const NATIVE_PAGE_VERSION: u32 = 1;
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativePage {
    pub version: u32,
    pub template_sha256: String,
    pub components: Vec<String>,
    /// Original root URL -> manifest logical name (includes CSS dependencies).
    pub assets: BTreeMap<String, String>,
}
/// Authoritative inventory page order; `component` differs from the region ID
/// for analytics-inline and must never be guessed from a filename.
pub fn native_index_components() -> Result<Vec<String>, String> {
    let inventory: serde_json::Value =
        serde_json::from_str(include_str!("../../../xtask/web/inventory.json"))
            .map_err(|e| e.to_string())?;
    let units = inventory
        .get("units")
        .and_then(serde_json::Value::as_array)
        .ok_or("inventory units missing")?;
    let page = inventory
        .get("pages")
        .and_then(|p| p.get("index.html"))
        .and_then(serde_json::Value::as_array)
        .ok_or("inventory index page missing")?;
    let mut ids = Vec::new();
    for unit in page {
        let name = unit.as_str().ok_or("invalid inventory unit")?;
        let id = units
            .iter()
            .find(|u| u.get("id").and_then(serde_json::Value::as_str) == Some(name))
            .and_then(|u| u.get("component"))
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| format!("missing component for {name}"))?;
        if !ids.iter().any(|v| v == id) {
            ids.push(id.to_string());
        }
    }
    Ok(ids)
}
