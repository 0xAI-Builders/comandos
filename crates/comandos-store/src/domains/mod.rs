//! Acceso por dominio sin cambiar consumidores ni modos en producción.
pub mod caller;
pub mod catalog;
pub mod commands;
mod document_dir;
mod layout;
mod legacy;
mod mirror;
use crate::{
    Error, Result,
    files::{FileLock, write_atomic},
    unified::{self, LogName, Mode},
};
pub use document_dir::{DocumentDir, DocumentRow};
pub use layout::LayoutSnapshot;
use rusqlite::{Connection, OptionalExtension};
use std::{
    fs,
    io::Write,
    os::unix::fs::OpenOptionsExt,
    path::{Path, PathBuf},
};
pub struct DocHandle<'a> {
    pub home: &'a Path,
    pub name: &'a str,
    pub domain: &'static str,
    pub file: PathBuf,
    pub lock: PathBuf,
}
impl DocHandle<'_> {
    /// Compatibility writers may use their existing guarded filesystem backend
    /// only while Legacy remains authoritative. No mode/database file is created.
    pub fn with_legacy_authority<T>(&self, body: impl FnOnce() -> Result<T>) -> Result<T> {
        unified::modes::with_readonly_access(self.home, self.domain, |mode, _| {
            if mode != Mode::Legacy {
                return Err(Error::Validation(
                    "Escritura UI S5a pendiente: dominio fuera de Legacy".into(),
                ));
            }
            body()
        })
    }
    /// Atomic owned read/modify/write. The callback only transforms bytes and
    /// must not call this facade or perform external I/O while its locks live.
    /// `None` leaves the document unchanged. Reads never create a missing DB;
    /// write admission checks the original source before creating file locks.
    pub fn update_owned(
        &self,
        now_ms: i64,
        update: impl FnOnce(Option<&[u8]>) -> Result<Option<Vec<u8>>>,
    ) -> Result<bool> {
        self.update_owned_when(now_ms, std::time::Duration::from_secs(5), || true, update)
    }
    /// The deadline bounds only mode-busy admission. Cancellation is checked
    /// before admission, file-lock acquisition and the byte transformation.
    pub fn update_owned_when(
        &self,
        now_ms: i64,
        wait: std::time::Duration,
        allowed: impl Fn() -> bool,
        update: impl FnOnce(Option<&[u8]>) -> Result<Option<Vec<u8>>>,
    ) -> Result<bool> {
        if !allowed() {
            return Ok(false);
        }
        let path = unified::unified_path(self.home);
        let deadline = std::time::Instant::now() + wait;
        let existing = loop {
            if !allowed() {
                return Ok(false);
            }
            match unified::modes::with_readonly_access(self.home, self.domain, |_, db| {
                Ok(db.is_some())
            }) {
                Err(Error::ModeBusy) if std::time::Instant::now() < deadline => {
                    std::thread::sleep(std::time::Duration::from_millis(5))
                }
                result => break result?,
            }
        };
        if !allowed() {
            return Ok(false);
        }
        let db = if existing {
            Some(unified::open_existing(&path)?)
        } else {
            None
        };
        let (mode, _mode_lock) = unified::modes::access_mode(self.home, db.as_ref(), self.domain)?;
        if db.is_none() && path.exists() {
            return Err(Error::Validation(
                "base apareció durante actualización de documento".into(),
            ));
        }
        if !allowed() {
            return Ok(false);
        }
        let _file_lock = if mode == Mode::Sealed {
            None
        } else {
            // New document/control directories are private; existing modes are
            // preserved. Sealed never recreates the legacy directory.
            if let Some(parent) = self.lock.parent() {
                use std::os::unix::fs::DirBuilderExt;
                fs::DirBuilder::new()
                    .recursive(true)
                    .mode(0o700)
                    .create(parent)?;
            }
            Some(FileLock::exclusive(&self.lock)?)
        };
        // In Sealed there is no legacy lock. SQLite must protect the read as
        // well as the write, including updates from other sealed writers.
        let tx = if mode != Mode::Legacy {
            db.as_ref()
                .map(|db| {
                    rusqlite::Transaction::new_unchecked(
                        db,
                        rusqlite::TransactionBehavior::Immediate,
                    )
                })
                .transpose()?
        } else {
            None
        };
        if let Some(tx) = &tx {
            crate::migrate::move_db::admit_write(tx)?;
        }
        if !allowed() {
            return Ok(false);
        }
        let old = if matches!(mode, Mode::Unified | Mode::Sealed) {
            unified::doc_get(
                db.as_ref()
                    .ok_or_else(|| Error::Validation("estado único sin base".into()))?,
                self.name,
            )?
            .map(|d| d.body)
        } else {
            legacy::read(&self.file)?
        };
        let Some(body) = update(old.as_deref())? else {
            return Ok(false);
        };
        if matches!(mode, Mode::Legacy | Mode::Mirror) {
            write_atomic(&self.file, &body)?;
        }
        if let Some(tx) = tx {
            unified::doc_put(
                &tx,
                self.name,
                self.domain,
                &body,
                if mode == Mode::Mirror {
                    unified::Origin::Mirror
                } else {
                    unified::Origin::Unified
                },
                now_ms,
            )?;
            tx.commit()?;
        }
        if mode == Mode::Unified {
            write_atomic(&self.file, &body)?;
        }
        Ok(true)
    }
    pub fn read_readonly(&self) -> Result<Option<Vec<u8>>> {
        unified::modes::with_readonly_access(self.home, self.domain, |mode, db| {
            if matches!(mode, Mode::Unified | Mode::Sealed) {
                let db = db.ok_or_else(|| Error::Validation("estado único sin base".into()))?;
                return Ok(unified::doc_get(db, self.name)?.map(|d| d.body));
            }
            legacy::read(&self.file)
        })
    }
    pub fn read(&self, db: Option<&Connection>) -> Result<Option<Vec<u8>>> {
        let (mode, _guard) = unified::modes::access_mode(self.home, db, self.domain)?;
        if matches!(mode, Mode::Unified | Mode::Sealed)
            && let Some(db) = db
        {
            return Ok(unified::doc_get(db, self.name)?.map(|d| d.body));
        }
        legacy::read(&self.file)
    }
    pub fn write(&self, db: Option<&Connection>, body: &[u8], now_ms: i64) -> Result<()> {
        let (mode, _guard) = unified::modes::access_mode(self.home, db, self.domain)?;
        let _lock = if mode == Mode::Sealed {
            None
        } else {
            Some(FileLock::exclusive(&self.lock)?)
        };
        mirror::write(
            mode,
            db,
            || Ok(write_atomic(&self.file, body)?),
            |db, origin| {
                unified::doc_put(db, self.name, self.domain, body, origin, now_ms).map(|_| ())
            },
        )
    }
}
pub struct StatusDir<'a> {
    pub home: &'a Path,
    pub dir: PathBuf,
    pub domain: &'static str,
}
impl StatusDir<'_> {
    /// No source writes, new lock files or migrations. Legacy/Mirror preserve filesystem
    /// iteration and follow regular JSON symlinks as Python glob did. Unified
    /// and Sealed deliberately use stable file-key order for equal timestamps.
    pub fn list_readonly(&self) -> Result<Vec<(String, Vec<u8>)>> {
        unified::modes::with_readonly_access(self.home, self.domain, |mode, db| {
            if matches!(mode, Mode::Unified | Mode::Sealed) {
                let db = db.ok_or_else(|| Error::Validation("estado único sin base".into()))?;
                return Ok(db.prepare("SELECT file_key,body FROM session_status WHERE file_key NOT LIKE '.%' AND substr(file_key,-5)='.json' ORDER BY file_key")?
                    .query_map([], |r| Ok((r.get(0)?,r.get(1)?)))?
                    .collect::<rusqlite::Result<_>>()?);
            }
            // glob.glob silently ignores inaccessible directories; opening each
            // matched file is also best-effort in the original cc-next.
            let Ok(entries) = fs::read_dir(&self.dir) else {
                return Ok(vec![]);
            };
            let mut rows = vec![];
            for entry in entries.flatten() {
                let Ok(name) = entry.file_name().into_string() else {
                    continue;
                };
                if !name.starts_with('.')
                    && name.ends_with(".json")
                    && fs::metadata(entry.path()).is_ok_and(|meta| meta.is_file())
                    && let Ok(body) = fs::read(entry.path())
                {
                    rows.push((name, body));
                }
            }
            Ok(rows)
        })
    }
    fn file(&self, key: &str) -> Result<PathBuf> {
        if key.is_empty()
            || key.contains('/')
            || key.contains('\\')
            || key == ".."
            || !key.ends_with(".json")
        {
            return Err(Error::Validation("clave de estado inválida".into()));
        }
        Ok(self.dir.join(key))
    }
    pub fn read(&self, db: Option<&Connection>, key: &str) -> Result<Option<Vec<u8>>> {
        let file = self.file(key)?;
        let (mode, _guard) = unified::modes::access_mode(self.home, db, self.domain)?;
        if matches!(mode, Mode::Unified | Mode::Sealed)
            && let Some(db) = db
        {
            return Ok(db
                .query_row(
                    "SELECT body FROM session_status WHERE file_key=?1",
                    [key],
                    |r| r.get(0),
                )
                .optional()?);
        }
        legacy::read(&file)
    }
    pub fn write(
        &self,
        db: Option<&Connection>,
        key: &str,
        body: &[u8],
        now_ns: i64,
    ) -> Result<()> {
        let file = self.file(key)?;
        let (mode, _guard) = unified::modes::access_mode(self.home, db, self.domain)?;
        let _lock = if mode == Mode::Sealed {
            None
        } else {
            Some(FileLock::exclusive(&lock_path(&file))?)
        };
        mirror::write(
            mode,
            db,
            || Ok(write_atomic(&file, body)?),
            |db, origin| unified::status_put(db, key, body, now_ns, origin),
        )
    }
    pub fn list(&self, db: Option<&Connection>) -> Result<Vec<(String, Vec<u8>)>> {
        let (mode, _guard) = unified::modes::access_mode(self.home, db, self.domain)?;
        if matches!(mode, Mode::Unified | Mode::Sealed)
            && let Some(db) = db
        {
            return Ok(db
                .prepare("SELECT file_key,body FROM session_status ORDER BY file_key")?
                .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
                .collect::<rusqlite::Result<_>>()?);
        }
        let entries = match fs::read_dir(&self.dir) {
            Ok(entries) => entries,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
            Err(e) => return Err(e.into()),
        };
        let mut rows = vec![];
        for entry in entries {
            let entry = entry?;
            let name = entry
                .file_name()
                .into_string()
                .map_err(|_| Error::Validation("nombre de estado no UTF-8".into()))?;
            if name.ends_with(".json")
                && entry.file_type()?.is_file()
                && let Some(body) = legacy::read(&entry.path())?
            {
                rows.push((name, body));
            }
        }
        rows.sort_by(|a, b| a.0.cmp(&b.0));
        Ok(rows)
    }
}
pub struct LogHandle<'a> {
    pub home: &'a Path,
    pub log: LogName,
    pub file: PathBuf,
    pub lock: PathBuf,
}
impl LogHandle<'_> {
    pub fn append(&self, db: Option<&Connection>, line: &[u8]) -> Result<()> {
        if line.contains(&b'\n') {
            return Err(Error::Validation(
                "el registro recibe una línea sin LF".into(),
            ));
        }
        let (mode, _guard) = unified::modes::access_mode(self.home, db, "logs")?;
        let _lock = if mode == Mode::Sealed {
            None
        } else {
            Some(FileLock::exclusive(&self.lock)?)
        };
        mirror::write(
            mode,
            db,
            || {
                if let Some(p) = self.file.parent() {
                    fs::create_dir_all(p)?;
                }
                let mut file = fs::OpenOptions::new()
                    .append(true)
                    .create(true)
                    .mode(0o600)
                    .open(&self.file)?;
                file.write_all(line)?;
                file.write_all(b"\n")?;
                file.sync_all()?;
                Ok(())
            },
            |db, _origin| unified::log_append(db, self.log, line).map(|_| ()),
        )
    }
    pub fn tail(&self, db: Option<&Connection>, n: usize) -> Result<Vec<Vec<u8>>> {
        let (mode, _guard) = unified::modes::access_mode(self.home, db, "logs")?;
        if matches!(mode, Mode::Unified | Mode::Sealed)
            && let Some(db) = db
        {
            return unified::log_tail(db, self.log, n);
        }
        let mut lines = legacy::lines(&legacy::read(&self.file)?.unwrap_or_default());
        let skip = lines.len().saturating_sub(n);
        Ok(lines.drain(skip..).collect())
    }
}
fn lock_path(file: &Path) -> PathBuf {
    let mut path = file.as_os_str().to_os_string();
    path.push(".lock");
    PathBuf::from(path)
}
/// Fachada que conserva las rutas y el formato definidos por el escritor actual.
pub struct DomainStore<'a> {
    pub home: &'a Path,
}
impl<'a> DomainStore<'a> {
    /// Read all domain modes from one guarded source snapshot. Does not create
    /// the database, lock, HOME, WAL, or guards, and never scans live processes.
    pub fn modes_readonly(&self) -> Result<Vec<(&'static str, Mode)>> {
        unified::modes::with_readonly_access(self.home, "session-status", |_, db| {
            catalog::DOMAINS
                .iter()
                .map(|domain| Ok((domain.name, unified::mode_of(db, domain.name)?)))
                .collect()
        })
    }
    pub fn document<'b>(&self, name: &'b str, domain: &'static str, file: PathBuf) -> DocHandle<'b>
    where
        'a: 'b,
    {
        let lock = lock_path(&file);
        DocHandle {
            home: self.home,
            name,
            domain,
            file,
            lock,
        }
    }
    pub fn document_dir(
        &self,
        domain: &'static str,
        prefix: &str,
        dir: PathBuf,
    ) -> Result<DocumentDir<'a>> {
        DocumentDir::new(self.home, domain, prefix, dir)
    }
}
