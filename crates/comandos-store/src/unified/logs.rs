use crate::{Error, Result};
use rusqlite::{Connection, params};
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogName {
    Events,
    UiEvents,
    FocusQueue,
}
impl LogName {
    fn as_str(self) -> &'static str {
        match self {
            Self::Events => "events",
            Self::UiEvents => "ui-events",
            Self::FocusQueue => "focus-queue",
        }
    }
}
pub fn log_append(c: &Connection, log: LogName, line: &[u8]) -> Result<i64> {
    Ok(c.query_row("INSERT INTO log_lines(log,seq,line) SELECT ?1,COALESCE(MAX(seq),0)+1,?2 FROM log_lines WHERE log=?1 RETURNING seq",params![log.as_str(),line],|r|r.get(0))?)
}
pub fn log_tail(c: &Connection, log: LogName, max_lines: usize) -> Result<Vec<Vec<u8>>> {
    let limit = i64::try_from(max_lines)
        .map_err(|_| Error::Validation("límite de líneas demasiado grande".into()))?;
    let mut rows=c.prepare("SELECT line FROM (SELECT seq,line FROM log_lines WHERE log=?1 ORDER BY seq DESC LIMIT ?2) ORDER BY seq")?;
    Ok(rows
        .query_map(params![log.as_str(), limit], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?)
}
