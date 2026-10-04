//! Archivos JSON de `~/.claude/hooks` como los lee y escribe `bin/cc-dash`.
use comandos_core::json::{MAX_WORKSPACE_JSON_DEPTH, response_dumps, workspace_loads};
use serde_json::Value;
use std::{
    fs, io,
    io::Write,
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::Path,
};

pub enum Strict {
    /// `FileNotFoundError`.
    Missing,
    /// El Python cae en su `except` con certeza: JSON roto o texto con BOM
    /// (`json.loads` de un `str` que empieza por U+FEFF: «Unexpected UTF-8 BOM»).
    Unreadable,
    /// No se sabe con certeza qué leería el Python: otro error de E/S, bytes
    /// que no son UTF-8 (dependen de la codificación del proceso), sustitutos
    /// sueltos o anidamiento que el parser portado rechaza y `json` acepta.
    /// Quien lo reciba declina.
    Unsure,
    Value(Value),
}

/// `json.load(open(path))` del Python: lectura en modo texto (UTF-8), así que
/// un BOM o UTF-16 no se decodifican como en `json.loads(bytes)`.
pub fn read_json_strict(path: &Path) -> Strict {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Strict::Missing,
        Err(_) => return Strict::Unsure,
    };
    match std::str::from_utf8(&bytes) {
        Ok(text) => loads_strict(text),
        Err(_) => Strict::Unsure,
    }
}

/// `json.loads(texto)` sobre un `str` ya decodificado, con la misma
/// clasificación que `read_json_strict` (BOM → ilegible; sustitutos sueltos o
/// anidamiento profundo → incierto).
pub fn loads_strict(text: &str) -> Strict {
    if text.starts_with('\u{feff}') {
        return Strict::Unreadable;
    }
    match workspace_loads(text) {
        Ok(value) => Strict::Value(value),
        Err(_) if has_surrogate_escape(text) || deep(text) => Strict::Unsure,
        Err(_) => Strict::Unreadable,
    }
}

/// `\uD800`–`\uDFFF`: el `json` del Python los admite sueltos; el Rust no.
fn has_surrogate_escape(text: &str) -> bool {
    text.match_indices("\\u").any(|(i, _)| {
        let hex = text.as_bytes().get(i + 2..i + 4).unwrap_or_default();
        matches!(hex, [b'd' | b'D', b'8'..=b'9' | b'a'..=b'f' | b'A'..=b'F'])
    })
}

/// El límite de anidamiento del parser portado no es el de CPython.
fn deep(text: &str) -> bool {
    text.bytes().filter(|b| matches!(b, b'[' | b'{')).count() >= MAX_WORKSPACE_JSON_DEPTH
}

/// `write_json_file` (5090) → `write_file_atomic` (5063): temporal en el mismo
/// directorio, fsync, permisos del archivo previo (0600 si es nuevo) y rename.
/// Bloquea: llamar dentro de `spawn_blocking`.
pub fn write_json_atomic(path: &Path, value: &Value) -> io::Result<()> {
    let text = response_dumps(value).map_err(io::Error::other)?;
    write_text_atomic(path, &text)
}

/// `write_file_atomic` (5063) con texto ya formado.
/// Bloquea: llamar dentro de `spawn_blocking`.
pub fn write_text_atomic(path: &Path, text: &str) -> io::Result<()> {
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

/// `file_lock` (5168): `flock` exclusivo sobre `<ruta>.lock` (creado 0600),
/// el mismo que toman el Python y cc-app. Se pide sin esperar: `Ok(None)` si
/// otro lo tiene; quien llama declina antes de leer o escribir nada. Se suelta
/// al soltar el valor (cerrar el descriptor suelta el `flock`).
pub struct FileLock {
    _file: fs::File,
}

impl FileLock {
    pub fn try_acquire(path: &Path) -> io::Result<Option<FileLock>> {
        let mut name = path.as_os_str().to_owned();
        name.push(".lock");
        let lock = std::path::PathBuf::from(name);
        if let Some(dir) = lock.parent().filter(|d| !d.as_os_str().is_empty()) {
            fs::create_dir_all(dir)?;
        }
        let file = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .open(&lock)?;
        match file.try_lock() {
            Ok(()) => Ok(Some(FileLock { _file: file })),
            Err(fs::TryLockError::WouldBlock) => Ok(None),
            Err(fs::TryLockError::Error(e)) => Err(e),
        }
    }
}
