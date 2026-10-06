//! Private, non-overwriting Node backups and guarded exact rollback.
use super::config;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{self, Read, Write},
    os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
};
pub(super) fn parents(path: &Path) -> Result<(), String> {
    if !path.is_absolute()
        || path
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return Err(format!(
            "ruta absoluta sin '..' requerida: {}",
            path.display()
        ));
    }
    for dir in path.parent().ok_or("ruta sin directorio")?.ancestors() {
        match dir.symlink_metadata() {
            Ok(m) if m.is_dir() => {}
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Ok(_) => return Err(format!("{} no es un directorio regular", dir.display())),
            Err(e) => return Err(format!("{}: {e}", dir.display())),
        }
    }
    Ok(())
}
pub(super) fn private_regular(path: &Path) -> Result<Vec<u8>, String> {
    parents(path)?;
    let meta = path
        .symlink_metadata()
        .map_err(|e| format!("{}: {e}", path.display()))?;
    if !meta.is_file() || meta.permissions().mode() & 0o7777 != 0o600 {
        return Err(format!("{} requiere archivo regular 0600", path.display()));
    }
    let mut f = fs::OpenOptions::new()
        .read(true)
        .custom_flags(nix::libc::O_NOFOLLOW | nix::libc::O_NONBLOCK | nix::libc::O_NOCTTY)
        .open(path)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    let held = f.metadata().map_err(|e| e.to_string())?;
    if !held.is_file()
        || held.permissions().mode() & 0o7777 != 0o600
        || (meta.dev(), meta.ino()) != (held.dev(), held.ino())
    {
        return Err("archivo cambió durante lectura".into());
    }
    let mut bytes = Vec::new();
    Read::by_ref(&mut f)
        .take(1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > 1024 * 1024 {
        return Err("archivo supera 1 MiB".into());
    }
    Ok(bytes)
}
fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
pub(super) fn destination(home: &Path) -> Result<PathBuf, String> {
    let root = home.join(".local/share/comandos/backups");
    parents(&root.join("placeholder"))?;
    match root.symlink_metadata() {
        Ok(m) if m.is_dir() && m.permissions().mode() & 0o7777 == 0o700 => {}
        Ok(_) => {
            return Err(format!(
                "{} requiere directorio regular 0700",
                root.display()
            ));
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => return Err(format!("{}: {e}", root.display())),
    }
    let stamp = chrono::Utc::now().format("%Y%m%d-%H%M%S-%f").to_string();
    for suffix in 0..10000 {
        let name = if suffix == 0 {
            stamp.clone()
        } else {
            format!("{stamp}-{suffix}")
        };
        let path = root.join(name).join("tailscale-serve.json");
        match path
            .parent()
            .ok_or("backup sin directorio")?
            .symlink_metadata()
        {
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(path),
            Ok(_) => {}
            Err(e) => return Err(format!("{}: {e}", path.display())),
        }
    }
    Err("no se encontró destino libre de backup".into())
}
fn write_new(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    file.write_all(bytes)
        .and_then(|()| file.sync_all())
        .map_err(|e| format!("{}: {e}", path.display()))
}
pub(super) fn save(path: &Path, before: &[u8], after: &Value) -> Result<(), String> {
    parents(path)?;
    let dir = path.parent().ok_or("backup sin directorio")?;
    let root = dir.parent().ok_or("backup sin raíz")?;
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(root)
        .map_err(|e| format!("{}: {e}", root.display()))?;
    if root
        .symlink_metadata()
        .map_err(|e| e.to_string())?
        .permissions()
        .mode()
        & 0o7777
        != 0o700
    {
        return Err("raíz de backups dejó de ser privada".into());
    }
    fs::DirBuilder::new()
        .mode(0o700)
        .create(dir)
        .map_err(|e| format!("{}: {e}", dir.display()))?;
    let after = serde_json::to_vec(after).map_err(|e| e.to_string())?;
    write_new(path, before)?;
    write_new(&dir.join("tailscale-serve.after.json"), &after)?;
    let manifest = json!({"version":1,"artifact":"comandos-mobile","before_sha256":hash(before),"after_sha256":hash(&after)});
    write_new(
        &dir.join("mobile.json"),
        &serde_json::to_vec(&manifest).map_err(|e| e.to_string())?,
    )?;
    fs::File::open(dir)
        .and_then(|f| f.sync_all())
        .map_err(|e| e.to_string())
}
pub(super) struct Restore {
    pub before: Vec<u8>,
    pub before_config: Value,
    pub after: Value,
}
pub(super) fn restore(home: &Path, path: &Path) -> Result<Restore, String> {
    let root = home.join(".local/share/comandos/backups");
    let dir = path.parent().ok_or("backup sin directorio")?;
    if path.file_name().is_none_or(|n| n != "tailscale-serve.json")
        || dir.parent() != Some(root.as_path())
    {
        return Err("rollback requiere backup propio de comandos mobile".into());
    }
    parents(path)?;
    for p in [root.as_path(), dir] {
        if !p
            .symlink_metadata()
            .is_ok_and(|m| m.is_dir() && m.permissions().mode() & 0o7777 == 0o700)
        {
            return Err(format!("{} requiere directorio privado 0700", p.display()));
        }
    }
    let before = private_regular(path)?;
    let after = private_regular(&dir.join("tailscale-serve.after.json"))?;
    let manifest: Value = serde_json::from_slice(&private_regular(&dir.join("mobile.json"))?)
        .map_err(|e| e.to_string())?;
    if manifest.as_object().is_none_or(|m| m.len() != 4)
        || manifest["version"] != 1
        || manifest["artifact"] != "comandos-mobile"
        || manifest["before_sha256"] != hash(&before)
        || manifest["after_sha256"] != hash(&after)
    {
        return Err("manifiesto de rollback desconocido o alterado".into());
    }
    let before_config = config::parse(&before)?;
    config::safe_to_change(&before_config)?;
    let after = config::parse(&after)?;
    config::safe_to_change(&after)?;
    Ok(Restore {
        before,
        before_config,
        after,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn collision_never_overwrites_original_or_existing_metadata() {
        let home =
            std::env::temp_dir().join(format!("mobile-backup-collision-{}", std::process::id()));
        fs::DirBuilder::new().mode(0o700).create(&home).unwrap();
        let path = home.join(".local/share/comandos/backups/fixed-date/tailscale-serve.json");
        save(&path, b"{}\n", &json!({"TCP":{"443":{"HTTPS":true}}})).unwrap();
        let before = fs::read(&path).unwrap();
        let manifest = fs::read(path.parent().unwrap().join("mobile.json")).unwrap();
        assert!(save(&path, b"changed", &json!({})).is_err());
        assert_eq!(fs::read(&path).unwrap(), before);
        assert_eq!(
            fs::read(path.parent().unwrap().join("mobile.json")).unwrap(),
            manifest
        );
        fs::remove_dir_all(home).unwrap();
    }
}
