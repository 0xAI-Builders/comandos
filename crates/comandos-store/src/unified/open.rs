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
fn validate(conn: &Connection) -> Result<BTreeSet<i64>> {
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
pub fn open_unified(path: &Path) -> Result<Connection> {
    if path != Path::new(":memory:") {
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            use std::os::unix::fs::DirBuilderExt;
            std::fs::DirBuilder::new()
                .recursive(true)
                .mode(0o700)
                .create(parent)?;
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
    let conn = Connection::open(path)?;
    validate(&conn)?;
    conn.busy_timeout(Duration::from_secs(5))?;
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
    Ok(conn)
}
