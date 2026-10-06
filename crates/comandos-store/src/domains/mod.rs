//! Acceso por dominio sin cambiar consumidores ni modos en producción.
pub mod catalog;
mod layout;
mod legacy;
mod mirror;
use crate::{
    Error, Result,
    files::{FileLock, write_atomic},
    unified::{self, LogName, Mode},
};
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
    pub name: &'static str,
    pub domain: &'static str,
    pub file: PathBuf,
    pub lock: PathBuf,
}
impl DocHandle<'_> {
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
    pub fn document(
        &self,
        name: &'static str,
        domain: &'static str,
        file: PathBuf,
    ) -> DocHandle<'a> {
        let lock = lock_path(&file);
        DocHandle {
            home: self.home,
            name,
            domain,
            file,
            lock,
        }
    }
}
