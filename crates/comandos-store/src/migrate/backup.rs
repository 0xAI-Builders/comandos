//! Copias privadas verificadas antes del cambio de modo.
use super::sources::{self, Source};
use crate::{
    Error, Result, domains::catalog::SourceSpec, files::write_atomic, unified::unified_path,
};
use serde_json::{Value, json};
use std::{
    collections::BTreeSet,
    fs,
    io::Write,
    os::unix::fs::{DirBuilderExt, OpenOptionsExt},
    path::{Path, PathBuf},
};
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    File,
    Sqlite,
    Dir,
}
pub struct BackupEntry {
    pub source: PathBuf,
    pub copy: PathBuf,
    pub sha256: String,
    pub size: u64,
    pub mode: u32,
    pub mtime_ns: i64,
    pub kind: EntryKind,
}
pub struct BackupManifest {
    pub created_at_ms: i64,
    pub entries: Vec<BackupEntry>,
}
impl BackupManifest {
    pub fn json(&self) -> Value {
        json!({"created_at_ms":self.created_at_ms,"entries":self.entries.iter().map(|e|json!({"source":e.source,"copy":e.copy,"sha256":e.sha256,"size":e.size,"mode":e.mode,"mtime_ns":e.mtime_ns,"kind":match e.kind{EntryKind::File=>"file",EntryKind::Sqlite=>"sqlite",EntryKind::Dir=>"dir"}})).collect::<Vec<_>>()})
    }
}
pub fn backup_sources(home: &Path, dest: &Path, specs: &[SourceSpec]) -> Result<BackupManifest> {
    let sources = sources::collect(home, &unified_path(home), specs)?;
    create(home, dest, &sources, super::journal::now_ms()?)
}
pub(super) fn create(
    home: &Path,
    dest: &Path,
    sources: &[Source],
    now_ms: i64,
) -> Result<BackupManifest> {
    if dest.exists() {
        return Err(Error::Validation("el respaldo ya existe".into()));
    }
    let parent = dest
        .parent()
        .ok_or_else(|| Error::Validation("respaldo sin padre".into()))?;
    private_dirs(home, parent)?;
    fs::DirBuilder::new().mode(0o700).create(dest)?;
    fs::File::open(parent)?.sync_all()?;
    let mut manifest = BackupManifest {
        created_at_ms: now_ms,
        entries: vec![],
    };
    for source in sources {
        manifest.entries.push(copy(home, dest, source)?);
    }
    save(dest, &manifest)?;
    Ok(manifest)
}
fn copy(home: &Path, dest: &Path, source: &Source) -> Result<BackupEntry> {
    sources::check_parents(home, &source.path)?;
    let _lock = sources::lock(source)?;
    let snapshot = sources::read_source(home, source)?;
    let mut copy = dest.join(&source.symbolic);
    private_dirs(
        dest,
        copy.parent()
            .ok_or_else(|| Error::Validation("copia sin padre".into()))?,
    )?;
    // Una copia que quedó antes del commit del manifiesto se conserva. La nueva
    // obtiene otro nombre exclusivo; no se presume que aquella ya era completa.
    let mut file = loop {
        match fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .mode(0o600)
            .open(&copy)
        {
            Ok(file) => break file,
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                copy = sources::suffix(
                    &dest.join(&source.symbolic),
                    &format!(
                        ".unregistered-{}",
                        super::journal::new_id(super::journal::now_ms()?)?
                    ),
                )
            }
            Err(e) => return Err(e.into()),
        }
    };
    file.write_all(&snapshot.body)?;
    file.sync_all()?;
    fs::File::open(
        copy.parent()
            .ok_or_else(|| Error::Validation("copia sin padre".into()))?,
    )?
    .sync_all()?;
    Ok(BackupEntry {
        source: source.path.clone(),
        copy,
        sha256: snapshot.sha256,
        size: snapshot.body.len() as u64,
        mode: snapshot.mode,
        mtime_ns: snapshot.mtime_ns,
        kind: EntryKind::File,
    })
}
pub(super) fn extend_backup(home: &Path, dest: &Path, sources: &[Source]) -> Result<()> {
    let mut manifest = load(&dest.join("manifest.json"))?;
    for source in sources {
        if !manifest.entries.iter().any(|e| e.source == source.path) {
            manifest.entries.push(copy(home, dest, source)?);
            save(dest, &manifest)?;
        }
    }
    verify_backup(&dest.join("manifest.json"))
}
fn save(dest: &Path, manifest: &BackupManifest) -> Result<()> {
    let body = serde_json::to_vec_pretty(&manifest.json())
        .map_err(|e| Error::Validation(e.to_string()))?;
    write_atomic(&dest.join("manifest.json"), &body)?;
    fs::File::open(dest)?.sync_all()?;
    Ok(())
}
pub(super) fn private_dirs(root: &Path, path: &Path) -> Result<()> {
    if !path.starts_with(root) {
        return Err(Error::Validation(
            "directorio fuera de la raíz privada".into(),
        ));
    }
    let mut p = root.to_owned();
    for component in path
        .strip_prefix(root)
        .map_err(|e| Error::Validation(e.to_string()))?
        .components()
    {
        if !matches!(component, std::path::Component::Normal(_)) {
            return Err(Error::Validation("directorio inválido".into()));
        }
        p.push(component);
        match fs::symlink_metadata(&p) {
            Ok(m) if m.is_dir() => {}
            Ok(_) => {
                return Err(Error::Validation(format!(
                    "{}: padre no regular",
                    p.display()
                )));
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                fs::DirBuilder::new().mode(0o700).create(&p)?;
                if let Some(parent) = p.parent() {
                    fs::File::open(parent)?.sync_all()?;
                }
            }
            Err(e) => return Err(e.into()),
        }
    }
    Ok(())
}
pub fn load(path: &Path) -> Result<BackupManifest> {
    let snapshot = sources::read(path)?;
    let value: Value =
        serde_json::from_slice(&snapshot.body).map_err(|e| Error::Validation(e.to_string()))?;
    let created_at_ms = value
        .get("created_at_ms")
        .and_then(Value::as_i64)
        .ok_or_else(|| Error::Validation("fecha de respaldo inválida".into()))?;
    let entries = value
        .get("entries")
        .and_then(Value::as_array)
        .ok_or_else(|| Error::Validation("entradas de respaldo inválidas".into()))?;
    let mut out = vec![];
    for e in entries {
        let text = |name| {
            e.get(name)
                .and_then(Value::as_str)
                .ok_or_else(|| Error::Validation(format!("respaldo: {name} inválido")))
        };
        let unsigned = |name| {
            e.get(name)
                .and_then(Value::as_u64)
                .ok_or_else(|| Error::Validation(format!("respaldo: {name} inválido")))
        };
        let kind = match text("kind")? {
            "file" => EntryKind::File,
            "sqlite" => EntryKind::Sqlite,
            "dir" => EntryKind::Dir,
            _ => return Err(Error::Validation("tipo de respaldo inválido".into())),
        };
        out.push(BackupEntry {
            source: PathBuf::from(text("source")?),
            copy: PathBuf::from(text("copy")?),
            sha256: text("sha256")?.into(),
            size: unsigned("size")?,
            mode: u32::try_from(unsigned("mode")?).map_err(|e| Error::Validation(e.to_string()))?,
            mtime_ns: e
                .get("mtime_ns")
                .and_then(Value::as_i64)
                .ok_or_else(|| Error::Validation("mtime inválido".into()))?,
            kind,
        });
    }
    Ok(BackupManifest {
        created_at_ms,
        entries: out,
    })
}
pub fn verify_backup(manifest_path: &Path) -> Result<()> {
    let root = manifest_path
        .parent()
        .ok_or_else(|| Error::Validation("manifiesto sin padre".into()))?;
    let manifest = load(manifest_path)?;
    let mut sources = BTreeSet::new();
    let mut copies = BTreeSet::new();
    for entry in manifest.entries {
        if entry.kind != EntryKind::File
            || !entry.source.is_absolute()
            || !entry.copy.is_absolute()
            || !sources.insert(entry.source.clone())
            || !copies.insert(entry.copy.clone())
        {
            return Err(Error::Validation(
                "entradas de respaldo repetidas o no admitidas en S3".into(),
            ));
        }
        sources::check_parents(root, &entry.copy)?;
        let copy = sources::read(&entry.copy)?;
        if copy.body.len() as u64 != entry.size || copy.sha256 != entry.sha256 {
            return Err(Error::Validation(format!(
                "{}: hash o tamaño de respaldo distinto",
                entry.copy.display()
            )));
        }
    }
    Ok(())
}
pub fn list(home: &Path) -> Result<Value> {
    let root = home.join(".local/share/comandos/backups");
    let entries = match fs::read_dir(root) {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(json!([])),
        Err(e) => return Err(e.into()),
    };
    let mut out = vec![];
    for entry in entries {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let path = entry.path().join("manifest.json");
        if !path.exists() {
            continue;
        }
        match load(&path) {
            Ok(m) => {
                let result = verify_backup(&path);
                out.push(json!({"manifest":path,"created_at_ms":m.created_at_ms,"bytes":m.entries.iter().map(|e|e.size).sum::<u64>(),"verified":result.is_ok(),"error":result.err().map(|e|e.to_string())}));
            }
            Err(e) => out.push(json!({"manifest":path,"verified":false,"error":e.to_string()})),
        }
    }
    out.sort_by(|a, b| {
        a.get("manifest")
            .and_then(Value::as_str)
            .cmp(&b.get("manifest").and_then(Value::as_str))
    });
    Ok(json!(out))
}
