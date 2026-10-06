//! Toda escritura de archivos de la app. La ruta se vuelve a resolver en el
//! momento de escribir, componente a componente desde una raíz canónica con
//! `O_NOFOLLOW`: un enlace o un `..` metido después de `parse_args` no saca la
//! escritura de su raíz (TOCTOU). La sombra solo escribe su candado y su volcado.
use crate::config::{AppConfig, RunMode};
use nix::fcntl::{OFlag, open, openat, renameat};
use nix::sys::stat::{Mode, mkdirat};
use nix::unistd::unlinkat;
use std::ffi::{OsStr, OsString};
use std::io::Write;
use std::os::fd::OwnedFd;
use std::path::{Component, Path, PathBuf};

#[derive(Debug)]
pub enum GuardError {
    Shadow(PathBuf),
    Outside(PathBuf),
    Escape(PathBuf),
    Io(PathBuf, String),
}

#[derive(Debug, Clone)]
pub struct WriteGuard {
    mode: RunMode,
    /// (ruta configurada, ruta canónica).
    roots: Vec<(PathBuf, PathBuf)>,
    /// Archivos sueltos permitidos (candados, volcado de la sombra, log opt-in).
    files: Vec<PathBuf>,
    snapshot_history: Option<PathBuf>,
}

const DIR_FLAGS: OFlag = OFlag::O_DIRECTORY
    .union(OFlag::O_NOFOLLOW)
    .union(OFlag::O_CLOEXEC);

fn io(path: &Path, e: impl std::fmt::Display) -> GuardError {
    GuardError::Io(path.to_path_buf(), e.to_string())
}

fn normal_parts(rel: &Path, whole: &Path) -> Result<Vec<OsString>, GuardError> {
    rel.components()
        .map(|c| match c {
            Component::Normal(n) => Ok(n.to_os_string()),
            _ => Err(GuardError::Escape(whole.to_path_buf())),
        })
        .collect()
}

/// Abre `dir` desde `/` sin seguir ningún enlace.
fn open_dir_nofollow(dir: &Path) -> Result<OwnedFd, GuardError> {
    let mut fd = open("/", DIR_FLAGS, Mode::empty()).map_err(|e| io(dir, e))?;
    let rel = dir
        .strip_prefix("/")
        .map_err(|_| GuardError::Escape(dir.to_path_buf()))?;
    for part in normal_parts(rel, dir)? {
        fd = openat(&fd, part.as_os_str(), DIR_FLAGS, Mode::empty())
            .map_err(|_| GuardError::Escape(dir.to_path_buf()))?;
    }
    Ok(fd)
}

impl WriteGuard {
    /// `display` decide el nombre del candado de instancia única (T7).
    pub fn from_config(cfg: &AppConfig, display: &str) -> WriteGuard {
        let lock = cfg.lock_file_name(display);
        let uid = nix::unistd::getuid().as_raw();
        let mut files = Vec::new();
        match cfg.mode() {
            RunMode::Shadow => {
                files.push(cfg.runtime_dir().join(&lock));
                files.push(cfg.layout_dump_path());
            }
            RunMode::Live => {
                // Candados de _base del Python (52): XDG_RUNTIME_DIR, /tmp/comandos-<uid>, /tmp.
                files.push(cfg.runtime_dir().join(&lock));
                files.push(PathBuf::from(format!("/tmp/comandos-{uid}")).join(&lock));
                files.push(PathBuf::from("/tmp").join(&lock));
                // Log opt-in de snip_log (472): solo se anexa si ya existe.
                files.push(PathBuf::from("/tmp/cc-app-snip.log"));
                for name in [
                    "app-tabs.json",
                    "app-tabs.json.lock",
                    "app-tabs-history.json",
                    "app-tabs-history.json.lock",
                    "app-tabs-snapshot.json",
                    "app-sessions-v2.json",
                    "app-sessions-v2.json.bak",
                    "app-sessions-v2.json.history",
                    "app-tab-active.json",
                    "app-tab-models.json",
                    "app-extension-shelf.json",
                    "app-layout.json",
                    "app-pane-position.json",
                    "app-focus.json",
                    "app-tab-close.json",
                    "app-tab-open.json",
                    "app-command.json",
                    "snippets.json",
                    "acp-panes.json",
                ] {
                    files.push(cfg.hooks_dir().join(name));
                }
            }
            RunMode::Sandbox => {}
        }
        let roots = match cfg.mode() {
            // Ya resueltas y validadas por parse_args (0700, del usuario, fuera de estado real).
            RunMode::Sandbox => [cfg.sandbox_root(), cfg.sandbox_temp()]
                .into_iter()
                .flatten()
                .map(|p| (p.to_path_buf(), p.to_path_buf()))
                .collect(),
            RunMode::Shadow => Vec::new(),
            RunMode::Live => Vec::new(),
        };
        WriteGuard {
            mode: cfg.mode(),
            roots,
            files,
            snapshot_history: (cfg.mode() == RunMode::Live)
                .then(|| cfg.hooks_dir().join("app-sessions-v2.json.history")),
        }
    }

