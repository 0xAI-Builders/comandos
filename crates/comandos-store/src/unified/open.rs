use crate::{Error, Result, state::UNIFIED_MIGRATIONS};
use rusqlite::{Connection, Transaction, TransactionBehavior};
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
pub const MOVED_SENTINEL: i64 = 1000;
pub fn unified_path(home: &Path) -> PathBuf {
    std::env::var_os("COMANDOS_DB")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".local/share/comandos/comandos.sqlite3"))
}
pub(crate) fn validate(conn: &Connection) -> Result<BTreeSet<i64>> {
    let uv: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    if uv > crate::usage::SCHEMA_VERSION {
        return Err(Error::Validation(
            "base creada por una versión más nueva de ComandOS".into(),
        ));
    }
    let has: bool=conn.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='schema_migrations')",[],|r|r.get(0))?;
    if !has {
        return Ok(BTreeSet::new());
    }
    let existing = conn
        .prepare("SELECT version FROM schema_migrations")?
        .query_map([], |r| r.get::<_, i64>(0))?
        .collect::<rusqlite::Result<BTreeSet<_>>>()?;
    if existing
        .iter()
        .any(|v| !UNIFIED_MIGRATIONS.iter().any(|m| m.version == *v))
    {
        return Err(Error::Validation(
            "base creada por una versión más nueva de ComandOS".into(),
        ));
    }
    Ok(existing)
}

pub(super) fn private_parent(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    if std::fs::metadata(parent)?.permissions().mode() & 0o7777 != 0o700 {
        return Err(Error::Validation(format!(
            "{}: la base única requiere un directorio privado 0700",
            parent.display()
        )));
    }
    Ok(())
}

pub fn open_unified(path: &Path) -> Result<Connection> {
    if path != Path::new(":memory:") {
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            use std::os::unix::fs::DirBuilderExt;
            std::fs::DirBuilder::new()
                .recursive(true)
                .mode(0o700)
                .create(parent)?;
        }
        match std::fs::symlink_metadata(path) {
            Ok(metadata) if !metadata.is_file() => {
                return Err(Error::Validation(
                    "la base única debe ser un archivo regular".into(),
                ));
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => private_parent(path)?,
            Err(error) => return Err(error.into()),
        }
        use std::os::unix::fs::OpenOptionsExt;
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(path)
        {
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e.into()),
        }
    }
    let conn = if path != Path::new(":memory:") {
        use std::os::unix::fs::PermissionsExt;
        // Toda privacidad se comprueba antes de abrir SQLite sobre la fuente.
        private_parent(path)?;
        if std::fs::symlink_metadata(path)?.permissions().mode() & 0o7777 != 0o600 {
            return Err(Error::Validation(format!(
                "{}: la base única requiere permisos 0600",
                path.display()
            )));
        }
        super::preflight::accept(path)?
    } else {
        Connection::open(path)?
    };
    // Un error posterior a la aceptación tampoco debe checkpoint al cerrar.
    conn.set_db_config(
        rusqlite::config::DbConfig::SQLITE_DBCONFIG_NO_CKPT_ON_CLOSE,
        true,
    )?;
    conn.busy_timeout(Duration::from_secs(5))?;
    validate(&conn)?;
    conn.pragma_update(None, "foreign_keys", true)?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    let tx = Transaction::new_unchecked(&conn, TransactionBehavior::Immediate)?;
    let existing = validate(&conn)?;
    conn.execute_batch("CREATE TABLE IF NOT EXISTS schema_migrations (version INTEGER PRIMARY KEY,name TEXT NOT NULL,applied_at REAL NOT NULL)")?;
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| Error::Validation(e.to_string()))?
        .as_secs_f64();
    for migration in UNIFIED_MIGRATIONS
        .iter()
        .filter(|m| !existing.contains(&m.version))
    {
        conn.execute_batch(migration.sql)?;
        conn.execute(
            "INSERT INTO schema_migrations VALUES (?1,?2,?3)",
            rusqlite::params![migration.version, migration.name, now],
        )?;
    }
    crate::usage::ensure_schema(&conn)?;
    tx.commit()?;
    conn.set_db_config(
        rusqlite::config::DbConfig::SQLITE_DBCONFIG_NO_CKPT_ON_CLOSE,
        false,
    )?;
    Ok(conn)
}

// Una base completa no necesita tomar el candado de migración al abrirse.
// La puerta es la misma; sólo se omite aplicar DDL que ya está confirmado.
pub(crate) fn open_existing(path: &Path) -> Result<Connection> {
    let conn = super::preflight::accept(path)?;
    let known = validate(&conn)?;
    let uv: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    if uv != crate::usage::SCHEMA_VERSION
        || known != UNIFIED_MIGRATIONS.iter().map(|m| m.version).collect()
    {
        drop(conn);
        return open_unified(path);
    }
    conn.busy_timeout(Duration::from_secs(5))?;
    conn.pragma_update(None, "foreign_keys", true)?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    Ok(conn)
}

/// Abre únicamente un esquema ya completo. Los hooks jamás ejecutan migraciones.
pub(crate) fn open_caller(path: &Path) -> Result<Connection> {
    let conn = super::preflight::accept(path)?;
    let known = validate(&conn)?;
    let uv: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    if uv != crate::usage::SCHEMA_VERSION
        || known != UNIFIED_MIGRATIONS.iter().map(|m| m.version).collect()
    {
        return Err(Error::Validation(
            "base única incompleta; el llamador no migra".into(),
        ));
    }
    conn.busy_timeout(Duration::from_secs(5))?;
    conn.pragma_update(None, "foreign_keys", true)?;
    Ok(conn)
}
