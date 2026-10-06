//! Bytes y flock compartidos por el tablero y los adaptadores de dominio.
use std::{
    fs, io,
    io::Write,
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::Path,
};

/// Temporal exclusivo en el mismo directorio, fsync y rename; conserva el modo.
pub fn write_atomic(path: &Path, body: &[u8]) -> io::Result<()> {
    let dir = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    fs::create_dir_all(dir)?;
    let mode = match fs::metadata(path) {
        Ok(meta) => meta.permissions().mode() & 0o7777,
        Err(error) if error.kind() == io::ErrorKind::NotFound => 0o600,
        Err(error) => return Err(error),
    };
    let name = path
        .file_name()
        .ok_or_else(|| io::Error::other("ruta sin nombre"))?
        .to_string_lossy();
    let mut random = [0u8; 8];
    getrandom::fill(&mut random).map_err(|error| io::Error::other(error.to_string()))?;
    let suffix: String = random.iter().map(|byte| format!("{byte:02x}")).collect();
    let tmp = dir.join(format!(".{name}.{suffix}.tmp"));
    let result = (|| {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&tmp)?;
        file.write_all(body)?;
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

/// El argumento es la ruta completa del candado, incluido `.lock`.
pub struct FileLock {
    _file: fs::File,
}
impl FileLock {
    fn open(path: &Path) -> io::Result<fs::File> {
        if let Some(dir) = path.parent().filter(|dir| !dir.as_os_str().is_empty()) {
            fs::create_dir_all(dir)?;
        }
        fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .open(path)
    }
    pub fn exclusive(path: &Path) -> io::Result<Self> {
        let file = Self::open(path)?;
        file.lock()?;
        Ok(Self { _file: file })
    }
    pub fn try_exclusive(path: &Path) -> io::Result<Option<Self>> {
        let file = Self::open(path)?;
        match file.try_lock() {
            Ok(()) => Ok(Some(Self { _file: file })),
            Err(fs::TryLockError::WouldBlock) => Ok(None),
            Err(fs::TryLockError::Error(error)) => Err(error),
        }
    }
}
