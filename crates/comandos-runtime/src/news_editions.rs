//! Edition scheduling, bounded fetching, summarization and atomic publication.
mod build;
mod fetchers;
mod summarize;
pub use build::*;
use chrono::{NaiveDate, Offset, TimeZone};
use comandos_store::news;
pub use comandos_store::news::default_policy;
pub use fetchers::*;
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use serde_json::{Map, Value, json};
use std::sync::Mutex;
pub use summarize::*;
pub type Policy = Map<String, Value>;
pub type Result<T> = std::result::Result<T, String>;
pub type Fetcher = dyn Fn(&Policy, usize) -> Result<Value> + Send + Sync;
pub type Summarizer = dyn Fn(&Value) -> std::result::Result<Value, SummaryError> + Send + Sync;
pub type LeadWriter = dyn Fn(&[Value]) -> Result<String> + Send + Sync;
pub type Notify = dyn Fn(&Value) -> Result<()> + Send + Sync;
pub(crate) fn s(v: &Value) -> &str {
    v.as_str().unwrap_or("")
}
pub(crate) fn n(p: &Policy, key: &str, default: i64) -> i64 {
    p.get(key).and_then(Value::as_i64).unwrap_or(default)
}
pub(crate) fn clip(s: &str, limit: usize) -> String {
    let s = s.trim();
    if s.chars().count() <= limit {
        s.into()
    } else {
        format!(
            "{}…",
            s.chars().take(limit.saturating_sub(1)).collect::<String>()
        )
    }
}
pub(crate) fn sql<T>(result: rusqlite::Result<T>) -> Result<T> {
    result.map_err(|e| e.to_string())
}
pub fn normalize_url(url: &str) -> Option<String> {
    crate::news_radar::network_normalize(url)
}
fn zone(policy: &Policy) -> Result<chrono_tz::Tz> {
    policy
        .get("timezone")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .unwrap_or("America/Mexico_City")
        .parse()
        .map_err(|_| "zona horaria no válida".into())
}
pub fn slot_times(day: &str, policy: &Policy) -> Result<Vec<(String, i64)>> {
    let day = NaiveDate::parse_from_str(day, "%Y-%m-%d").map_err(|e| e.to_string())?;
    let tz = zone(policy)?;
    let slots = policy
        .get("slots")
        .and_then(Value::as_array)
        .ok_or("sin horarios")?;
    slots
        .iter()
        .map(|v| {
            let slot = s(v);
            let time =
                chrono::NaiveTime::parse_from_str(slot, "%H:%M").map_err(|e| e.to_string())?;
            let local = day.and_time(time);
            // ZoneInfo's default fold=0: earlier instant for an overlap,
            // and the pre-transition offset for a nonexistent wall time.
            let at = tz
                .from_local_datetime(&local)
                .earliest()
                .map(|dt| dt.timestamp_millis())
                .or_else(|| {
                    chrono_tz::GapInfo::new(&local, &tz)
                        .and_then(|g| g.begin)
                        .map(|(_, offset)| {
                            local.and_utc().timestamp_millis()
                                - i64::from(offset.fix().local_minus_utc()) * 1000
                        })
                })
                .ok_or("hora local fuera del rango conocido")?;
            Ok((slot.into(), at))
        })
        .collect()
}
pub fn schedule_editions(
    conn: &Connection,
    day: &str,
    policy: &Policy,
    not_before: Option<i64>,
) -> Result<Vec<Value>> {
    let tx = sql(rusqlite::Transaction::new_unchecked(
        conn,
        TransactionBehavior::Immediate,
    ))?;
    comandos_store::migrate::move_db::admit_write(conn).map_err(|e| e.to_string())?;
    let zone = zone(policy)?.to_string();
    for (slot, at) in slot_times(day, policy)? {
        if not_before.is_some_and(|n| at < n) {
            continue;
        }
        let id = format!("{day}@{slot}");
        sql(tx.execute("INSERT OR IGNORE INTO news_editions (id,local_date,slot,timezone,scheduled_at_ms,status) VALUES (?,?,?,?,?,'scheduled')",params![id,day,slot,zone,at]))?;
        sql(tx.execute(
            "INSERT OR IGNORE INTO news_jobs (edition_id,state) VALUES (?,'queued')",
            [id],
        ))?;
    }
    sql(tx.commit())?;
    let mut stmt =
        sql(conn
            .prepare("SELECT id FROM news_editions WHERE local_date=? ORDER BY scheduled_at_ms"))?;
    let ids = sql(stmt.query_map([day], |r| r.get::<_, String>(0)))?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|e| e.to_string())?;
    ids.iter()
        .map(|id| {
            edition(conn, id).map(|mut e| {
                if let Some(o) = e.as_object_mut() {
                    o.remove("job");
                    o.remove("models");
                    o.remove("lead");
                }
                e
            })
        })
        .collect()
}
fn edition(conn: &Connection, id: &str) -> Result<Value> {
    Ok(news::get_edition(conn, id)
        .map_err(|e| e.to_string())?
        .map(|mut v| v["edition"].take())
        .unwrap_or(Value::Null))
}

