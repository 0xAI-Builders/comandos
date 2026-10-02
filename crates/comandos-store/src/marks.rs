//! Human work marks, independent from the observed state of the agent.
use crate::{Error, Result, with_transaction};
use comandos_core::legacy_truthy;
use rusqlite::{Connection, OptionalExtension, Row, params};
use serde_json::{Value, json};

fn check(scope: &str, key: &str) -> Result<()> {
    if !["session", "pane"].contains(&scope) {
        return Err(Error::Validation("Ámbito inválido".into()));
    }
    if key.is_empty() || key.chars().count() > 200 || key.chars().any(|c| c < '\u{20}') {
        return Err(Error::Validation("Clave inválida".into()));
    }
    Ok(())
}

fn row(scope: &str, key: &str, r: &Row<'_>, offset: usize) -> rusqlite::Result<Value> {
    Ok(
        json!({"scope":scope,"key":key,"mark":r.get::<_,String>(offset)?,
        "favorite":r.get::<_,i64>(offset+1)?!=0,"revision":r.get::<_,i64>(offset+2)?,
        "updatedAtMs":r.get::<_,i64>(offset+3)?,"updatedBy":r.get::<_,String>(offset+4)?}),
    )
}

pub fn get_mark(conn: &Connection, scope: &str, key: &str) -> Result<Value> {
    check(scope, key)?;
    Ok(conn.query_row("SELECT mark, favorite, revision, updated_at_ms, updated_by FROM work_marks WHERE scope = ? AND key = ?",
        params![scope,key],|r|row(scope,key,r,0)).optional()?.unwrap_or_else(||json!({"scope":scope,"key":key,"mark":"none","favorite":false,"revision":0,"updatedAtMs":null,"updatedBy":null})))
}

pub fn list_marks(conn: &Connection) -> Result<Vec<Value>> {
    let mut statement=conn.prepare("SELECT scope, key, mark, favorite, revision, updated_at_ms, updated_by FROM work_marks ORDER BY scope, key")?;
    Ok(statement
        .query_map([], |r| {
            row(&r.get::<_, String>(0)?, &r.get::<_, String>(1)?, r, 2)
        })?
        .collect::<rusqlite::Result<_>>()?)
}

fn write(
    conn: &Connection,
    current: &Value,
    mark: &str,
    favorite: bool,
    by: &str,
    now: i64,
) -> Result<Value> {
    let revision = current["revision"]
        .as_i64()
        .expect("stored revision")
        .checked_add(1)
        .ok_or_else(|| Error::Validation("revision fuera de rango SQLite".into()))?;
    let scope = current["scope"].as_str().expect("stored scope");
    let key = current["key"].as_str().expect("stored key");
    conn.execute(
        "INSERT OR REPLACE INTO work_marks VALUES (?, ?, ?, ?, ?, ?, ?)",
        params![scope, key, mark, favorite, revision, now, by],
    )?;
    get_mark(conn, scope, key)
}

pub fn set_mark(
    conn: &Connection,
    scope: &str,
    key: &str,
    value: &Value,
    expected_revision: &Value,
    now: i64,
) -> Result<Value> {
    check(scope, key)?;
    let value = if value.is_string() {
        json!({"mark":value})
    } else {
        value.clone()
    };
    let change = value
        .as_object()
        .filter(|v| !v.is_empty() && v.keys().all(|k| k == "mark" || k == "favorite"))
        .ok_or_else(|| Error::Validation("Valor inválido".into()))?;
    if change.contains_key("mark")
        && !value["mark"]
            .as_str()
            .is_some_and(|s| ["none", "resolved", "frozen", "awaiting_reply"].contains(&s))
    {
        return Err(Error::Validation("Marca inválida".into()));
    }
    if change.contains_key("favorite") && !value["favorite"].is_boolean() {
        return Err(Error::Validation("Favorito inválido".into()));
    }
    let expected = expected_revision
        .as_u64()
        .ok_or_else(|| Error::Validation("expectedRevision inválido".into()))?;
    with_transaction(conn, || {
        let current = get_mark(conn, scope, key)?;
        if current["revision"].as_u64() != Some(expected) {
            return Err(Error::Conflict(current));
        }
        write(
            conn,
            &current,
            value["mark"]
                .as_str()
                .unwrap_or_else(|| current["mark"].as_str().expect("stored mark")),
            value["favorite"]
                .as_bool()
                .unwrap_or_else(|| current["favorite"].as_bool().expect("stored favorite")),
            "user",
            now,
        )
    })
}

pub fn apply_turn_event(conn: &Connection, event: &Value, now: i64) -> Result<Vec<Value>> {
    if !legacy_truthy(&event["eventId"]) || event["kind"] != "prompt_accepted" {
        return Ok(vec![]);
    }
    let targets: Vec<_> = [("pane", "paneKey"), ("session", "sessionKey")]
        .into_iter()
        .filter(|(_, field)| legacy_truthy(&event[*field]))
        .collect();
    if targets.is_empty() {
        return Ok(vec![]);
    }
    with_transaction(conn, || {
        let mut changed = Vec::new();
        for (scope, field) in targets {
            let key = event[field]
                .as_str()
                .ok_or_else(|| Error::Validation("Clave inválida".into()))?;
            check(scope, key)?;
            let id = event["eventId"]
                .as_str()
                .ok_or_else(|| Error::Validation("eventId inválido".into()))?;
            if conn.execute(
                "INSERT OR IGNORE INTO work_mark_applied VALUES (?, ?, ?)",
                params![id, scope, key],
            )? == 0
            {
                continue;
            }
            let current = get_mark(conn, scope, key)?;
            if current["mark"] == "resolved" && event["evidence"] == "confirmed" {
                changed.push(write(
                    conn,
                    &current,
                    "none",
                    current["favorite"].as_bool().expect("stored favorite"),
                    &format!("event:{id}"),
                    now,
                )?);
            }
        }
        Ok(changed)
    })
}
