//! Pure platform choices shared by the CLI and daemon. No GTK/AppKit.
use std::{
    io,
    path::{Path, PathBuf},
};
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Host {
    Linux,
    Macos,
}
pub const fn host() -> Host {
    if cfg!(target_os = "macos") {
        Host::Macos
    } else {
        Host::Linux
    }
}
pub fn runtime_path(
    host: Host,
    xdg: Option<&Path>,
    tmp: Option<&Path>,
    uid: u32,
) -> io::Result<Option<PathBuf>> {
    if host == Host::Linux {
        return Ok(xdg.map(Path::to_path_buf));
    }
    let xdg = xdg.filter(|p| !p.as_os_str().is_empty());
    let path = match xdg {
        Some(path) => path.to_path_buf(),
        None => tmp
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("/tmp"))
            .join(format!("comandos-{uid}")),
    };
    if !path.is_absolute()
        || path
            .components()
            .any(|p| matches!(p, std::path::Component::ParentDir))
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "runtime path must be absolute without '..'",
        ));
    }
    Ok(Some(path))
}
pub fn runtime_directory_from_env(create: bool) -> io::Result<Option<PathBuf>> {
    let xdg = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from);
    let tmp = std::env::var_os("TMPDIR").map(PathBuf::from);
    let path = runtime_path(
        host(),
        xdg.as_deref(),
        tmp.as_deref(),
        nix::unistd::getuid().as_raw(),
    )?;
    prepare_runtime_directory(host(), path.as_deref(), create)?;
    Ok(path)
}

/// Prepare only Darwin's directory. Readonly operations never create it;
/// Linux retains the caller's original path and directory effects.
pub fn prepare_runtime_directory(host: Host, path: Option<&Path>, create: bool) -> io::Result<()> {
    if host == Host::Macos
        && let Some(path) = path
    {
        use std::os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt};
        if create {
            match std::fs::DirBuilder::new().mode(0o700).create(path) {
                Ok(()) => {}
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
                Err(e) => return Err(e),
            }
        }
        match path.symlink_metadata() {
            Ok(m)
                if m.is_dir()
                    && m.uid() == nix::unistd::getuid().as_raw()
                    && m.permissions().mode() & 0o7777 == 0o700 => {}
            Err(e) if !create && e.kind() == io::ErrorKind::NotFound => {}
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "runtime directory must be owned, a directory and 0700",
                ));
            }
        }
    }
    Ok(())
}

/// Same Linux hostname source; Darwin has no procfs and uses uname's nodename.
pub fn hostname() -> io::Result<String> {
    #[cfg(target_os = "macos")]
    {
        nix::sys::utsname::uname()
            .map(|u| u.nodename().to_string_lossy().into_owned())
            .map_err(io::Error::from)
    }
    #[cfg(not(target_os = "macos"))]
    {
        std::fs::read_to_string("/proc/sys/kernel/hostname")
    }
}
