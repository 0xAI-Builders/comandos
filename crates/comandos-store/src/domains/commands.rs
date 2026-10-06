//! File-compatible command publication and acknowledgement across mode changes.
use super::caller::CallerAccess;
use crate::{
    Error, Result,
    unified::{self, CommandKind, Mode},
};
use rusqlite::OptionalExtension;
use std::path::Path;
pub fn kind(name: &str) -> Result<(&'static str, CommandKind)> {
    Ok(match name {
        "app-focus.json" => ("focus", CommandKind::Focus),
        "app-tab-open.json" => ("open", CommandKind::Open),
        "app-tab-close.json" => ("close", CommandKind::Close),
        "app-command.json" => ("command", CommandKind::Command),
        "app-tab-back.json" => ("back", CommandKind::Back),
        _ => return Err(Error::Validation("unknown app command".into())),
    })
}
pub type PendingCommand = (i64, Vec<u8>);
pub type QueuePeek = (Mode, Option<PendingCommand>);
/// Peek only; acknowledgement follows successful handling by the app.
pub fn peek(home: &Path, name: &str) -> Result<QueuePeek> {
    let (name, _) = kind(name)?;
    unified::modes::with_readonly_access(home, "app-commands", |mode, db| {
        let row = if matches!(mode, Mode::Unified | Mode::Sealed) {
            let db = db.ok_or_else(|| Error::Validation("command database absent".into()))?;
            db.query_row("SELECT seq,body FROM app_commands WHERE kind=?1 AND consumed_at_ms IS NULL ORDER BY seq LIMIT 1", [name], |r| Ok((r.get(0)?, r.get(1)?))).optional()?
        } else {
            None
        };
        Ok((mode, row))
    })
}
/// Read one app-command polling batch under a single unchanged admission lease.
/// The callback, including legacy reads, runs before authority is revalidated.
/// An invalid individual queue does not prevent reading the other queues.
pub fn with_peeked<T>(
    home: &Path,
    names: &[&str],
    body: impl FnOnce(Mode, Vec<Result<Option<PendingCommand>>>) -> T,
) -> Result<T> {
    unified::modes::with_readonly_access(home, "app-commands", |mode, db| {
        let rows = names
            .iter()
            .map(|name| {
                let (name, _) = kind(name)?;
                if matches!(mode, Mode::Unified | Mode::Sealed) {
                    let db = db.ok_or_else(|| Error::Validation("command database absent".into()))?;
                    Ok(db.query_row("SELECT seq,body FROM app_commands WHERE kind=?1 AND consumed_at_ms IS NULL ORDER BY seq LIMIT 1", [name], |r| Ok((r.get(0)?, r.get(1)?))).optional()?)
                } else {
                    Ok(None)
                }
            })
            .collect();
        Ok(body(mode, rows))
    })
}
pub fn publish(
    home: &Path,
    name: &str,
    body: &[u8],
    now_ms: i64,
    legacy: impl FnOnce() -> Result<()>,
) -> Result<()> {
    let (_, kind) = kind(name)?;
    CallerAccess::open(home, "app-commands")?.write(legacy, |db, _| {
        unified::command_push(db, kind, body, now_ms).map(|_| ())
    })
}
/// Also acknowledges a legacy request mirrored immediately before a mode flip.
pub fn acknowledge(
    home: &Path,
    name: &str,
    seq: Option<i64>,
    body: &[u8],
    now_ms: i64,
    legacy: impl FnOnce() -> Result<()>,
) -> Result<()> {
    let (kind, _) = kind(name)?;
    CallerAccess::open(home, "app-commands")?.write(legacy, |db,_| {
        let pending: Option<(i64, Vec<u8>)> = db.query_row("SELECT seq,body FROM app_commands WHERE kind=?1 AND consumed_at_ms IS NULL AND (?2 IS NULL OR seq=?2) ORDER BY seq LIMIT 1", rusqlite::params![kind,seq], |r| Ok((r.get(0)?,r.get(1)?))).optional()?;
        if let Some((pending_seq, pending_body)) = pending {
            let same = pending_body == body || comandos_core::json::workspace_loads_bytes(&pending_body).zip(comandos_core::json::workspace_loads_bytes(body)).is_some_and(|(a,b)| a == b);
            if same { db.execute("UPDATE app_commands SET consumed_at_ms=?1 WHERE seq=?2 AND consumed_at_ms IS NULL", rusqlite::params![now_ms,pending_seq])?; }
        }
        db.execute("DELETE FROM app_commands WHERE consumed_at_ms IS NOT NULL AND consumed_at_ms < ?1", [now_ms.saturating_sub(86_400_000)])?;
        Ok(())
    })
}
