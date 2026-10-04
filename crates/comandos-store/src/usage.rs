//! Contabilidad de uso de los hooks: las ramas `capture-hook`, `lifecycle` y
//! `tool-event` de `bin/cc_usage.py`. Cada función reproduce las sentencias SQL
//! de su rama Python (mismas tablas, claves de deduplicación y cláusulas de
//! conflicto); la paridad la verifica `tests/usage_parity.rs` contra el oráculo.
//! Nunca se guardan textos de prompts, respuestas ni argumentos de herramientas.
use crate::{Error, Result, with_transaction};
use rusqlite::{
    Connection, OptionalExtension, ToSql, params,
    types::{ToSqlOutput, ValueRef},
};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::HashSet,
    path::Path,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

const DB_FILENAME: &str = "comandos-usage.sqlite";
const SCHEMA_VERSION: i64 = 11;
/// Una configuración registrada poco después del primer turno es la de esa sesión.
const CONFIG_RACE_WINDOW: i64 = 900;
const OPEN_INTERACTION_SQL: &str = "select id,config_id from usage_interactions
  where tmux_session=? and tmux_pane=? and finished_at_ms is null
  order by started_at_ms desc limit 1";

/// Abre `<home>/.claude/hooks/comandos-usage.sqlite` con los PRAGMAs de
/// `connect()` en Python. La anulación por `COMANDOS_USAGE_DB` la resuelve quien
/// llama (la CLI): esta función no lee el entorno.
pub fn open_usage_db(home: &Path) -> Result<Connection> {
    let path = home.join(".claude").join("hooks").join(DB_FILENAME);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(Error::Io)?;
    }
    let conn = Connection::open(&path)?;
    conn.busy_timeout(Duration::from_secs(10))?;
    conn.execute_batch("pragma foreign_keys=on")?;
    // Python ignora el fallo de WAL (sistemas de archivos que no lo admiten).
    let _ = conn.query_row("pragma journal_mode=wal", [], |_| Ok(()));
    Ok(conn)
}

/// Turno con tokens medidos por el hook; sin ningún número no toca la base.
pub fn capture_hook(conn: &Connection, payload: &Value) -> Result<()> {
    let p = object(payload)?;
    let input = as_int(p.get("input_tokens"), 0);
    let output = as_int(p.get("output_tokens"), 0);
    let cache_read = as_int(p.get("cache_read_tokens"), 0);
    let cache_write = as_int(p.get("cache_write_tokens"), 0);
    let cost = as_float(p.get("cost_usd"));
    let total = match as_int(p.get("total_tokens"), 0) {
        0 => input.saturating_add(output),
        n => n,
    };
    if [input, output, cache_read, cache_write, total] == [0; 5] && cost == 0.0 {
        return Ok(());
    }
    let now = unix_secs();
    let text = |s: &str| Cell::Text(s.into());
    let agent = first(p, &["agent"])?.unwrap_or_else(|| text(""));
    let provider = match first(p, &["provider"])? {
        Some(c) => c,
        None => provider_for_agent(p.get("agent"))?,
    };
    let session = first(p, &["tmux_session", "session"])?.unwrap_or_else(|| text(""));
    let pane = first(p, &["tmux_pane", "pane"])?.unwrap_or_else(|| text(""));
    let pane_pwd = first(p, &["pane_pwd", "cwd"])?.unwrap_or_else(|| text(""));
    let git_root = first(p, &["git_root", "pane_pwd", "cwd"])?.unwrap_or_else(|| text(""));
    let model = first(p, &["model"])?.unwrap_or_else(|| text(""));
    let effort = first(p, &["reasoning_effort"])?.unwrap_or_else(|| text(""));
    let started = as_int(p.get("turn_started_at"), now);
    let finished = as_int(p.get("turn_finished_at"), now);
    let source = first(p, &["source"])?.unwrap_or_else(|| text("hook"));
    let confidence = first(p, &["confidence"])?.unwrap_or_else(|| text("exact"));
    init_db(conn)?;
    with_transaction(conn, || {
        let mut interaction_id = text("");
        let mut config = None;
        let open = conn
            .query_row(OPEN_INTERACTION_SQL, params![session, pane], |r| {
                Ok((cell(r.get_ref(0)?), cell(r.get_ref(1)?)))
            })
            .optional()?;
        if let Some((id, config_id)) = open {
            interaction_id = id;
            config = conn
                .query_row(
                    "select harness,motor,route_id,harness_account,motor_account from usage_session_configs where id=?",
                    [config_id],
                    |r| (0..5).map(|i| r.get_ref(i).map(cell)).collect::<rusqlite::Result<Vec<_>>>(),
                )
                .optional()?;
        }
        let [harness, motor, route_id, harness_account, motor_account] = match config {
            Some(row) => <[Cell; 5]>::try_from(row).expect("cinco columnas"),
            None => {
                let harness = agent.clone();
                let model_lower = model.py_text().to_lowercase();
                let motor = if model_lower.starts_with("grok-") {
                    text("grok")
                } else if model_lower.starts_with("gpt-") || model_lower.starts_with("codex") {
                    text("codex")
                } else if harness == text("codex") || harness == text("grok") {
                    harness.clone()
                } else {
                    text("claude")
                };
                let route = Cell::Text(format!("{}:{}", harness.py_text(), motor.py_text()));
                [harness, motor, route, text("unknown"), text("unknown")]
            }
        };
        let id = stable_id(&[
            provider.py_text(),
            session.py_text(),
            pane.py_text(),
            started.to_string(),
            finished.to_string(),
            model.py_text(),
            input.to_string(),
            output.to_string(),
            py_float_repr(cost),
        ]);
        let duration = finished.saturating_sub(started).saturating_mul(1000).max(0);
        let values: [(&str, &dyn ToSql); 31] = [
            (":id", &id),
            (":provider", &provider),
            (":agent", &agent),
            (":tmux_session", &session),
            (":tmux_pane", &pane),
            (":pane_pwd", &pane_pwd),
            (":git_root", &git_root),
            (":model", &model),
            (":reasoning_effort", &effort),
            (":turn_started_at", &started),
            (":turn_finished_at", &finished),
            (":input_tokens", &input),
            (":output_tokens", &output),
            (":cache_read_tokens", &cache_read),
            (":cache_write_tokens", &cache_write),
            (":total_tokens", &total),
            (":cost_usd", &cost),
            (":source", &source),
            (":confidence", &confidence),
            (":raw", &"{}"),
            (":harness", &harness),
            (":motor", &motor),
            (":route_id", &route_id),
            (":harness_account", &harness_account),
            (":motor_account", &motor_account),
            (":interaction_id", &interaction_id),
            (":experiment_run_id", &""),
            (":tool_profile", &""),
            (":duration_ms", &duration),
            (":outcome", &"unknown"),
            (":reasoning_tokens", &None::<i64>),
        ];
        conn.execute(TURN_INSERT_SQL, &values[..])?;
        Ok(())
    })
}

