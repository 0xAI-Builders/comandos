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
impl Drop for FileLock {
    fn drop(&mut self) {
        // A concurrent fork can retain the same open file description until
        // exec. Closing our descriptor alone would extend the lock's lifetime.
        let _ = self._file.unlock();
    }
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
    pub fn shared(path: &Path) -> io::Result<Self> {
        let file = Self::open(path)?;
        file.lock_shared()?;
        Ok(Self { _file: file })
    }
    pub fn try_shared(path: &Path) -> io::Result<Option<Self>> {
        let file = Self::open(path)?;
        match file.try_lock_shared() {
            Ok(()) => Ok(Some(Self { _file: file })),
            Err(fs::TryLockError::WouldBlock) => Ok(None),
            Err(fs::TryLockError::Error(error)) => Err(error),
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::{BufRead, BufReader},
        os::unix::fs::DirBuilderExt,
        process::{Child, Command, Stdio},
    };

    struct OwnedChild(Child);
    impl Drop for OwnedChild {
        fn drop(&mut self) {
            if matches!(self.0.try_wait(), Ok(None)) {
                let _ = self.0.kill();
            }
            let _ = self.0.wait();
        }
    }

    #[test]
    fn dropping_lock_releases_it_while_an_owned_child_retains_the_descriptor() {
        let root =
            std::env::temp_dir().join(format!("comandos-lock-inherited-{}", std::process::id()));
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&root)
            .unwrap();
        let path = root.join("mode.lock");
        let lock = FileLock::exclusive(&path).unwrap();
        let mut command = Command::new("/bin/sh");
        command
            .args(["-c", "printf 'ready\\n'; exec /bin/sleep 30"])
            .env_clear()
            .stdin(Stdio::from(lock._file.try_clone().unwrap()))
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        for (key, name) in [
            ("HOME", "home"),
            ("XDG_CONFIG_HOME", "config"),
            ("XDG_DATA_HOME", "data"),
            ("XDG_CACHE_HOME", "cache"),
            ("XDG_STATE_HOME", "state"),
            ("XDG_RUNTIME_DIR", "run"),
            ("TMPDIR", "tmp"),
            ("TMP", "tmp"),
            ("TEMP", "tmp"),
        ] {
            let dir = root.join(name);
            fs::DirBuilder::new()
                .recursive(true)
                .mode(0o700)
                .create(&dir)
                .unwrap();
            command.env(key, dir);
        }
        let mut child = OwnedChild(command.spawn().unwrap());
        let mut ready = String::new();
        BufReader::new(child.0.stdout.take().unwrap())
            .read_line(&mut ready)
            .unwrap();
        assert_eq!(ready, "ready\n");
        assert!(FileLock::try_exclusive(&path).unwrap().is_none());
        drop(lock);
        assert!(child.0.try_wait().unwrap().is_none());
        assert!(
            FileLock::try_exclusive(&path).unwrap().is_some(),
            "a child retaining the file descriptor must not extend the owner's lock"
        );
        drop(child);
        fs::remove_dir_all(root).unwrap();
    }
}
