//! Ensayo sobre SQLite temporal; no abre el SQLite fuente ni crea su SHM.
use super::{MigrateOptions, MigrateReport, journal, process, sources};
use crate::{
    Error, Result,
    domains::catalog::{DOMAINS, SourceSpec},
    files::write_atomic,
    unified::{self, Mode},
};
use rusqlite::backup::Backup;
use std::{
    fs,
    os::unix::fs::{DirBuilderExt, PermissionsExt},
    path::{Path, PathBuf},
    time::Duration,
};
pub(super) struct Scratch(pub(super) PathBuf);
impl Scratch {
    pub(super) fn new(home: &Path) -> Result<Self> {
        let home = fs::canonicalize(home)?;
        // TMPDIR puede caer en HOME: se ignora y se valida físicamente el padre.
        for root in [Path::new("/tmp"), Path::new("/var/tmp")] {
            let root = fs::canonicalize(root)?;
            if root.starts_with(&home) {
                continue;
            }
            for _ in 0..8 {
                let id = journal::new_id(journal::now_ms()?)?;
                let path = root.join(format!("comandos-dry-{}-{id}", std::process::id()));
                match fs::DirBuilder::new().mode(0o700).create(&path) {
                    Ok(()) => return Ok(Self(path)),
                    Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
                    Err(e) => return Err(e.into()),
                }
            }
        }
        Err(Error::Validation(
            "no hay scratch privado fuera de HOME".into(),
        ))
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn optional(path: &Path) -> Result<Option<sources::Snapshot>> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(Some(sources::read(path)?)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}
fn same(a: &Option<sources::Snapshot>, b: &Option<sources::Snapshot>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => a.identity == b.identity && a.sha256 == b.sha256,
        _ => false,
    }
}
fn validate_stage(
    db: &Path,
    original: &Option<sources::Snapshot>,
    wal: &Option<sources::Snapshot>,
    journal: &Option<sources::Snapshot>,
) -> Result<()> {
    if !same(original, &optional(db)?)
        || !same(wal, &optional(&sources::suffix(db, "-wal"))?)
        || !same(journal, &optional(&sources::suffix(db, "-journal"))?)
    {
        return Err(Error::Validation(
            "DB/WAL/journal cambió durante staging".into(),
        ));
    }
    Ok(())
}
pub(super) fn with_snapshot<T>(
    home: &Path,
    db: &Path,
    body: impl FnOnce(&rusqlite::Connection) -> Result<T>,
) -> Result<T> {
    let scratch = Scratch::new(home)?;
    let copy_path = scratch.0.join("import.sqlite3");
    let _guard = match fs::File::open(sources::suffix(db, ".domain-modes.lock")) {
        Ok(file) => {
            file.lock()?;
            Some(file)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(e.into()),
    };
    let original = optional(db)?;
    let wal_path = sources::suffix(db, "-wal");
    let wal = optional(&wal_path)?;
    let journal = optional(&sources::suffix(db, "-journal"))?;
    if original.is_none() && wal.is_some() {
        return Err(Error::Validation("WAL sin base; no se omite".into()));
    }
    let conn = if let Some(source_db) = &original {
        let parent = db
            .parent()
            .ok_or_else(|| Error::Validation("base sin padre".into()))?;
        if source_db.mode != 0o600 || fs::metadata(parent)?.permissions().mode() & 0o7777 != 0o700 {
            return Err(Error::Validation(
                "base fuente sin privacidad 0600/0700".into(),
            ));
        }
        if journal.as_ref().is_some_and(|s| !s.body.is_empty()) {
            return Err(Error::Validation(
                "journal activo: ensayo requiere snapshot estable".into(),
            ));
        }
        let staged_path = scratch.0.join("staged.sqlite3");
        write_atomic(&staged_path, &source_db.body)?;
        if let Some(wal) = &wal {
            write_atomic(&sources::suffix(&staged_path, "-wal"), &wal.body)?;
        }
        // La segunda lectura prueba identidad, bytes y presencia de DB/WAL alrededor de la copia.
        validate_stage(db, &original, &wal, &journal)?;
        let staged = unified::open_unified(&staged_path)?;
        let mut target = unified::open_unified(&copy_path)?;
        Backup::new(&staged, &mut target)?.run_to_completion(64, Duration::from_millis(1), None)?;
        target
    } else {
        validate_stage(db, &original, &wal, &journal)?;
        unified::open_unified(&copy_path)?
    };
    for domain in DOMAINS {
        let sealed = unified::modes::guarded(db, domain.name)?;
        let mode = unified::mode_of(Some(&conn), domain.name)?;
        if sealed != (mode == Mode::Sealed) {
            return Err(Error::Validation(format!(
                "{}: guardia y modo sin confirmar",
                domain.name
            )));
        }
    }
    body(&conn)
}
pub(super) fn run(opts: &MigrateOptions, specs: &[SourceSpec]) -> Result<MigrateReport> {
    let sources = sources::collect(&opts.home, &opts.db, specs)?;
    let usage = super::spec_for(&opts.home, "db-usage")?;
    let usage_estimate = match fs::symlink_metadata(&usage.legacy) {
        Ok(_)
            if !crate::domains::catalog::UnifiedControlFiles::inspect(&opts.db)?
                .is_control(&usage.legacy)? =>
        {
            Some(super::estimate_at_home(&opts.home, &usage)?.copy_ms)
        }
        Ok(_) => None,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(e.into()),
    };
    with_snapshot(&opts.home, &opts.db, |conn| {
        let run_id = if opts.resume {
            let (id, backup) = journal::latest_running(conn)?;
            super::verify_backup(&backup.join("manifest.json"))?;
            id
        } else {
            let id = journal::new_id(opts.now_ms)?;
            journal::start(conn, &id, Path::new("dry-run"), opts.now_ms)?;
            id
        };
        let mut report = MigrateReport {
            run_id,
            backup_dir: None,
            steps: vec![],
            usage_move_estimate_ms: usage_estimate,
        };
        process(opts, conn, &sources, &mut report, true)?;
        Ok(report)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn staging_rejects_wal_presence_changes_and_entry_replacement() {
        let home = std::env::temp_dir().join(format!(
            "comandos-staging-{}-{}",
            std::process::id(),
            journal::new_id(journal::now_ms().unwrap()).unwrap()
        ));
        fs::create_dir(&home).unwrap();
        let db = home.join("db");
        fs::write(&db, b"original").unwrap();
        let original = optional(&db).unwrap();
        let wal = sources::suffix(&db, "-wal");
        fs::write(&wal, b"new WAL").unwrap();
        assert!(validate_stage(&db, &original, &None, &None).is_err());
        fs::remove_file(&wal).unwrap();
        let swap = home.join("swap");
        fs::write(&swap, b"original").unwrap();
        fs::rename(swap, &db).unwrap();
        assert!(validate_stage(&db, &original, &None, &None).is_err());
        fs::remove_dir_all(home).unwrap();
    }
    #[test]
    fn staging_rejects_journal_created_after_copy() {
        let home = std::env::temp_dir().join(format!(
            "comandos-staging-journal-{}-{}",
            std::process::id(),
            journal::new_id(journal::now_ms().unwrap()).unwrap()
        ));
        fs::create_dir(&home).unwrap();
        let db = home.join("db");
        fs::write(&db, b"original").unwrap();
        let original = optional(&db).unwrap();
        fs::write(sources::suffix(&db, "-journal"), b"hot journal").unwrap();
        assert!(validate_stage(&db, &original, &None, &None).is_err());
        fs::remove_dir_all(home).unwrap();
    }
}
