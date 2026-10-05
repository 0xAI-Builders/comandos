//! Dónde está el checkout de ComandOS para quien sirve el tablero o pinta sus
//! popups (`config/themes.json`, `dash/icons`). Única lectura del disco de
//! este crate: resolver un enlace simbólico, que es la regla y no se puede
//! decidir sin mirar.
use std::path::{Path, PathBuf};

/// Variable que fija el checkout a mano (la misma de `comandos dash`).
pub const REPO_ENV: &str = "COMANDOS_DASH_REPO";

/// `COMANDOS_DASH_REPO` si está y no es vacía; si no, el destino canónico de
/// `<dash_dir>/index.html` dos niveles arriba (`install.sh` y el arnés
/// enlazan `dash/*` al checkout desde el que corre el Python).
pub fn repo_root(dash_dir: &Path, override_dir: Option<&str>) -> Option<PathBuf> {
    if let Some(raw) = override_dir.filter(|v| !v.is_empty()) {
        return Some(PathBuf::from(raw));
    }
    let index = std::fs::canonicalize(dash_dir.join("index.html")).ok()?;
    index.parent()?.parent().map(Path::to_path_buf)
}
