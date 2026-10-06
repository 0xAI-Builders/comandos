//! El paso y sus filas se confirman en la misma transacción del importador.
use super::StepReport;
use crate::{Error, Result};
use rusqlite::{Connection, OptionalExtension, params};
use std::{
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};
pub fn now_ms() -> Result<i64> {
    i64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| Error::Validation(e.to_string()))?
            .as_millis(),
    )
    .map_err(|e| Error::Validation(e.to_string()))
}
pub(super) fn new_id(now_ms: i64) -> Result<String> {
    let date = chrono::DateTime::from_timestamp_millis(now_ms)
        .ok_or_else(|| Error::Validation("fecha fuera de rango".into()))?;
    let mut random = [0u8; 2];
    getrandom::fill(&mut random).map_err(|e| Error::Validation(e.to_string()))?;
    Ok(format!(
        "{}-{}",
        date.format("%Y%m%d-%H%M%S"),
        random
            .iter()
            .map(|v| format!("{v:02x}"))
            .collect::<String>()
    ))
}
pub(super) fn start(c: &Connection, id: &str, backup: &Path, now: i64) -> Result<()> {
    c.execute("INSERT INTO migration_runs(run_id,started_at_ms,backup_dir,status) VALUES(?1,?2,?3,'running')",params![id,now,backup.to_string_lossy()])?;
    Ok(())
}
pub(super) fn latest_running(c: &Connection) -> Result<(String, PathBuf)> {
    c.query_row("SELECT run_id,backup_dir FROM migration_runs WHERE status='running' ORDER BY started_at_ms DESC,run_id DESC LIMIT 1",[],|r|Ok((r.get(0)?,PathBuf::from(r.get::<_,String>(1)?)))).optional()?.ok_or_else(||Error::Validation("no hay una migración running que reanudar".into()))
}
pub(super) fn finish(c: &Connection, id: &str, now: i64) -> Result<()> {
    c.execute(
        "UPDATE migration_runs SET status='done',finished_at_ms=?2 WHERE run_id=?1",
        params![id, now],
    )?;
    Ok(())
}
pub(super) fn unchanged(
    c: &Connection,
    id: &str,
    domain: &str,
    source: &str,
    hash: &str,
) -> Result<bool> {
    Ok(c.query_row("SELECT EXISTS(SELECT 1 FROM migration_steps WHERE run_id=?1 AND domain=?2 AND source=?3 AND source_sha256=?4 AND status='done')",params![id,domain,source,hash],|r|r.get(0))?)
}
pub(super) fn record(
    c: &Connection,
    id: &str,
    step: &StepReport,
    hash: &str,
    now: i64,
) -> Result<()> {
    let bytes = i64::try_from(step.bytes).map_err(|e| Error::Validation(e.to_string()))?;
    let rows = i64::try_from(step.rows).map_err(|e| Error::Validation(e.to_string()))?;
    c.execute("INSERT INTO migration_steps(run_id,domain,source,source_sha256,source_size,rows,status,detail,finished_at_ms) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9) ON CONFLICT(run_id,domain,source) DO UPDATE SET source_sha256=excluded.source_sha256,source_size=excluded.source_size,rows=excluded.rows,status=excluded.status,detail=excluded.detail,finished_at_ms=excluded.finished_at_ms",params![id,step.domain,step.source,hash,bytes,rows,step.status,step.detail,now])?;
    Ok(())
}
pub fn last_verify(c: &Connection, domain: &str) -> Result<Option<(i64, String)>> {
    Ok(c.query_row("SELECT finished_at_ms,detail FROM migration_steps WHERE domain=?1 ORDER BY finished_at_ms DESC LIMIT 1",[format!("verify:{domain}")],|r|Ok((r.get(0)?,r.get(1)?))).optional()?)
}
pub fn status(home: &Path, path: &Path) -> Result<serde_json::Value> {
    super::dry_run::with_snapshot(home, path, |c| {
        let mut rows = vec![];
        for domain in crate::domains::catalog::DOMAINS {
            let mode = crate::unified::mode_of(Some(c), domain.name)?;
            let sealed = crate::unified::modes::guarded(path, domain.name)?;
            if sealed != (mode == crate::unified::Mode::Sealed) {
                return Err(Error::Validation(format!(
                    "{}: sellado sin base disponible o modo confirmado",
                    domain.name
                )));
            }
            let mode = match mode {
                crate::unified::Mode::Legacy => "legacy",
                crate::unified::Mode::Mirror => "mirror",
                crate::unified::Mode::Unified => "unified",
                crate::unified::Mode::Sealed => "sealed",
            };
            let last = last_verify(c, domain.name)?;
            rows.push(serde_json::json!({"domain":domain.name,"mode":mode,"last_verify":last.map(|(at,detail)|serde_json::json!({"at_ms":at,"detail":detail}))}));
        }
        Ok(serde_json::json!({"home":home,"db":path,"domains":rows}))
    })
}

pub fn record_verify(c: &Connection, report: &super::VerifyReport, now: i64) -> Result<()> {
    let id = new_id(now)?;
    let tx = c.unchecked_transaction()?;
    start(c, &id, Path::new(""), now)?;
    let step = StepReport {
        domain: format!("verify:{}", report.domain),
        source: "verification".into(),
        status: if report.mismatches.is_empty() {
            "done"
        } else {
            "failed"
        }
        .into(),
        rows: 0,
        bytes: 0,
        elapsed_ms: 0,
        detail: serde_json::to_string(&report.mismatches)
            .map_err(|e| Error::Validation(e.to_string()))?,
    };
    record(c, &id, &step, "", now)?;
    finish(c, &id, now)?;
    tx.commit()?;
    Ok(())
}
