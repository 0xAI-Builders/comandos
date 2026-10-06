use super::{
    MigrateOptions, StepReport, journal,
    sources::{self, Snapshot, Source},
};
use crate::{
    Error, Result,
    domains::catalog::TargetKind,
    unified::{self, Mode},
};
use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior, params};
use std::time::Instant;
pub(super) fn source(
    opts: &MigrateOptions,
    c: &Connection,
    source: &Source,
    id: &str,
    dry: bool,
) -> Result<StepReport> {
    let started = Instant::now();
    let (mode, _guard) = if dry {
        (unified::mode_of(Some(c), source.spec.domain)?, None)
    } else {
        unified::modes::access_mode(&opts.home, Some(c), source.spec.domain)?
    };
    let mut report = StepReport {
        domain: source.spec.domain.into(),
        source: source.symbolic.clone(),
        status: "done".into(),
        rows: 0,
        bytes: 0,
        elapsed_ms: 0,
        detail: String::new(),
    };
    if matches!(mode, Mode::Unified | Mode::Sealed) {
        report.status = "skipped".into();
        report.detail = "autoridad unified/sealed conservada; sin backfill".into();
        return Ok(report);
    }
    let _lock = if dry {
        None
    } else {
        Some(sources::lock(source)?)
    };
    sources::check_parents(&opts.home, &source.path)?;
    let snapshot = sources::read_source(&opts.home, source)?;
    report.bytes = snapshot.body.len() as u64;
    if opts.resume
        && journal::unchanged(
            c,
            id,
            source.spec.domain,
            &source.symbolic,
            &snapshot.sha256,
        )?
    {
        report.status = "skipped".into();
        report.detail = "paso confirmado con el mismo hash".into();
        return Ok(report);
    }
    let tx = Transaction::new_unchecked(c, TransactionBehavior::Immediate)?;
    let result = apply(c, source, &snapshot, opts.now_ms, &mut report);
    if let Err(error) = result {
        tx.rollback()?;
        report.status = "failed".into();
        report.detail = error.to_string();
        journal::record(c, id, &report, &snapshot.sha256, opts.now_ms)?;
        return Err(error);
    }
    report.elapsed_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
    journal::record(c, id, &report, &snapshot.sha256, opts.now_ms)?;
    tx.commit()?;
    Ok(report)
}
pub(super) fn layout_key(source: &Source, snapshot: &Snapshot) -> Result<(&'static str, i64)> {
    if source.symbolic.contains(".history/") {
        let stamp = source
            .path
            .file_stem()
            .and_then(|s| s.to_str())
            .ok_or_else(|| Error::Validation("stamp de layout inválido".into()))?
            .parse::<i64>()
            .map_err(|e| Error::Validation(e.to_string()))?;
        return Ok(("minute", stamp));
    }
    let value: serde_json::Value = serde_json::from_slice(&snapshot.body).map_err(|e| {
        Error::Validation(format!("{}: layout JSON inválido: {e}", source.symbolic))
    })?;
    let stamp = match value.get("saved_at") {
        Some(v) => {
            let n = v
                .as_f64()
                .filter(|n| n.is_finite() && *n >= 0.0 && *n < i64::MAX as f64)
                .ok_or_else(|| Error::Validation("saved_at inválido".into()))?;
            n as i64
        }
        None => snapshot.mtime_ns / 1_000_000_000,
    };
    Ok((
        if source.symbolic.ends_with(".bak") {
            "previous"
        } else {
            "current"
        },
        stamp,
    ))
}
pub(super) fn command_kind(symbolic: &str) -> Result<&'static str> {
    match symbolic {
        "H/app-focus.json" => Ok("focus"),
        "H/app-tab-open.json" => Ok("open"),
        "H/app-tab-close.json" => Ok("close"),
        "H/app-command.json" => Ok("command"),
        "H/app-tab-back.json" => Ok("back"),
        _ => Err(Error::Validation("tipo de comando inválido".into())),
    }
}
pub(super) fn log_name(symbolic: &str) -> Result<&'static str> {
    match symbolic {
        "H/events.jsonl" => Ok("events"),
        "H/ui-events.jsonl" => Ok("ui-events"),
        "H/focus-queue.jsonl" => Ok("focus-queue"),
        _ => Err(Error::Validation("registro inválido".into())),
    }
}
pub(super) fn complete_lines(body: &[u8]) -> (Vec<&[u8]>, bool) {
    match body.iter().rposition(|b| *b == b'\n') {
        Some(last) => (
            body.get(..last)
                .unwrap_or_default()
                .split(|b| *b == b'\n')
                .collect(),
            last + 1 < body.len(),
        ),
        None => (vec![], !body.is_empty()),
    }
}
pub(super) fn apply(
    c: &Connection,
    source: &Source,
    snapshot: &Snapshot,
    now_ms: i64,
    report: &mut StepReport,
) -> Result<()> {
    let bytes = &snapshot.body;
    match source.spec.kind {
        TargetKind::Document => {
            report.rows = u64::from(unified::doc_put_if_newer(
                c,
                &sources::doc_name(&source.symbolic),
                source.spec.domain,
                bytes,
                snapshot.mtime_ns / 1_000_000,
            )?);
        }
        TargetKind::SessionStatus => {
            let key = source
                .symbolic
                .strip_prefix("H/state/")
                .ok_or_else(|| Error::Validation("clave de status inválida".into()))?;
            if key.contains('/') {
                return Err(Error::Validation("estado anidado no admitido".into()));
            }
            report.rows = u64::from(unified::status_put_if_newer(
                c,
                key,
                bytes,
                snapshot.mtime_ns,
            )?);
        }
        TargetKind::NativeProcess => {
            let pid = source
                .path
                .file_stem()
                .and_then(|s| s.to_str())
                .ok_or_else(|| Error::Validation("pid inválido".into()))?
                .parse::<i64>()
                .map_err(|e| Error::Validation(e.to_string()))?;
            if pid <= 0 {
                return Err(Error::Validation("pid no positivo".into()));
            }
            report.rows=c.execute("INSERT INTO native_processes(pid,body,mtime_ns,origin) VALUES(?1,?2,?3,'import') ON CONFLICT(pid) DO UPDATE SET body=excluded.body,mtime_ns=excluded.mtime_ns,origin='import' WHERE native_processes.mtime_ns<excluded.mtime_ns",params![pid,bytes,snapshot.mtime_ns])? as u64;
        }
        TargetKind::LogLines => {
            let name = log_name(&source.symbolic)?;
            let count: i64 =
                c.query_row("SELECT COUNT(*) FROM log_lines WHERE log=?1", [name], |r| {
                    r.get(0)
                })?;
            let (lines, incomplete) = complete_lines(bytes);
            if incomplete {
                report.detail = "línea final incompleta ignorada".into();
            }
            if count > 0 {
                report
                    .detail
                    .push_str("; registro no vacío: sin reimportar ni duplicar");
                return Ok(());
            }
            for (index, line) in lines.iter().enumerate() {
                let seq = i64::try_from(index + 1).map_err(|e| Error::Validation(e.to_string()))?;
                c.execute(
                    "INSERT INTO log_lines(log,seq,line) VALUES(?1,?2,?3)",
                    params![name, seq, line],
                )?;
                report.rows += 1;
            }
        }
        TargetKind::LayoutSnapshot => {
            let (generation, stamp) = layout_key(source, snapshot)?;
            if generation == "minute" && stamp < now_ms / 1000 - 7 * 86400 {
                report.detail = "fuera de retención de 7 días; solo respaldo".into();
                return Ok(());
            }
            if generation != "minute" {
                let newest: Option<i64> = c.query_row(
                    "SELECT MAX(stamp) FROM layout_snapshots WHERE generation=?1",
                    [generation],
                    |r| r.get(0),
                )?;
                if newest.is_some_and(|n| n >= stamp) {
                    return Ok(());
                }
                c.execute(
                    "DELETE FROM layout_snapshots WHERE generation=?1",
                    [generation],
                )?;
            }
            report.rows = c.execute(
                "INSERT OR IGNORE INTO layout_snapshots(stamp,generation,body) VALUES(?1,?2,?3)",
                params![stamp, generation, bytes],
            )? as u64;
        }
        TargetKind::AppCommand => {
            let kind = command_kind(&source.symbolic)?;
            // El archivo representa la última orden; el journal evita revivir una consumida.
            let seen:bool=c.query_row("SELECT EXISTS(SELECT 1 FROM migration_steps WHERE domain=?1 AND source=?2 AND source_sha256=?3 AND status='done')",params![source.spec.domain,source.symbolic,snapshot.sha256],|r|r.get(0))?;
            if seen {
                return Ok(());
            }
            let newest: Option<i64> = c.query_row(
                "SELECT MAX(created_at_ms) FROM app_commands WHERE kind=?1",
                [kind],
                |r| r.get(0),
            )?;
            let at = snapshot.mtime_ns / 1_000_000;
            if newest.is_some_and(|n| n >= at) {
                return Ok(());
            }
            let old:Option<String>=c.query_row("SELECT detail FROM migration_steps WHERE domain=?1 AND source=?2 AND status='done' AND detail LIKE 'command_seq:%' ORDER BY finished_at_ms DESC LIMIT 1",params![source.spec.domain,source.symbolic],|r|r.get(0)).optional()?;
            if let Some(seq) = old
                .as_deref()
                .and_then(|s| s.strip_prefix("command_seq:"))
                .and_then(|s| s.parse::<i64>().ok())
            {
                c.execute(
                    "DELETE FROM app_commands WHERE seq=?1 AND kind=?2",
                    params![seq, kind],
                )?;
            }
            let seq: i64 = c.query_row(
                "INSERT INTO app_commands(kind,body,created_at_ms) VALUES(?1,?2,?3) RETURNING seq",
                params![kind, bytes, at],
                |r| r.get(0),
            )?;
            report.rows = 1;
            report.detail = format!("command_seq:{seq}");
        }
        TargetKind::Sqlite => {
            return Err(Error::Validation("traslado SQLite pendiente de S4".into()));
        }
    }
    Ok(())
}
