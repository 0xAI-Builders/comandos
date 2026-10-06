//! Flat document collections authorized by the existing symbolic source catalog.
use super::{
    DocHandle, DomainStore,
    catalog::{self, SourcePattern, TargetKind, UnifiedControlFiles},
};
use crate::{
    Error, Result,
    unified::{self, Mode},
};
use rusqlite::OptionalExtension;
use std::{
    fs,
    io::{self, Read},
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
};
pub struct DocumentDir<'a> {
    home: &'a Path,
    domain: &'static str,
    dir: PathBuf,
    name_prefix: String,
    suffix: &'static str,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentRow {
    pub key: String,
    pub body: Vec<u8>,
    pub modified_ns: i64,
}
impl<'a> DocumentDir<'a> {
    pub fn directory(&self) -> &Path {
        &self.dir
    }
    pub fn mode_readonly(&self) -> Result<Mode> {
        unified::modes::with_readonly_access(self.home, self.domain, |mode, _| Ok(mode))
    }
    pub(super) fn new(
        home: &'a Path,
        domain: &'static str,
        prefix: &str,
        dir: PathBuf,
    ) -> Result<Self> {
        if !home.is_absolute()
            || !dir.is_absolute()
            || dir
                .components()
                .any(|p| matches!(p, std::path::Component::ParentDir))
        {
            return Err(Error::Validation(
                "colección requiere raíces absolutas sin traversal".into(),
            ));
        }
        let suffix = catalog::catalog()
            .iter()
            .find_map(|s| match s.pattern {
                SourcePattern::Dir { dir, suffix }
                    if s.domain == domain && s.kind == TargetKind::Document && dir == prefix =>
                {
                    Some(suffix)
                }
                _ => None,
            })
            .ok_or_else(|| {
                Error::Validation("colección Document no catalogada para el dominio".into())
            })?;
        let name_prefix = if let Some(t) = prefix.strip_prefix("H/") {
            format!("hooks/{t}/")
        } else if let Some(t) = prefix.strip_prefix("STATE/") {
            format!("state/{t}/")
        } else if let Some(t) = prefix.strip_prefix("SHARE/") {
            format!("share/{t}/")
        } else {
            return Err(Error::Validation("raíz simbólica desconocida".into()));
        };
        Ok(Self {
            home,
            domain,
            dir,
            name_prefix,
            suffix,
        })
    }
    fn valid(&self, key: &str) -> bool {
        !key.is_empty()
            && !key.starts_with('.')
            && !key.contains(['/', '\\', '\0'])
            && key.ends_with(self.suffix)
            && key.len() > self.suffix.len()
    }
    fn name(&self, key: &str) -> Result<String> {
        if !self.valid(key) {
            return Err(Error::Validation("basename de documento inválido".into()));
        }
        Ok(format!("{}{key}", self.name_prefix))
    }
    /// The borrowed name lives for the callback; the facade itself is not borrowed.
    pub fn with_document<T>(
        &self,
        key: &str,
        body: impl FnOnce(DocHandle<'_>) -> Result<T>,
    ) -> Result<T> {
        let name = self.name(key)?;
        body(DomainStore { home: self.home }.document(&name, self.domain, self.dir.join(key)))
    }
    fn parents(&self) -> Result<()> {
        for p in self.dir.ancestors() {
            match p.symlink_metadata() {
                Ok(m) if m.is_dir() => {}
                Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                Ok(_) => {
                    return Err(Error::Validation(format!(
                        "{}: padre no regular",
                        p.display()
                    )));
                }
                Err(e) => return Err(e.into()),
            }
        }
        Ok(())
    }
    fn legacy(&self, key: &str) -> Result<Option<DocumentRow>> {
        self.name(key)?;
        self.parents()?;
        let path = self.dir.join(key);
        let m = match path.symlink_metadata() {
            Ok(m) => m,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e.into()),
        };
        if !m.is_file() {
            return Err(Error::Validation(format!(
                "{}: documento no regular",
                path.display()
            )));
        }
        let controls = UnifiedControlFiles::inspect(&unified::unified_path(self.home))?;
        if controls.is_control(&path)? {
            return Err(Error::Validation(
                "control no puede leerse como documento".into(),
            ));
        }
        let mut f = fs::File::open(&path)?;
        let opened = f.metadata()?;
        if (m.dev(), m.ino()) != (opened.dev(), opened.ino()) {
            return Err(Error::Validation("documento cambió al abrir".into()));
        }
        let mut body = Vec::new();
        f.read_to_end(&mut body)?;
        let after = f.metadata()?;
        let entry = path.symlink_metadata()?;
        let stamp = |m: &fs::Metadata| (m.dev(), m.ino(), m.len(), m.mtime(), m.mtime_nsec());
        if stamp(&m) != stamp(&after) || stamp(&m) != stamp(&entry) || controls.is_control(&path)? {
            return Err(Error::Validation("documento cambió durante lectura".into()));
        }
        let modified_ns = m
            .mtime()
            .checked_mul(1_000_000_000)
            .and_then(|n| n.checked_add(m.mtime_nsec()))
            .ok_or_else(|| Error::Validation("mtime fuera de rango".into()))?;
        Ok(Some(DocumentRow {
            key: key.into(),
            body,
            modified_ns,
        }))
    }
    pub fn read_readonly(&self, key: &str) -> Result<Option<Vec<u8>>> {
        let name = self.name(key)?;
        unified::modes::with_readonly_access(self.home, self.domain, |mode, db| {
            if matches!(mode, Mode::Unified | Mode::Sealed) {
                let db = db.ok_or_else(|| Error::Validation("estado único sin base".into()))?;
                return Ok(db
                    .query_row(
                        "SELECT body FROM documents WHERE name=?1 AND domain=?2",
                        [name.as_str(), self.domain],
                        |r| r.get(0),
                    )
                    .optional()?);
            }
            Ok(self.legacy(key)?.map(|r| r.body))
        })
    }
    pub fn list_readonly(&self) -> Result<Vec<DocumentRow>> {
        unified::modes::with_readonly_access(self.home, self.domain, |mode, db| {
            if matches!(mode, Mode::Unified | Mode::Sealed) {
                let db = db.ok_or_else(|| Error::Validation("estado único sin base".into()))?;
                let mut rows = Vec::new();
                let mut q=db.prepare("SELECT name,body,updated_at_ms FROM documents WHERE domain=?1 AND substr(name,1,length(?2))=?2 ORDER BY name")?;
                let found = q.query_map([self.domain, self.name_prefix.as_str()], |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, Vec<u8>>(1)?,
                        r.get::<_, i64>(2)?,
                    ))
                })?;
                for row in found {
                    let (name, body, ms) = row?;
                    let Some(key) = name
                        .strip_prefix(&self.name_prefix)
                        .filter(|k| self.valid(k))
                    else {
                        continue;
                    };
                    let modified_ns = ms.checked_mul(1_000_000).ok_or_else(|| {
                        Error::Validation("timestamp de documento fuera de rango".into())
                    })?;
                    rows.push(DocumentRow {
                        key: key.into(),
                        body,
                        modified_ns,
                    });
                }
                return Ok(rows);
            }
            self.parents()?;
            let entries = match fs::read_dir(&self.dir) {
                Ok(e) => e,
                Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(vec![]),
                Err(e) => return Err(e.into()),
            };
            let mut rows = Vec::new();
            for e in entries {
                let e = e?;
                let name = e
                    .file_name()
                    .into_string()
                    .map_err(|_| Error::Validation("basename de documento no UTF-8".into()))?;
                if self.valid(&name)
                    && let Some(row) = self.legacy(&name)?
                {
                    rows.push(row);
                }
            }
            rows.sort_by(|a, b| a.key.cmp(&b.key));
            Ok(rows)
        })
    }
}
