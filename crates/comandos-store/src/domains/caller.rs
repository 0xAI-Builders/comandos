//! Admisión de llamadores sin DDL y con el backend legado original inyectado.
use crate::{
    Error, Result,
    files::FileLock,
    unified::{self, Mode, Origin},
};
use rusqlite::{Connection, Transaction, TransactionBehavior};
use std::{fs, path::Path};
pub struct CallerAccess {
    mode: Mode,
    db: Option<Connection>,
    _guard: Option<FileLock>,
    reader: Option<super::live_reader::Token>,
}
enum Lease {
    Shared,
    RuntimeRead,
    Exclusive,
}
/// Runtime readers hold the domain lease while ordinary payload commits continue.
/// Inspection tools keep using the strict, immutable snapshot reader instead.
pub fn read<T>(
    home: &Path,
    domain: &str,
    body: impl FnOnce(Mode, Option<&Connection>) -> Result<T>,
) -> Result<T> {
    let access = CallerAccess::open_read(home, domain)?;
    body(access.mode(), access.db())
}
impl CallerAccess {
    pub fn open(home: &Path, domain: &str) -> Result<Self> {
        Self::open_with(home, domain, Lease::Shared)
    }
    pub fn open_read(home: &Path, domain: &str) -> Result<Self> {
        Self::open_with(home, domain, Lease::RuntimeRead)
    }
    /// Owned layout plans keep their existing exclusive lease through commit.
    pub(super) fn open_exclusive(home: &Path, domain: &str) -> Result<Self> {
        Self::open_with(home, domain, Lease::Exclusive)
    }
    fn open_with(home: &Path, domain: &str, lease: Lease) -> Result<Self> {
        let path = unified::unified_path(home);
        let mut reader = None;
        let db = match fs::symlink_metadata(&path) {
            Ok(_) => {
                let (db, token) = super::live_reader::open(&path)?;
                reader = Some(token);
                Some(db)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => return Err(e.into()),
        };
        let mut access = Self {
            mode: Mode::Legacy,
            db,
            _guard: None,
            reader,
        };
        let (mode, guard) = match lease {
            Lease::Shared => unified::modes::access_caller_mode(home, access.db.as_ref(), domain)?,
            Lease::RuntimeRead => {
                unified::modes::access_runtime_read_mode(home, access.db.as_ref(), domain)?
            }
            Lease::Exclusive => unified::modes::access_mode(home, access.db.as_ref(), domain)?,
        };
        if access.db.is_none() && path.exists() {
            return Err(Error::Validation("base apareció durante admisión".into()));
        }
        access.mode = mode;
        access._guard = guard;
        Ok(access)
    }
    pub fn mode(&self) -> Mode {
        self.mode
    }
    pub fn db(&self) -> Option<&Connection> {
        self.db.as_ref()
    }
    /// El llamador adquiere su flock después de esta admisión y lo conserva hasta regresar.
    pub fn write(
        &self,
        legacy: impl FnOnce() -> Result<()>,
        row: impl FnOnce(&Connection, Origin) -> Result<()>,
    ) -> Result<()> {
        let Some(db) = self.db.as_ref().filter(|_| self.mode != Mode::Legacy) else {
            return legacy();
        };
        let tx = if db.is_autocommit() {
            Some(Transaction::new_unchecked(
                db,
                TransactionBehavior::Immediate,
            )?)
        } else {
            None
        };
        crate::migrate::move_db::admit_write(db)?;
        let mut legacy = Some(legacy);
        if self.mode == Mode::Mirror
            && let Some(write) = legacy.take()
        {
            write()?;
        }
        row(
            db,
            if self.mode == Mode::Mirror {
                Origin::Mirror
            } else {
                Origin::Unified
            },
        )?;
        if let Some(tx) = tx {
            tx.commit()?;
        } else {
            db.execute_batch("COMMIT")?;
        }
        if self.mode == Mode::Unified
            && let Some(write) = legacy.take()
        {
            write()?;
        }
        Ok(())
    }
    /// Protect an authoritative RMW without blocking unrelated mode readers.
    /// Acquire the original per-file flock before entering this transaction.
    pub fn with_write_transaction<T, E>(
        &self,
        body: impl FnOnce() -> std::result::Result<T, E>,
    ) -> Result<std::result::Result<T, E>> {
        let Some(db) = self.db.as_ref().filter(|_| self.mode != Mode::Legacy) else {
            return Ok(body());
        };
        db.execute_batch("BEGIN IMMEDIATE")?;
        if let Err(error) = crate::migrate::move_db::admit_write(db) {
            let _ = db.execute_batch("ROLLBACK");
            return Err(error);
        }
        let result = body();
        if !db.is_autocommit() {
            db.execute_batch(if result.is_ok() { "COMMIT" } else { "ROLLBACK" })?;
        }
        Ok(result)
    }
    pub fn read_document(&self, name: &str, file: &Path) -> Result<Option<Vec<u8>>> {
        if matches!(self.mode, Mode::Unified | Mode::Sealed) {
            return Ok(unified::doc_get(
                self.db
                    .as_ref()
                    .ok_or_else(|| Error::Validation("base ausente".into()))?,
                name,
            )?
            .map(|d| d.body));
        }
        super::legacy::read(file)
    }
    /// Transformación y escritura bajo el flock original. El callback conserva WriteGuard y formato.
    #[allow(clippy::too_many_arguments)] // Identidad de documento y callbacks del backend original.
    pub fn update_document(
        &self,
        name: &str,
        domain: &str,
        file: &Path,
        lock: &Path,
        now_ms: i64,
        transform: impl FnOnce(Option<&[u8]>) -> Result<Option<Vec<u8>>>,
        legacy_write: impl FnOnce(&[u8]) -> Result<()>,
    ) -> Result<bool> {
        let _lock = if self.mode == Mode::Sealed {
            None
        } else {
            Some(FileLock::exclusive(lock)?)
        };
        // Sealed requiere proteger también la lectura frente a otro escritor sellado.
        let tx = self
            .db
            .as_ref()
            .filter(|_| self.mode != Mode::Legacy)
            .map(|db| Transaction::new_unchecked(db, TransactionBehavior::Immediate))
            .transpose()?;
        if let Some(tx) = &tx {
            crate::migrate::move_db::admit_write(tx)?;
        }
        let old = self.read_document(name, file)?;
        let Some(body) = transform(old.as_deref())? else {
            return Ok(false);
        };
        let mut legacy_write = Some(legacy_write);
        if matches!(self.mode, Mode::Legacy | Mode::Mirror)
            && let Some(write) = legacy_write.take()
        {
            write(&body)?;
        }
        if let Some(tx) = tx {
            unified::doc_put(
                &tx,
                name,
                domain,
                &body,
                if self.mode == Mode::Mirror {
                    Origin::Mirror
                } else {
                    Origin::Unified
                },
                now_ms,
            )?;
            tx.commit()?;
        }
        if self.mode == Mode::Unified
            && let Some(write) = legacy_write.take()
        {
            write(&body)?;
        }
        Ok(true)
    }
}
impl Drop for CallerAccess {
    fn drop(&mut self) {
        if let Some(token) = self.reader.take()
            && let Some(db) = self.db.take()
        {
            super::live_reader::release(db, token);
        }
    }
}
pub fn write(
    home: &Path,
    domain: &str,
    lock: &Path,
    legacy: impl FnOnce() -> Result<()>,
    row: impl FnOnce(&Connection, Origin) -> Result<()>,
) -> Result<()> {
    let access = CallerAccess::open(home, domain)?;
    let _lock = if matches!(access.mode(), Mode::Legacy | Mode::Sealed) {
        None
    } else {
        Some(FileLock::exclusive(lock)?)
    };
    access.write(legacy, row)
}
