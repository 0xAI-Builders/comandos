//! Migración explícita de archivos y traslado reversible de bases SQLite.
pub mod backup;
mod dry_run;
mod import;
pub mod move_db;
pub use move_db::{
    DbLocation, Marker, MoveEstimate, MoveSpec, demote_db, estimate, estimate_at_home, move_db,
    move_estimate, resolve_configured, resolve_db, spec_for,
};
pub mod journal;
mod sources;
#[cfg(test)]
mod tests;
mod verify;
use crate::{
    Error, Result,
    domains::catalog::{SourceSpec, TargetKind, catalog, domain},
    files::FileLock,
    unified,
};
pub use backup::{BackupEntry, BackupManifest, EntryKind, backup_sources, verify_backup};
use rusqlite::Connection;
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
};
pub use verify::{VerifyReport, verify};
pub struct MigrateOptions {
    pub home: PathBuf,
    pub db: PathBuf,
    pub dry_run: bool,
    pub resume: bool,
    pub domains: Option<Vec<String>>,
    pub now_ms: i64,
}
#[derive(Debug)]
pub struct StepReport {
    pub domain: String,
    pub source: String,
    pub status: String,
    pub rows: u64,
    pub bytes: u64,
    pub elapsed_ms: u64,
    pub detail: String,
}
impl StepReport {
    pub fn json(&self) -> Value {
        json!({"domain":self.domain,"source":self.source,"status":self.status,"rows":self.rows,"bytes":self.bytes,"elapsed_ms":self.elapsed_ms,"detail":self.detail})
    }
}
#[derive(Debug)]
pub struct MigrateReport {
    pub run_id: String,
    pub backup_dir: Option<PathBuf>,
    pub steps: Vec<StepReport>,
    pub usage_move_estimate_ms: Option<u64>,
}
impl MigrateReport {
    pub fn json(&self) -> Value {
        json!({"run_id":self.run_id,"backup_dir":self.backup_dir,"steps":self.steps.iter().map(StepReport::json).collect::<Vec<_>>(),"usage_move_estimate_ms":self.usage_move_estimate_ms})
    }
}
pub(crate) fn specs(opts: &MigrateOptions) -> Result<Vec<SourceSpec>> {
    if let Some(names) = &opts.domains {
        for name in names {
            if domain(name).is_none() {
                return Err(Error::Validation(format!("dominio desconocido: {name}")));
            }
            if name.starts_with("db-") {
                return Err(Error::Validation(format!(
                    "{name}: use state move para trasladar SQLite"
                )));
            }
        }
    }
    Ok(catalog()
        .iter()
        .filter(|s| {
            s.kind != TargetKind::Sqlite
                && opts
                    .domains
                    .as_ref()
                    .is_none_or(|names| names.iter().any(|name| name == s.domain))
        })
        .copied()
        .collect())
}
pub fn migrate(opts: &MigrateOptions) -> Result<MigrateReport> {
    if opts.now_ms < 0 || !opts.home.is_absolute() || !opts.db.is_absolute() {
        return Err(Error::Validation(
            "migrate requiere rutas absolutas y fecha válida".into(),
        ));
    }
    let specs = specs(opts)?;
    if opts.dry_run {
        return dry_run::run(opts, &specs);
    }
    // El preflight usa un snapshot temporal: ni una base futura ni un sellado
    // sin base provocan creación de SQLite, WAL/SHM, candados o respaldos.
    let preflight = MigrateOptions {
        home: opts.home.clone(),
        db: opts.db.clone(),
        dry_run: true,
        resume: false,
        domains: None,
        now_ms: opts.now_ms,
    };
    let preflight_report = dry_run::run(&preflight, &[])?;
    let conn = unified::open_unified(&opts.db)?;
    let _migration_lock = FileLock::exclusive(&sources::suffix(&opts.db, ".migration.lock"))?;
    let sources = sources::collect(&opts.home, &opts.db, &specs)?;
    let (run_id, backup_dir) = if opts.resume {
        let (run_id, backup_dir) = journal::latest_running(&conn)?;
        sources::check_parents(&opts.home, &backup_dir.join("manifest.json"))?;
        if !backup_dir.starts_with(opts.home.join(".local/share/comandos/backups")) {
            return Err(Error::Validation("respaldo del run fuera de HOME".into()));
        }
        verify_backup(&backup_dir.join("manifest.json"))?;
        backup::extend_backup(&opts.home, &backup_dir, &sources)?;
        (run_id, backup_dir)
    } else {
        let run_id = journal::new_id(opts.now_ms)?;
        let backup_dir = opts
            .home
            .join(".local/share/comandos/backups")
            .join(&run_id);
        backup::create(&opts.home, &backup_dir, &sources, opts.now_ms)?;
        verify_backup(&backup_dir.join("manifest.json"))?;
        journal::start(&conn, &run_id, &backup_dir, opts.now_ms)?;
        (run_id, backup_dir)
    };
    let mut report = MigrateReport {
        run_id,
        backup_dir: Some(backup_dir),
        steps: vec![],
        usage_move_estimate_ms: preflight_report.usage_move_estimate_ms,
    };
    process(opts, &conn, &sources, &mut report, false)?;
    let manifest = backup::load(
        &report
            .backup_dir
            .as_ref()
            .ok_or_else(|| Error::Validation("run sin respaldo".into()))?
            .join("manifest.json"),
    )?;
    // Un recibo antiguo no prueba que la fuente actual esté importada. El
    // candado global evita cambios de modo y todos los flocks se conservan
    // hasta confirmar el run, en el mismo orden que los importadores.
    let (_, _mode_lock) = unified::modes::access_mode(&opts.home, Some(&conn), "tabs")?;
    let mut current = BTreeMap::new();
    for entry in manifest.entries {
        let symbolic = sources::symbolic(&opts.home, &entry.source)?;
        let spec = crate::domains::catalog::source(&symbolic)
            .ok_or_else(|| Error::Validation("fuente de respaldo fuera del catálogo".into()))?;
        current.entry(symbolic.clone()).or_insert(sources::Source {
            path: entry.source,
            symbolic,
            spec: *spec,
            db: opts.db.clone(),
        });
    }
    let mut complete = true;
    let mut source_locks = vec![];
    for source in current.values() {
        let mode = unified::mode_of(Some(&conn), source.spec.domain)?;
        if unified::modes::guarded(&opts.db, source.spec.domain)? != (mode == unified::Mode::Sealed)
        {
            return Err(Error::Validation(
                "guardia y modo sin confirmar al finalizar".into(),
            ));
        }
        if matches!(mode, unified::Mode::Unified | unified::Mode::Sealed) {
            continue;
        }
        // No recrear el directorio/archivo de una fuente desaparecida.
        match std::fs::symlink_metadata(&source.path) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                complete = false;
                continue;
            }
            Err(e) => return Err(e.into()),
            Ok(_) => {}
        }
        sources::check_parents(&opts.home, &source.path)?;
        source_locks.push(sources::lock(source)?);
        let snapshot = match sources::read_source(&opts.home, source) {
            Ok(snapshot) => snapshot,
            Err(Error::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => {
                complete = false;
                continue;
            }
            Err(e) => return Err(e),
        };
        complete &= journal::unchanged(
            &conn,
            &report.run_id,
            source.spec.domain,
            &source.symbolic,
            &snapshot.sha256,
        )?;
    }
    if complete {
        journal::finish(&conn, &report.run_id, opts.now_ms)?;
    } else {
        report.steps.push(StepReport {
            domain: "migration".into(),
            source: String::new(),
            status: "skipped".into(),
            rows: 0,
            bytes: 0,
            elapsed_ms: 0,
            detail: "run sigue running: quedan fuentes pendientes fuera de esta selección".into(),
        });
    }
    Ok(report)
}
fn process(
    opts: &MigrateOptions,
    conn: &Connection,
    sources: &[sources::Source],
    report: &mut MigrateReport,
    dry: bool,
) -> Result<()> {
    let domains: BTreeSet<_> = specs(opts)?.iter().map(|s| s.domain).collect();
    for domain in domains {
        if !dry {
            unified::modes::prepare_mirror(&opts.home, conn, domain, opts.now_ms)?;
        }
        for source in sources.iter().filter(|s| s.spec.domain == domain) {
            report
                .steps
                .push(import::source(opts, conn, source, &report.run_id, dry)?);
        }
    }
    Ok(())
}