/// Fronteras de prompt y asentamiento (sin texto): abre, cuenta esperas o cierra
/// la interacción abierta del pane.
pub fn lifecycle(conn: &Connection, payload: &Value) -> Result<()> {
    let p = object(payload)?;
    init_db(conn)?;
    let status = py_text(p.get("status"));
    let (session, pane) = (py_text(p.get("tmux_session")), py_text(p.get("tmux_pane")));
    let at_ms = event_ms(p);
    let prompt_id = py_text(p.get("prompt_id"));
    if session.is_empty() || pane.is_empty() {
        return Ok(());
    }
    with_transaction(conn, || {
        if status == "working" {
            let key = if prompt_id.is_empty() {
                at_ms.to_string()
            } else {
                prompt_id.clone()
            };
            let ident = stable_id(&["interaction".into(), session.clone(), pane.clone(), key]);
            let config_id = latest_config_id(conn, &session, &pane, at_ms.div_euclid(1000))?;
            let root = match py_text(p.get("git_root")) {
                r if r.is_empty() => pane_git_root(conn, &session, &pane)?,
                r => r,
            };
            conn.execute(
                "insert into usage_interactions
              (id,tmux_session,tmux_pane,task_id,config_id,prompt_id,agent_session_id,started_at_ms,completion_status,source,confidence,created_at,git_root)
              values(?,?,?,?,?,?,?,?,?,?,?,?,?) on conflict(id) do nothing",
                params![
                    ident,
                    session,
                    pane,
                    py_text(p.get("task_id")),
                    config_id,
                    prompt_id,
                    py_text(p.get("agent_session_id")),
                    at_ms,
                    "unknown",
                    or_default(py_text(p.get("source")), "hook"),
                    or_default(py_text(p.get("confidence")), "exact"),
                    at_ms.div_euclid(1000),
                    root
                ],
            )?;
            return Ok(());
        }
        // Un permiso o aviso de atención no cierra la respuesta, pero cada uno es
        // una interrupción del humano y se cuenta.
        if status == "waiting" {
            conn.execute(
                "update usage_interactions set waits=waits+1 where id=(
                 select id from usage_interactions where tmux_session=? and tmux_pane=? and finished_at_ms is null
                 order by started_at_ms desc limit 1)",
                params![session, pane],
            )?;
            return Ok(());
        }
        let open = |by_prompt: bool| {
            let sql = if by_prompt {
                "select id,started_at_ms from usage_interactions
                 where tmux_session=? and tmux_pane=? and prompt_id=? and finished_at_ms is null
                 order by started_at_ms desc limit 1"
            } else {
                "select id,started_at_ms from usage_interactions where tmux_session=? and tmux_pane=? and finished_at_ms is null
                 order by started_at_ms desc limit 1"
            };
            let mut args: Vec<&dyn ToSql> = vec![&session, &pane];
            if by_prompt {
                args.push(&prompt_id);
            }
            conn.query_row(sql, &args[..], |r| {
                Ok((cell(r.get_ref(0)?), r.get::<_, Option<i64>>(1)?))
            })
            .optional()
        };
        let mut row = None;
        if !prompt_id.is_empty() {
            row = open(true)?;
        }
        if row.is_none() {
            row = open(false)?;
        }
        let Some((id, started)) = row else {
            return Ok(());
        };
        let completion = match status.as_str() {
            "done" => "completed",
            "error" => "failed",
            "idle" | "cancelled" | "end" => "cancelled",
            _ => "unknown",
        };
        let started = started.filter(|s| *s != 0).unwrap_or(at_ms);
        let duration = at_ms.saturating_sub(started).max(0);
        conn.execute(
            "update usage_interactions set finished_at_ms=?,duration_ms=?,completion_status=?,error_class=? where id=?",
            params![at_ms, duration, completion, py_text(p.get("error_class")), id],
        )?;
        Ok(())
    })
}

