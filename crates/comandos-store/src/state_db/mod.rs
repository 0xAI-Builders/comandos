//! Shared state connection and append-only migrations.
use crate::{Error, Result};
use rusqlite::{
    Connection, OptionalExtension, Transaction, TransactionBehavior,
    backup::{Backup, StepResult},
    params,
};
use std::collections::BTreeSet;
use std::fs::{self, DirBuilder, OpenOptions};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
mod migrations;
pub use migrations::{MIGRATIONS, UNIFIED_MIGRATIONS};

#[derive(Clone, Copy)]
pub struct Migration {
    pub version: i64,
    pub name: &'static str,
    pub sql: &'static str,
}
pub struct MigrationOutcome {
    pub version: i64,
    pub backup: Option<PathBuf>,
}
pub fn connect(path: &Path) -> Result<Connection> {
    if let crate::migrate::DbLocation::Unified(path) = crate::migrate::resolve_configured(path)? {
        return crate::unified::open_unified(&path);
    }
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        let mut builder = DirBuilder::new();
        builder.recursive(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        builder.create(parent)?;
    }
    let conn = Connection::open(path)?;
    conn.busy_timeout(Duration::from_millis(5000))?;
    conn.pragma_update(None, "foreign_keys", true)?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.execute_batch("CREATE TABLE IF NOT EXISTS schema_migrations (version INTEGER PRIMARY KEY, name TEXT NOT NULL, applied_at REAL NOT NULL)")?;
    Ok(conn)
}

pub fn schema_version(conn: &Connection) -> Result<i64> {
    Ok(conn.query_row(
        "SELECT COALESCE(MAX(version), 0) FROM schema_migrations",
        [],
        |r| r.get(0),
    )?)
}

fn applied(conn: &Connection) -> Result<BTreeSet<i64>> {
    Ok(conn
        .prepare("SELECT version FROM schema_migrations")?
        .query_map([], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?)
}

fn backup(conn: &Connection, version: i64, now_seconds: f64) -> Result<Option<PathBuf>> {
    let path: String = conn.query_row("PRAGMA database_list", [], |r| r.get(2))?;
    if path.is_empty() {
        return Ok(None);
    }
    let base = format!("{path}.pre-migration-v{version}-{}", now_seconds as i64);
    let mut suffix = 0u64;
    // Never overwrite a previous backup, including concurrent upgrades within
    // the same second. The source file and its permissions are not changed.
    let target = loop {
        let target = PathBuf::from(if suffix == 0 {
            base.clone()
        } else {
            format!("{base}.{suffix}")
        });
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        match options.open(&target) {
            Ok(file) => {
                drop(file);
                break target;
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                suffix += 1;
            }
            Err(error) => return Err(error.into()),
        }
    };
    let copy = || -> Result<()> {
        let mut dest = Connection::open(&target)?;
        let backup = Backup::new(conn, &mut dest)?;
        let started = Instant::now();
        loop {
            match backup.step(128)? {
                StepResult::Done => return Ok(()),
                StepResult::More => {}
                _ if started.elapsed() >= Duration::from_secs(5) => {
                    return Err(Error::Sql(rusqlite::Error::SqliteFailure(
                        rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_BUSY),
                        Some("backup ocupado".into()),
                    )));
                }
                _ => std::thread::sleep(Duration::from_millis(10)),
            }
        }
    };
    if let Err(error) = copy() {
        let _ = fs::remove_file(&target);
        return Err(error);
    }
    Ok(Some(target))
}

/// Apply every missing version under the writer lock. An older executable
/// never downgrades an already newer schema. `now_seconds` is supplied by the
/// native clock, allowing deterministic cross-language migration verification.
pub fn migrate(
    conn: &Connection,
    migrations: &[Migration],
    now_seconds: f64,
) -> Result<MigrationOutcome> {
    let current = schema_version(conn)?;
    let existing = applied(conn)?;
    let mut pending: Vec<_> = migrations
        .iter()
        .filter(|m| !existing.contains(&m.version))
        .collect();
    if pending.is_empty() {
        return Ok(MigrationOutcome {
            version: current,
            backup: None,
        });
    }
    if !conn.is_autocommit() {
        return Err(Error::Validation(
            "migración requiere una conexión sin transacción activa".into(),
        ));
    }
    if !now_seconds.is_finite() || now_seconds < 0.0 {
        return Err(Error::Validation("fecha de migración inválida".into()));
    }
    let has_tables=conn.query_row("SELECT 1 FROM sqlite_master WHERE type='table' AND name NOT IN ('schema_migrations') LIMIT 1",[],|r|r.get::<_,i64>(0)).optional()?.is_some();
    let saved = if has_tables {
        backup(conn, current, now_seconds)?
    } else {
        None
    };
    let tx = Transaction::new_unchecked(conn, TransactionBehavior::Immediate)?;
    let existing = applied(conn)?;
    pending.retain(|m| !existing.contains(&m.version));
    pending.sort_by_key(|m| m.version);
    for migration in pending {
        // rusqlite's execute_batch prepares statements without Python
        // executescript's implicit COMMIT; DDL participates in this transaction.
        conn.execute_batch(migration.sql)?;
        conn.execute(
            "INSERT INTO schema_migrations VALUES (?, ?, ?)",
            params![migration.version, migration.name, now_seconds],
        )?;
    }
    tx.commit()?;
    Ok(MigrationOutcome {
        version: schema_version(conn)?,
        backup: saved,
    })
}
