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
pub(crate) fn guarded(path: &Path, name: &str) -> Result<bool> {
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
    set_mode_locked(conn, name, mode, by, now_ms)
}

// El llamador conserva el candado de modo y ha validado el acceso.
pub(crate) fn set_mode_locked(
    conn: &Connection,
    name: &str,
    mode: Mode,
    by: &str,
    now_ms: i64,
) -> Result<()> {
    check_domain(name)?;
    let path = connection_path(conn);
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

/// El migrador solo prepara legacy; nunca desella ni degrada una autoridad nueva.
pub(crate) fn prepare_mirror(
    home: &Path,
    conn: &Connection,
    name: &str,
    now_ms: i64,
) -> Result<Mode> {
    let (mode, _lock) = access_mode(home, Some(conn), name)?;
    if mode != Mode::Legacy {
        return Ok(mode);
    }
    let tx = Transaction::new_unchecked(conn, TransactionBehavior::Immediate)?;
    conn.execute("INSERT INTO domain_modes(domain,mode,changed_at_ms,changed_by) VALUES(?1,'mirror',?2,'state migrate') ON CONFLICT(domain) DO UPDATE SET mode='mirror',changed_at_ms=excluded.changed_at_ms,changed_by=excluded.changed_by WHERE domain_modes.mode='legacy'", params![name,now_ms])?;
    tx.commit()?;
    Ok(Mode::Mirror)
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

/// Mode authority stays at the source path even when SQLite reads a WAL copy.
/// Existing mode locks are shared, read-only and nonblocking; none is created.
pub(crate) fn with_readonly_access<T>(
    home: &Path,
    name: &str,
    body: impl FnOnce(Mode, Option<&Connection>) -> Result<T>,
) -> Result<T> {
    check_domain(name)?;
    let path = super::unified_path(home);
    if !home.is_absolute() || !path.is_absolute() {
        return Err(Error::Validation(
            "lectura de estado requiere rutas absolutas".into(),
        ));
    }
    let controls = ReadControls::capture(&path)?;
    let lock_path = mode_lock_path(&path);
    let _lock = if let Some(expected) = read_identity(&lock_path)? {
        let file = fs::File::open(&lock_path)?;
        if read_stamp(&file.metadata()?) != expected {
            return Err(Error::Validation(
                "candado de modo cambió durante lectura".into(),
            ));
        }
        match file.try_lock_shared() {
            Ok(()) => {}
            Err(fs::TryLockError::WouldBlock) => return Err(Error::ModeBusy),
            Err(fs::TryLockError::Error(error)) => return Err(error.into()),
        }
        Some(file)
    } else {
        None
    };
    controls.check(&path)?;
    let result = if read_identity(&path)?.is_some() {
        super::with_readonly_unified(home, &path, |conn| {
            body(mode_of(Some(conn), name)?, Some(conn))
        })
    } else {
        // No base cannot override any durable seal, including another domain.
        for domain in crate::domains::catalog::DOMAINS {
            if guarded(&path, domain.name)? {
                return Err(Error::Validation(
                    "dominio sellado y base no disponible".into(),
                ));
            }
        }
        if read_identity(&read_suffix(&path, "-wal"))?.is_some()
            || read_identity(&read_suffix(&path, "-journal"))?.is_some()
        {
            return Err(Error::Validation(
                "base ausente con WAL/journal presente".into(),
            ));
        }
        body(Mode::Legacy, None)
    };
    controls.check(&path)?;
    result
}
type ReadStamp = (u64, u64, u64, u32, i64, i64, i64, i64);
fn read_stamp(meta: &fs::Metadata) -> ReadStamp {
    use std::os::unix::fs::MetadataExt;
    (
        meta.dev(),
        meta.ino(),
        meta.len(),
        meta.mode(),
        meta.mtime(),
        meta.mtime_nsec(),
        meta.ctime(),
        meta.ctime_nsec(),
    )
}
fn read_identity(path: &Path) -> Result<Option<ReadStamp>> {
    match fs::symlink_metadata(path) {
        Ok(m) if m.is_file() => Ok(Some(read_stamp(&m))),
        Ok(_) => Err(Error::Validation(format!(
            "{}: control de estado no es archivo regular",
            path.display()
        ))),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}
fn read_suffix(path: &Path, tail: &str) -> PathBuf {
    let mut p = path.as_os_str().to_os_string();
    p.push(tail);
    p.into()
}
#[derive(PartialEq, Eq)]
struct ReadControls {
    files: Vec<Option<ReadStamp>>,
    parent: (PathBuf, ReadStamp),
}
impl ReadControls {
    fn capture(path: &Path) -> Result<Self> {
        let mut paths = vec![
            path.to_owned(),
            read_suffix(path, "-wal"),
            read_suffix(path, "-journal"),
            mode_lock_path(path),
        ];
        for domain in crate::domains::catalog::DOMAINS {
            paths.push(seal_guard_path(path, domain.name)?);
        }
        let files = paths
            .iter()
            .map(|p| read_identity(p))
            .collect::<Result<_>>()?;
        let mut parent = path
            .parent()
            .ok_or_else(|| Error::Validation("base sin directorio".into()))?;
        let meta = loop {
            match fs::metadata(parent) {
                Ok(m) => break m,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    parent = parent.parent().ok_or(e)?;
                }
                Err(e) => return Err(e.into()),
            }
        };
        Ok(Self {
            files,
            // An absent lock gives no writer exclusion. Parent mtime/ctime
            // also detects controls created and removed between both samples.
            parent: (parent.to_owned(), read_stamp(&meta)),
        })
    }
    fn check(&self, path: &Path) -> Result<()> {
        if *self != Self::capture(path)? {
            return Err(Error::Validation(
                "autoridad DB/modo cambió durante lectura".into(),
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod readonly_tests {
    use super::*;
    use std::os::unix::fs::DirBuilderExt;
    #[test]
    fn readonly_missing_controls_reject_transient_database_creation() {
        let home =
            std::env::temp_dir().join(format!("mode-readonly-transient-{}", std::process::id()));
        let path = super::super::unified_path(&home);
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(path.parent().unwrap())
            .unwrap();
        fs::File::open(path.parent().unwrap())
            .unwrap()
            .set_times(fs::FileTimes::new().set_modified(std::time::UNIX_EPOCH))
            .unwrap();
        let result = with_readonly_access(&home, "session-status", |_, _| {
            fs::write(&path, b"transient db")?;
            fs::remove_file(&path)?;
            Ok(())
        });
        assert!(matches!(result, Err(Error::Validation(_))));
        fs::remove_dir_all(home).unwrap();
    }
    #[test]
    fn readonly_mode_rejects_database_creation_or_guard_changes_during_body() {
        let home = std::env::temp_dir().join(format!("mode-readonly-race-{}", std::process::id()));
        fs::DirBuilder::new().mode(0o700).create(&home).unwrap();
        let path = super::super::unified_path(&home);
        let result = with_readonly_access(&home, "session-status", |mode, db| {
            assert_eq!(mode, Mode::Legacy);
            assert!(db.is_none());
            let db = super::super::open_unified(&path)?;
            set_mode(&db, "session-status", Mode::Unified, "test", 1)?;
            Ok(())
        });
        assert!(matches!(result, Err(Error::Validation(_))));
        let result = with_readonly_access(&home, "session-status", |mode, db| {
            assert_eq!(mode, Mode::Unified);
            assert!(db.is_some());
            fs::write(seal_guard_path(&path, "session-status")?, GUARD)?;
            Ok(())
        });
        assert!(matches!(result, Err(Error::Validation(_))));
        fs::remove_dir_all(home).unwrap();
    }
}
