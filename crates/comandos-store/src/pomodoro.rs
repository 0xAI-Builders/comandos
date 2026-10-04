//! SQLite timer authority; supplied callbacks participate in its transaction.
use rusqlite::Connection;
use serde_json::{Value, json};
pub type Result<T> = std::result::Result<T, Error>;
#[derive(Debug)]
pub enum Error {
    Domain(comandos_core::pomodoro::Error),
    Persistence(crate::Error),
}
impl From<crate::Error> for Error {
    fn from(e: crate::Error) -> Self {
        Self::Persistence(e)
    }
}
impl From<rusqlite::Error> for Error {
    fn from(e: rusqlite::Error) -> Self {
        Self::Persistence(e.into())
    }
}
impl From<comandos_core::pomodoro::Error> for Error {
    fn from(e: comandos_core::pomodoro::Error) -> Self {
        Self::Domain(e)
    }
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Domain(e) => e.fmt(f),
            Self::Persistence(e) => e.fmt(f),
        }
    }
}
impl std::error::Error for Error {}
pub type Emit<'a> = dyn Fn(&Connection, &Value) -> Result<()> + 'a;
pub type Rewards<'a> = dyn Fn(&Connection, &Value, i64) -> Result<()> + 'a;
pub struct PomodoroStore<'a> {
    pub conn: &'a Connection,
    pub clock: &'a dyn Fn() -> i64,
    pub new_id: &'a dyn Fn() -> String,
    pub emit: Option<&'a Emit<'a>>,
    pub rewards: Option<&'a Rewards<'a>>,
    pub log: Option<&'a dyn Fn(&str)>,
}
use comandos_core::{focus::int as legacy_int, json as codec, pomodoro as rules};
use rusqlite::{OptionalExtension, Row, params, types::Value as SqlValue};
use sha2::{Digest, Sha256};
const BLOCK_COLUMNS: &str = "block_id, mode, status, target_ms, active_ms, resumed_at_ms, deadline_ms, started_at_ms, ended_at_ms, project, session_key, pane_key, cycle_index, cycle_total, source";
const RECORD_COLUMNS: &str = "block_id, mode, status, target_ms, active_ms, planned_ms, started_at_ms, ended_at_ms, project, session_key, pane_key, provenance";
fn validation(message: impl Into<String>) -> Error {
    crate::Error::Validation(message.into()).into()
}
fn checked(n: i128) -> Result<i64> {
    i64::try_from(n).map_err(|_| validation("entero fuera de rango SQLite"))
}
/// Own the write transaction; timer commands cannot join an outer transaction
/// because pre-command settlement must commit independently.
fn transaction<T>(conn: &Connection, read: bool, run: impl FnOnce() -> Result<T>) -> Result<T> {
    if !conn.is_autocommit() {
        if read {
            return run();
        }
        return Err(validation(
            "temporizador requiere una conexión sin transacción activa",
        ));
    }
    let behavior = if read {
        rusqlite::TransactionBehavior::Deferred
    } else {
        rusqlite::TransactionBehavior::Immediate
    };
    let tx = rusqlite::Transaction::new_unchecked(conn, behavior)?;
    let out = run()?;
    tx.commit()?;
    Ok(out)
}
fn block_row(row: &Row<'_>, revision: i64) -> rusqlite::Result<Value> {
    Ok(
        json!({"blockId":row.get::<_,String>(0)?,"revision":revision,"mode":row.get::<_,String>(1)?,"status":row.get::<_,String>(2)?,"targetMs":row.get::<_,i64>(3)?,"activeMs":row.get::<_,i64>(4)?,"resumedAtMs":row.get::<_,Option<i64>>(5)?,"deadlineMs":row.get::<_,Option<i64>>(6)?,"startedAtMs":row.get::<_,i64>(7)?,"endedAtMs":row.get::<_,Option<i64>>(8)?,"project":row.get::<_,String>(9)?,"sessionKey":row.get::<_,String>(10)?,"paneKey":row.get::<_,String>(11)?,"cycleIndex":row.get::<_,Option<i64>>(12)?,"cycleTotal":row.get::<_,Option<i64>>(13)?,"source":row.get::<_,String>(14)?}),
    )
}
fn record_row(row: &Row<'_>) -> rusqlite::Result<Value> {
    Ok(
        json!({"blockId":row.get::<_,String>(0)?,"mode":row.get::<_,String>(1)?,"status":row.get::<_,String>(2)?,"targetMs":row.get::<_,i64>(3)?,"activeMs":row.get::<_,Option<i64>>(4)?,"plannedMs":row.get::<_,i64>(5)?,"startedAtMs":row.get::<_,i64>(6)?,"endedAtMs":row.get::<_,Option<i64>>(7)?,"project":row.get::<_,String>(8)?,"sessionKey":row.get::<_,String>(9)?,"paneKey":row.get::<_,String>(10)?,"provenance":row.get::<_,String>(11)?}),
    )
}
fn live(block: &Value) -> bool {
    matches!(block["status"].as_str(), Some("running" | "paused"))
}
impl<'a> PomodoroStore<'a> {
    pub fn new(
        conn: &'a Connection,
        clock: &'a dyn Fn() -> i64,
        new_id: &'a dyn Fn() -> String,
    ) -> Self {
        Self {
            conn,
            clock,
            new_id,
            emit: None,
            rewards: None,
            log: None,
        }
    }
    fn state(&self) -> Result<(i64, Option<String>, Option<i64>)> {
        Ok(self.conn.query_row(
            "SELECT revision,block_id,settled_from FROM pomodoro_state WHERE id=1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )?)
    }
    fn current(&self) -> Result<(i64, Value)> {
        let (revision, id, _) = self.state()?;
        let block = if let Some(id) = id {
            self.conn
                .query_row(
                    &format!("SELECT {BLOCK_COLUMNS} FROM pomodoro_blocks WHERE block_id=?"),
                    [id],
                    |r| block_row(r, revision),
                )
                .optional()?
                .unwrap_or(Value::Null)
        } else {
            Value::Null
        };
        Ok((revision, block))
    }
    fn snapshot_locked(&self, now: i64) -> Result<Value> {
        let (revision, block) = self.current()?;
        Ok(json!({"revision":revision,"serverNowMs":now,"block":block}))
    }
    pub fn snapshot(&self) -> Result<Value> {
        let now = (self.clock)();
        transaction(self.conn, true, || self.snapshot_locked(now))
    }
    pub fn next_deadline_ms(&self) -> Result<Option<i64>> {
        let (_, b) = self.current()?;
        Ok(if b["status"] == "running" {
            b["deadlineMs"].as_i64()
        } else {
            None
        })
    }
    fn bump(&self, new_block: Option<&str>, keep: bool, settled: bool) -> Result<i64> {
        let (revision, current, _) = self.state()?;
        let target = if keep && new_block.is_none() {
            current.as_deref()
        } else {
            new_block
        };
        let next = checked(revision as i128 + 1)?;
        self.conn.execute(
            "UPDATE pomodoro_state SET revision=?,block_id=?,settled_from=? WHERE id=1",
            params![next, target, settled.then_some(revision)],
        )?;
        Ok(next)
    }
    fn record(
        &self,
        block: &Value,
        status: &str,
        active: i64,
        end: i64,
        now: i64,
    ) -> Result<Option<Value>> {
        let inserted=self.conn.execute("INSERT INTO pomodoro_records (block_id,mode,status,target_ms,active_ms,planned_ms,started_at_ms,ended_at_ms,project,session_key,pane_key,provenance,recorded_at_ms) VALUES (?,?,?,?,?,?,?,?,?,?,?,'measured',?) ON CONFLICT(block_id) DO NOTHING",params![block["blockId"].as_str(),block["mode"].as_str(),status,block["targetMs"].as_i64(),active,block["targetMs"].as_i64(),block["startedAtMs"].as_i64(),end,block["project"].as_str(),block["sessionKey"].as_str(),block["paneKey"].as_str(),now])?;
        if inserted != 1 {
            return Ok(None);
        }
        let record = json!({"blockId":block["blockId"],"mode":block["mode"],"status":status,"targetMs":block["targetMs"],"activeMs":active,"plannedMs":block["targetMs"],"startedAtMs":block["startedAtMs"],"endedAtMs":end,"project":block["project"],"sessionKey":block["sessionKey"],"paneKey":block["paneKey"],"provenance":"measured"});
        if block["mode"] == "focus"
            && let Some(rewards) = self.rewards
        {
            rewards(self.conn, &record, now)?
        }
        Ok(Some(record))
    }
    fn settle_locked(&self, now: i64) -> Result<Option<String>> {
        let (_, block) = self.current()?;
        if block["status"] != "running" {
            return Ok(None);
        }
        let end = block["deadlineMs"]
            .as_i64()
            .ok_or_else(|| validation("deadlineMs inválido"))?;
        if end > now {
            return Ok(None);
        }
        let id = block["blockId"]
            .as_str()
            .ok_or_else(|| validation("blockId inválido"))?;
        let target = block["targetMs"]
            .as_i64()
            .ok_or_else(|| validation("targetMs inválido"))?;
        self.conn.execute("UPDATE pomodoro_blocks SET status='completed',active_ms=target_ms,resumed_at_ms=NULL,deadline_ms=NULL,ended_at_ms=?,updated_at_ms=? WHERE block_id=? AND status='running'",params![end,now,id])?;
        let record = self.record(&block, "completed", target, end, now)?;
        self.bump(None, true, true)?;
        if record.is_some()
            && block["mode"] == "focus"
            && let Some(emit) = self.emit
        {
            let project = rules::event_ident(&block["project"]).unwrap_or("sin proyecto");
            emit(
                self.conn,
                &json!({"eventId":format!("pomodoro:{id}:completed"),"source":"pomodoro","kind":"focus_completed","evidence":"confirmed","correlation":"source","projectKey":rules::event_ident(&block["project"]),"sessionKey":rules::event_ident(&block["sessionKey"]),"paneKey":rules::event_ident(&block["paneKey"]),"occurredAtMs":end,"receivedAtMs":now,"title":"Pomodoro completado","excerpt":format!("{} min de foco · {project}",target/rules::MINUTE_MS),"blockId":id}),
            )?
        }
        Ok(Some(id.into()))
    }
    pub fn settle_due(&self, now: Option<i64>) -> Result<Vec<String>> {
        let now = now.unwrap_or_else(|| (self.clock)());
        let done = transaction(self.conn, false, || self.settle_locked(now))?;
        if let (Some(done), Some(log)) = (&done, self.log) {
            log(&format!("pomodoro: block {done} completed"));
        }
        Ok(done.into_iter().collect())
    }
    pub fn digest(request: &Value) -> Result<String> {
        // Validate the borrowed request before recursively cloning retained JSON.
        codec::workspace_dumps_with_options(request, true, false).map_err(validation)?;
        let mut body = request.clone();
        let object = body
            .as_object_mut()
            .ok_or_else(|| rules::Error::invalid("invalid_request", "Solicitud inválida"))?;
        object.remove("requestId");
        object.remove("expectedRevision");
        let bytes = codec::workspace_dumps_with_options(&body, true, false).map_err(validation)?;
        Ok(format!("{:x}", Sha256::digest(bytes.as_bytes())))
    }
    pub fn command(&self, request: &Value) -> Result<Value> {
        if !request.is_object() {
            return Err(rules::Error::invalid("invalid_request", "Solicitud inválida").into());
        }
        let rid = request["requestId"]
            .as_str()
            .filter(|s| {
                !s.trim_matches(|c: char| c.is_whitespace() || matches!(c, '\u{1c}'..='\u{1f}'))
                    .is_empty()
                    && s.chars().count() <= 120
            })
            .ok_or_else(|| rules::Error::invalid("invalid_request_id", "requestId requerido"))?;
        let action = request["action"]
            .as_str()
            .filter(|a| matches!(*a, "start" | "pause" | "resume" | "extend" | "cancel"))
            .ok_or_else(|| rules::Error::invalid("invalid_action", "Acción no soportada"))?;
        let expected = if request["expectedRevision"].is_null() {
            None
        } else {
            Some(rules::integer(
                &request["expectedRevision"],
                "expectedRevision",
            )?)
        };
        let digest = Self::digest(request)?;
        let now = (self.clock)();
        self.settle_due(Some(now))?;
        transaction(self.conn, false, || {
            let existing = self
                .conn
                .query_row(
                    "SELECT digest,response FROM pomodoro_requests WHERE request_id=?",
                    [rid],
                    |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
                )
                .optional()?;
            if let Some((old, response)) = existing {
                if old != digest {
                    return Err(rules::Error::conflict(
                        "request_reused",
                        "requestId ya usado con otra operación",
                        self.snapshot_locked(now)?,
                    )
                    .into());
                }
                let mut out =
                    codec::parse_value(&response).map_err(|e| validation(e.to_string()))?;
                out["serverNowMs"] = json!(now);
                out["replayed"] = json!(true);
                return Ok(out);
            }
            self.settle_locked(now)?;
            let (revision, block) = self.current()?;
            let (_, _, settled_from) = self.state()?;
            if let Some(expected) = expected {
                let expected = expected.as_str().parse::<i128>().ok();
                if expected != Some(revision as i128)
                    && expected.is_none_or(|e| Some(e) != settled_from.map(i128::from))
                {
                    return Err(rules::Error::conflict(
                        "stale_revision",
                        "El temporizador cambió en otro dispositivo",
                        self.snapshot_locked(now)?,
                    )
                    .into());
                }
            }
            self.execute_action(action, request, &block, now)?;
            let (revision, block) = self.current()?;
            let mut out = json!({"ok":true,"revision":revision,"block":block,"replayed":false});
            let expires = checked(now as i128 - rules::REQUEST_TTL_MS as i128)?;
            self.conn.execute(
                "DELETE FROM pomodoro_requests WHERE created_at_ms < ?",
                [expires],
            )?;
            self.conn.execute(
                "INSERT INTO pomodoro_requests VALUES (?,?,?,?,?)",
                params![
                    rid,
                    digest,
                    revision,
                    codec::dumps(&out, true, false).map_err(validation)?,
                    now
                ],
            )?;
            out["serverNowMs"] = json!(now);
            Ok(out)
        })
    }
    fn require_live(&self, block: &Value, now: i64, only: Option<&str>) -> Result<()> {
        if !live(block) || only.is_some_and(|s| block["status"] != s) {
            return Err(rules::Error::conflict(
                "not_active",
                "No hay un bloque en ese estado",
                self.snapshot_locked(now)?,
            )
            .into());
        }
        Ok(())
    }
    fn execute_action(&self, action: &str, request: &Value, block: &Value, now: i64) -> Result<()> {
        if action == "start" {
            if live(block) {
                return Err(rules::Error::conflict(
                    "active_block",
                    "Ya hay un bloque en curso",
                    self.snapshot_locked(now)?,
                )
                .into());
            }
            let mode = request.get("mode").unwrap_or(&Value::Null);
            let mode = if request.get("mode").is_none() {
                "focus"
            } else {
                mode.as_str()
                    .filter(|s| matches!(*s, "focus" | "break"))
                    .ok_or_else(|| rules::Error::invalid("invalid_mode", "Modo inválido"))?
            };
            let target = rules::integer(&request["targetMs"], "targetMs")?
                .as_i64()
                .filter(|t| (rules::MIN_TARGET_MS..=rules::MAX_TARGET_MS).contains(t))
                .ok_or_else(|| {
                    rules::Error::invalid(
                        "invalid_targetMs",
                        "La duración debe estar entre 1 y 180 minutos",
                    )
                })?;
            let id = (self.new_id)();
            let project = rules::destination(&request["project"], "project")?;
            let session = rules::destination(&request["sessionKey"], "sessionKey")?;
            let pane = rules::destination(&request["paneKey"], "paneKey")?;
            let end = checked(now as i128 + target as i128)?;
            self.conn.execute(&format!("INSERT INTO pomodoro_blocks ({BLOCK_COLUMNS},updated_at_ms) VALUES (?,?,'running',?,0,?,?,?,NULL,?,?,?,?,?,'comandos',?)"),params![id,mode,target,now,end,now,project,session,pane,rules::optional_integer(&request["cycleIndex"],1,99),rules::optional_integer(&request["cycleTotal"],1,99),now])?;
            self.bump(Some(&id), false, false)?;
            return Ok(());
        }
        self.require_live(
            block,
            now,
            match action {
                "pause" => Some("running"),
                "resume" => Some("paused"),
                _ => None,
            },
        )?;
        let id = block["blockId"].as_str();
        let target = block["targetMs"]
            .as_i64()
            .ok_or_else(|| validation("targetMs inválido"))?;
        let active = block["activeMs"]
            .as_i64()
            .ok_or_else(|| validation("activeMs inválido"))?;
        match action {
            "pause" => {
                self.conn.execute("UPDATE pomodoro_blocks SET status='paused',active_ms=?,resumed_at_ms=NULL,deadline_ms=NULL,updated_at_ms=? WHERE block_id=?",params![rules::elapsed_ms(block,now),now,id])?;
            }
            "resume" => {
                let deadline = checked(now as i128 + target as i128 - active as i128)?;
                self.conn.execute("UPDATE pomodoro_blocks SET status='running',resumed_at_ms=?,deadline_ms=?,updated_at_ms=? WHERE block_id=?",params![now,deadline,now,id])?;
            }
            "extend" => {
                let delta = rules::integer(&request["deltaMs"], "deltaMs")?;
                let d = delta.as_str().parse::<i128>().ok();
                if d == Some(0) {
                    return Err(
                        rules::Error::invalid("invalid_deltaMs", "deltaMs no puede ser 0").into(),
                    );
                }
                let changed = d.and_then(|d| d.checked_add(target as i128));
                let spent = rules::elapsed_ms(block, now);
                let lower = (rules::MIN_TARGET_MS as i128).max(spent as i128 + 1000);
                let changed=changed.filter(|n|*n>=lower&&*n<=rules::MAX_TARGET_MS as i128).ok_or_else(||rules::Error::invalid("invalid_deltaMs","La duración resultante queda fuera de 1 a 180 minutos o antes del tiempo ya trabajado"))?;
                let deadline = if block["status"] == "running" {
                    Some(checked(
                        block["resumedAtMs"]
                            .as_i64()
                            .ok_or_else(|| validation("resumedAtMs inválido"))?
                            as i128
                            + changed
                            - active as i128,
                    )?)
                } else {
                    None
                };
                self.conn.execute("UPDATE pomodoro_blocks SET target_ms=?,deadline_ms=?,updated_at_ms=? WHERE block_id=?",params![checked(changed)?,deadline,now,id])?;
            }
            "cancel" => {
                let spent = rules::elapsed_ms(block, now);
                self.conn.execute("UPDATE pomodoro_blocks SET status='cancelled',active_ms=?,resumed_at_ms=NULL,deadline_ms=NULL,ended_at_ms=?,updated_at_ms=? WHERE block_id=?",params![spent,now,now,id])?;
                self.record(block, "cancelled", spent, now, now)?;
            }
            _ => unreachable!("validated action"),
        }
        self.bump(None, true, false)?;
        Ok(())
    }
    pub fn import_legacy_focus(&self, data: &Value, now: Option<i64>) -> Result<bool> {
        let now = now.unwrap_or_else(|| (self.clock)());
        if !data.is_object() {
            return Ok(false);
        }
        let Some(until) = legacy_seconds(&data["until"])? else {
            return Ok(false);
        };
        let Some(started) = legacy_seconds(&data["startedAt"])? else {
            return Ok(false);
        };
        let mins = if codec::truthy(&data["mins"]) {
            let Some(mins) = legacy_integer_input(&data["mins"])? else {
                return Ok(false);
            };
            mins.parse::<i64>().ok()
        } else {
            let diff = match (until.parse::<i128>(), started.parse::<i128>()) {
                (Ok(u), Ok(s)) => u.checked_sub(s).map(|n| n as f64),
                _ => until
                    .parse::<f64>()
                    .ok()
                    .zip(started.parse::<f64>().ok())
                    .map(|(u, s)| u - s),
            };
            diff.and_then(|d| {
                let n = d / rules::MINUTE_MS as f64;
                if n >= i64::MIN as f64 && n < i64::MAX as f64 {
                    Some(n.round_ties_even() as i64)
                } else {
                    None
                }
            })
        };
        let Some(target) = mins.and_then(|m| m.checked_mul(rules::MINUTE_MS)) else {
            return Ok(false);
        };
        let expired = until
            .parse::<i128>()
            .map(|u| u <= now as i128)
            .unwrap_or_else(|_| until.starts_with('-'));
        if expired || !(rules::MIN_TARGET_MS..=rules::MAX_TARGET_MS).contains(&target) {
            return Ok(false);
        }
        let block_id = rules::text(&data["blockId"]);
        let block_id = if block_id.is_empty() {
            format!("legacy-focus-{started}")
        } else {
            block_id
        };
        let mode = data["mode"]
            .as_str()
            .filter(|s| matches!(*s, "focus" | "break"))
            .unwrap_or("focus");
        transaction(self.conn, false, || {
            let (_, block) = self.current()?;
            if live(&block)
                || self
                    .conn
                    .query_row(
                        "SELECT 1 FROM pomodoro_blocks WHERE block_id=?",
                        [&block_id],
                        |r| r.get::<_, i64>(0),
                    )
                    .optional()?
                    .is_some()
            {
                return Ok(false);
            }
            let until = until
                .parse::<i64>()
                .map_err(|_| validation("entero fuera de rango SQLite"))?;
            let started = started
                .parse::<i64>()
                .map_err(|_| validation("entero fuera de rango SQLite"))?;
            let resumed = checked(until as i128 - target as i128)?;
            self.conn.execute(&format!("INSERT INTO pomodoro_blocks ({BLOCK_COLUMNS},updated_at_ms) VALUES (?,?,'running',?,0,?,?,?,NULL,?,?,?,?,?,'legacy-focus-file',?)"),params![block_id,mode,target,resumed,until,started,rules::text(&data["project"]),rules::text(&data["session"]),rules::text(&data["pane"]),rules::optional_integer(&data["cycleIndex"],1,99),rules::optional_integer(&data["cycleTotal"],1,99),now])?;
            self.bump(Some(&block_id), false, false)?;
            Ok(true)
        })
    }
    pub fn import_legacy_history(&self, rows: &[Value], now: Option<i64>) -> Result<usize> {
        let now = now.unwrap_or_else(|| (self.clock)());
        transaction(self.conn, false, || {
            let mut added = 0;
            for row in rows {
                let id = rules::text(&row["id"]);
                if id.is_empty() {
                    continue;
                }
                if self
                    .conn
                    .query_row(
                        "SELECT 1 FROM pomodoro_blocks WHERE block_id=?",
                        [&id],
                        |r| r.get::<_, i64>(0),
                    )
                    .optional()?
                    .is_some()
                {
                    continue;
                }
                let planned = if codec::truthy(&row["planned_minutes"]) {
                    legacy_integer_input(&row["planned_minutes"])?
                } else {
                    Some("0".into())
                };
                let Some(planned) = planned else { continue };
                let Some(started) = legacy_integer_input(&row["started_at_ms"])? else {
                    continue;
                };
                let planned = if planned.starts_with('-') {
                    0
                } else {
                    let planned = planned
                        .parse::<i128>()
                        .map_err(|_| validation("entero fuera de rango SQLite"))?;
                    checked(
                        planned
                            .checked_mul(rules::MINUTE_MS as i128)
                            .ok_or_else(|| validation("entero fuera de rango SQLite"))?,
                    )?
                };
                let started = started
                    .parse::<i64>()
                    .map_err(|_| validation("entero fuera de rango SQLite"))?;
                let ended = if row["ended_at_ms"].is_number() || row["ended_at_ms"].is_boolean() {
                    Some(legacy_int(&row["ended_at_ms"]).map_err(validation)?)
                } else {
                    None
                };
                let mode = row["mode"]
                    .as_str()
                    .filter(|s| matches!(*s, "focus" | "break"))
                    .unwrap_or("focus");
                let status = row["status"]
                    .as_str()
                    .filter(|s| matches!(*s, "completed" | "cancelled" | "skipped"))
                    .unwrap_or("unknown");
                added+=self.conn.execute("INSERT INTO pomodoro_records (block_id,mode,status,target_ms,active_ms,planned_ms,started_at_ms,ended_at_ms,project,session_key,pane_key,provenance,recorded_at_ms) VALUES (?,?,?,?,NULL,?,?,?,?,?,?,'legacy-planned',?) ON CONFLICT(block_id) DO NOTHING",params![id,mode,status,planned,planned,started,ended,rules::text(&row["project"]),rules::text(&row["tmux_session"]),rules::text(&row["tmux_pane"]),now])?;
            }
            Ok(added)
        })
    }
}
fn legacy_integer_input(value: &Value) -> Result<Option<String>> {
    match comandos_core::focus::integer_string(value) {
        Ok(s) => Ok(Some(s)),
        Err(comandos_core::focus::IntegerError::Invalid) => Ok(None),
        Err(e) => Err(validation(e.to_string())),
    }
}
fn legacy_seconds(value: &Value) -> Result<Option<String>> {
    let seconds = match comandos_core::focus::float(value) {
        Ok(n) => n,
        Err(_) if value.is_number() => return Err(validation("fecha fuera de rango")),
        Err(_) => return Ok(None),
    };
    let ms = seconds * 1000.0;
    if ms.is_nan() {
        Ok(None)
    } else if !ms.is_finite() {
        Err(validation("fecha fuera de rango"))
    } else {
        Ok(Some(format!("{:.0}", ms.trunc())))
    }
}
pub fn records(
    conn: &Connection,
    from: Option<i64>,
    to: Option<i64>,
    project: Option<&str>,
) -> crate::Result<Vec<Value>> {
    let mut sql = format!("SELECT {RECORD_COLUMNS} FROM pomodoro_records WHERE 1=1");
    let mut values = vec![];
    if let Some(from) = from {
        sql.push_str(" AND started_at_ms >= ?");
        values.push(SqlValue::Integer(from));
    }
    if let Some(to) = to {
        sql.push_str(" AND started_at_ms < ?");
        values.push(SqlValue::Integer(to));
    }
    if let Some(project) = project {
        sql.push_str(" AND project = ?");
        values.push(SqlValue::Text(project.into()));
    }
    sql.push_str(" ORDER BY started_at_ms DESC,block_id");
    Ok(conn
        .prepare(&sql)?
        .query_map(rusqlite::params_from_iter(values), record_row)?
        .collect::<rusqlite::Result<_>>()?)
}
pub fn focus_report(
    conn: &Connection,
    from: i64,
    to: i64,
    project: Option<&str>,
    tz: &str,
) -> crate::Result<Value> {
    let rows = records(conn, Some(from), Some(to), project)?;
    let projects=conn.prepare("SELECT DISTINCT project FROM pomodoro_records WHERE started_at_ms>=? AND started_at_ms<? AND project!='' ORDER BY project")?.query_map(params![from,to],|r|r.get::<_,String>(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
    rules::focus_report(&rows, projects, from, to, project, tz).map_err(crate::Error::Validation)
}
