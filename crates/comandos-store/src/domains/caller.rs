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
}
impl CallerAccess {
    pub fn open(home: &Path, domain: &str) -> Result<Self> {
        let path = unified::unified_path(home);
        let db = match fs::symlink_metadata(&path) {
            Ok(_) => Some(unified::open_caller(&path)?),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => return Err(e.into()),
        };
        let (mode, guard) = unified::modes::access_mode(home, db.as_ref(), domain)?;
        if db.is_none() && path.exists() {
            return Err(Error::Validation("base apareció durante admisión".into()));
        }
        Ok(Self {
            mode,
            db,
            _guard: guard,
        })
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
        let tx = Transaction::new_unchecked(db, TransactionBehavior::Immediate)?;
        crate::migrate::move_db::admit_write(&tx)?;
        let mut legacy = Some(legacy);
        if self.mode == Mode::Mirror
            && let Some(write) = legacy.take()
        {
            write()?;
        }
        row(
            &tx,
            if self.mode == Mode::Mirror {
                Origin::Mirror
            } else {
                Origin::Unified
            },
        )?;
        tx.commit()?;
        if self.mode == Mode::Unified
            && let Some(write) = legacy.take()
        {
            write()?;
        }
        Ok(())
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
