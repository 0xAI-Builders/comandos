//! Apertura capaz del ledger de acciones; el consumidor Python se retira en S5.
use crate::{
    Result,
    migrate::{DbLocation, resolve_configured},
    unified,
};
use rusqlite::{Connection, Transaction, TransactionBehavior};
use std::{path::Path, time::Duration};
pub struct OperatorDb {
    pub conn: Connection,
    pub table: &'static str,
}
pub fn open_operator_db_at(path: &Path) -> Result<OperatorDb> {
    if let DbLocation::Unified(path) = resolve_configured(path)? {
        return Ok(OperatorDb {
            conn: unified::open_unified(&path)?,
            table: "operator_actions",
        });
    }
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)?;
    }
    let conn = Connection::open(path)?;
    conn.busy_timeout(Duration::from_secs(3))?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    let tx = Transaction::new_unchecked(&conn, TransactionBehavior::Immediate)?;
    crate::migrate::move_db::admit_write(&conn)?;
    conn.execute_batch("CREATE TABLE IF NOT EXISTS actions (id TEXT PRIMARY KEY,tool TEXT,status TEXT,detail TEXT,created REAL,updated REAL)")?;
    tx.commit()?;
    Ok(OperatorDb {
        conn,
        table: "actions",
    })
}