/// Nombre, tiempos y estado de una herramienta; nunca argumentos ni resultados.
pub fn tool_event(conn: &Connection, payload: &Value) -> Result<()> {
    let p = object(payload)?;
    init_db(conn)?;
    let phase = py_text(p.get("phase"));
    let (session, pane) = (py_text(p.get("tmux_session")), py_text(p.get("tmux_pane")));
    let tool_name: String = py_text(p.get("tool_name")).chars().take(120).collect();
    let at_ms = event_ms(p);
    if !matches!(phase.as_str(), "start" | "success" | "failed")
        || session.is_empty()
        || pane.is_empty()
        || tool_name.is_empty()
    {
        return Ok(());
    }
    let confidence = or_default(py_text(p.get("confidence")), "exact");
    let error_class = if phase == "failed" { "tool_error" } else { "" };
    with_transaction(conn, || {
        let Some(interaction_id) = conn
            .query_row(OPEN_INTERACTION_SQL, params![session, pane], |r| {
                r.get_ref(0).map(cell)
            })
            .optional()?
        else {
            return Ok(());
        };
        let iid = interaction_id.py_text();
        let external_id = py_text(p.get("tool_use_id"));
        let family = tool_family(&tool_name);
        let sequence = || -> rusqlite::Result<i64> {
            conn.query_row(
                "select count(*) from usage_tool_calls where interaction_id=?",
                [&interaction_id],
                |r| r.get(0),
            )
        };
        let tool_id =
            |key: String| stable_id(&["tool".into(), iid.clone(), key, tool_name.clone()]);
        let ident = if phase == "start" {
            let sequence = sequence()?;
            let key = if external_id.is_empty() {
                sequence.to_string()
            } else {
                external_id.clone()
            };
            let ident = tool_id(key);
            conn.execute(
                "insert into usage_tool_calls
              (id,interaction_id,sequence,tool_name,tool_family,started_at_ms,status,error_class,confidence)
              values(?,?,?,?,?,?,?,'',?) on conflict(id) do nothing",
                params![ident, interaction_id, sequence, tool_name, family, at_ms, "running", confidence],
            )?;
            ident
        } else {
            let (ident, row) = if !external_id.is_empty() {
                let ident = tool_id(external_id.clone());
                let row = conn
                    .query_row(
                        "select started_at_ms from usage_tool_calls where id=?",
                        [&ident],
                        |r| r.get::<_, Option<i64>>(0),
                    )
                    .optional()?;
                (ident, row)
            } else {
                let existing = conn
                    .query_row(
                        "select id,started_at_ms from usage_tool_calls
                  where interaction_id=? and tool_name=? and status='running'
                  order by sequence desc limit 1",
                        params![interaction_id, tool_name],
                        |r| Ok((r.get::<_, String>(0)?, r.get::<_, Option<i64>>(1)?)),
                    )
                    .optional()?;
                match existing {
                    Some((id, started)) => (id, Some(started)),
                    None => (tool_id(at_ms.to_string()), None),
                }
            };
            match row {
                None => {
                    let sequence = sequence()?;
                    conn.execute(
                        "insert into usage_tool_calls
                  (id,interaction_id,sequence,tool_name,tool_family,started_at_ms,finished_at_ms,duration_ms,status,error_class,confidence)
                  values(?,?,?,?,?,?,?,?,?,?,?)",
                        params![
                            ident,
                            interaction_id,
                            sequence,
                            tool_name,
                            family,
                            None::<i64>,
                            at_ms,
                            None::<i64>,
                            phase,
                            error_class,
                            confidence
                        ],
                    )?;
                }
                Some(started) => {
                    let duration = started.map(|s| at_ms.saturating_sub(s).max(0));
                    conn.execute(
                        "update usage_tool_calls set finished_at_ms=?,duration_ms=?,status=?,error_class=? where id=?",
                        params![at_ms, duration, phase, error_class, ident],
                    )?;
                }
            }
            ident
        };
        // Solo el selector explícito de Skill; nunca sus argumentos ni su resultado.
        if tool_name.to_lowercase() == "skill"
            && let Some(Value::String(skill)) = p.get("skill_name")
            && valid_skill(skill)
        {
            conn.execute(
                "update usage_tool_calls set skill_name=? where id=?",
                params![skill, ident],
            )?;
        }
        Ok(())
    })
}

/// `init_db` de Python: tablas base y migración aditiva hasta la versión 11
/// dentro de una transacción inmediata (un solo escritor entre hooks).
fn init_db(conn: &Connection) -> Result<()> {
    conn.execute_batch(BASE_SCHEMA)?;
    if user_version(conn)? >= SCHEMA_VERSION {
        return Ok(());
    }
    with_transaction(conn, || {
        let current = user_version(conn)?;
        if current < SCHEMA_VERSION {
            migrate_schema(conn, current)?;
        }
        Ok(())
    })
}