    /// (descriptor del directorio padre, nombre final) tras recorrer sin enlaces.
    fn resolve_parent(
        &self,
        path: &Path,
        create_dirs: bool,
    ) -> Result<(OwnedFd, OsString), GuardError> {
        if path
            .components()
            .any(|c| matches!(c, Component::ParentDir | Component::CurDir))
        {
            return Err(GuardError::Escape(path.to_path_buf()));
        }
        let name = path
            .file_name()
            .ok_or_else(|| GuardError::Escape(path.to_path_buf()))?
            .to_os_string();
        let parent = path
            .parent()
            .ok_or_else(|| GuardError::Escape(path.to_path_buf()))?;
        if self.files.iter().any(|f| f == path) {
            return Ok((open_dir_nofollow(parent)?, name));
        }
        if self.snapshot_history.as_deref() == Some(parent)
            && name
                .to_str()
                .and_then(|name| name.strip_suffix(".json"))
                .is_some_and(|name| name.len() == 12 && name.bytes().all(|b| b.is_ascii_digit()))
        {
            return Ok((open_dir_nofollow(parent)?, name));
        }
        if self.mode == RunMode::Shadow {
            return Err(GuardError::Shadow(path.to_path_buf()));
        }
        let (configured, canonical) = self
            .roots
            .iter()
            .find(|(conf, canon)| parent.starts_with(conf) || parent.starts_with(canon))
            .ok_or_else(|| GuardError::Outside(path.to_path_buf()))?;
        let rel = parent
            .strip_prefix(configured)
            .or_else(|_| parent.strip_prefix(canonical))
            .map_err(|_| GuardError::Outside(path.to_path_buf()))?;
        let mut fd = open_dir_nofollow(canonical)?;
        for part in normal_parts(rel, path)? {
            fd = match openat(&fd, part.as_os_str(), DIR_FLAGS, Mode::empty()) {
                Ok(next) => next,
                Err(nix::errno::Errno::ENOENT) if create_dirs => {
                    mkdirat(&fd, part.as_os_str(), Mode::from_bits_truncate(0o700))
                        .map_err(|e| io(path, e))?;
                    openat(&fd, part.as_os_str(), DIR_FLAGS, Mode::empty())
                        .map_err(|_| GuardError::Escape(path.to_path_buf()))?
                }
                Err(_) => return Err(GuardError::Escape(path.to_path_buf())),
            };
        }
        Ok((fd, name))
    }

    pub fn check(&self, path: &Path) -> Result<(), GuardError> {
        self.resolve_parent(path, false).map(|_| ())
    }

    /// Temporal `<prefijo><aleatorio>.tmp` en el mismo directorio + `renameat`
    /// (como `tempfile.mkstemp` + `os.replace` del Python).
    pub fn write_atomic(
        &self,
        path: &Path,
        bytes: &[u8],
        tmp_prefix: &str,
    ) -> Result<(), GuardError> {
        if !plain_name(OsStr::new(tmp_prefix)) {
            return Err(GuardError::Escape(path.to_path_buf()));
        }
        let (dir, name) = self.resolve_parent(path, false)?;
        let mut rnd = [0u8; 8];
        getrandom::fill(&mut rnd).map_err(|e| io(path, e))?;
        let tmp: OsString = format!(
            "{tmp_prefix}{}.tmp",
            rnd.iter().map(|b| format!("{b:02x}")).collect::<String>()
        )
        .into();
        let flags =
            OFlag::O_CREAT | OFlag::O_EXCL | OFlag::O_WRONLY | OFlag::O_NOFOLLOW | OFlag::O_CLOEXEC;
        let fd = openat(
            &dir,
            tmp.as_os_str(),
            flags,
            Mode::from_bits_truncate(0o600),
        )
        .map_err(|e| io(path, e))?;
        let mut file = std::fs::File::from(fd);
        let written = file.write_all(bytes).and_then(|()| file.sync_all());
        let renamed = written.map_err(|e| io(path, e)).and_then(|()| {
            renameat(&dir, tmp.as_os_str(), &dir, name.as_os_str()).map_err(|e| io(path, e))
        });
        if renamed.is_err() {
            let _ = unlinkat(
                &dir,
                tmp.as_os_str(),
                nix::unistd::UnlinkatFlags::NoRemoveDir,
            );
        }
        renamed
    }

