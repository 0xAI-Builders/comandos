//! Ciclo de vida explícito. El candado de modo cubre comprobación y cambio.
use super::{backup, sources, verify::verify_locked};
use crate::{
    Error, Result,
    domains::catalog::{TargetKind, catalog, domain},
    files::{FileLock, write_atomic},
    unified::{self, Mode},
};
use rusqlite::{Connection, params};
use std::{
    collections::BTreeMap,
    fs,
    os::unix::fs::DirBuilderExt,
    path::{Component, Path, PathBuf},
};
const DAY: i64 = 86_400_000;
fn reject(message: impl Into<String>) -> Error {
    Error::Validation(message.into())
}
fn file_domain(name: &str) -> Result<()> {
    if domain(name).is_none() || name.starts_with("db-") {
        return Err(reject("operación requiere dominio de archivos conocido"));
    }
    Ok(())
}
fn clean(home: &Path, c: &Connection, name: &str) -> Result<()> {
    let report = verify_locked(home, c, name)?;
    if !report.mismatches.is_empty() {
        return Err(reject(format!(
            "{name}: verify no limpio: {:?}",
            report.mismatches
        )));
    }
    Ok(())
}
pub fn flip(home: &Path, c: &Connection, name: &str, now: i64) -> Result<()> {
    file_domain(name)?;
    let (mode, _guard) = unified::modes::access_mode(home, Some(c), name)?;
    if mode != Mode::Mirror {
        return Err(reject("flip requiere mirror"));
    }
    let changed: i64 = c.query_row(
        "SELECT changed_at_ms FROM domain_modes WHERE domain=?1",
        [name],
        |r| r.get(0),
    )?;
    let mut times = vec![];
    for row in c.prepare("SELECT finished_at_ms,status,detail FROM migration_steps WHERE domain=?1 AND finished_at_ms>=?2 AND finished_at_ms<=?3 ORDER BY finished_at_ms")?.query_map(params![format!("verify:{name}"),changed,now],|r|Ok((r.get::<_,i64>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?)))? {
 let (time,status,detail)=row?; if status == "done" && detail == "[]" {times.push(time);}else{times.clear();}
 }
    times.dedup();
    if times.len() < 3
        || times
            .last()
            .zip(times.first())
            .is_none_or(|(last, first)| last - first < DAY)
    {
        return Err(reject(
            "flip requiere tres verify limpios durante al menos 24 horas en mirror",
        ));
    }
    clean(home, c, name)?;
    unified::modes::set_mode_locked(c, name, Mode::Unified, "state flip", now)
}
pub fn demote(home: &Path, c: &Connection, name: &str, now: i64) -> Result<()> {
    file_domain(name)?;
    let (mode, _guard) = unified::modes::access_mode(home, Some(c), name)?;
    if mode == Mode::Sealed {
        return Err(reject("sealed requiere export-legacy antes de demote"));
    }
    if mode != Mode::Unified {
        return Ok(());
    }
    clean(home, c, name)?;
    unified::modes::set_mode_locked(c, name, Mode::Mirror, "state demote", now)
}
fn symbolic_doc(name: &str) -> Result<String> {
    for (prefix, root) in [("hooks/", "H/"), ("state/", "STATE/"), ("share/", "SHARE/")] {
        if let Some(tail) = name.strip_prefix(prefix) {
            if tail.is_empty()
                || Path::new(tail)
                    .components()
                    .any(|x| !matches!(x, Component::Normal(_)))
            {
                return Err(reject("clave de documento inválida"));
            }
            return Ok(format!("{root}{tail}"));
        }
    }
    Err(reject("raíz de documento inválida"))
}
fn rows(c: &Connection, name: &str) -> Result<BTreeMap<String, Vec<u8>>> {
    let mut out = BTreeMap::new();
    for row in c
        .prepare("SELECT name,body FROM documents WHERE domain=?1 ORDER BY name")?
        .query_map([name], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, Vec<u8>>(1)?))
        })?
    {
        let (key, body) = row?;
        out.insert(symbolic_doc(&key)?, body);
    }
    let query = match name {
        "session-status" => Some("SELECT 'H/state/'||file_key,body FROM session_status"),
        "processes" => {
            Some("SELECT 'H/native-processes/'||pid||'.json',body FROM native_processes")
        }
        "layout" => Some(
            "SELECT CASE generation WHEN 'current' THEN 'H/app-sessions-v2.json' WHEN 'previous' THEN 'H/app-sessions-v2.json.bak' ELSE 'H/app-sessions-v2.json.history/'||stamp||'.json' END,body FROM layout_snapshots ORDER BY stamp",
        ),
        "app-commands" => Some(
            "SELECT CASE kind WHEN 'focus' THEN 'H/app-focus.json' WHEN 'open' THEN 'H/app-tab-open.json' WHEN 'close' THEN 'H/app-tab-close.json' WHEN 'command' THEN 'H/app-command.json' WHEN 'back' THEN 'H/app-tab-back.json' END,body FROM app_commands ORDER BY seq",
        ),
        _ => None,
    };
    if let Some(query) = query {
        for row in c.prepare(query)?.query_map([], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, Vec<u8>>(1)?))
        })? {
            let (key, body) = row?;
            out.insert(key, body);
        }
    }
    if name == "logs" {
        // Un registro vacío no tiene líneas: su recibo conserva la presencia del archivo.
        for row in c.prepare("SELECT DISTINCT source FROM migration_steps WHERE domain='logs' AND status='done' AND source_size=0")?.query_map([], |r|r.get::<_,String>(0))? {
            let key=row?;
            if crate::domains::catalog::source(&key).is_some_and(|s|s.domain=="logs") {out.entry(key).or_default();}
        }
        for row in c
            .prepare("SELECT log,line FROM log_lines ORDER BY seq")?
            .query_map([], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, Vec<u8>>(1)?))
            })?
        {
            let (log, line) = row?;
            let file = match log.as_str() {
                "events" => "H/events.jsonl",
                "ui-events" => "H/ui-events.jsonl",
                "focus-queue" => "H/focus-queue.jsonl",
                _ => return Err(reject("registro desconocido")),
            };
            let body = out.entry(file.into()).or_default();
            body.extend(line);
            body.push(b'\n');
        }
    }
    for key in out.keys() {
        if Path::new(key)
            .components()
            .any(|x| !matches!(x, Component::Normal(_)))
            || crate::domains::catalog::source(key)
                .is_none_or(|s| s.domain != name || s.kind == TargetKind::Sqlite)
        {
            return Err(reject(format!("clave fuera del dominio: {key}")));
        }
    }
    Ok(out)
}
fn prepare_path(home: &Path, path: &Path) -> Result<()> {
    let tail = path
        .strip_prefix(home)
        .map_err(|_| reject("destino fuera de HOME"))?;
    if tail
        .components()
        .any(|x| !matches!(x, Component::Normal(_)))
    {
        return Err(reject("destino inválido"));
    }
    let mut dir = home.to_owned();
    if !fs::symlink_metadata(home)?.is_dir() {
        return Err(reject("HOME no regular"));
    }
    for part in tail.parent().unwrap_or(Path::new("")).components() {
        dir.push(part);
        match fs::symlink_metadata(&dir) {
            Ok(m) if m.is_dir() => {}
            Ok(_) => return Err(reject("padre no regular")),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                fs::DirBuilder::new().mode(0o700).create(&dir)?;
            }
            Err(e) => return Err(e.into()),
        }
    }
    match fs::symlink_metadata(path) {
        Ok(m) if !m.is_file() => Err(reject("destino no regular")),
        Ok(_) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.into()),
    }
}
pub fn export_legacy(home: &Path, c: &Connection, name: &str, now: i64) -> Result<()> {
    file_domain(name)?;
    let (mode, _guard) = unified::modes::access_mode(home, Some(c), name)?;
    if !matches!(mode, Mode::Unified | Mode::Sealed) {
        return Err(reject("export-legacy requiere unified o sealed"));
    }
    let rows = rows(c, name)?;
    let controls = crate::domains::catalog::UnifiedControlFiles::inspect(
        &unified::modes::connection_path(c).ok_or_else(|| reject("base sin ruta"))?,
    )?;
    for (key, body) in rows {
        let path = sources::resolve(home, &key)?;
        if controls.is_control(&path)? {
            return Err(reject(
                "exportación no puede sobrescribir controles de base",
            ));
        }
        prepare_path(home, &path)?;
        let _lock = FileLock::exclusive(&sources::suffix(&path, ".lock"))?;
        write_atomic(&path, &body)?;
    }
    clean(home, c, name)?;
    if mode == Mode::Sealed {
        unified::modes::set_mode_locked(c, name, Mode::Unified, "state export-legacy", now)?;
    }
    Ok(())
}
pub fn seal(home: &Path, c: &Connection, name: &str, now: i64) -> Result<PathBuf> {
    file_domain(name)?;
    let (mode, _guard) = unified::modes::access_mode(home, Some(c), name)?;
    if mode != Mode::Unified {
        return Err(reject("seal requiere unified"));
    }
    let changed: i64 = c.query_row(
        "SELECT changed_at_ms FROM domain_modes WHERE domain=?1",
        [name],
        |r| r.get(0),
    )?;
    if now.checked_sub(changed).is_none_or(|age| age < 7 * DAY) {
        return Err(reject("seal requiere siete días en unified"));
    }
    clean(home, c, name)?;
    let specs: Vec<_> = catalog()
        .iter()
        .filter(|s| s.domain == name)
        .copied()
        .collect();
    let dest = home
        .join(".local/share/comandos/backups")
        .join(format!("{}-seal-{name}", super::journal::new_id(now)?));
    let manifest = backup::backup_sources(home, &dest, &specs)?;
    backup::verify_backup(&dest.join("manifest.json"))?;
    // Confirmar autoridad sellada antes de retirar archivos; un corte nunca habilita legacy.
    unified::modes::set_mode_locked(c, name, Mode::Sealed, "state seal", now)?;
    for entry in manifest.entries {
        sources::check_parents(home, &entry.source)?;
        let _lock = FileLock::exclusive(&sources::suffix(&entry.source, ".lock"))?;
        fs::remove_file(&entry.source)?;
        if let Some(parent) = entry.source.parent() {
            fs::File::open(parent)?.sync_all()?;
        }
    }
    Ok(dest)
}
/// Valida todos antes de degradar cualquiera. SQLite tiene su propia copia inversa.
pub fn demote_all(home: &Path, c: &Connection, now: i64) -> Result<()> {
    let (_, guard) = unified::modes::access_mode(home, Some(c), "tabs")?;
    let mut names = vec![];
    for d in crate::domains::catalog::DOMAINS {
        let mode = unified::mode_of(Some(c), d.name)?;
        if mode == Mode::Sealed {
            return Err(reject(format!(
                "{}: sealed requiere export-legacy antes",
                d.name
            )));
        }
        if mode == Mode::Unified {
            if d.name.starts_with("db-") {
                return Err(reject(
                    "modo db unificado inesperado; demote individual requerido",
                ));
            }
            clean(home, c, d.name)?;
            names.push(d.name);
        }
    }
    for name in names {
        unified::modes::set_mode_locked(c, name, Mode::Mirror, "state demote --all", now)?;
    }
    drop(guard);
    let path = unified::modes::connection_path(c).ok_or_else(|| reject("base sin ruta"))?;
    for name in [
        "db-operator",
        "db-news",
        "db-operations",
        "db-app-state",
        "db-usage",
    ] {
        let spec = super::spec_for(home, name)?;
        if spec.legacy.exists() {
            super::demote_db(&spec, &path)?;
        }
    }
    Ok(())
}
pub struct RollbackReport {
    pub archived_db: PathBuf,
    pub data_lost_since_ms: i64,
    pub data_lost_until_ms: i64,
}
/// Recuperación destructiva, solo para un HOME sin escritores activos.
/// Conserva también una copia del estado actual antes de restaurar el respaldo.
pub fn rollback(home: &Path, path: &Path, id: &str, now: i64) -> Result<RollbackReport> {
    if !home.is_absolute()
        || !path.is_absolute()
        || id.is_empty()
        || Path::new(id).components().count() != 1
        || !matches!(
            Path::new(id).components().next(),
            Some(Component::Normal(_))
        )
    {
        return Err(reject("rollback requiere rutas absolutas y run_id válido"));
    }
    let c = unified::open_existing(path)?;
    let dir: String = c.query_row(
        "SELECT backup_dir FROM migration_runs WHERE run_id=?1 AND backup_dir<>''",
        [id],
        |r| r.get(0),
    )?;
    let dir = PathBuf::from(dir);
    let root = home.join(".local/share/comandos/backups");
    if dir != root.join(id) {
        return Err(reject("respaldo no coincide con run_id dentro de HOME"));
    }
    sources::check_parents(home, &dir.join("manifest.json"))?;
    backup::verify_backup(&dir.join("manifest.json"))?;
    let mut initial = backup::load(&dir.join("manifest.json"))?;
    // S4 mantiene respaldos separados: incorporar el primer traslado posterior al run.
    let moved:Vec<String>=c.prepare("SELECT DISTINCT r.backup_dir FROM migration_runs r JOIN migration_steps s ON s.run_id=r.run_id WHERE s.domain LIKE 'db-%' AND s.status='done' AND json_extract(s.detail,'$.target_committed_before_source_marker')=1 AND r.started_at_ms >= (SELECT started_at_ms FROM migration_runs WHERE run_id=?1) ORDER BY r.started_at_ms")?.query_map([id],|r|r.get(0))?.collect::<rusqlite::Result<_>>()?;
    for moved in moved {
        let dir = PathBuf::from(moved);
        if !dir.starts_with(&root) {
            return Err(reject("respaldo SQLite fuera de HOME"));
        }
        sources::check_parents(home, &dir.join("manifest.json"))?;
        backup::verify_backup(&dir.join("manifest.json"))?;
        for entry in backup::load(&dir.join("manifest.json"))?.entries {
            if entry.kind != backup::EntryKind::Sqlite {
                return Err(reject("recibo de traslado sin respaldo SQLite"));
            }
            if !initial.entries.iter().any(|e| e.source == entry.source) {
                initial.entries.push(entry);
            }
        }
    }
    // Un dominio sellado necesita primero una exportación explícita.
    demote_all(home, &c, now)?;
    let _migration = FileLock::exclusive(&sources::suffix(path, ".migration.lock"))?;
    let (_, guard) = unified::modes::access_mode(home, Some(&c), "tabs")?;
    let controls = crate::domains::catalog::UnifiedControlFiles::inspect(path)?;
    for entry in &initial.entries {
        let key = sources::symbolic(home, &entry.source)?;
        if crate::domains::catalog::source(&key).is_none() || controls.is_control(&entry.source)? {
            return Err(reject(
                "respaldo fuera del catálogo o contiene control de base",
            ));
        }
        prepare_path(home, &entry.source)?;
    }
    // Respaldar todas las fuentes actuales antes de cualquier pérdida.
    let specs: Vec<_> = catalog()
        .iter()
        .filter(|s| s.kind != TargetKind::Sqlite)
        .copied()
        .collect();
    let safety = root.join(format!("{}-pre-rollback", super::journal::new_id(now)?));
    backup::backup_sources(home, &safety, &specs)?;
    // Solo se retiran extras de los dominios que el run respaldó/importó.
    let affected:Vec<String>=c.prepare("SELECT DISTINCT domain FROM migration_steps WHERE run_id=?1 AND domain NOT LIKE 'verify:%'")?.query_map([id],|r|r.get(0))?.collect::<rusqlite::Result<_>>()?;
    let specs: Vec<_> = specs
        .into_iter()
        .filter(|s| affected.iter().any(|d| d == s.domain))
        .collect();
    for source in sources::collect(home, path, &specs)? {
        if !initial.entries.iter().any(|e| e.source == source.path) {
            let _lock = sources::lock(&source)?;
            fs::remove_file(&source.path)?;
        }
    }
    for entry in &initial.entries {
        if entry.kind == backup::EntryKind::Sqlite {
            let old = Connection::open_with_flags(
                &entry.source,
                rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE,
            )?;
            let (busy, _, _): (i64, i64, i64) =
                old.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |r| {
                    Ok((r.get(0)?, r.get(1)?, r.get(2)?))
                })?;
            if busy != 0 {
                return Err(reject("rollback SQLite requiere cerrar lectores"));
            }
            old.close().map_err(|(_, e)| Error::from(e))?;
            for suffix in ["-wal", "-shm"] {
                match fs::remove_file(sources::suffix(&entry.source, suffix)) {
                    Ok(()) => {}
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                    Err(e) => return Err(e.into()),
                }
            }
        }
        let _lock = FileLock::exclusive(&sources::suffix(&entry.source, ".lock"))?;
        let body = fs::read(&entry.copy)?;
        write_atomic(&entry.source, &body)?;
        fs::set_permissions(
            &entry.source,
            std::os::unix::fs::PermissionsExt::from_mode(entry.mode),
        )?;
        let file = fs::File::open(&entry.source)?;
        let stamp = std::time::UNIX_EPOCH
            .checked_add(std::time::Duration::from_nanos(
                u64::try_from(entry.mtime_ns).map_err(|_| reject("mtime de respaldo negativo"))?,
            ))
            .ok_or_else(|| reject("mtime inválido"))?;
        file.set_times(fs::FileTimes::new().set_modified(stamp))?;
        file.sync_all()?;
    }
    for d in crate::domains::catalog::DOMAINS {
        unified::modes::set_mode_locked(&c, d.name, Mode::Legacy, "state rollback", now)?;
    }
    let (busy, _, _): (i64, i64, i64) =
        c.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        })?;
    if busy != 0 {
        return Err(reject(
            "rollback requiere cerrar lectores SQLite antes de archivar la base",
        ));
    }
    let archived = sources::suffix(
        path,
        &format!(".rolled-back-{now}-{}", super::journal::new_id(now)?),
    );
    drop(c);
    fs::rename(path, &archived)?;
    if let Some(parent) = path.parent() {
        fs::File::open(parent)?.sync_all()?;
    }
    drop(guard);
    Ok(RollbackReport {
        archived_db: archived,
        data_lost_since_ms: initial.created_at_ms,
        data_lost_until_ms: now,
    })
}
