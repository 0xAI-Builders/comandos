//! Dónde está el checkout de ComandOS para quien sirve el tablero o pinta sus
//! popups (`config/themes.json`, `dash/icons`). Regla pura: resolver el
//! enlace simbólico de `<dash>/index.html` es I/O y lo hace quien llama
//! (`comandos dash` y `comandos-notifyd`).
use std::path::{Path, PathBuf};

/// Variable que fija el checkout a mano (la misma de `comandos dash`).
pub const REPO_ENV: &str = "COMANDOS_DASH_REPO";

/// `COMANDOS_DASH_REPO` si está y no es vacía; si no, el destino canónico de
/// `<dash_dir>/index.html` (`index_target`, ya resuelto por quien llama) dos
/// niveles arriba (`install.sh` y el arnés enlazan `dash/*` al checkout desde
/// el que corre el Python).
pub fn repo_root_from(index_target: Option<&Path>, override_dir: Option<&str>) -> Option<PathBuf> {
    if let Some(raw) = override_dir.filter(|v| !v.is_empty()) {
        return Some(PathBuf::from(raw));
    }
    index_target?.parent()?.parent().map(Path::to_path_buf)
}
