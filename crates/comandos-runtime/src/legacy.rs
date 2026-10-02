use comandos_store::{Error, Result, append_event};
use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior, params};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::Path;

pub const LEGACY_MARKER: &str = "events.legacy_import";

/// Import once, without a pane/session target or live-state side effects.
/// Existing callers can group this import with their own transaction.
pub fn import_legacy(conn: &Connection, path: &Path) -> Result<u64> {
    let tx = if conn.is_autocommit() {
        Some(Transaction::new_unchecked(
            conn,
            TransactionBehavior::Immediate,
        )?)
    } else {
        None
    };
    let done = conn
        .query_row(
            "SELECT value FROM workspace_meta WHERE key = ?",
            [LEGACY_MARKER],
            |r| r.get::<_, String>(0),
        )
        .optional()?;
    let mut count = 0u64;
    if done.is_none() {
        let first: Option<i64> = conn.query_row(
            "SELECT MIN(occurred_at_ms) FROM events WHERE evidence != 'historical'",
            [],
            |r| r.get(0),
        )?;
        let cutoff = first.map(|n| n.div_euclid(1000));
        let bytes = match std::fs::read(path) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => vec![],
            Err(e) => return Err(e.into()),
        };
        let text = String::from_utf8_lossy(&bytes);
        let mut seen = HashMap::<String, u64>::new();
        // Match Python splitlines, including Unicode line separators and lone CR.
        for line in text.split([
            '\n', '\r', '\u{b}', '\u{c}', '\u{1c}', '\u{1d}', '\u{1e}', '\u{85}', '\u{2028}',
            '\u{2029}',
        ]) {
            let Ok(row) = serde_json::from_str::<Value>(line) else {
                continue;
            };
            let kind = match row["status"].as_str() {
                Some("done") => "turn_completed",
                Some("waiting") => "input_requested",
                Some("working") => "prompt_accepted",
                Some("error") => "turn_failed",
                Some("cancelled") => "turn_cancelled",
                _ => continue,
            };
            let Some(seconds) = row["ts"].as_f64().filter(|n| *n >= 0.0) else {
                continue;
            };
            if cutoff.is_some_and(|c| seconds >= c as f64) {
                continue;
            }
            let milliseconds = if let Some(n) = row["ts"].as_u64() {
                n.checked_mul(1000)
            } else if seconds * 1000.0 < u64::MAX as f64 {
                Some((seconds * 1000.0) as u64)
            } else {
                None
            }
            .ok_or_else(|| Error::Validation("timestamp histórico fuera de rango".into()))?;
            let digest = format!("{:x}", Sha256::digest(line.as_bytes()))[..32].to_owned();
            let occurrence = seen.entry(digest.clone()).or_default();
            *occurrence += 1;
            let project = row["project"].as_str().unwrap_or("");
            let project_key = if project.is_empty()
                || project.chars().count() > 200
                || project.chars().any(|c| c < '\u{20}')
            {
                None
            } else {
                Some(project)
            };
            append_event(
                conn,
                &json!({"source":"legacy-timeline","sourceEventId":format!("legacy:{digest}:{occurrence}"),
                "projectKey":project_key,"kind":kind,"evidence":"historical","correlation":"unknown","occurredAtMs":milliseconds,
                "title":project,"excerpt":row["detail"].as_str().unwrap_or("")}),
                crate::now_ms()?,
                &crate::fresh_id("event")?,
                &crate::fresh_id("receipt")?,
            )?;
            count += 1;
        }
        conn.execute(
            "INSERT INTO workspace_meta VALUES (?, ?)",
            params![
                LEGACY_MARKER,
                json!({"at":crate::now_ms()? as f64/1000.0,"count":count}).to_string()
            ],
        )?;
    }
    if let Some(tx) = tx {
        tx.commit()?;
    }
    Ok(count)
}
