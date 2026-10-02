//! Notification SQLite state; callers supply their isolated or application connection.
use crate::{Error, Result, claim_delivery, get_event, latest_sequence, list_events};
use comandos_core::notifications::{self as policy, LiveCheck};
use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior, params};
use serde_json::{Value, json};
use std::collections::BTreeSet;

// The Python notification adapter always begins its own immediate transaction.
// A nested call fails before it can commit or roll back the caller's transaction.
fn transaction<T>(conn: &Connection, run: impl FnOnce() -> Result<T>) -> Result<T> {
    let tx = Transaction::new_unchecked(conn, TransactionBehavior::Immediate)?;
    let result = run()?;
    tx.commit()?;
    Ok(result)
}

/// Upsert a heartbeat; interaction timestamps change only on explicit interaction.
pub fn record_presence(
    conn: &Connection,
    device_id: &str,
    visible: bool,
    can_play_audio: bool,
    interaction: bool,
    now_ms: i64,
    kind: &Value,
) -> Result<()> {
    if device_id.is_empty() || device_id.chars().count() > 200 {
        return Err(Error::Validation("deviceId inválido".into()));
    }
    let kind = if !comandos_core::legacy_truthy(kind) {
        "web".into()
    } else {
        match kind {
            Value::String(s) => s.clone(),
            Value::Bool(true) => "True".into(),
            other => other.to_string(),
        }
    };
    let kind: String = kind.chars().take(16).collect();
    transaction(conn, || {
        let row: Option<(bool, Option<i64>)> = conn
            .query_row(
                "SELECT visible,hidden_since_ms FROM client_presence WHERE device_id=?",
                [device_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        let hidden = if visible {
            None
        } else {
            Some(
                row.filter(|(visible, _)| !*visible)
                    .and_then(|(_, hidden)| hidden)
                    .filter(|t| *t != 0)
                    .unwrap_or(now_ms),
            )
        };
        conn.execute("INSERT INTO client_presence(device_id,kind,visible,can_play_audio,last_seen_at_ms,last_interaction_at_ms,hidden_since_ms) VALUES(?,?,?,?,?,?,?) ON CONFLICT(device_id) DO UPDATE SET kind=excluded.kind,visible=excluded.visible,can_play_audio=excluded.can_play_audio,last_seen_at_ms=excluded.last_seen_at_ms,last_interaction_at_ms=COALESCE(excluded.last_interaction_at_ms,last_interaction_at_ms),hidden_since_ms=excluded.hidden_since_ms",params![device_id,kind,visible,can_play_audio,now_ms,interaction.then_some(now_ms),hidden])?;
        Ok(())
    })
}

pub fn clients(conn: &Connection, _now_ms: i64) -> Result<Vec<Value>> {
    let mut stmt = conn.prepare("SELECT device_id,kind,visible,can_play_audio,last_seen_at_ms,last_interaction_at_ms,hidden_since_ms FROM client_presence")?;
    Ok(stmt.query_map([], |r|Ok(json!({"deviceId":r.get::<_,String>(0)?,"kind":r.get::<_,String>(1)?,"visible":r.get::<_,bool>(2)?,"canPlayAudio":r.get::<_,bool>(3)?,"lastSeenAt":r.get::<_,i64>(4)?,"lastInteractionAt":r.get::<_,Option<i64>>(5)?,"hiddenSince":r.get::<_,Option<i64>>(6)?,"connected":true})))?.collect::<rusqlite::Result<Vec<_>>>()?)
}

pub fn mark_read(conn: &Connection, event_ids: &[Value], now_ms: i64) -> Result<Vec<String>> {
    let ids: Vec<_> = event_ids
        .iter()
        .filter_map(Value::as_str)
        .filter(|id| !id.is_empty())
        .take(1000)
        .map(str::to_string)
        .collect();
    transaction(conn, || {
        let mut stmt =
            conn.prepare("INSERT OR IGNORE INTO notice_reads(event_id,read_at_ms) VALUES(?,?)")?;
        for id in &ids {
            stmt.execute(params![id, now_ms])?;
        }
        Ok(())
    })?;
    Ok(ids)
}

pub fn load_prefs(conn: &Connection) -> Result<Value> {
    let row: Option<String> = conn
        .query_row("SELECT value FROM notice_prefs WHERE id=1", [], |r| {
            r.get(0)
        })
        .optional()?;
    let default = policy::default_prefs();
    Ok(row
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .and_then(|update| policy::merge_prefs(&default, &update).ok())
        .unwrap_or(default))
}

pub fn save_prefs(conn: &Connection, update: &Value) -> Result<Value> {
    let prefs = policy::merge_prefs(&load_prefs(conn)?, update).map_err(Error::Validation)?;
    transaction(conn, || {
        conn.execute(
            "INSERT OR REPLACE INTO notice_prefs(id,value) VALUES(1,?)",
            [prefs.to_string()],
        )?;
        Ok(())
    })?;
    Ok(prefs)
}

fn recent(conn: &Connection) -> Result<Vec<Value>> {
    list_events(
        conn,
        (latest_sequence(conn)? - policy::HISTORY_WINDOW).max(0),
        policy::HISTORY_WINDOW,
    )
}
fn read_ids(conn: &Connection) -> Result<BTreeSet<String>> {
    let mut stmt = conn.prepare("SELECT event_id FROM notice_reads")?;
    Ok(stmt
        .query_map([], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?)
}

fn each_notice(conn: &Connection, mut visit: impl FnMut(&Value)) -> Result<()> {
    let mut after = 0;
    loop {
        let page = list_events(conn, after, 500)?;
        if page.is_empty() {
            break;
        }
        for event in &page {
            if policy::classify(event)["notice"] == true {
                visit(event);
            }
        }
        after = page.last().unwrap()["sequence"].as_i64().unwrap();
    }
    Ok(())
}

pub fn revision(conn: &Connection) -> Result<String> {
    let sequence = latest_sequence(conn)?;
    let (reads, last): (i64, i64) = conn.query_row(
        "SELECT COUNT(*),COALESCE(MAX(read_at_ms),0) FROM notice_reads",
        [],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    Ok(format!("{sequence}.{reads}.{last}"))
}

/// All-history unread count; pending requests use the same 500-event window as Python.
pub fn badge_count(conn: &Connection, is_live: LiveCheck<'_>) -> Result<i64> {
    let read = read_ids(conn)?;
    let pending: BTreeSet<_> = policy::live_pending(&recent(conn)?, is_live)
        .into_iter()
        .collect();
    let mut count = 0;
    each_notice(conn, |event| {
        let id = event["eventId"].as_str().unwrap();
        if !read.contains(id) || pending.contains(id) {
            count += 1;
        }
    })?;
    Ok(count)
}

pub fn unread_notice_ids(conn: &Connection, project: Option<&str>) -> Result<Vec<String>> {
    let read = read_ids(conn)?;
    let project = project.filter(|s| !s.is_empty());
    let mut ids = vec![];
    each_notice(conn, |e| {
        let id = e["eventId"].as_str().unwrap();
        if !read.contains(id)
            && project
                .is_none_or(|p| policy::classify(e)["category"] != "news" && e["projectKey"] == p)
        {
            ids.push(id.to_string());
        }
    })?;
    Ok(ids)
}

pub fn list_notices(
    conn: &Connection,
    after: i64,
    limit: i64,
    now_ms: i64,
    _device_id: Option<&str>,
    focus_active: bool,
    is_live: LiveCheck<'_>,
) -> Result<Value> {
    let prefs = load_prefs(conn)?;
    let present = clients(conn, now_ms)?;
    let history = recent(conn)?;
    let groups = policy::group_keys(&history, &prefs);
    let page = list_events(conn, after, limit)?;
    let read = read_ids(conn)?;
    let notices: Vec<_> = page
        .iter()
        .filter(|e| policy::classify(e)["notice"] == true)
        .map(|e| {
            let id = e["eventId"].as_str().unwrap();
            policy::notice(
                e,
                &present,
                &prefs,
                now_ms,
                &read,
                groups.get(id).map(String::as_str).unwrap_or(id),
                focus_active,
            )
        })
        .collect();
    Ok(
        json!({"notices":notices,"nextAfter":page.last().map(|e|e["sequence"].clone()).unwrap_or(json!(after)),"pending":policy::live_pending(&history,is_live),"prefs":prefs,"focusActive":focus_active}),
    )
}

pub fn focus_block_active(conn: &Connection) -> bool {
    conn.query_row("SELECT b.mode,b.status FROM pomodoro_state s JOIN pomodoro_blocks b ON b.block_id=s.block_id WHERE s.id=1",[],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?))).is_ok_and(|(mode,status)|mode=="focus"&&status=="running")
}

fn operational(error: &Error) -> bool {
    // sqlite3.OperationalError includes a failed nested BEGIN and busy/locked DBs.
    matches!(error,Error::Sql(rusqlite::Error::SqliteFailure(e,_)) if matches!(e.extended_code & 0xff, 1|3|4|5|6|8|9|10|13|14|15|16|17))
}

/// Claim on the chosen device, using the shared empty-device delivery key so
/// changing the chosen speaker never permits the same event to sound twice.
pub fn claim_sound(
    conn: &Connection,
    event_id: &str,
    device_id: &str,
    now_ms: i64,
    focus_active: Option<bool>,
) -> Result<Value> {
    let Some(event) = get_event(conn, event_id)? else {
        return Ok(json!({"play":false,"reason":"Evento desconocido"}));
    };
    let focus_active = focus_active.unwrap_or_else(|| focus_block_active(conn));
    let route = policy::route_event(
        &event,
        &clients(conn, now_ms)?,
        &load_prefs(conn)?,
        now_ms,
        focus_active,
    );
    if route["sound"] != true {
        return Ok(json!({"play":false,"reason":"Solo visual"}));
    }
    if route["soundDevice"] != device_id {
        return Ok(json!({"play":false,"reason":"Suena en otro dispositivo"}));
    }
    match transaction(conn, || claim_delivery(conn, event_id, "sound", "", now_ms)) {
        Err(error) if operational(&error) => Ok(json!({"play":false,"reason":"Ocupado"})),
        Err(error) => Err(error),
        Ok(false) => Ok(json!({"play":false,"reason":"Ya sonó"})),
        Ok(true) => Ok(json!({"play":true,"cue":route["cue"]})),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sqlite_error_classes_match_cpython_operational_error_mapping() {
        // Mapping in CPython 3.11 Modules/_sqlite/util.c, get_exception_class.
        let operational_codes = [1, 3, 4, 5, 6, 8, 9, 10, 13, 14, 15, 16, 17];
        for code in 1..=28 {
            let error = Error::Sql(rusqlite::Error::SqliteFailure(
                rusqlite::ffi::Error::new(code),
                None,
            ));
            assert_eq!(
                operational(&error),
                operational_codes.contains(&code),
                "SQLite primary code {code}"
            );
        }
        let extended_busy = Error::Sql(rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(5 | (2 << 8)),
            None,
        ));
        assert!(operational(&extended_busy));
    }
}