fn user_version(conn: &Connection) -> Result<i64> {
    Ok(conn.query_row("pragma user_version", [], |r| r.get(0))?)
}

fn migrate_schema(conn: &Connection, current: i64) -> Result<()> {
    ensure_columns(
        conn,
        "usage_turns",
        &[
            ("harness", "text not null default ''"),
            ("motor", "text not null default ''"),
            ("route_id", "text not null default ''"),
            ("harness_account", "text not null default 'unknown'"),
            ("motor_account", "text not null default 'unknown'"),
            ("interaction_id", "text not null default ''"),
            ("experiment_run_id", "text not null default ''"),
            ("tool_profile", "text not null default ''"),
            ("duration_ms", "integer"),
            ("outcome", "text not null default 'unknown'"),
            ("reasoning_tokens", "integer"),
        ],
    )?;
    let tables: HashSet<String> = conn
        .prepare("select name from sqlite_master where type='table'")?
        .query_map([], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    if tables.contains("usage_interactions") {
        ensure_columns(
            conn,
            "usage_interactions",
            &[
                ("prompt_id", "text not null default ''"),
                ("agent_session_id", "text not null default ''"),
                ("waits", "integer not null default 0"),
                ("git_root", "text not null default ''"),
            ],
        )?;
    }
    if tables.contains("usage_experiments") {
        ensure_columns(
            conn,
            "usage_experiments",
            &[
                ("project_id", "text not null default ''"),
                ("primary_metric", "text not null default 'outcome'"),
                ("min_pairs", "integer not null default 10"),
            ],
        )?;
    }
    if tables.contains("usage_experiment_runs") {
        ensure_columns(
            conn,
            "usage_experiment_runs",
            &[
                ("task_id", "text not null default ''"),
                ("project_id", "text not null default ''"),
                ("launch_order", "integer not null default 0"),
            ],
        )?;
    }
    conn.execute_batch(MIGRATION_TABLES)?;
    conn.execute_batch(
        "update usage_turns set harness=case when harness='' then agent else harness end;
    update usage_turns set motor=case
      when motor!='' then motor
      when lower(model) like 'grok-%' then 'grok'
      when lower(model) like 'gpt-%' or lower(model) like 'codex%' then 'codex'
      when harness in ('codex','grok') then harness
      else 'claude' end;
    update usage_turns set route_id=harness||':'||motor where route_id='';",
    )?;
    ensure_columns(
        conn,
        "usage_tool_calls",
        &[("skill_name", "text not null default ''")],
    )?;
    conn.execute_batch(
        "create index if not exists idx_usage_tools_interaction on usage_tool_calls(interaction_id);
    create index if not exists idx_usage_tools_time on usage_tool_calls(coalesce(finished_at_ms,started_at_ms));",
    )?;
    conn.execute_batch(SPAN_TABLES)?;
    if current != 0 && current < 11 {
        // v11 cuenta cada respuesta una sola vez: el próximo import rehace estas filas.
        conn.execute(
            "delete from usage_turns where source in ('claude_jsonl', 'codex_state_db')",
            [],
        )?;
    }
    conn.execute_batch(&format!("pragma user_version={SCHEMA_VERSION}"))?;
    Ok(())
}

fn ensure_columns(conn: &Connection, table: &str, columns: &[(&str, &str)]) -> Result<()> {
    let present: HashSet<String> = conn
        .prepare(&format!("pragma table_info({table})"))?
        .query_map([], |r| r.get(1))?
        .collect::<rusqlite::Result<_>>()?;
    for (name, ddl) in columns {
        if present.contains(*name) {
            continue;
        }
        match conn.execute(&format!("alter table {table} add column {name} {ddl}"), []) {
            Ok(_) => {}
            // Otro proceso la agregó entre el pragma y el ALTER: ya está.
            Err(e) if e.to_string().to_lowercase().contains("duplicate column") => {}
            Err(e) => return Err(e.into()),
        }
    }
    Ok(())
}

/// `_latest_config`: la vigente al instante, o la que aparece poco después
/// (el hook del primer turno puede llegar antes que la configuración del pane).
fn latest_config_id(conn: &Connection, session: &str, pane: &str, at: i64) -> Result<Cell> {
    let pick = |order: &str, limit: i64| {
        conn.query_row(
            &format!(
                "select id from usage_session_configs where tmux_session=? and tmux_pane=? and effective_at<=?
                         order by effective_at {order} limit 1"
            ),
            params![session, pane, limit],
            |r| r.get_ref(0).map(cell),
        )
        .optional()
    };
    let found = match pick("desc", at)? {
        Some(id) => Some(id),
        None => pick("asc", at.saturating_add(CONFIG_RACE_WINDOW))?,
    };
    Ok(found.unwrap_or_else(|| Cell::Text(String::new())))
}

fn pane_git_root(conn: &Connection, session: &str, pane: &str) -> Result<String> {
    let row = conn
        .query_row(
            "select git_root, pane_pwd from usage_panes where tmux_session=? and tmux_pane=?
                         order by last_seen_at desc limit 1",
            params![session, pane],
            |r| Ok((cell(r.get_ref(0)?).py_text(), cell(r.get_ref(1)?).py_text())),
        )
        .optional()?;
    Ok(match row {
        Some((root, _)) if !root.is_empty() => root,
        Some((_, pwd)) => pwd,
        None => String::new(),
    })
}

fn tool_family(name: &str) -> &'static str {
    let value = name.to_lowercase();
    let any = |parts: &[&str]| parts.iter().any(|p| value.contains(p));
    if any(&["bash", "shell", "terminal", "computer"]) {
        "execution"
    } else if any(&["read", "grep", "glob", "search", "fetch"]) {
        "retrieval"
    } else if any(&["write", "edit", "notebook"]) {
        "mutation"
    } else if any(&["agent", "task", "workflow"]) {
        "orchestration"
    } else {
        "other"
    }
}