pub fn reconcile(conn: &Connection, now: i64, p: &Policy) -> Result<()> {
    let tx = sql(rusqlite::Transaction::new_unchecked(
        conn,
        TransactionBehavior::Immediate,
    ))?;
    comandos_store::migrate::move_db::admit_write(conn).map_err(|e| e.to_string())?;
    for (query, state, error, note) in [
        (
            "SELECT edition_id FROM news_jobs WHERE state='running' AND lease_until_ms <= ?1",
            "failed",
            "interrumpida",
            "La generación se interrumpió; esta edición no se recupera.",
        ),
        (
            "SELECT j.edition_id FROM news_jobs j JOIN news_editions e ON e.id=j.edition_id WHERE j.state='queued' AND e.scheduled_at_ms + ?2 <= ?1",
            "skipped",
            "fuera de horario",
            "No se generó a su hora (servicio detenido); no se recupera.",
        ),
    ] {
        let mut stmt = sql(tx.prepare(query))?;
        let args: Vec<i64> = if query.contains("?2") {
            vec![now, n(p, "graceMinutes", 30) * 60000]
        } else {
            vec![now]
        };
        let ids = sql(stmt.query_map(rusqlite::params_from_iter(args), |r| r.get::<_, String>(0)))?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(|e| e.to_string())?;
        for id in ids {
            sql(tx.execute(
                "UPDATE news_jobs SET state=?,finished_at_ms=?,error=? WHERE edition_id=?",
                params![state, now, error, id],
            ))?;
            sql(tx.execute(
                "UPDATE news_editions SET status='not_published',notes=? WHERE id=?",
                params![json!([note]).to_string(), id],
            ))?;
        }
    }
    sql(tx.commit())
}
pub fn requeue_orphans(conn: &Connection, now: i64, p: &Policy) -> Result<()> {
    let tx = sql(rusqlite::Transaction::new_unchecked(
        conn,
        TransactionBehavior::Immediate,
    ))?;
    comandos_store::migrate::move_db::admit_write(conn).map_err(|e| e.to_string())?;
    let rows = {
        let mut stmt=sql(tx.prepare("SELECT j.edition_id,e.scheduled_at_ms FROM news_jobs j JOIN news_editions e ON e.id=j.edition_id WHERE j.state='running'"))?;
        sql(stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))))?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(|e| e.to_string())?
    };
    for (id, scheduled) in rows {
        if scheduled + n(p, "graceMinutes", 30) * 60000 > now {
            sql(tx.execute(
                "UPDATE news_jobs SET state='queued',lease_until_ms=NULL WHERE edition_id=?",
                [&id],
            ))?;
            sql(tx.execute(
                "UPDATE news_editions SET status='scheduled' WHERE id=?",
                [id],
            ))?;
        } else {
            sql(tx.execute("UPDATE news_jobs SET state='failed',finished_at_ms=?,error='interrumpida' WHERE edition_id=?",params![now,id]))?;
            sql(tx.execute("UPDATE news_editions SET status='not_published',notes=? WHERE id=?",params![json!(["El servicio se reinició a mitad de la generación y ya pasó su hora; este resumen no se recupera."]).to_string(),id]))?;
        }
    }
    sql(tx.commit())
}
pub fn claim_due_job(conn: &Connection, now: i64, p: &Policy) -> Result<Option<Value>> {
    let tx = sql(rusqlite::Transaction::new_unchecked(
        conn,
        TransactionBehavior::Immediate,
    ))?;
    comandos_store::migrate::move_db::admit_write(conn).map_err(|e| e.to_string())?;
    if sql(tx
        .query_row(
            "SELECT 1 FROM news_jobs WHERE state='running' AND lease_until_ms>? LIMIT 1",
            [now],
            |r| r.get::<_, i64>(0),
        )
        .optional())?
    .is_some()
    {
        return Ok(None);
    }
    let row=sql(tx.query_row("SELECT e.id,e.scheduled_at_ms FROM news_jobs j JOIN news_editions e ON e.id=j.edition_id WHERE j.state='queued' AND e.scheduled_at_ms<=? AND e.scheduled_at_ms+?>? ORDER BY e.scheduled_at_ms LIMIT 1",params![now,n(p,"graceMinutes",30)*60000,now],|r|Ok((r.get::<_,String>(0)?,r.get::<_,i64>(1)?))).optional())?;
    let Some((id, at)) = row else { return Ok(None) };
    let lease = now + n(p, "maxSeconds", 600) * 1000;
    sql(tx.execute("UPDATE news_jobs SET state='running',attempts=attempts+1,lease_until_ms=?,started_at_ms=? WHERE edition_id=?",params![lease,now,id]))?;
    sql(tx.execute(
        "UPDATE news_editions SET status='running' WHERE id=?",
        [&id],
    ))?;
    sql(tx.commit())?;
    Ok(Some(
        json!({"editionId":id,"scheduledAt":at,"claimedAt":now,"leaseUntil":lease}),
    ))
}
#[allow(clippy::too_many_arguments)]
pub fn run_due(
    conn: &Connection,
    now: i64,
    p: &Policy,
    fetch: &Fetcher,
    summarize: &Summarizer,
    clock: &dyn Fn() -> i64,
    notify: Option<&Notify>,
    lead: Option<&LeadWriter>,
) -> Result<Value> {
    let day = chrono::DateTime::from_timestamp_millis(now)
        .ok_or("fecha no válida")?
        .with_timezone(&zone(p)?)
        .date_naive();
    let first = sql(conn
        .query_row("SELECT 1 FROM news_editions LIMIT 1", [], |r| {
            r.get::<_, i64>(0)
        })
        .optional())?
    .is_none();
    let before = first.then(|| now - n(p, "graceMinutes", 30) * 60000);
    schedule_editions(conn, &day.to_string(), p, before)?;
    schedule_editions(
        conn,
        &day.succ_opt().ok_or("fecha fuera de rango")?.to_string(),
        p,
        before,
    )?;
    reconcile(conn, now, p)?;
    let Some(job) = claim_due_job(conn, now, p)? else {
        return Ok(json!({"built":null}));
    };
    let e = build_edition(conn, &job, fetch, summarize, p, clock, lead)?;
    if let Some(notify) = notify
        && matches!(
            s(&e["status"]),
            "published" | "partial" | "empty" | "failed"
        )
    {
        let _ = notify(&e);
    }
    Ok(json!({"built":job["editionId"],"edition":e}))
}
pub fn recent_story_urls(conn: &Connection, since: i64) -> Result<Vec<String>> {
    let mut stmt=sql(conn.prepare("SELECT DISTINCT src.url FROM news_sources src JOIN news_story_sources ss ON ss.source_id=src.id JOIN news_stories s ON s.id=ss.story_id JOIN news_editions e ON e.id=s.edition_id WHERE e.status IN ('published','partial') AND e.scheduled_at_ms>=?"))?;
    sql(stmt.query_map([since], |r| r.get(0)))?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|e| e.to_string())
}
pub fn notice_text(e: &Value) -> (String, String) {
    let slot = s(&e["slot"]);
    if matches!(s(&e["status"]), "published" | "partial") {
        let names: Vec<_> = e["models"]
            .as_object()
            .into_iter()
            .flat_map(|m| m.keys())
            .map(|m| {
                m.split_once(':')
                    .map(|(_, m)| m)
                    .unwrap_or(m)
                    .rsplit('/')
                    .next()
                    .unwrap_or("")
            })
            .collect();
        let who = if names.len() > 1 {
            format!(
                "{} y {}",
                names[..names.len() - 1].join(", "),
                names[names.len() - 1]
            )
        } else {
            names.join("")
        };
        (
            format!("Resumen de las {slot} listo"),
            format!(
                "{} noticias{}",
                e["storyCount"].as_i64().unwrap_or(0),
                if who.is_empty() {
                    String::new()
                } else {
                    format!(" · resumido con {who}")
                }
            ),
        )
    } else {
        (
            format!("El resumen de las {slot} no se generó"),
            e["notes"]
                .as_array()
                .and_then(|v| v.first())
                .map(s)
                .unwrap_or(if e["status"] == "empty" {
                    "Las fuentes no trajeron novedades."
                } else {
                    "Revisa los detalles del resumen."
                })
                .into(),
        )
    }
}
/// Lock spans the whole synchronous tick, including notification. Cancellation
/// of an HTTP waiter cannot abandon publication; the system thread owns it.
pub struct EditionScheduler {
    state: Mutex<SchedulerState>,
}
struct SchedulerState {
    booted: bool,
    status: Value,
}
impl Default for EditionScheduler {
    fn default() -> Self {
        Self {
            state: Mutex::new(SchedulerState {
                booted: false,
                status: json!({"configured":false,"reason":"sin configurar"}),
            }),
        }
    }
}
impl EditionScheduler {
    #[allow(clippy::too_many_arguments)]
    pub fn tick(
        &self,
        connect: &dyn Fn() -> Result<Connection>,
        config: Option<&Policy>,
        env: &dyn Fn(&str) -> bool,
        now: i64,
        fetch: &Fetcher,
        summarize: &Summarizer,
        notify: Option<&Notify>,
        lead: Option<&LeadWriter>,
    ) -> Result<Value> {
        let Ok(mut state) = self.state.try_lock() else {
            return Ok(json!({"built":null,"busy":true}));
        };
        state.status = news::config_status(config, env);
        if state.status["configured"] != true {
            let mut result = state.status.clone();
            result["built"] = Value::Null;
            return Ok(result);
        }
        let p = news::policy_from_config(config).map_err(|e| e.to_string())?;
        let conn = connect()?;
        if !state.booted {
            requeue_orphans(&conn, now, &p)?;
            state.booted = true
        }
        let start = std::time::Instant::now();
        run_due(
            &conn,
            now,
            &p,
            fetch,
            summarize,
            &|| now + start.elapsed().as_millis() as i64,
            notify,
            lead,
        )
    }
    pub fn status(&self) -> Value {
        self.state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .status
            .clone()
    }
}
/// news_reading.recover: interrupted conversations/translations never stay pending.
pub fn recover_reading(conn: &Connection, now: i64) -> Result<()> {
    let tx = sql(rusqlite::Transaction::new_unchecked(
        conn,
        TransactionBehavior::Immediate,
    ))?;
    comandos_store::migrate::move_db::admit_write(conn).map_err(|e| e.to_string())?;
    sql(tx.execute("UPDATE news_chat SET state='failed',text='Se interrumpió (el servicio se reinició). Vuelve a preguntar.' WHERE state='pending'",[]))?;
    sql(tx.execute("UPDATE news_translations SET state='failed',error='interrumpida',updated_at_ms=? WHERE state='running'",[now]))?;
    sql(tx.commit())
}
