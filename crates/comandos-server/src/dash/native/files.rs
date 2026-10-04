//! Archivos JSON de `~/.claude/hooks` como los lee y escribe `bin/cc-dash`.
use comandos_core::json::{response_dumps, workspace_loads_bytes};
use serde_json::Value;
use std::{
    fs, io,
    io::Write,
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::Path,
};

/// `json.load(open(path))` con `except Exception`: cualquier fallo es `None`.
pub fn read_json(path: &Path) -> Option<Value> {
    fs::read(path)
        .ok()
        .and_then(|bytes| workspace_loads_bytes(&bytes))
}

pub enum Strict {
    /// `FileNotFoundError`.
    Missing,
    /// Otro `OSError` o `ValueError` (JSON roto, UTF-8 inválido).
    Unreadable,
    Value(Value),
}

/// `_tab_registry` (6348): ausente ≠ ilegible.
pub fn read_json_strict(path: &Path) -> Strict {
    match fs::read(path) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => Strict::Missing,
        Err(_) => Strict::Unreadable,
        Ok(bytes) => workspace_loads_bytes(&bytes).map_or(Strict::Unreadable, Strict::Value),
    }
}

/// `write_json_file` (5090) → `write_file_atomic` (5063): temporal en el mismo
/// directorio, fsync, permisos del archivo previo (0600 si es nuevo) y rename.
/// Bloquea: llamar dentro de `spawn_blocking`.
pub fn write_json_atomic(path: &Path, value: &Value) -> io::Result<()> {
    let text = response_dumps(value).map_err(io::Error::other)?;
    let dir = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    fs::create_dir_all(dir)?;
    let mode = match fs::metadata(path) {
        Ok(meta) => meta.permissions().mode() & 0o7777,
        Err(e) if e.kind() == io::ErrorKind::NotFound => 0o600,
        Err(e) => return Err(e),
    };
    let name = path
        .file_name()
        .ok_or_else(|| io::Error::other("ruta sin nombre"))?
        .to_string_lossy()
        .into_owned();
    let mut random = [0u8; 8];
    getrandom::fill(&mut random).map_err(|e| io::Error::other(e.to_string()))?;
    let suffix: String = random.iter().map(|b| format!("{b:02x}")).collect();
    let tmp = dir.join(format!(".{name}.{suffix}.tmp"));
    let result = (|| {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&tmp)?;
        file.write_all(text.as_bytes())?;
        file.flush()?;
        file.sync_all()?;
        fs::set_permissions(&tmp, fs::Permissions::from_mode(mode))?;
        fs::rename(&tmp, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}