/// `[A-Za-z0-9_][A-Za-z0-9_.:@/-]{0,159}` completo.
fn valid_skill(s: &str) -> bool {
    let mut chars = s.chars();
    let Some(head) = chars.next() else {
        return false;
    };
    (head.is_ascii_alphanumeric() || head == '_')
        && s.chars().count() <= 160
        && chars.all(|c| c.is_ascii_alphanumeric() || "_.:@/-".contains(c))
}

fn provider_for_agent(agent: Option<&Value>) -> Result<Cell> {
    Ok(match agent {
        Some(Value::String(s)) if s == "codex" || s == "claude" => Cell::Text(s.clone()),
        Some(v) if truthy(v) => Cell::from_json(v)?,
        _ => Cell::Text("unknown".into()),
    })
}

fn object(payload: &Value) -> Result<&Map<String, Value>> {
    payload
        .as_object()
        .ok_or_else(|| Error::Validation("el evento de uso debe ser un objeto JSON".into()))
}

/// `a or b or ...` de Python sobre claves del payload.
fn first(p: &Map<String, Value>, keys: &[&str]) -> Result<Option<Cell>> {
    for key in keys {
        if let Some(v) = p.get(*key).filter(|v| truthy(v)) {
            return Cell::from_json(v).map(Some);
        }
    }
    Ok(None)
}

fn or_default(value: String, fallback: &str) -> String {
    if value.is_empty() {
        fallback.into()
    } else {
        value
    }
}

/// `_as_int(data.get("at_ms") or now_ms)`.
fn event_ms(p: &Map<String, Value>) -> i64 {
    match p.get("at_ms").filter(|v| truthy(v)) {
        Some(v) => as_int(Some(v), 0),
        None => i64::try_from(now().as_millis()).unwrap_or(i64::MAX),
    }
}

fn now() -> Duration {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
}

fn unix_secs() -> i64 {
    i64::try_from(now().as_secs()).unwrap_or(i64::MAX)
}

fn stable_id(parts: &[String]) -> String {
    let digest = Sha256::digest(parts.join("\x1f").as_bytes());
    digest.iter().take(16).map(|b| format!("{b:02x}")).collect()
}

/// Número JSON entero según Python (`json` lo trata como float si lleva `.`, `e`).
fn is_float_text(n: &serde_json::Number) -> bool {
    n.to_string().contains(['.', 'e', 'E'])
}

fn truthy(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().is_some_and(|x| x != 0.0),
        Value::String(s) => !s.is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => !o.is_empty(),
    }
}

/// `_as_int` de Python: entero o el valor por omisión.
fn as_int(v: Option<&Value>, default: i64) -> i64 {
    match v {
        Some(Value::Bool(b)) => i64::from(*b),
        Some(Value::Number(n)) if is_float_text(n) => match n.as_f64() {
            Some(x) if x.is_finite() => x.trunc() as i64,
            _ => default,
        },
        Some(Value::Number(n)) => n.as_i64().unwrap_or(default),
        Some(Value::String(s)) => s.trim().parse().unwrap_or(default),
        _ => default,
    }
}

/// `_as_float` de Python: real o 0.0.
fn as_float(v: Option<&Value>) -> f64 {
    match v {
        Some(Value::Bool(b)) => f64::from(u8::from(*b)),
        Some(Value::Number(n)) => n.as_f64().unwrap_or(0.0),
        Some(Value::String(s)) => s.trim().parse().unwrap_or(0.0),
        _ => 0.0,
    }
}

/// `_text` de Python: `str(value)`, con `None` como cadena vacía.
fn py_text(v: Option<&Value>) -> String {
    match v {
        None | Some(Value::Null) => String::new(),
        Some(Value::Array(_) | Value::Object(_)) => v.map(Value::to_string).unwrap_or_default(),
        Some(v) => Cell::from_json(v)
            .map(|c| c.py_text())
            .unwrap_or_else(|_| v.to_string()),
    }
}

