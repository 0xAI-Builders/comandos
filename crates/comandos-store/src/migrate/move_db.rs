//! Traslados SQLite explícitos; los cambios de modo pertenecen a S6.
use super::{
    backup::{self, BackupEntry, BackupManifest, EntryKind},
    journal, sources,
};
use crate::{Error, Result, unified};
use rusqlite::{
    Connection, OpenFlags, OptionalExtension, Transaction, TransactionBehavior,
    backup::{Backup, StepResult},
    params, params_from_iter,
    types::{ToSqlOutput, ValueRef},
};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::{
    collections::BTreeSet,
    fs,
    os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt},
    time::{Duration, Instant},
};
#[derive(Debug, PartialEq, Eq)]
pub enum DbLocation {
    Legacy(PathBuf),
    Unified(PathBuf),
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Marker {
    UserVersion,
    SchemaMigrationsRow,
}
pub struct MoveSpec {
    pub domain: &'static str,
    pub legacy: PathBuf,
    pub tables: &'static [(&'static str, &'static str)],
    pub marker: Marker,
}
#[derive(Debug)]
pub struct MoveEstimate {
    pub bytes: u64,
    pub copy_ms: u64,
}
const OPERATOR: &[(&str, &str)] = &[("actions", "operator_actions")];
const NEWS: &[(&str, &str)] = &[("events", "news_radar_events")];
const OPERATIONS: &[(&str, &str)] = &[
    ("session_operations", "session_operations"),
    ("session_operation_events", "session_operation_events"),
];
const APP_STATE: &[(&str, &str)] = &[
    ("workspace_current", "workspace_current"),
    ("workspace_previous", "workspace_previous"),
    ("workspace_requests", "workspace_requests"),
    ("workspace_clients", "workspace_clients"),
    ("workspace_meta", "workspace_meta"),
    ("events", "events"),
    ("event_receipts", "event_receipts"),
    ("deliveries", "deliveries"),
    ("work_marks", "work_marks"),
    ("work_mark_applied", "work_mark_applied"),
    ("pomodoro_state", "pomodoro_state"),
    ("pomodoro_blocks", "pomodoro_blocks"),
    ("pomodoro_requests", "pomodoro_requests"),
    ("pomodoro_records", "pomodoro_records"),
    ("focus_policies", "focus_policies"),
    ("focus_rewards", "focus_rewards"),
    ("news_editions", "news_editions"),
    ("news_jobs", "news_jobs"),
    ("news_sources", "news_sources"),
    ("news_stories", "news_stories"),
    ("news_story_sources", "news_story_sources"),
    ("quick_terminal_requests", "quick_terminal_requests"),
    ("push_subscriptions", "push_subscriptions"),
    ("push_deliveries", "push_deliveries"),
    ("client_presence", "client_presence"),
    ("notice_reads", "notice_reads"),
    ("notice_prefs", "notice_prefs"),
    ("news_captures", "news_captures"),
    ("news_translations", "news_translations"),
    ("news_chat", "news_chat"),
    ("news_notes", "news_notes"),
    ("news_saved", "news_saved"),
];
const USAGE: &[(&str, &str)] = &[
    ("usage_panes", "usage_panes"),
    ("usage_turns", "usage_turns"),
    ("provider_usage_buckets", "provider_usage_buckets"),
    ("provider_cost_buckets", "provider_cost_buckets"),
    ("usage_reconciliation", "usage_reconciliation"),
    ("usage_alerts", "usage_alerts"),
    ("model_presets", "model_presets"),
    ("usage_settings", "usage_settings"),
    ("usage_alert_rules", "usage_alert_rules"),
    ("usage_session_configs", "usage_session_configs"),
    ("usage_tasks", "usage_tasks"),
    ("usage_interactions", "usage_interactions"),
    ("usage_project_profiles", "usage_project_profiles"),
    ("usage_tool_calls", "usage_tool_calls"),
    ("usage_ratings", "usage_ratings"),
    ("usage_experiments", "usage_experiments"),
    ("usage_experiment_variants", "usage_experiment_variants"),
    ("usage_experiment_runs", "usage_experiment_runs"),
    ("usage_changes", "usage_changes"),
    ("focus_blocks", "focus_blocks"),
    ("focus_settings", "focus_settings"),
    ("usage_spans", "usage_spans"),
    ("usage_quota_snapshots", "usage_quota_snapshots"),
];
pub fn spec_for(home: &Path, domain: &str) -> Result<MoveSpec> {
    let (domain, relative, tables, marker) = match domain {
        "db-operator" => (
            "db-operator",
            ".claude/hooks/operator/actions.sqlite",
            OPERATOR,
            Marker::UserVersion,
        ),
        "db-news" => (
            "db-news",
            ".claude/hooks/news-history.sqlite",
            NEWS,
            Marker::UserVersion,
        ),
        "db-operations" => (
            "db-operations",
            ".claude/hooks/session-operations.sqlite3",
            OPERATIONS,
            Marker::UserVersion,
        ),
        "db-app-state" => (
            "db-app-state",
            ".local/state/comandos/app-state.sqlite3",
            APP_STATE,
            Marker::SchemaMigrationsRow,
        ),
        "db-usage" => (
            "db-usage",
            ".claude/hooks/comandos-usage.sqlite",
            USAGE,
            Marker::UserVersion,
        ),
        _ => {
            return Err(Error::Validation(format!(
                "dominio SQLite desconocido: {domain}"
            )));
        }
    };
    Ok(MoveSpec {
        domain,
        legacy: home.join(relative),
        tables,
        marker,
    })
}

fn validate_spec(spec: &MoveSpec) -> Result<()> {
    let known = spec_for(Path::new("/"), spec.domain)?;
    if !spec.legacy.is_absolute() || spec.tables != known.tables || spec.marker != known.marker {
        return Err(Error::Validation(
            "especificación SQLite fuera del catálogo".into(),
        ));
    }
    Ok(())
}
fn read_home(legacy: &Path) -> Result<PathBuf> {
    for suffix in [
        ".claude/hooks/operator/actions.sqlite",
        ".claude/hooks/news-history.sqlite",
        ".claude/hooks/session-operations.sqlite3",
        ".claude/hooks/comandos-usage.sqlite",
        ".local/state/comandos/app-state.sqlite3",
        ".local/share/comandos/comandos.sqlite3",
    ] {
        let suffix = Path::new(suffix);
        if legacy.ends_with(suffix) {
            let mut home = legacy.to_owned();
            for _ in suffix.components() {
                home.pop();
            }
            return Ok(home);
        }
    }
    legacy
        .parent()
        .map(Path::to_owned)
        .ok_or_else(|| Error::Validation("SQLite sin padre".into()))
}
fn marker(conn: &Connection) -> Result<bool> {
    let uv: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    if uv > crate::usage::SCHEMA_VERSION && uv != unified::MOVED_SENTINEL {
        return Err(Error::Validation(
            "base creada por una versión más nueva de ComandOS".into(),
        ));
    }
    let has:bool=conn.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='schema_migrations')",[],|r|r.get(0))?;
    let mut moved = uv == unified::MOVED_SENTINEL;
    if has {
        let mut statement = conn.prepare("SELECT version,name FROM schema_migrations")?;
        let rows =
            statement.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?;
        for row in rows {
            let (version, name) = row?;
            if version == 1000 && name == "moved-to-comandos.sqlite3" {
                moved = true;
            } else if !crate::state::MIGRATIONS
                .iter()
                .any(|m| m.version == version)
            {
                return Err(Error::Validation(
                    "base creada por una versión más nueva de ComandOS".into(),
                ));
            }
        }
    }
    Ok(moved)
}
/// Revalidar después de adquirir BEGIN IMMEDIATE evita escribir con una
/// conexión vieja que estuvo esperando el traslado. El llamador reabre por
/// resolve_db; esta puerta no redirige una transacción ya iniciada.
pub fn admit_write(conn: &Connection) -> Result<()> {
    let unified: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='domain_modes')",
        [],
        |r| r.get(0),
    )?;
    if unified {
        return unified::validate_schema(conn).map(|_| ());
    }
    if marker(conn)? {
        return Err(Error::Validation(
            "base movida: reabrir por resolve_db".into(),
        ));
    }
    Ok(())
}
fn source_domain(path: &Path) -> Option<&'static str> {
    match path.file_name()?.to_str()? {
        "actions.sqlite" => Some("db-operator"),
        "news-history.sqlite" => Some("db-news"),
        "session-operations.sqlite3" => Some("db-operations"),
        "app-state.sqlite3" => Some("db-app-state"),
        "comandos-usage.sqlite" => Some("db-usage"),
        _ => None,
    }
}
fn retry_resolution<T>(read: impl Fn() -> Result<T>) -> Result<T> {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let result = read();
        let unstable = matches!(&result, Err(Error::Validation(detail)) if detail=="DB/WAL/journal cambió durante validación" || detail=="journal activo: apertura requiere fuente estable");
        if !unstable || Instant::now() >= deadline {
            return result;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}
pub fn resolve_db(legacy: &Path, target: &Path) -> Result<DbLocation> {
    retry_resolution(|| resolve_db_snapshot(legacy, target))
}
fn resolve_db_snapshot(legacy: &Path, target: &Path) -> Result<DbLocation> {
    if !legacy.is_absolute() || !target.is_absolute() {
        return Err(Error::Validation(
            "resolver SQLite requiere rutas absolutas".into(),
        ));
    }
    let home = read_home(legacy)?;
    let domain = source_domain(legacy);
    if let Some(domain) = domain
        && unified::modes::guarded(target, domain)?
    {
        unified::with_readonly_unified(&home, target, |_| Ok(()))?;
        return Ok(DbLocation::Unified(target.to_owned()));
    }
    if let Some(domain) = domain
        && target.exists()
        && unified::with_readonly_unified(&home, target, |c| unified::mode_of(Some(c), domain))?
            == unified::Mode::Unified
    {
        return Ok(DbLocation::Unified(target.to_owned()));
    }
    match fs::symlink_metadata(legacy) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Ok(DbLocation::Legacy(legacy.to_owned()));
        }
        Err(e) => return Err(e.into()),
        Ok(_) => {}
    }
    let moved = unified::with_readonly_db(&home, legacy, marker)?;
    if moved {
        unified::with_readonly_unified(&home, target, |_| Ok(()))?;
        Ok(DbLocation::Unified(target.to_owned()))
    } else {
        Ok(DbLocation::Legacy(legacy.to_owned()))
    }
}
/// Configuración explícita o HOME derivado de la ruta original; no usa un HOME
/// global para adivinar el destino de una base de prueba o de otra instalación.
pub fn resolve_configured(legacy: &Path) -> Result<DbLocation> {
    retry_resolution(|| resolve_configured_snapshot(legacy))
}
fn resolve_configured_snapshot(legacy: &Path) -> Result<DbLocation> {
    if legacy == Path::new(":memory:") {
        return Ok(DbLocation::Legacy(legacy.to_owned()));
    }
    let path = if legacy.is_absolute() {
        legacy.to_owned()
    } else {
        std::env::current_dir()?.join(legacy)
    };
    let home = read_home(&path)?;
    let configured = std::env::var_os("COMANDOS_DB")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .map(|p| {
            if p.is_absolute() {
                Ok(p)
            } else {
                std::env::current_dir().map(|cwd| cwd.join(p))
            }
        })
        .transpose()?;
    let direct = path.ends_with(".local/share/comandos/comandos.sqlite3")
        || configured.as_ref().is_some_and(|p| {
            matches!((crate::domains::catalog::control_path_identity(p), crate::domains::catalog::control_path_identity(&path)), (Ok(a), Ok(b)) if a == b)
        });
    if direct {
        existing_target(&home, &path)?;
        return Ok(DbLocation::Unified(path));
    }
    let known = path.ends_with(".claude/hooks/operator/actions.sqlite")
        || path.ends_with(".claude/hooks/news-history.sqlite")
        || path.ends_with(".claude/hooks/session-operations.sqlite3")
        || path.ends_with(".claude/hooks/comandos-usage.sqlite")
        || path.ends_with(".local/state/comandos/app-state.sqlite3");
    if let Some(target) =
        configured.or_else(|| known.then(|| home.join(".local/share/comandos/comandos.sqlite3")))
    {
        return resolve_db_snapshot(&path, &target);
    }
    match fs::symlink_metadata(&path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Ok(DbLocation::Legacy(legacy.to_owned()));
        }
        Err(e) => return Err(e.into()),
        Ok(_) => {}
    }
    if unified::with_readonly_db(&home, &path, marker)? {
        return Err(Error::Validation(
            "base movida sin destino explícito ni ruta HOME reconocida".into(),
        ));
    }
    Ok(DbLocation::Legacy(legacy.to_owned()))
}
fn quote(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}
fn table_names(conn: &Connection) -> Result<BTreeSet<String>> {
    Ok(conn
        .prepare("SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%'")?
        .query_map([], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?)
}
fn validate_source(conn: &Connection, spec: &MoveSpec) -> Result<bool> {
    let moved = marker(conn)?;
    let mut expected: BTreeSet<String> =
        spec.tables.iter().map(|(from, _)| (*from).into()).collect();
    if spec.marker == Marker::SchemaMigrationsRow {
        expected.insert("schema_migrations".into());
    }
    if table_names(conn)? != expected {
        return Err(Error::Validation(format!(
            "{}: tablas SQLite no previstas; no se omiten datos",
            spec.domain
        )));
    }
    let uv: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    if !moved && uv != if spec.domain == "db-usage" { 11 } else { 0 } {
        return Err(Error::Validation(format!(
            "{}: versión fuente no compatible: {uv}",
            spec.domain
        )));
    }
    if spec.marker == Marker::SchemaMigrationsRow {
        let versions: BTreeSet<i64> = conn
            .prepare("SELECT version FROM schema_migrations WHERE version!=1000")?
            .query_map([], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        if versions != crate::state::MIGRATIONS.iter().map(|m| m.version).collect() {
            return Err(Error::Validation(
                "app-state requiere las migraciones liberadas 1–11".into(),
            ));
        }
    }
    Ok(moved)
}
type Column = (String, String, i64, Option<String>, i64);
fn columns(conn: &Connection, table: &str) -> Result<Vec<Column>> {
    Ok(conn
        .prepare(&format!("PRAGMA table_info({})", quote(table)))?
        .query_map([], |r| {
            Ok((
                r.get(1)?,
                r.get::<_, String>(2)?.to_ascii_uppercase(),
                r.get(3)?,
                r.get(4)?,
                r.get(5)?,
            ))
        })?
        .collect::<rusqlite::Result<_>>()?)
}
fn hash_value(hash: &mut Sha256, value: ValueRef<'_>) {
    match value {
        ValueRef::Null => hash.update([0]),
        ValueRef::Integer(v) => {
            hash.update([1]);
            hash.update(v.to_le_bytes());
        }
        ValueRef::Real(v) => {
            hash.update([2]);
            hash.update(v.to_bits().to_le_bytes());
        }
        ValueRef::Text(v) => {
            hash.update([3]);
            hash.update((v.len() as u64).to_le_bytes());
            hash.update(v);
        }
        ValueRef::Blob(v) => {
            hash.update([4]);
            hash.update((v.len() as u64).to_le_bytes());
            hash.update(v);
        }
    }
}
fn digest(conn: &Connection, table: &str) -> Result<(u64, String)> {
    let mut stmt = conn.prepare(&format!(
        "SELECT rowid,* FROM {} ORDER BY rowid",
        quote(table)
    ))?;
    let columns = stmt.column_count();
    let mut rows = stmt.query([])?;
    let mut count = 0;
    let mut hash = Sha256::new();
    while let Some(row) = rows.next()? {
        count += 1;
        for i in 0..columns {
            hash_value(&mut hash, row.get_ref(i)?);
        }
    }
    Ok((count, format!("{:x}", hash.finalize())))
}
struct CopyDeadline {
    cancel: std::sync::mpsc::Sender<()>,
    worker: Option<std::thread::JoinHandle<()>>,
}
impl CopyDeadline {
    fn new(old: &Connection, new: &Connection, ms: u64) -> Result<Self> {
        let old = old.get_interrupt_handle();
        let new = new.get_interrupt_handle();
        let (cancel, wait) = std::sync::mpsc::channel();
        let worker = std::thread::Builder::new()
            .name("sqlite-move-budget".into())
            .spawn(move || {
                if matches!(
                    wait.recv_timeout(Duration::from_millis(ms)),
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout)
                ) {
                    old.interrupt();
                    new.interrupt();
                }
            })?;
        Ok(Self {
            cancel,
            worker: Some(worker),
        })
    }
}
impl Drop for CopyDeadline {
    fn drop(&mut self) {
        let _ = self.cancel.send(());
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
fn budget(start: Instant, budget_ms: u64, domain: &str) -> Result<()> {
    if start.elapsed().as_millis() > u128::from(budget_ms) {
        return Err(Error::Validation(format!(
            "{domain}: copia excedió {budget_ms} ms; hacerlo en una ventana sin agentes"
        )));
    }
    Ok(())
}
fn copy_tables(
    source: &Connection,
    target: &Connection,
    spec: &MoveSpec,
    inverse: bool,
    timing: Option<(Instant, u64)>,
) -> Result<(u64, String)> {
    let pairs: Vec<_> = spec
        .tables
        .iter()
        .map(|(from, to)| if inverse { (*to, *from) } else { (*from, *to) })
        .collect();
    for (from, to) in &pairs {
        let autoincrement = |conn: &Connection, table: &str| -> Result<bool> {
            let sql: String = conn.query_row(
                "SELECT sql FROM sqlite_master WHERE type='table' AND name=?1",
                [table],
                |r| r.get(0),
            )?;
            Ok(sql.to_ascii_uppercase().contains("AUTOINCREMENT"))
        };
        if columns(source, from)? != columns(target, to)?
            || autoincrement(source, from)? != autoincrement(target, to)?
        {
            return Err(Error::Validation(format!(
                "columnas incompatibles: {from} → {to}"
            )));
        }
    }
    for (_, to) in pairs.iter().rev() {
        target.execute(&format!("DELETE FROM {}", quote(to)), [])?;
    }
    let mut count = 0;
    let mut hash = Sha256::new();
    for (from, to) in &pairs {
        if let Some((start, max)) = timing {
            budget(start, max, spec.domain)?;
        }
        let names = columns(source, from)?
            .into_iter()
            .map(|c| quote(&c.0))
            .collect::<Vec<_>>()
            .join(",");
        let mut read = source.prepare(&format!(
            "SELECT rowid,{names} FROM {} ORDER BY rowid",
            quote(from)
        ))?;
        let n = read.column_count();
        let mut write = target.prepare(&format!(
            "INSERT INTO {}(rowid,{names}) VALUES({})",
            quote(to),
            (0..n).map(|_| "?").collect::<Vec<_>>().join(",")
        ))?;
        let mut rows = read.query([])?;
        while let Some(row) = rows.next()? {
            if let Some((start, max)) = timing {
                budget(start, max, spec.domain)?;
            }
            let values = (0..n)
                .map(|i| row.get_ref(i).map(ToSqlOutput::Borrowed))
                .collect::<rusqlite::Result<Vec<_>>>()?;
            write.execute(params_from_iter(values))?;
        }
        let original = digest(source, from)?;
        if digest(target, to)? != original {
            return Err(Error::Validation(format!(
                "filas o bytes SQLite distintos: {from} → {to}"
            )));
        }
        count += original.0;
        hash.update(from.as_bytes());
        hash.update(original.1.as_bytes());
    }
    let has_sequence = source.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name='sqlite_sequence')",
        [],
        |r| r.get::<_, bool>(0),
    )?;
    if has_sequence {
        let target_sequence: bool = target.query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name='sqlite_sequence')",
            [],
            |r| r.get(0),
        )?;
        for (from, to) in &pairs {
            let seq: Option<i64> = source
                .query_row(
                    "SELECT seq FROM sqlite_sequence WHERE name=?1",
                    [from],
                    |r| r.get(0),
                )
                .optional()?;
            if target_sequence {
                target.execute("DELETE FROM sqlite_sequence WHERE name=?1", [to])?;
            }
            if let Some(seq) = seq {
                target.execute(
                    "INSERT INTO sqlite_sequence(name,seq) VALUES(?1,?2)",
                    params![to, seq],
                )?;
            }
        }
    }
    let violations: i64 =
        target.query_row("SELECT count(*) FROM pragma_foreign_key_check", [], |r| {
            r.get(0)
        })?;
    if violations != 0 {
        return Err(Error::Validation(
            "copia SQLite viola claves foráneas".into(),
        ));
    }
    if let Some((start, max)) = timing {
        budget(start, max, spec.domain)?;
    }
    Ok((count, format!("{:x}", hash.finalize())))
}
fn create_file(path: &Path) -> Result<()> {
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)?;
    Ok(())
}
fn backup_api(source: &Connection, target: &mut Connection) -> Result<()> {
    let copy = Backup::new(source, target)?;
    let start = Instant::now();
    loop {
        match copy.step(128)? {
            StepResult::Done => return Ok(()),
            StepResult::More => {}
            _ if start.elapsed() > Duration::from_secs(5) => {
                return Err(Error::Validation("respaldo SQLite ocupado".into()));
            }
            _ => std::thread::sleep(Duration::from_millis(10)),
        }
    }
}
fn backup_sqlite(spec: &MoveSpec, dest: &Path) -> Result<BackupManifest> {
    if dest.exists() {
        backup::verify_backup(&dest.join("manifest.json"))?;
        let manifest = backup::load(&dest.join("manifest.json"))?;
        if manifest.entries.len() != 1
            || !manifest
                .entries
                .iter()
                .any(|e| e.source == spec.legacy && e.kind == EntryKind::Sqlite)
        {
            return Err(Error::Validation("respaldo SQLite de otra fuente".into()));
        }
        return Ok(manifest);
    }
    let parent = dest
        .parent()
        .ok_or_else(|| Error::Validation("respaldo sin padre".into()))?;
    backup::private_dirs(Path::new("/"), parent)?;
    fs::DirBuilder::new().mode(0o700).create(dest)?;
    fs::File::open(parent)?.sync_all()?;
    let copy = dest.join(format!("{}.sqlite3", spec.domain));
    create_file(&copy)?;
    let home = read_home(&spec.legacy)?;
    unified::with_readonly_db(&home, &spec.legacy, |source| {
        validate_source(source, spec)?;
        let mut target = Connection::open(&copy)?;
        backup_api(source, &mut target)?;
        target.pragma_update(None, "journal_mode", "DELETE")?;
        if validate_source(&target, spec)? {
            return Err(Error::Validation("respaldo de una base ya movida".into()));
        }
        for (table, _) in spec.tables {
            if digest(source, table)? != digest(&target, table)? {
                return Err(Error::Validation(
                    "respaldo SQLite difiere de la fuente".into(),
                ));
            }
        }
        Ok(())
    })?;
    fs::File::open(&copy)?.sync_all()?;
    let snap = sources::read(&copy)?;
    let original = fs::metadata(&spec.legacy)?;
    use std::os::unix::fs::MetadataExt;
    let manifest = BackupManifest {
        created_at_ms: journal::now_ms()?,
        entries: vec![BackupEntry {
            source: spec.legacy.clone(),
            copy,
            sha256: snap.sha256,
            size: snap.body.len() as u64,
            mode: original.mode() & 0o7777,
            mtime_ns: original
                .mtime()
                .checked_mul(1_000_000_000)
                .and_then(|v| v.checked_add(original.mtime_nsec()))
                .ok_or_else(|| Error::Validation("mtime inválido".into()))?,
            kind: EntryKind::Sqlite,
        }],
    };
    crate::files::write_atomic(
        &dest.join("manifest.json"),
        &serde_json::to_vec_pretty(&manifest.json())
            .map_err(|e| Error::Validation(e.to_string()))?,
    )?;
    fs::File::open(dest)?.sync_all()?;
    backup::verify_backup(&dest.join("manifest.json"))?;
    Ok(manifest)
}
pub fn estimate(spec: &MoveSpec, scratch: &Path) -> Result<MoveEstimate> {
    validate_spec(spec)?;
    if !scratch.is_absolute() {
        return Err(Error::Validation("scratch requiere ruta absoluta".into()));
    }
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(scratch)?;
    if fs::symlink_metadata(scratch)?.permissions().mode() & 0o7777 != 0o700 {
        return Err(Error::Validation("scratch requiere 0700".into()));
    }
    let path = scratch.join(format!(
        "estimate-{}.sqlite3",
        journal::new_id(journal::now_ms()?)?
    ));
    let home = read_home(&spec.legacy)?;
    let start = Instant::now();
    let result = unified::with_readonly_db(&home, &spec.legacy, |source| {
        if validate_source(source, spec)? {
            return Ok(MoveEstimate {
                bytes: 0,
                copy_ms: 0,
            });
        }
        let target = unified::open_unified(&path)?;
        let tx = Transaction::new_unchecked(&target, TransactionBehavior::Immediate)?;
        copy_tables(source, &target, spec, false, None)?;
        tx.commit()?;
        let bytes = fs::metadata(&spec.legacy)?.len()
            + fs::metadata(sources::suffix(&spec.legacy, "-wal"))
                .map(|m| m.len())
                .unwrap_or(0);
        let ms = u64::try_from(start.elapsed().as_millis())
            .map_err(|e| Error::Validation(e.to_string()))?
            .max(1);
        Ok(MoveEstimate { bytes, copy_ms: ms })
    });
    for suffix in ["", "-wal", "-shm"] {
        let p = sources::suffix(&path, suffix);
        if p.exists() {
            fs::remove_file(p)?;
        }
    }
    result
}
pub fn estimate_at_home(home: &Path, spec: &MoveSpec) -> Result<MoveEstimate> {
    let scratch = super::dry_run::Scratch::new(home)?;
    estimate(spec, &scratch.0)
}
fn existing_target(home: &Path, target: &Path) -> Result<()> {
    let present = match fs::symlink_metadata(target) {
        Ok(meta) if meta.is_file() => true,
        Ok(_) => {
            return Err(Error::Validation(
                "destino SQLite requiere archivo regular".into(),
            ));
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => false,
        Err(e) => return Err(e.into()),
    };
    if present {
        unified::with_readonly_unified(home, target, |_| Ok(()))?;
    } else {
        let parent = target
            .parent()
            .ok_or_else(|| Error::Validation("destino sin padre".into()))?;
        match fs::metadata(parent) {
            Ok(meta) if !meta.is_dir() || meta.permissions().mode() & 0o7777 != 0o700 => {
                return Err(Error::Validation(
                    "destino requiere padre privado 0700".into(),
                ));
            }
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
        for tail in ["-wal", "-shm", "-journal"] {
            match fs::symlink_metadata(sources::suffix(target, tail)) {
                Ok(_) => {
                    return Err(Error::Validation(
                        "sidecar SQLite sin destino; no se omite".into(),
                    ));
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(e.into()),
            }
        }
        for domain in crate::domains::catalog::DOMAINS {
            if unified::modes::guarded(target, domain.name)? {
                return Err(Error::Validation("base sellada ausente".into()));
            }
        }
    }
    Ok(())
}
fn open_old(path: &Path) -> Result<Connection> {
    let c = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    c.busy_timeout(Duration::from_secs(5))?;
    c.set_db_config(
        rusqlite::config::DbConfig::SQLITE_DBCONFIG_NO_CKPT_ON_CLOSE,
        true,
    )?;
    c.pragma_update(None, "foreign_keys", true)?;
    Ok(c)
}
fn mark(conn: &Connection, marker: Marker) -> Result<()> {
    match marker {
        Marker::UserVersion => conn.pragma_update(None, "user_version", 1000)?,
        Marker::SchemaMigrationsRow => {
            conn.execute(
                "INSERT INTO schema_migrations VALUES(1000,'moved-to-comandos.sqlite3',?1)",
                [journal::now_ms()? as f64 / 1000.0],
            )?;
        }
    }
    Ok(())
}
pub fn move_db(spec: &MoveSpec, target: &Path, backup_dir: &Path, budget_ms: u64) -> Result<()> {
    validate_spec(spec)?;
    let home = read_home(&spec.legacy)?;
    if !target.is_absolute() || !backup_dir.is_absolute() {
        return Err(Error::Validation(
            "traslado requiere rutas absolutas".into(),
        ));
    }
    if crate::domains::catalog::UnifiedControlFiles::inspect(target)?.is_control(&spec.legacy)? {
        return Err(Error::Validation(
            "destino/control no es fuente SQLite".into(),
        ));
    }
    existing_target(&home, target)?;
    let moved = unified::with_readonly_db(&home, &spec.legacy, |conn| validate_source(conn, spec))?;
    if moved {
        resolve_db(&spec.legacy, target)?;
        let new = unified::open_existing(target)?;
        let _lock = crate::files::FileLock::exclusive(&sources::suffix(target, ".migration.lock"))?;
        let (_, _mode_lock) = unified::modes::access_mode(&home, Some(&new), spec.domain)?;
        if !unified::with_readonly_db(&home, &spec.legacy, |c| validate_source(c, spec))? {
            return Err(Error::Validation(
                "marca cambió durante el reintento".into(),
            ));
        }
        new.execute("UPDATE migration_runs SET status='done',finished_at_ms=?3 WHERE status='running' AND EXISTS(SELECT 1 FROM migration_steps s WHERE s.run_id=migration_runs.run_id AND s.domain=?1 AND s.source=?2 AND s.status='done' AND json_extract(s.detail,'$.target_committed_before_source_marker')=1)",params![spec.domain,spec.legacy.to_string_lossy(),journal::now_ms()?])?;
        new.set_db_config(
            rusqlite::config::DbConfig::SQLITE_DBCONFIG_NO_CKPT_ON_CLOSE,
            false,
        )?;
        return Ok(());
    }
    if target.exists() {
        let mode = unified::with_readonly_unified(&home, target, |c| {
            unified::mode_of(Some(c), spec.domain)
        })?;
        if matches!(mode, unified::Mode::Unified | unified::Mode::Sealed) {
            return Err(Error::Validation(
                "no se traslada un dominio autoritativo".into(),
            ));
        }
    }
    let manifest = backup_sqlite(spec, backup_dir)?;
    let estimate = estimate_at_home(&home, spec)?;
    if estimate.copy_ms > budget_ms {
        return Err(Error::Validation(format!(
            "{}: la copia estimada tarda {} ms (> {} ms); hacerlo en una ventana sin agentes",
            spec.domain, estimate.copy_ms, budget_ms
        )));
    }
    let new = if target.exists() {
        unified::open_existing(target)?
    } else {
        unified::open_unified(target)?
    };
    let _lock = crate::files::FileLock::exclusive(&sources::suffix(target, ".migration.lock"))?;
    let (mode, _mode_lock) = unified::modes::access_mode(&home, Some(&new), spec.domain)?;
    if matches!(mode, unified::Mode::Unified | unified::Mode::Sealed) {
        return Err(Error::Validation(
            "no se traslada un dominio autoritativo".into(),
        ));
    }
    let old = open_old(&spec.legacy)?;
    let old_tx = Transaction::new_unchecked(&old, TransactionBehavior::Immediate)?;
    if validate_source(&old, spec)? {
        return Ok(());
    }
    let uv: i64 = old.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    let start = Instant::now();
    new.busy_timeout(Duration::from_millis(budget_ms.min(5000)))?;
    let id = journal::new_id(journal::now_ms()?)?;
    let _deadline = CopyDeadline::new(&old, &new, budget_ms)?;
    let copied = (|| -> Result<()> {
        let tx = Transaction::new_unchecked(&new, TransactionBehavior::Immediate)?;
        unified::validate_schema(&new)?;
        let (rows, hash) = copy_tables(&old, &new, spec, false, Some((start, budget_ms)))?;
        journal::start(&new, &id, backup_dir, journal::now_ms()?)?;
        let detail=serde_json::json!({"user_version":uv,"marker":match spec.marker{Marker::UserVersion=>"user_version",Marker::SchemaMigrationsRow=>"schema_migrations"},"backup":backup_dir,"target_committed_before_source_marker":true}).to_string();
        let step = super::StepReport {
            domain: spec.domain.into(),
            source: spec.legacy.to_string_lossy().into_owned(),
            status: "done".into(),
            rows,
            bytes: manifest.entries.iter().map(|e| e.size).sum(),
            elapsed_ms: u64::try_from(start.elapsed().as_millis())
                .map_err(|e| Error::Validation(e.to_string()))?,
            detail,
        };
        journal::record(&new, &id, &step, &hash, journal::now_ms()?)?;
        tx.commit()?;
        Ok(())
    })();
    if let Err(error) = copied {
        old_tx.rollback()?;
        let tx = Transaction::new_unchecked(&new, TransactionBehavior::Immediate)?;
        journal::start(&new, &id, backup_dir, journal::now_ms()?)?;
        let step = super::StepReport {
            domain: spec.domain.into(),
            source: spec.legacy.to_string_lossy().into_owned(),
            status: "failed".into(),
            rows: 0,
            bytes: manifest.entries.iter().map(|e| e.size).sum(),
            elapsed_ms: u64::try_from(start.elapsed().as_millis())
                .map_err(|e| Error::Validation(e.to_string()))?,
            detail: error.to_string(),
        };
        journal::record(&new, &id, &step, "", journal::now_ms()?)?;
        new.execute(
            "UPDATE migration_runs SET status='failed' WHERE run_id=?1",
            [&id],
        )?;
        tx.commit()?;
        return Err(error);
    }
    let marked = mark(&old, spec.marker).and_then(|()| old_tx.commit().map_err(Error::from));
    if let Err(error) = marked {
        new.execute(
            "UPDATE migration_runs SET status='failed' WHERE run_id=?1",
            [&id],
        )?;
        return Err(error);
    }
    journal::finish(&new, &id, journal::now_ms()?)?;
    new.set_db_config(
        rusqlite::config::DbConfig::SQLITE_DBCONFIG_NO_CKPT_ON_CLOSE,
        false,
    )?;
    old.set_db_config(
        rusqlite::config::DbConfig::SQLITE_DBCONFIG_NO_CKPT_ON_CLOSE,
        false,
    )?;
    Ok(())
}
pub fn demote_db(spec: &MoveSpec, target: &Path) -> Result<()> {
    validate_spec(spec)?;
    let home = read_home(&spec.legacy)?;
    existing_target(&home, target)?;
    let moved = unified::with_readonly_db(&home, &spec.legacy, |conn| validate_source(conn, spec))?;
    if !moved {
        return Ok(());
    }
    resolve_db(&spec.legacy, target)?;
    let new = if target.exists() {
        unified::open_existing(target)?
    } else {
        unified::open_unified(target)?
    };
    let _lock = crate::files::FileLock::exclusive(&sources::suffix(target, ".migration.lock"))?;
    let (mode, _mode_lock) = unified::modes::access_mode(&home, Some(&new), spec.domain)?;
    if matches!(mode, unified::Mode::Unified | unified::Mode::Sealed) {
        return Err(Error::Validation(
            "no se degrada un dominio autoritativo".into(),
        ));
    }
    let old = open_old(&spec.legacy)?;
    let old_tx = Transaction::new_unchecked(&old, TransactionBehavior::Immediate)?;
    if !validate_source(&old, spec)? {
        return Ok(());
    }
    let new_tx = Transaction::new_unchecked(&new, TransactionBehavior::Immediate)?;
    unified::validate_schema(&new)?;
    let detail:String=new.query_row("SELECT detail FROM migration_steps WHERE domain=?1 AND source=?2 AND status='done' AND json_extract(detail,'$.target_committed_before_source_marker')=1 ORDER BY finished_at_ms DESC LIMIT 1",params![spec.domain,spec.legacy.to_string_lossy()],|r|r.get(0))?;
    let detail: serde_json::Value =
        serde_json::from_str(&detail).map_err(|e| Error::Validation(e.to_string()))?;
    let uv = detail
        .get("user_version")
        .and_then(serde_json::Value::as_i64)
        .ok_or_else(|| Error::Validation("recibo sin versión anterior".into()))?;
    let backup = detail
        .get("backup")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| Error::Validation("recibo sin respaldo".into()))?;
    backup::verify_backup(&Path::new(backup).join("manifest.json"))?;
    let started = Instant::now();
    let (rows, hash) = copy_tables(&new, &old, spec, true, None)?;
    match spec.marker {
        Marker::UserVersion => old.pragma_update(None, "user_version", uv)?,
        Marker::SchemaMigrationsRow => {
            old.execute("DELETE FROM schema_migrations WHERE version=1000", [])?;
        }
    }
    // La copia inversa y el retiro de marca son atómicos en la fuente. La única
    // conserva sus datos para reintentar si el journal final falla.
    old_tx.commit()?;
    let id = journal::new_id(journal::now_ms()?)?;
    journal::start(&new, &id, Path::new(backup), journal::now_ms()?)?;
    let step = super::StepReport {
        domain: spec.domain.into(),
        source: spec.legacy.to_string_lossy().into_owned(),
        status: "done".into(),
        rows,
        bytes: 0,
        elapsed_ms: started
            .elapsed()
            .as_millis()
            .try_into()
            .map_err(|e: std::num::TryFromIntError| Error::Validation(e.to_string()))?,
        detail: serde_json::json!({"direction":"inverse","user_version":uv,"backup":backup})
            .to_string(),
    };
    journal::record(&new, &id, &step, &hash, journal::now_ms()?)?;
    journal::finish(&new, &id, journal::now_ms()?)?;
    new_tx.commit()?;
    new.set_db_config(
        rusqlite::config::DbConfig::SQLITE_DBCONFIG_NO_CKPT_ON_CLOSE,
        false,
    )?;
    old.set_db_config(
        rusqlite::config::DbConfig::SQLITE_DBCONFIG_NO_CKPT_ON_CLOSE,
        false,
    )?;
    Ok(())
}

pub fn move_estimate(home: &Path, spec: &MoveSpec, target: &Path) -> Result<MoveEstimate> {
    validate_spec(spec)?;
    existing_target(home, target)?;
    if crate::domains::catalog::UnifiedControlFiles::inspect(target)?.is_control(&spec.legacy)? {
        return Err(Error::Validation(
            "destino/control no es fuente SQLite".into(),
        ));
    }
    estimate_at_home(home, spec)
}
