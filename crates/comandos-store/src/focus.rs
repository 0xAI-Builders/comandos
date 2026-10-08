//! Focus policy activation and paid ledger. Callers own the transaction.
use crate::{Error, Result};
use comandos_core::{focus, json as codec};
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::{Value, json};
fn version(policy: &Value) -> Result<&str> {
    policy["policyVersion"]
        .as_str()
        .ok_or_else(|| Error::Validation("policyVersion inválido".into()))
}
pub fn ensure_policy(conn: &Connection, policy: &Value, now: i64) -> Result<i64> {
    crate::with_transaction(conn, || {
        let version = version(policy)?;
        let document =
            codec::workspace_dumps_with_options(policy, true, false).map_err(Error::Validation)?;
        conn.execute("INSERT INTO focus_policies (policy_version,document,activated_at_ms) VALUES (?,?,?) ON CONFLICT(policy_version) DO NOTHING",params![version,document,now])?;
        activation(conn, policy)?.ok_or_else(|| rusqlite::Error::QueryReturnedNoRows.into())
    })
}
fn activation(conn: &Connection, policy: &Value) -> Result<Option<i64>> {
    Ok(conn
        .query_row(
            "SELECT activated_at_ms FROM focus_policies WHERE policy_version=?",
            [version(policy)?],
            |r| r.get(0),
        )
        .optional()?)
}
pub fn award(conn: &Connection, record: &Value, policy: &Value, now: i64) -> Result<Option<Value>> {
    crate::with_transaction(conn, || {
        let Some(at) = activation(conn, policy)? else {
            return Ok(None);
        };
        if !focus::eligible(record, policy, Some(at)) {
            return Ok(None);
        }
        let reward = focus::reward_for(record, policy).map_err(Error::Validation)?;
        let version = version(policy)?;
        let before: i64 = conn.query_row(
            "SELECT COALESCE(SUM(xp),0) FROM focus_rewards WHERE policy_version=?",
            [version],
            |r| r.get(0),
        )?;
        let xp = focus::int(&reward["xp"]).map_err(Error::Validation)?;
        let minutes = focus::int(&reward["minutes"]).map_err(Error::Validation)?;
        let level_before = focus::level_for(before, policy).map_err(Error::Validation)?;
        let level_after = focus::level_for(
            before
                .checked_add(xp)
                .ok_or_else(|| Error::Validation("XP fuera de rango SQLite".into()))?,
            policy,
        )
        .map_err(Error::Validation)?;
        let reached = (level_after > level_before).then_some(level_after);
        let start = if codec::truthy(&record["startedAtMs"]) {
            &record["startedAtMs"]
        } else {
            &record["endedAtMs"]
        };
        let start = focus::int(start).map_err(Error::Validation)?;
        let end = focus::int(&record["endedAtMs"]).map_err(Error::Validation)?;
        let changed=conn.execute("INSERT INTO focus_rewards (block_id,policy_version,xp,minutes,status,started_at_ms,ended_at_ms,level_reached,awarded_at_ms) VALUES (?,?,?,?,?,?,?,?,?) ON CONFLICT(block_id,policy_version) DO NOTHING",params![record["blockId"].as_str(),version,xp,minutes,record["status"].as_str(),start,end,reached,now])?;
        Ok((changed==1).then(||json!({"blockId":record["blockId"],"policyVersion":version,"xp":xp,"minutes":minutes,"levelReached":reached})))
    })
}
pub fn ledger_progress(conn: &Connection, policy: &Value, now: i64) -> Result<Value> {
    let version = version(policy)?;
    let blocks=conn.prepare("SELECT block_id,minutes,status,started_at_ms,ended_at_ms FROM focus_rewards WHERE policy_version=?")?.query_map([version],|r|{let minutes=r.get::<_,i64>(1)?;let active=minutes.checked_mul(comandos_core::pomodoro::MINUTE_MS).ok_or(rusqlite::Error::IntegralValueOutOfRange(1,minutes))?;Ok(json!({"blockId":r.get::<_,String>(0)?,"mode":"focus","status":r.get::<_,String>(2)?,"activeMs":active,"startedAtMs":r.get::<_,i64>(3)?,"endedAtMs":r.get::<_,i64>(4)?,"provenance":"measured"}))})?.collect::<rusqlite::Result<Vec<_>>>()?;
    let mut out = focus::progress(&blocks, policy, now, None).map_err(Error::Validation)?;
    out["lastLevelUp"]=conn.query_row("SELECT block_id,level_reached,awarded_at_ms FROM focus_rewards WHERE policy_version=? AND level_reached IS NOT NULL ORDER BY awarded_at_ms DESC,level_reached DESC LIMIT 1",[version],|r|Ok(json!({"blockId":r.get::<_,String>(0)?,"level":r.get::<_,i64>(1)?,"awardedAtMs":r.get::<_,i64>(2)?}))).optional()?.unwrap_or(Value::Null);
    out["activatedAtMs"] = json!(activation(conn, policy)?);
    Ok(out)
}