/// `repr(float)` de Python: dígitos mínimos de ida y vuelta, notación fija para
/// exponentes de -4 a 15 y científica fuera de ese rango.
fn py_float_repr(x: f64) -> String {
    if x.is_nan() {
        return "nan".into();
    }
    if x.is_infinite() {
        return if x > 0.0 { "inf" } else { "-inf" }.into();
    }
    let sci = format!("{x:e}");
    let (mantissa, exp) = sci.split_once('e').expect("formato científico");
    let exp: i32 = exp.parse().expect("exponente");
    let (sign, mantissa) = match mantissa.strip_prefix('-') {
        Some(m) => ("-", m),
        None => ("", mantissa),
    };
    let digits: String = mantissa.chars().filter(|c| *c != '.').collect();
    let body = if (-4..16).contains(&exp) {
        if exp >= 0 {
            let int_len = exp as usize + 1;
            if digits.len() <= int_len {
                format!("{digits}{}.0", "0".repeat(int_len - digits.len()))
            } else {
                format!("{}.{}", &digits[..int_len], &digits[int_len..])
            }
        } else {
            format!("0.{}{digits}", "0".repeat((-exp - 1) as usize))
        }
    } else {
        let lead = if digits.len() > 1 {
            format!("{}.{}", &digits[..1], &digits[1..])
        } else {
            digits
        };
        format!("{lead}e{}{:02}", if exp < 0 { '-' } else { '+' }, exp.abs())
    };
    format!("{sign}{body}")
}

/// Valor escalar tal como lo vería Python: conserva el tipo para enlazarlo en
/// SQLite y para reproducir `str()` en las claves de deduplicación.
#[derive(Clone, Debug, PartialEq)]
enum Cell {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Text(String),
}

impl Cell {
    fn from_json(v: &Value) -> Result<Self> {
        Ok(match v {
            Value::Null => Self::Null,
            Value::Bool(b) => Self::Bool(*b),
            Value::String(s) => Self::Text(s.clone()),
            Value::Number(n) if is_float_text(n) => Self::Float(n.as_f64().unwrap_or(f64::NAN)),
            Value::Number(n) => Self::Int(
                n.as_i64()
                    .ok_or_else(|| Error::Validation("entero fuera de rango SQLite".into()))?,
            ),
            Value::Array(_) | Value::Object(_) => {
                return Err(Error::Validation("valor de uso no escalar".into()));
            }
        })
    }

    fn py_text(&self) -> String {
        match self {
            Self::Null => String::new(),
            Self::Bool(true) => "True".into(),
            Self::Bool(false) => "False".into(),
            Self::Int(n) => n.to_string(),
            Self::Float(x) => py_float_repr(*x),
            Self::Text(s) => s.clone(),
        }
    }
}

fn cell(v: ValueRef<'_>) -> Cell {
    match v {
        ValueRef::Null => Cell::Null,
        ValueRef::Integer(n) => Cell::Int(n),
        ValueRef::Real(x) => Cell::Float(x),
        ValueRef::Text(t) | ValueRef::Blob(t) => {
            Cell::Text(String::from_utf8_lossy(t).into_owned())
        }
    }
}

impl ToSql for Cell {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        Ok(match self {
            Self::Null => ToSqlOutput::Borrowed(ValueRef::Null),
            Self::Bool(b) => ToSqlOutput::Borrowed(ValueRef::Integer(i64::from(*b))),
            Self::Int(n) => ToSqlOutput::Borrowed(ValueRef::Integer(*n)),
            Self::Float(x) => ToSqlOutput::Borrowed(ValueRef::Real(*x)),
            Self::Text(s) => ToSqlOutput::Borrowed(ValueRef::Text(s.as_bytes())),
        })
    }
}

// Esquema copiado literalmente de `init_db` y `_migrate_schema` en `bin/cc_usage.py`:
// sqlite_master guarda el texto exacto de cada CREATE, y el volcado lo compara.
const BASE_SCHEMA: &str = r#"
        create table if not exists usage_panes (
          tmux_session text not null,
          tmux_pane text not null,
          pane_pwd text not null,
          git_root text not null,
          tab_label text not null default '',
          agent text not null,
          provider text not null,
          agent_pid integer not null default 0,
          model text not null default '',
          reasoning_effort text not null default '',
          started_at integer not null,
          last_seen_at integer not null,
          raw text not null default '{}',
          primary key (tmux_session, tmux_pane)
        );

        create table if not exists usage_turns (
          id text primary key,
          provider text not null,
          agent text not null,
          tmux_session text not null,
          tmux_pane text not null,
          pane_pwd text not null,
          git_root text not null,
          model text not null default '',
          reasoning_effort text not null default '',
          turn_started_at integer not null,
          turn_finished_at integer not null,
          input_tokens integer not null default 0,
          output_tokens integer not null default 0,
          cache_read_tokens integer not null default 0,
          cache_write_tokens integer not null default 0,
          total_tokens integer not null default 0,
          cost_usd real not null default 0,
          source text not null,
          confidence text not null,
          raw text not null default '{}'
        );

        create table if not exists provider_usage_buckets (
          id text primary key,
          provider text not null,
          start_time integer not null,
          end_time integer not null,
          input_tokens integer not null default 0,
          output_tokens integer not null default 0,
          cache_read_tokens integer not null default 0,
          cache_write_tokens integer not null default 0,
          total_tokens integer not null default 0,
          request_count integer not null default 0,
          project_id text not null default '',
          workspace_id text not null default '',
          user_id text not null default '',
          api_key_id text not null default '',
          model text not null default '',
          service_tier text not null default '',
          confidence text not null default 'exact',
          raw text not null default '{}'
        );

        create table if not exists provider_cost_buckets (
          id text primary key,
          provider text not null,
          start_time integer not null,
          end_time integer not null,
          cost_usd real not null default 0,
          currency text not null default 'usd',
          project_id text not null default '',
          workspace_id text not null default '',
          api_key_id text not null default '',
          line_item text not null default '',
          model text not null default '',
          confidence text not null default 'exact',
          raw text not null default '{}'
        );

        create table if not exists usage_reconciliation (
          id integer primary key autoincrement,
          provider_bucket_id text not null,
          usage_turn_id text not null,
          confidence text not null,
          created_at integer not null
        );

        create table if not exists usage_alerts (
          id text primary key,
          kind text not null,
          level text not null,
          message text not null,
          provider text not null default '',
          tmux_session text not null default '',
          tmux_pane text not null default '',
          created_at integer not null,
          last_seen_at integer not null,
          raw text not null default '{}'
        );

        create table if not exists model_presets (
          id text primary key,
          label text not null,
          description text not null,
          provider text not null default '',
          model text not null default '',
          reasoning_effort text not null default '',
          raw text not null default '{}'
        );

        create table if not exists usage_settings (
          key text primary key,
          value text not null
        );

        create table if not exists usage_alert_rules (
          id text primary key,
          scope text not null,
          target text not null,
          label text not null default '',
          threshold integer not null,
          created_at integer not null
        );
