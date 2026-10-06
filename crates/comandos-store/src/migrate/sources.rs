use crate::{
    Error, Result,
    domains::catalog::{SourcePattern, SourceSpec, UnifiedControlFiles},
    files::FileLock,
};
use std::{
    collections::BTreeSet,
    fs,
    io::Read,
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
};
#[derive(Clone)]
pub(super) struct Source {
    pub path: PathBuf,
    pub symbolic: String,
    pub spec: SourceSpec,
    pub db: PathBuf,
}
pub(super) struct Snapshot {
    pub body: Vec<u8>,
    pub mtime_ns: i64,
    pub mode: u32,
    pub sha256: String,
    pub identity: (u64, u64, u64, i64, i64),
}
pub(super) fn suffix(path: &Path, tail: &str) -> PathBuf {
    let mut p = path.as_os_str().to_os_string();
    p.push(tail);
    p.into()
}
pub(super) fn resolve(home: &Path, symbolic: &str) -> Result<PathBuf> {
    let (prefix, tail) = symbolic
        .split_once('/')
        .ok_or_else(|| Error::Validation("fuente sin raíz".into()))?;
    let root = match prefix {
        "H" => ".claude/hooks",
        "STATE" => ".local/state/comandos",
        "SHARE" => ".local/share/comandos",
        _ => return Err(Error::Validation("raíz desconocida".into())),
    };
    Ok(home.join(root).join(tail))
}
pub(super) fn collect(home: &Path, db: &Path, specs: &[SourceSpec]) -> Result<Vec<Source>> {
    let controls = UnifiedControlFiles::inspect(db)?;
    let mut sources = vec![];
    let mut seen = BTreeSet::new();
    for spec in specs {
        match spec.pattern {
            SourcePattern::File(symbolic) => add(
                home,
                resolve(home, symbolic)?,
                symbolic.into(),
                *spec,
                &controls,
                &mut seen,
                &mut sources,
            )?,
            SourcePattern::Dir { dir, suffix } => walk(
                home,
                &resolve(home, dir)?,
                dir,
                suffix,
                *spec,
                &controls,
                &mut seen,
                &mut sources,
            )?,
            SourcePattern::Sqlite(_) => {}
        }
    }
    sources.sort_by(|a, b| a.symbolic.cmp(&b.symbolic));
    Ok(sources)
}
#[allow(clippy::too_many_arguments)]
fn add(
    home: &Path,
    path: PathBuf,
    symbolic: String,
    spec: SourceSpec,
    controls: &UnifiedControlFiles,
    seen: &mut BTreeSet<PathBuf>,
    out: &mut Vec<Source>,
) -> Result<()> {
    if controls.is_control(&path)? || !seen.insert(path.clone()) {
        return Ok(());
    }
    match fs::symlink_metadata(&path) {
        Ok(m) if m.is_dir() => Ok(()),
        Ok(m) if m.is_file() => {
            check_parents(home, &path)?;
            out.push(Source {
                path,
                symbolic,
                spec,
                db: controls.database_path().to_owned(),
            });
            Ok(())
        }
        Ok(_) => Err(Error::Validation(format!(
            "{}: fuente no regular; no se siguen enlaces",
            path.display()
        ))),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.into()),
    }
}
#[allow(clippy::too_many_arguments)]
fn walk(
    home: &Path,
    dir: &Path,
    symbolic: &str,
    suffix: &str,
    spec: SourceSpec,
    controls: &UnifiedControlFiles,
    seen: &mut BTreeSet<PathBuf>,
    out: &mut Vec<Source>,
) -> Result<()> {
    let metadata = match fs::symlink_metadata(dir) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e.into()),
    };
    if metadata.is_file()
        && crate::domains::catalog::source(symbolic)
            .is_some_and(|s| matches!(s.pattern, SourcePattern::File(_)))
    {
        // sizes existió como documento antes de tener entradas por modelo.
        return Ok(());
    }
    if !metadata.is_dir() {
        return Err(Error::Validation(format!(
            "{}: directorio de fuentes inválido",
            dir.display()
        )));
    }
    check_parents(home, &dir.join("entry"))?;
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| Error::Validation("fuente no UTF-8".into()))?;
        let child = format!("{symbolic}/{name}");
        if entry.file_type()?.is_dir() {
            walk(
                home,
                &entry.path(),
                &child,
                suffix,
                spec,
                controls,
                seen,
                out,
            )?;
        } else if name.ends_with(suffix) {
            add(home, entry.path(), child, spec, controls, seen, out)?;
        } else if entry.file_type()?.is_symlink() {
            return Err(Error::Validation(format!(
                "{}: enlace en directorio de fuentes",
                entry.path().display()
            )));
        }
    }
    Ok(())
}
pub(super) fn check_parents(root: &Path, path: &Path) -> Result<()> {
    let tail = path
        .strip_prefix(root)
        .map_err(|_| Error::Validation("ruta fuera de la raíz privada".into()))?;
    let mut p = root.to_owned();
    if !fs::symlink_metadata(root)?.is_dir() {
        return Err(Error::Validation("raíz no regular".into()));
    }
    for part in tail.parent().unwrap_or(Path::new("")).components() {
        if !matches!(part, std::path::Component::Normal(_)) {
            return Err(Error::Validation("componente de ruta inválido".into()));
        }
        p.push(part);
        if !fs::symlink_metadata(&p)?.is_dir() {
            return Err(Error::Validation(format!(
                "{}: padre no regular",
                p.display()
            )));
        }
    }
    Ok(())
}
pub(super) fn lock(source: &Source) -> Result<FileLock> {
    Ok(FileLock::exclusive(&suffix(&source.path, ".lock"))?)
}
pub(super) fn read(path: &Path) -> Result<Snapshot> {
    let before = fs::symlink_metadata(path)?;
    if !before.is_file() {
        return Err(Error::Validation(format!(
            "{}: fuente no regular",
            path.display()
        )));
    }
    let mut file = fs::File::open(path)?;
    let opened = file.metadata()?;
    if identity(&before) != identity(&opened) {
        return Err(Error::Validation("fuente sustituida al abrir".into()));
    }
    let mut body = vec![];
    file.read_to_end(&mut body)?;
    let after = file.metadata()?;
    let entry = fs::symlink_metadata(path)?;
    if identity(&before) != identity(&after)
        || identity(&before) != identity(&entry)
        || body.len() as u64 != before.len()
    {
        return Err(Error::Validation(format!(
            "{}: cambió durante la lectura",
            path.display()
        )));
    }
    let mtime_ns = before
        .mtime()
        .checked_mul(1_000_000_000)
        .and_then(|n| n.checked_add(before.mtime_nsec()))
        .ok_or_else(|| Error::Validation("mtime fuera de rango".into()))?;
    Ok(Snapshot {
        sha256: hash(&body),
        body,
        mtime_ns,
        mode: before.mode() & 0o7777,
        identity: identity(&before),
    })
}
pub(super) fn read_source(home: &Path, source: &Source) -> Result<Snapshot> {
    check_parents(home, &source.path)?;
    let controls = UnifiedControlFiles::inspect(&source.db)?;
    if controls.is_control(&source.path)? {
        return Err(Error::Validation(format!(
            "{}: fuente sustituida por control",
            source.path.display()
        )));
    }
    let snapshot = read(&source.path)?;
    let controls = UnifiedControlFiles::inspect(&source.db)?;
    if controls.has_regular_identity(snapshot.identity.0, snapshot.identity.1)
        || controls.is_control(&source.path)?
    {
        return Err(Error::Validation(format!(
            "{}: identidad de control durante lectura",
            source.path.display()
        )));
    }
    Ok(snapshot)
}
fn identity(m: &fs::Metadata) -> (u64, u64, u64, i64, i64) {
    (m.dev(), m.ino(), m.len(), m.mtime(), m.mtime_nsec())
}
pub(super) fn hash(body: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(body))
}
pub(super) fn doc_name(symbolic: &str) -> String {
    if let Some(tail) = symbolic.strip_prefix("H/") {
        format!("hooks/{tail}")
    } else if let Some(tail) = symbolic.strip_prefix("STATE/") {
        format!("state/{tail}")
    } else {
        format!(
            "share/{}",
            symbolic.strip_prefix("SHARE/").unwrap_or(symbolic)
        )
    }
}
pub(super) fn symbolic(home: &Path, path: &Path) -> Result<String> {
    for (prefix, root) in [
        ("H", ".claude/hooks"),
        ("STATE", ".local/state/comandos"),
        ("SHARE", ".local/share/comandos"),
    ] {
        if let Ok(tail) = path.strip_prefix(home.join(root)) {
            if tail
                .components()
                .any(|c| !matches!(c, std::path::Component::Normal(_)))
            {
                return Err(Error::Validation("fuente de respaldo inválida".into()));
            }
            return Ok(format!(
                "{prefix}/{}",
                tail.to_str()
                    .ok_or_else(|| Error::Validation("fuente no UTF-8".into()))?
            ));
        }
    }
    Err(Error::Validation("fuente de respaldo fuera de HOME".into()))
}