    /// Anexa solo si el archivo ya existe (`snip_log`, 495). `Ok(false)` si no existe.
    pub fn append_if_exists(&self, path: &Path, bytes: &[u8]) -> Result<bool, GuardError> {
        let (dir, name) = self.resolve_parent(path, false)?;
        let flags = OFlag::O_WRONLY | OFlag::O_APPEND | OFlag::O_NOFOLLOW | OFlag::O_CLOEXEC;
        match openat(&dir, name.as_os_str(), flags, Mode::empty()) {
            Ok(fd) => std::fs::File::from(fd)
                .write_all(bytes)
                .map(|()| true)
                .map_err(|e| io(path, e)),
            Err(nix::errno::Errno::ENOENT) => Ok(false),
            Err(e) => Err(io(path, e)),
        }
    }

    /// Consumir un archivo IPC (`on_app_command`, 8968). Nunca en sombra.
    pub fn remove_file(&self, path: &Path) -> Result<(), GuardError> {
        let (dir, name) = self.resolve_parent(path, false)?;
        unlinkat(
            &dir,
            name.as_os_str(),
            nix::unistd::UnlinkatFlags::NoRemoveDir,
        )
        .map_err(|e| io(path, e))
    }

    /// `os.makedirs(..., exist_ok=True)` dentro de una raíz, sin seguir enlaces.
    pub fn create_dir_all(&self, path: &Path, mode: u32) -> Result<(), GuardError> {
        let (dir, name) = self.resolve_parent(path, true)?;
        match mkdirat(&dir, name.as_os_str(), Mode::from_bits_truncate(mode)) {
            Ok(()) | Err(nix::errno::Errno::EEXIST) => {}
            Err(e) => return Err(io(path, e)),
        }
        openat(&dir, name.as_os_str(), DIR_FLAGS, Mode::empty())
            .map(|_| ())
            .map_err(|_| GuardError::Escape(path.to_path_buf()))
    }

    /// Reserva fechada exclusiva, sin resolver enlaces fuera de las raíces permitidas.
    pub fn reserve_quick_directory(
        &self,
        base: &Path,
        now: chrono::DateTime<chrono::FixedOffset>,
    ) -> Result<PathBuf, GuardError> {
        let stem = comandos_runtime::quick_terminal::directory_stem(now);
        // El descriptor del padre permanece anclado durante todas las colisiones.
        let (dir, _) = self.resolve_parent(&base.join(&stem), true)?;
        let mut suffix = 1u64;
        loop {
            let name = if suffix == 1 {
                stem.clone()
            } else {
                format!("{stem}-{suffix}")
            };
            let path = base.join(&name);
            match mkdirat(&dir, name.as_str(), Mode::from_bits_truncate(0o700)) {
                Ok(()) => return Ok(path),
                Err(nix::errno::Errno::EEXIST) => {
                    suffix = suffix
                        .checked_add(1)
                        .ok_or_else(|| io(base, "sufijos de carpeta agotados"))?;
                }
                Err(error) => return Err(io(&path, error)),
            }
        }
    }

    /// Abre (creándolo) un archivo de candado para `flock`.
    pub fn open_lock(&self, path: &Path) -> Result<std::fs::File, GuardError> {
        let (dir, name) = self.resolve_parent(path, false)?;
        let flags = OFlag::O_CREAT | OFlag::O_WRONLY | OFlag::O_NOFOLLOW | OFlag::O_CLOEXEC;
        openat(
            &dir,
            name.as_os_str(),
            flags,
            Mode::from_bits_truncate(0o600),
        )
        .map(std::fs::File::from)
        .map_err(|e| io(path, e))
    }

    /// A minute archive is created once and cannot overwrite another generation.
    pub fn archive_once(&self, path: &Path, bytes: &[u8]) -> Result<bool, GuardError> {
        use std::io::Write;
        let (dir, name) = self.resolve_parent(path, false)?;
        let fd = match openat(
            &dir,
            name.as_os_str(),
            OFlag::O_CREAT | OFlag::O_EXCL | OFlag::O_WRONLY | OFlag::O_NOFOLLOW | OFlag::O_CLOEXEC,
            Mode::from_bits_truncate(0o600),
        ) {
            Ok(fd) => fd,
            Err(nix::errno::Errno::EEXIST) => return Ok(false),
            Err(error) => return Err(io(path, error)),
        };
        let mut file = std::fs::File::from(fd);
        file.write_all(bytes)
            .and_then(|()| file.sync_all())
            .map_err(|error| io(path, error))?;
        Ok(true)
    }
}

/// `true` si `name` es un nombre de componente sin separadores (para los llamadores
/// que componen nombres de archivo a partir de datos externos).
pub fn plain_name(name: &OsStr) -> bool {
    let s = name.as_encoded_bytes();
    !s.is_empty() && !s.contains(&b'/') && s != b"." && s != b".."
}
