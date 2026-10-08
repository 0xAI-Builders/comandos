use super::Origin;
use crate::Result;
use rusqlite::{Connection, OptionalExtension, params};
pub fn status_put(
    c: &Connection,
    key: &str,
    body: &[u8],
    mtime_ns: i64,
    origin: Origin,
) -> Result<()> {
    c.execute("INSERT INTO session_status VALUES (?1,?2,?3,?4) ON CONFLICT(file_key) DO UPDATE SET body=excluded.body,mtime_ns=excluded.mtime_ns,origin=excluded.origin",params![key,body,mtime_ns,origin.as_str()])?;
    Ok(())
}
pub fn status_put_if_newer(c: &Connection, key: &str, body: &[u8], mtime_ns: i64) -> Result<bool> {
    Ok(c.execute("INSERT INTO session_status VALUES (?1,?2,?3,'import') ON CONFLICT(file_key) DO UPDATE SET body=excluded.body,mtime_ns=excluded.mtime_ns,origin='import' WHERE session_status.mtime_ns<excluded.mtime_ns",params![key,body,mtime_ns])?!=0)
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Generation {
    Current,
    Previous,
    Minute,
}
impl Generation {
    fn as_str(self) -> &'static str {
        match self {
            Self::Current => "current",
            Self::Previous => "previous",
            Self::Minute => "minute",
        }
    }
}
pub fn layout_put(c: &Connection, generation: Generation, stamp: i64, body: &[u8]) -> Result<()> {
    c.execute("INSERT INTO layout_snapshots VALUES (?1,?2,?3) ON CONFLICT(generation,stamp) DO UPDATE SET body=excluded.body",params![stamp,generation.as_str(),body])?;
    Ok(())
}
pub fn layout_prune_minutes(c: &Connection, older_than: i64) -> Result<usize> {
    Ok(c.execute(
        "DELETE FROM layout_snapshots WHERE generation='minute' AND stamp<?1",
        [older_than],
    )?)
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandKind {
    Focus,
    Open,
    Close,
    Command,
    Back,
}
impl CommandKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Focus => "focus",
            Self::Open => "open",
            Self::Close => "close",
            Self::Command => "command",
            Self::Back => "back",
        }
    }
}
pub fn command_push(c: &Connection, kind: CommandKind, body: &[u8], now_ms: i64) -> Result<i64> {
    Ok(c.query_row(
        "INSERT INTO app_commands(kind,body,created_at_ms) VALUES (?1,?2,?3) RETURNING seq",
        params![kind.as_str(), body, now_ms],
        |r| r.get(0),
    )?)
}
pub fn command_take(
    c: &Connection,
    kind: CommandKind,
    now_ms: i64,
) -> Result<Option<(i64, Vec<u8>)>> {
    Ok(c.query_row("UPDATE app_commands SET consumed_at_ms=?2 WHERE seq=(SELECT seq FROM app_commands WHERE kind=?1 AND consumed_at_ms IS NULL ORDER BY seq LIMIT 1) AND consumed_at_ms IS NULL RETURNING seq,body",params![kind.as_str(),now_ms],|r|Ok((r.get(0)?,r.get(1)?))).optional()?)
}