"#;
const MIGRATION_TABLES: &str = r#"
    create table if not exists usage_session_configs (
      id text primary key, tmux_session text not null, tmux_pane text not null,
      effective_at integer not null, harness text not null, motor text not null,
      model text not null, effort text not null default '',
      harness_account text not null default 'unknown', motor_account text not null default 'unknown',
      route_id text not null, source text not null, confidence text not null
    );
    create table if not exists usage_tasks (
      id text primary key, task_type text not null default 'unclassified',
      type_source text not null default 'manual', type_confidence text not null default 'exact',
      label text not null default '', design text not null default 'observational',
      created_at integer not null, updated_at integer not null
    );
    create table if not exists usage_interactions (
      id text primary key, tmux_session text not null, tmux_pane text not null,
      task_id text not null default '', config_id text not null default '',
      prompt_id text not null default '', agent_session_id text not null default '',
      started_at_ms integer, first_output_at_ms integer, finished_at_ms integer,
      duration_ms integer, completion_status text not null default 'unknown',
      error_class text not null default '', source text not null, confidence text not null,
      created_at integer not null,
      waits integer not null default 0, git_root text not null default ''
    );
    -- Perfil de negocio por proyecto: lo que ningun algoritmo puede deducir del
    -- codigo. Lo declara el usuario y el reparto lo usa como prioridad.
    create table if not exists usage_project_profiles (
      git_root text primary key,
      value text not null default 'medio',
      complexity text not null default 'media',
      autonomy text not null default 'supervisada',
      updated_at integer not null
    );
    create table if not exists usage_tool_calls (
      id text primary key, interaction_id text not null, sequence integer not null,
      tool_name text not null, tool_family text not null default '',
      started_at_ms integer, finished_at_ms integer, duration_ms integer,
      status text not null default 'unknown', error_class text not null default '',
      confidence text not null, foreign key(interaction_id) references usage_interactions(id) on delete cascade
    );
    create table if not exists usage_ratings (
      interaction_id text primary key, rated_at integer not null,
      outcome text not null default 'unknown', rating integer, note text not null default '',
      foreign key(interaction_id) references usage_interactions(id) on delete cascade
    );
    create table if not exists usage_experiments (
      id text primary key, label text not null, task_type text not null,
      status text not null, design text not null default 'paired',
      project_id text not null default '', primary_metric text not null default 'outcome',
      min_pairs integer not null default 10,
      created_at integer not null, updated_at integer not null
    );
    create table if not exists usage_experiment_variants (
      experiment_id text not null, variant_index integer not null,
      label text not null default '', harness text not null, motor text not null,
      model text not null, effort text not null default '', route_id text not null,
      harness_account text not null default 'unknown', motor_account text not null default 'unknown',
      primary key(experiment_id,variant_index),
      foreign key(experiment_id) references usage_experiments(id) on delete cascade
    );
    create table if not exists usage_experiment_runs (
      id text primary key, experiment_id text not null, interaction_id text not null default '',
      task_id text not null default '', project_id text not null default '',
      variant_index integer not null, harness text not null, motor text not null,
      model text not null, effort text not null default '', route_id text not null,
      harness_account text not null default 'unknown', motor_account text not null default 'unknown',
      tmux_session text not null default '', tmux_pane text not null default '',
      launch_order integer not null default 0,
      status text not null default 'planned', started_at integer, finished_at integer,
      foreign key(experiment_id) references usage_experiments(id) on delete cascade
    );
    create table if not exists usage_changes (
      id text primary key, created_at integer not null,
      origin text not null default 'manual', kind text not null default 'switch',
      tmux_session text not null default '', tmux_pane text not null default '',
      project text not null default '',
      before_model text not null default '', before_effort text not null default '',
      before_route text not null default '',
      after_model text not null default '', after_effort text not null default '',
      after_route text not null default '',
      status text not null default 'applied', note text not null default ''
    );
    create index if not exists idx_usage_changes_created on usage_changes(created_at);
    create table if not exists focus_blocks (
      id text primary key, mode text not null, project text not null default '',
      tmux_session text not null default '', tmux_pane text not null default '',
      planned_minutes integer not null, cycle_index integer not null default 1,
      cycle_total integer not null default 1, started_at_ms integer not null,
      ended_at_ms integer, status text not null default 'running',
      interruptions integer not null default 0, source text not null default 'comandos'
    );
    create table if not exists focus_settings (
      key text primary key, value text not null
    );
    create index if not exists idx_focus_blocks_started on focus_blocks(started_at_ms);
    create index if not exists idx_focus_blocks_project on focus_blocks(project, started_at_ms);
    create index if not exists idx_usage_turns_finished on usage_turns(turn_finished_at);
    create index if not exists idx_usage_turns_route_finished on usage_turns(route_id, turn_finished_at);
    create index if not exists idx_usage_turns_interaction on usage_turns(interaction_id);
    create index if not exists idx_session_configs_pane_time on usage_session_configs(tmux_session, tmux_pane, effective_at);
    create index if not exists idx_interactions_config_time on usage_interactions(config_id, finished_at_ms);
    create index if not exists idx_interactions_prompt on usage_interactions(tmux_session, tmux_pane, prompt_id);
    create index if not exists idx_tasks_type_time on usage_tasks(task_type, created_at);
    create index if not exists idx_experiment_variants_experiment on usage_experiment_variants(experiment_id, variant_index);
    create index if not exists idx_experiment_runs_experiment on usage_experiment_runs(experiment_id, variant_index);
