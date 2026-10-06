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
