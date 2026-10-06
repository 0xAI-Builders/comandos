//! Los cambios de modo poseen su transacción y un guardia durable fuera de SQLite.
use crate::{
    Error, Result,
    domains::catalog::domain,
    files::{FileLock, write_atomic},
};
use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior, params};
use std::{
    fs,
    path::{Path, PathBuf},
};
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    Legacy,
    Mirror,
    Unified,
    Sealed,
}
impl Mode {
    fn as_str(self) -> &'static str {
        match self {
            Self::Legacy => "legacy",
            Self::Mirror => "mirror",
            Self::Unified => "unified",
            Self::Sealed => "sealed",
        }
    }
}
pub fn mode_of(conn: Option<&Connection>, name: &str) -> Result<Mode> {
    let Some(conn) = conn else {
        return Ok(Mode::Legacy);
    };
    let mode: Option<String> = conn
        .query_row(
            "SELECT mode FROM domain_modes WHERE domain=?1",
            [name],
            |r| r.get(0),
        )
        .optional()?;
    match mode.as_deref() {
        None | Some("legacy") => Ok(Mode::Legacy),
        Some("mirror") => Ok(Mode::Mirror),
        Some("unified") => Ok(Mode::Unified),
        Some("sealed") => Ok(Mode::Sealed),
        _ => Err(Error::Validation("modo de dominio inválido".into())),
    }
}
fn check_domain(name: &str) -> Result<()> {
    if domain(name).is_none() {
        return Err(Error::Validation(format!("dominio desconocido: {name}")));
    }
    Ok(())
}
pub fn seal_guard_path(path: &Path, name: &str) -> Result<PathBuf> {
    check_domain(name)?;
    let mut os = path.as_os_str().to_os_string();
    os.push(format!(".sealed-{name}"));
    Ok(PathBuf::from(os))
}
fn mode_lock_path(path: &Path) -> PathBuf {
    let mut os = path.as_os_str().to_os_string();
    os.push(".domain-modes.lock");
    PathBuf::from(os)
}
pub(crate) fn connection_path(conn: &Connection) -> Option<PathBuf> {
    conn.path()
        .filter(|p| !p.is_empty() && *p != ":memory:")
        .map(PathBuf::from)
}
pub(crate) fn mode_lock(path: &Path) -> Result<FileLock> {
    use std::os::unix::fs::DirBuilderExt;
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(parent)?;
    }
    Ok(FileLock::exclusive(&mode_lock_path(path))?)
}
const GUARD: &[u8] = b"comandos-state-protocol-2:sealed\n";
fn guarded(path: &Path, name: &str) -> Result<bool> {
    let guard = seal_guard_path(path, name)?;
    match fs::symlink_metadata(&guard) {
        Ok(meta) if !meta.is_file() => Err(Error::Validation(format!(
            "{}: guardia de sellado inválido",
            guard.display()
        ))),
        Ok(_) => {
            if fs::read(&guard)? != GUARD {
                return Err(Error::Validation(format!(
                    "{}: guardia de sellado corrupto",
                    guard.display()
                )));
            }
            Ok(true)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e.into()),
    }
}
/// El llamador conserva el candado de modo hasta terminar el acceso al estado.
pub(crate) fn access_mode(
    home: &Path,
    db: Option<&Connection>,
    name: &str,
) -> Result<(Mode, Option<FileLock>)> {
    check_domain(name)?;
    if let Some(db) = db {
        validate_access(db)?;
    }
    let path = db
        .and_then(connection_path)
        .unwrap_or_else(|| super::unified_path(home));
    let lock = if db.is_some_and(|c| connection_path(c).is_none()) {
        None
    } else {
        Some(mode_lock(&path)?)
    };
    let sealed = guarded(&path, name)?;
    let mode = mode_of(db, name)?;
    if sealed && (db.is_none() || mode != Mode::Sealed) {
        return Err(Error::Validation(format!(
            "{name}: dominio sellado y base no disponible o modo sin confirmar"
        )));
    }
    if mode == Mode::Sealed && !sealed {
        return Err(Error::Validation(format!(
            "{name}: falta el guardia durable de sellado"
        )));
    }
    Ok((mode, lock))
}
pub fn set_mode(conn: &Connection, name: &str, mode: Mode, by: &str, now_ms: i64) -> Result<()> {
    check_domain(name)?;
    if by.is_empty() {
        return Err(Error::Validation("autor del cambio de modo vacío".into()));
    }
    if !conn.is_autocommit() {
        return Err(Error::Validation(
            "set_mode requiere una transacción propia confirmada".into(),
        ));
    }
    validate_access(conn)?;
    let path = connection_path(conn);
    if mode == Mode::Sealed && path.is_none() {
        return Err(Error::Validation(
            "sealed requiere una base en disco y guardia durable".into(),
        ));
    }
    let _lock = path.as_deref().map(mode_lock).transpose()?;
    let tx = Transaction::new_unchecked(conn, TransactionBehavior::Immediate)?;
    // El guardia se sincroniza antes de confirmar: un corte o fallo deja bloqueo conservador.
    if mode == Mode::Sealed
        && let Some(path) = &path
    {
        let guard = seal_guard_path(path, name)?;
        write_atomic(&guard, GUARD)?;
        sync_parent(&guard)?;
    }
    conn.execute("INSERT INTO domain_modes(domain,mode,changed_at_ms,changed_by) VALUES(?1,?2,?3,?4) ON CONFLICT(domain) DO UPDATE SET mode=excluded.mode,changed_at_ms=excluded.changed_at_ms,changed_by=excluded.changed_by",params![name,mode.as_str(),now_ms,by])?;
    tx.commit()?;
    // Solo un desellado explícito confirmado puede quitar el guardia.
    if mode != Mode::Sealed
        && let Some(path) = &path
    {
        let guard = seal_guard_path(path, name)?;
        match fs::remove_file(&guard) {
            Ok(()) => sync_parent(&guard)?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
    }
    Ok(())
}

fn validate_access(conn: &Connection) -> Result<()> {
    // Una instantánea anterior al sellado no puede habilitar una escritura legado.
    // Se comprueba antes del flock para evitar invertir el orden candado/transacción.
    if !conn.is_autocommit() {
        return Err(Error::Validation(
            "el handle requiere una conexión sin transacción del llamador".into(),
        ));
    }
    super::open::validate(conn)?;
    if let Some(path) = connection_path(conn) {
        use std::os::unix::fs::PermissionsExt;
        super::open::private_parent(&path)?;
        let metadata = fs::symlink_metadata(&path)?;
        if !metadata.is_file() || metadata.permissions().mode() & 0o7777 != 0o600 {
            return Err(Error::Validation(format!(
                "{}: la base única requiere un archivo regular 0600",
                path.display()
            )));
        }
    }
    Ok(())
}

fn sync_parent(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::File::open(parent)?.sync_all()?;
    }
    Ok(())
}
