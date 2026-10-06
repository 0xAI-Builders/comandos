//! Copia para ensayo: solo lee el origen, sin crear sus candados ni SQLite.
use super::sources;
use crate::{
    Error, Result,
    domains::catalog::{SourcePattern, catalog},
    unified,
};
use std::{
    fs,
    os::unix::fs::{DirBuilderExt, PermissionsExt},
    path::Path,
};
pub fn copy_home(source: &Path, dest: &Path) -> Result<()> {
    if !source.is_absolute()
        || !dest.is_absolute()
        || dest.starts_with(source)
        || source.starts_with(dest)
        || !fs::symlink_metadata(dest)?.is_dir()
        || fs::read_dir(dest)?.next().is_some()
    {
        return Err(Error::Validation(
            "ensayo requiere destino vacío, independiente y absoluto".into(),
        ));
    }
    let db = unified::unified_path(source);
    for s in sources::collect(source, &db, catalog())? {
        let snapshot = sources::read_source(source, &s)?;
        let path = sources::resolve(dest, &s.symbolic)?;
        private_parent(dest, &path)?;
        crate::files::write_atomic(&path, &snapshot.body)?;
        let stamp = std::time::UNIX_EPOCH
            .checked_add(std::time::Duration::from_nanos(
                u64::try_from(snapshot.mtime_ns)
                    .map_err(|_| Error::Validation("mtime negativo".into()))?,
            ))
            .ok_or_else(|| Error::Validation("mtime inválido".into()))?;
        fs::File::open(&path)?.set_times(fs::FileTimes::new().set_modified(stamp))?;
    }
    for spec in catalog() {
        if let SourcePattern::Sqlite(symbolic) = spec.pattern {
            let path = sources::resolve(source, symbolic)?;
            match fs::symlink_metadata(&path) {
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                Err(e) => return Err(e.into()),
                Ok(m) if !m.is_file() => {
                    return Err(Error::Validation("SQLite origen no regular".into()));
                }
                Ok(_) => {}
            }
            let target = sources::resolve(dest, symbolic)?;
            private_parent(dest, &target)?;
            unified::with_readonly_db(source, &path, |c| {
                let mut copy = rusqlite::Connection::open(&target)?;
                let backup = rusqlite::backup::Backup::new(c, &mut copy)?;
                backup.run_to_completion(128, std::time::Duration::from_millis(1), None)?;
                Ok(())
            })?;
            fs::set_permissions(&target, fs::Permissions::from_mode(0o600))?;
        }
    }
    Ok(())
}
fn private_parent(root: &Path, path: &Path) -> Result<()> {
    let tail = path
        .strip_prefix(root)
        .map_err(|_| Error::Validation("ruta de copia fuera de raíz".into()))?;
    let mut dir = root.to_owned();
    for part in tail.parent().unwrap_or(Path::new("")).components() {
        if !matches!(part, std::path::Component::Normal(_)) {
            return Err(Error::Validation("ruta de copia inválida".into()));
        }
        dir.push(part);
        match fs::symlink_metadata(&dir) {
            Ok(m) if m.is_dir() => {}
            Ok(_) => return Err(Error::Validation("padre de copia no regular".into())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                fs::DirBuilder::new().mode(0o700).create(&dir)?
            }
            Err(e) => return Err(e.into()),
        }
    }
    Ok(())
}
/// Escrituras sintéticas en las fuentes existentes de la copia privada.
/// Usa las mismas tablas, flocks y orden mirror/unified del contrato de dominio.
pub fn synthetic_write(
    home: &Path,
    c: &rusqlite::Connection,
    phase: &str,
    now: i64,
) -> Result<usize> {
    use crate::{
        domains::{DocHandle, LogHandle, StatusDir},
        unified::{LogName, Mode, Origin},
    };
    let path = unified::modes::connection_path(c)
        .ok_or_else(|| Error::Validation("ensayo sin base en disco".into()))?;
    let mut count = 0;
    for s in sources::collect(home, &path, catalog())? {
        let snapshot = sources::read_source(home, &s)?;
        let mut body = snapshot.body.clone();
        body.push(b'\n');
        match s.spec.kind {
            crate::domains::catalog::TargetKind::Document => {
                let key = sources::doc_name(&s.symbolic);
                DocHandle {
                    home,
                    name: &key,
                    domain: s.spec.domain,
                    file: s.path.clone(),
                    lock: sources::suffix(&s.path, ".lock"),
                }
                .write(Some(c), &body, now)?;
            }
            crate::domains::catalog::TargetKind::SessionStatus => {
                StatusDir {
                    home,
                    dir: home.join(".claude/hooks/state"),
                    domain: "session-status",
                }
                .write(
                    Some(c),
                    s.symbolic
                        .strip_prefix("H/state/")
                        .ok_or_else(|| Error::Validation("status sin clave".into()))?,
                    &body,
                    now * 1_000_000,
                )?;
            }
            crate::domains::catalog::TargetKind::LogLines => {
                let log = match super::import::log_name(&s.symbolic)? {
                    "events" => LogName::Events,
                    "ui-events" => LogName::UiEvents,
                    _ => LogName::FocusQueue,
                };
                LogHandle {
                    home,
                    file: s.path.clone(),
                    lock: sources::suffix(&s.path, ".lock"),
                    log,
                }
                .append(Some(c), format!("{{\"drill\":\"{phase}\"}}").as_bytes())?;
            }
            kind => {
                let (mode, _guard) = unified::modes::access_mode(home, Some(c), s.spec.domain)?;
                let _lock = sources::lock(&s)?;
                let write_db = || -> Result<()> {
                    match kind {
                        crate::domains::catalog::TargetKind::NativeProcess => {
                            let pid = s
                                .path
                                .file_stem()
                                .and_then(|x| x.to_str())
                                .ok_or_else(|| Error::Validation("pid inválido".into()))?
                                .parse::<i64>()
                                .map_err(|e| Error::Validation(e.to_string()))?;
                            c.execute("INSERT INTO native_processes VALUES(?1,?2,?3,?4) ON CONFLICT(pid) DO UPDATE SET body=excluded.body,mtime_ns=excluded.mtime_ns,origin=excluded.origin",rusqlite::params![pid,body,now*1_000_000,if mode==Mode::Mirror{Origin::Mirror.as_str()}else{Origin::Unified.as_str()}])?;
                        }
                        crate::domains::catalog::TargetKind::LayoutSnapshot => {
                            let (generation, stamp) = super::import::layout_key(&s, &snapshot)?;
                            c.execute("UPDATE layout_snapshots SET body=?3 WHERE generation=?1 AND stamp=?2",rusqlite::params![generation,stamp,body])?;
                        }
                        crate::domains::catalog::TargetKind::AppCommand => {
                            let kind = match super::import::command_kind(&s.symbolic)? {
                                "focus" => unified::CommandKind::Focus,
                                "open" => unified::CommandKind::Open,
                                "close" => unified::CommandKind::Close,
                                "command" => unified::CommandKind::Command,
                                _ => unified::CommandKind::Back,
                            };
                            unified::command_push(c, kind, &body, now)?;
                        }
                        _ => return Err(Error::Validation("tipo sintético inválido".into())),
                    }
                    Ok(())
                };
                if mode == Mode::Mirror {
                    crate::files::write_atomic(&s.path, &body)?;
                    write_db()?;
                } else if mode == Mode::Unified {
                    write_db()?;
                    crate::files::write_atomic(&s.path, &body)?;
                } else {
                    return Err(Error::Validation(
                        "escritura de ensayo fuera de mirror/unified".into(),
                    ));
                }
            }
        }
        count += 1;
    }
    Ok(count)
}