"#;
const SPAN_TABLES: &str = r#"
    create table if not exists usage_spans (
      id text primary key,
      provider text not null,
      account text not null default 'main',
      session_id text not null default '',
      git_root text not null default '',
      started_at real not null,
      finished_at real not null,
      source text not null default ''
    );
    create index if not exists idx_usage_spans_finished on usage_spans(finished_at);
    create table if not exists usage_quota_snapshots (
      limit_id text not null,
      provider text not null,
      account text not null,
      win text not null default '',
      scope text not null default '',
      resets_at integer not null,
      percent real not null,
      captured_at integer not null,
      primary key (limit_id, resets_at)
    );
"#;
const TURN_INSERT_SQL: &str = r#"
insert into usage_turns (id, provider, agent, tmux_session, tmux_pane, pane_pwd, git_root, model, reasoning_effort, turn_started_at, turn_finished_at, input_tokens, output_tokens, cache_read_tokens, cache_write_tokens, total_tokens, cost_usd, source, confidence, raw, harness, motor, route_id, harness_account, motor_account, interaction_id, experiment_run_id, tool_profile, duration_ms, outcome, reasoning_tokens)
values (:id, :provider, :agent, :tmux_session, :tmux_pane, :pane_pwd, :git_root, :model, :reasoning_effort, :turn_started_at, :turn_finished_at, :input_tokens, :output_tokens, :cache_read_tokens, :cache_write_tokens, :total_tokens, :cost_usd, :source, :confidence, :raw, :harness, :motor, :route_id, :harness_account, :motor_account, :interaction_id, :experiment_run_id, :tool_profile, :duration_ms, :outcome, :reasoning_tokens)
on conflict(id) do update set
  provider=excluded.provider,
  agent=excluded.agent,
  tmux_session=excluded.tmux_session,
  tmux_pane=excluded.tmux_pane,
  pane_pwd=excluded.pane_pwd,
  git_root=excluded.git_root,
  model=excluded.model,
  reasoning_effort=excluded.reasoning_effort,
  turn_started_at=excluded.turn_started_at,
  turn_finished_at=excluded.turn_finished_at,
  input_tokens=excluded.input_tokens,
  output_tokens=excluded.output_tokens,
  cache_read_tokens=excluded.cache_read_tokens,
  cache_write_tokens=excluded.cache_write_tokens,
  total_tokens=excluded.total_tokens,
  cost_usd=excluded.cost_usd,
  source=excluded.source,
  confidence=excluded.confidence,
  raw=excluded.raw,
  harness=excluded.harness,
  motor=excluded.motor,
  route_id=excluded.route_id,
  harness_account=case when coalesce(usage_turns.harness_account,'') in ('','unknown') then excluded.harness_account else usage_turns.harness_account end,
  motor_account=case when coalesce(usage_turns.motor_account,'') in ('','unknown') then excluded.motor_account else usage_turns.motor_account end,
  interaction_id=excluded.interaction_id,
  experiment_run_id=excluded.experiment_run_id,
  tool_profile=excluded.tool_profile,
  duration_ms=excluded.duration_ms,
  outcome=excluded.outcome,
  reasoning_tokens=excluded.reasoning_tokens
"#;
