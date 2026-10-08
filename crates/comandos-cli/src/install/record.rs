//! Registro de rollback por nombre: `FILE:<ruta>`, `ABSENT` o `LINK:<destino>`, en bytes crudos.
use std::{
    ffi::OsStr,
    fs,
    os::unix::ffi::OsStrExt,
    path::{Path, PathBuf},
};

#[derive(Debug, PartialEq, Eq)]
pub enum Record {
    /// Había un archivo real, respaldado en esta ruta.
    File(PathBuf),
    /// No existía nada en la ruta.
    Absent,
    /// Era un symlink con este destino.
    Link(PathBuf),
}

pub fn path(home: &Path, name: &str) -> PathBuf {
    home.join(".local/share/comandos/rollback")
        .join(format!("{name}.target"))
}

/// Exact last-installed alias target, separate from the unchanged legacy receipt.
pub fn installed_path(home: &Path, name: &str) -> PathBuf {
    path(home, name).with_extension("installed")
}
pub fn write_installed(home: &Path, name: &str, target: &Path) -> Result<(), String> {
    let path = installed_path(home, name);
    super::release::check_app_parents(&path)?;
    match path.symlink_metadata() {
        Ok(meta) if !meta.is_file() => {
            return Err(format!("{} no es un recibo regular", path.display()));
        }
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e.to_string()),
        _ => {}
    }
    comandos_store::files::write_atomic(&path, target.as_os_str().as_bytes())
        .map_err(|e| e.to_string())
}
pub fn installed(home: &Path, name: &str) -> Result<Option<PathBuf>, String> {
    let path = installed_path(home, name);
    super::release::check_app_parents(&path)?;
    match path.symlink_metadata() {
        Ok(meta) if meta.is_file() => {
            let raw = fs::read(&path).map_err(|e| e.to_string())?;
            if raw.is_empty() {
                return Err("recibo instalado vacío".into());
            }
            Ok(Some(PathBuf::from(OsStr::from_bytes(&raw))))
        }
        Ok(_) => Err(format!("{} no es un recibo regular", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}

fn encode(rec: &Record) -> Vec<u8> {
    let mut out = match rec {
        Record::File(p) => [b"FILE:".as_slice(), p.as_os_str().as_bytes()].concat(),
        Record::Absent => b"ABSENT".to_vec(),
        Record::Link(p) => [b"LINK:".as_slice(), p.as_os_str().as_bytes()].concat(),
    };
    out.push(b'\n');
    out
}

fn decode(raw: &[u8]) -> Result<Record, String> {
    let raw = raw.strip_suffix(b"\n").unwrap_or(raw);
    let from = |b: &[u8]| PathBuf::from(OsStr::from_bytes(b));
    if raw == b"ABSENT" {
        Ok(Record::Absent)
    } else if let Some(p) = raw.strip_prefix(b"FILE:") {
        Ok(Record::File(from(p)))
    } else if let Some(p) = raw.strip_prefix(b"LINK:") {
        Ok(Record::Link(from(p)))
    } else {
        Err("registro de rollback desconocido".into())
    }
}

/// Escritura atómica: `<name>.target.tmp` y `rename`.
pub fn write(home: &Path, name: &str, rec: &Record) -> Result<(), String> {
    let dest = path(home, name);
    let tmp = dest.with_file_name(format!("{name}.target.tmp"));
    fs::write(&tmp, encode(rec))
        .map_err(|e| format!("no se pudo escribir el registro {}: {e}", tmp.display()))?;
    fs::rename(&tmp, &dest)
        .map_err(|e| format!("no se pudo mover el registro a {}: {e}", dest.display()))
}

pub fn read(home: &Path, name: &str) -> Result<Record, String> {
    let p = path(home, name);
    let raw = fs::read(&p).map_err(|e| {
        format!(
            "sin registro de rollback para {name} ({}): {e}",
            p.display()
        )
    })?;
    decode(&raw)
}

pub fn exists(home: &Path, name: &str) -> bool {
    path(home, name).exists()
}
