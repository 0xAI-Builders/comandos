//! Lecturas de la base de uso contra `bin/cc_usage.py` y `lib/session_profiles.py`,
//! sobre la misma base sembrada y el mismo instante.
mod support;

use comandos_core::json::response_dumps;
use comandos_core::usage_state::{self, LocalZone};
use comandos_store::{usage, usage_read};
use rusqlite::{Connection, params};
use serde_json::{Map, Value, json};
use std::{ffi::OsStr, path::Path};
use support::python::run_python;

const NOW: i64 = 1_791_115_200;
const NOW_MS: i64 = NOW * 1000;

struct Scratch(std::path::PathBuf);

impl Scratch {
    fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("cmd-usage-read-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// `(id, provider, sesión, pane, carpeta, raíz, modelo, fin, tokens, costo, confianza, raw)`.
type TurnSeed = (
    &'static str,
    &'static str,
    &'static str,
    &'static str,
    &'static str,
    &'static str,
    &'static str,
    i64,
    i64,
    f64,
    &'static str,
    &'static str,
);

/// `(id, interacción, herramienta, skill, duración, estado)`.
type CallSeed = (
    &'static str,
    &'static str,
    &'static str,
    &'static str,
    Option<i64>,
    &'static str,
);

fn exec(conn: &Connection, sql: &str, args: impl rusqlite::Params) {
    conn.execute(sql, args).unwrap();
}

fn seed(db: &Path) {
    let conn = usage::open_usage_db_at(db).unwrap();
    usage::ensure_schema(&conn).unwrap();
    for (key, value) in [
        ("COMANDOS_DAILY_BUDGET_USD", "5"),
        ("NO_ES_LIMITE", "1"),
        ("COMANDOS_CODEX_DAILY_TOKEN_LIMIT", " 1_000 "),
    ] {
        exec(
            &conn,
            "insert into usage_settings(key,value) values(?,?)",
            params![key, value],
        );
    }
    // Dos turnos con el mismo segundo (empate de orden, D12), uno fuera de 14 d, uno
    // sintético fuera de 24 h, uno de otra carpeta sin pane y uno `openai` (alias de Codex).
    let turns: &[TurnSeed] = &[
        (
            "t1",
            "claude",
            "s1",
            "%1",
            "/r/a",
            "/r/a",
            "claude-fable-5",
            NOW - 60,
            1200,
            0.25,
            "exact",
            r#"{"path": "/x/subagents/a.jsonl"}"#,
        ),
        (
            "t2",
            "claude",
            "s1",
            "%1",
            "/r/a",
            "/r/a",
            "claude-sonnet-5",
            NOW - 60,
            1200,
            0.1,
            "exact",
            r#"{"path": "/x/subagents/b.jsonl"}"#,
        ),
        (
            "t2b",
            "claude",
            "s1",
            "%2",
            "/r/a",
            "/r/a",
            "claude-sonnet-5",
            NOW - 120,
            100,
            0.0,
            "exact",
            "{bad",
        ),
        (
            "t3",
            "grok",
            "s1",
            "%4",
            "/r/b",
            "/r/b",
            "grok-4.5",
            NOW - 3600,
            50,
            0.0,
            "exact",
            "{}",
        ),
        (
            "t4",
            "codex",
            "s2",
            "%3",
            "/r/c",
            "/r/c",
            "gpt-5.6-sol",
            NOW - 20 * 86_400,
            7,
            0.0,
            "exact",
            "{}",
        ),
        (
            "t5",
            "codex",
            "s2",
            "%3",
            "/r/c",
            "/r/c",
            "<synthetic>",
            NOW - 30 * 3600,
            300,
            0.333_333_333,
            "exact",
            "{}",
        ),
        (
            "t6",
            "claude",
            "s3",
            "%9",
            "/r/d",
            "",
            "claude-fable-5",
            NOW - 200,
            10,
            0.1,
            "estimated",
            r#"{"path": 5}"#,
        ),
        (
            "t7",
            "openai",
            "s2",
            "%3",
            "/r/c",
            "/r/c",
            "gpt-5.6-sol",
            NOW - 100,
            40,
            0.0,
            "exact",
            "[]",
        ),
    ];
    for (id, provider, session, pane, pwd, root, model, at, tokens, cost, confidence, raw) in turns
    {
        let agent = if *provider == "openai" {
            "codex"
        } else {
            provider
        };
        exec(
            &conn,
            "insert into usage_turns(id,provider,agent,tmux_session,tmux_pane,pane_pwd,git_root,model,\
             turn_started_at,turn_finished_at,total_tokens,cost_usd,source,confidence,raw,interaction_id,\
             cache_read_tokens,reasoning_tokens) \
             values(?,?,?,?,?,?,?,?,?,?,?,?,'claude_jsonl',?,?,?,?,?)",
            params![
                id,
                provider,
                agent,
                session,
                pane,
                pwd,
                root,
                model,
                at - 5,
                at,
                tokens,
                cost,
                confidence,
                raw,
                if id.starts_with("t1") || *id == "t2" {
                    "i1"
                } else {
                    ""
                },
                30,
                4
            ],
        );
    }
    for (session, pane, pwd, agent, model) in [
        ("s1", "%1", "/r/a", "claude", ""),
        ("s1", "%2", "/r/a", "claude", "claude-opus-5"),
        ("s2", "%3", "/r/c", "codex", ""),
    ] {
        exec(
            &conn,
            "insert into usage_panes(tmux_session,tmux_pane,pane_pwd,git_root,agent,provider,model,\
             started_at,last_seen_at,raw) values(?,?,?,?,?,?,?,?,?,'{}')",
            params![
                session,
                pane,
                pwd,
                pwd,
                agent,
                agent,
                model,
                NOW - 100,
                NOW - 5
            ],
        );
    }
    exec(
        &conn,
        "insert into provider_usage_buckets(id,provider,start_time,end_time,total_tokens) \
         values('u1','anthropic',?,?,500)",
        params![NOW - 7200, NOW - 3600],
    );
    for (id, end, cost) in [("c1", NOW - 3600, 0.1), ("c2", NOW - 60, 0.2)] {
        exec(
            &conn,
            "insert into provider_cost_buckets(id,provider,start_time,end_time,cost_usd,line_item) \
             values(?,'anthropic',?,?,?,'tokens')",
            params![id, end - 3600, end, cost],
        );
    }
    for (id, at) in [("a1", NOW - 50), ("a2", NOW - 10)] {
        exec(
            &conn,
            "insert into usage_alerts(id,kind,level,message,created_at,last_seen_at) \
             values(?,'budget','warning','Presupuesto',?,?)",
            params![id, at, at],
        );
    }
    for (id, resets, percent) in [
        ("claude_weekly", NOW + 86_400, 41.5),
        ("claude_5h", NOW - 30 * 86_400, 12.0),
    ] {
        exec(
            &conn,
            "insert into usage_quota_snapshots(limit_id,provider,account,win,scope,resets_at,percent,captured_at) \
             values(?,'claude','main','7d','',?,?,?)",
            params![id, resets, percent, NOW - 10],
        );
    }
    exec(
        &conn,
        "insert into usage_session_configs(id,tmux_session,tmux_pane,effective_at,harness,motor,model,effort,\
         route_id,source,confidence) values('cfg1','s1','%1',?,'claude','claude','claude-fable-5','high',\
         'claude:claude','manual','exact')",
        params![NOW - 9000],
    );
    for (id, ty) in [("k1", "implementation"), ("k2", "debugging")] {
        exec(
            &conn,
            "insert into usage_tasks(id,task_type,created_at,updated_at) values(?,?,?,?)",
            params![id, ty, NOW, NOW],
        );
    }
    for (id, task, start, finish, duration) in [
        (
            "i1",
            "k1",
            NOW_MS - 65_000,
            Some(NOW_MS - 5_000),
            Some(60_000),
        ),
        (
            "i2",
            "k1",
            NOW_MS - 3_000_000,
            Some(NOW_MS - 2_000_000),
            Some(1_000_000),
        ),
        ("i3", "k2", NOW_MS - 1_000, None, None),
        (
            "i4",
            "k2",
            NOW_MS - 9 * 86_400_000,
            Some(NOW_MS - 9 * 86_400_000 + 10),
            Some(10),
        ),
    ] {
        exec(
            &conn,
            "insert into usage_interactions(id,tmux_session,tmux_pane,task_id,config_id,started_at_ms,\
             finished_at_ms,duration_ms,source,confidence,created_at) values(?,'s1','%1',?,'cfg1',?,?,?,\
             'claude_hook','exact',?)",
            params![id, task, start, finish, duration, NOW],
        );
    }
    for (id, outcome, rating) in [("i1", "solved", Some(5)), ("i2", "failed", None::<i64>)] {
        exec(
            &conn,
            "insert into usage_ratings(interaction_id,rated_at,outcome,rating) values(?,?,?,?)",
            params![id, NOW, outcome, rating],
        );
    }
    let calls: &[CallSeed] = &[
        ("tc1", "i1", "Skill", "tdd", Some(100), "ok"),
        ("tc2", "i1", "mcp__odoo__search_records", "", None, "failed"),
        ("tc3", "i1", "Skill", "", Some(5), "ok"),
        ("tc4", "i1", "Bash", "", Some(7), "ok"),
        ("tc5", "i2", "mcp__odoo__get_record", "", Some(50), "ok"),
        ("tc6", "i1", "mcp__bad name!__x", "", Some(1), "ok"),
        ("tc7", "i1", "SKILL", "tdd", Some(25), "failed"),
        ("tc8", "i3", "mcp__zz__a", "", None, "ok"),
    ];
    for (index, (id, interaction, tool, skill, duration, status)) in calls.iter().enumerate() {
        exec(
            &conn,
            "insert into usage_tool_calls(id,interaction_id,sequence,tool_name,started_at_ms,finished_at_ms,\
             duration_ms,status,confidence,skill_name) values(?,?,?,?,?,?,?,?,'observed',?)",
            params![
                id,
                interaction,
                index as i64,
                tool,
                NOW_MS - 7_000,
                NOW_MS - 6_000 + index as i64,
                duration,
                status,
                skill
            ],
        );
    }
    exec(
        &conn,
        "insert into usage_experiments(id,label,task_type,status,created_at,updated_at) \
         values('e1','A/B','implementation','active',?,?)",
        params![NOW, NOW],
    );
    for (id, variant, interaction) in [("r0", 0, "i1"), ("r1", 1, "i2")] {
        exec(
            &conn,
            "insert into usage_experiment_runs(id,experiment_id,interaction_id,task_id,variant_index,harness,\
             motor,model,route_id,status) values(?,'e1',?,'k1',?,'claude','claude','claude-fable-5',\
             'claude:claude','completed')",
            params![id, interaction, variant],
        );
    }
}

/// Paneles vivos que comparten carpeta (uso «compartido») y uno sin turnos.
fn live_panes() -> Value {
    json!([
        {"tmux_session": "s1", "tmux_pane": "%1", "pane_pwd": "/r/a", "git_root": "/r/a", "agent": "claude",
         "provider": "claude", "model": ""},
        {"tmux_session": "s1", "tmux_pane": "%2", "pane_pwd": "/r/a", "git_root": "/r/a", "agent": "claude",
         "provider": "claude", "model": "claude-opus-5"},
        {"tmux_session": "s2", "tmux_pane": "%3", "pane_pwd": "/r/c", "agent": "codex"},
        {"tmux_session": "s4", "tmux_pane": "%7", "pane_pwd": "/r/z", "agent": "grok", "confidence": "detected"}
    ])
}

const ORACLE: &str = r#"
import json, os, sys
repo, db, now, live = sys.argv[1], sys.argv[2], int(sys.argv[3]), json.loads(sys.argv[4])
sys.path[:0] = [os.path.join(repo, "bin"), os.path.join(repo, "lib")]
import cc_usage, session_profiles
cc_usage.time.time = lambda: float(now)
settings = {"COMANDOS_CLAUDE_DAILY_TOKEN_LIMIT": "900", "COMANDOS_DAILY_BUDGET_USD": "2.5",
            "CODEX_WEEKLY_TOKEN_LIMIT": "40"}
print(json.dumps({
  "settings": cc_usage.read_usage_settings(db),
  "state": cc_usage.build_usage_state(db, [], now=now, settings=settings),
  "live": cc_usage.build_usage_state(db, live, now=now, settings=settings),
  "panes": cc_usage.list_panes(db),
  "alerts": cc_usage.list_alerts(db),
  "alerts1": cc_usage.list_alerts(db, 1),
  "recent": cc_usage.recent_interactions(db, 1),
  "recent0": cc_usage.recent_interactions(db, 0),
  "snapshots": cc_usage.quota_snapshots(db, since=now - 40 * 86400),
  "grok": cc_usage.grok_measured_usage(db, now),
  "groq": cc_usage.groq_measured_usage(db, now),
  "guard": cc_usage.token_guard_report(db, now),
  "analytics": cc_usage.experiment_analytics(db, 7, ""),
  "analytics_debug": cc_usage.experiment_analytics(db, 99, "debugging"),
  "ext": session_profiles.extension_usage(db, "s1", "%1", "7", now=float(now)),
  "ext_all": session_profiles.extension_usage(db, "", "", 0, now=float(now)),
}))
"#;

fn objects(value: &Value) -> Vec<Map<String, Value>> {
    value
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_object().cloned().unwrap())
        .collect()
}

#[test]
fn usage_reads_match_python_oracle() {
    let scratch = Scratch::new("main");
    let db = scratch.0.join("comandos-usage.sqlite");
    seed(&db);
    let live = live_panes();
    let home = scratch.0.join("home");
    let Some(expected) = run_python(
        ORACLE,
        &[
            db.as_os_str(),
            OsStr::new(&NOW.to_string()),
            OsStr::new(&response_dumps(&live).unwrap()),
        ],
        &home,
    ) else {
        return;
    };
    let conn = usage::open_usage_db_at(&db).unwrap();
    let zone = chrono_tz::America::Mexico_City;
    let rows = usage_read::state_rows(&conn, NOW - 14 * 86_400).unwrap();
    let mut settings = Map::new();
    settings.insert("COMANDOS_CLAUDE_DAILY_TOKEN_LIMIT".into(), json!("900"));
    settings.insert("COMANDOS_DAILY_BUDGET_USD".into(), json!("2.5"));
    settings.insert("CODEX_WEEKLY_TOKEN_LIMIT".into(), json!("40"));
    let build = |panes: Vec<Map<String, Value>>| {
        usage_state::build_state(
            NOW,
            panes,
            &rows.turns,
            &rows.provider_usage,
            &rows.provider_costs,
            &settings,
        )
        .unwrap()
    };
    let panes = usage_read::list_panes(&conn).unwrap();
    let day = zone.day_start(NOW).unwrap();
    let list =
        |v: Vec<Map<String, Value>>| Value::Array(v.into_iter().map(Value::Object).collect());
    let got = json!({
        "settings": Value::Object(usage_read::usage_settings(&conn).unwrap().into_iter().collect()),
        "state": build(panes.clone()),
        "live": build(objects(&live)),
        "panes": list(panes),
        "alerts": list(usage_read::list_alerts(&conn, 12).unwrap()),
        "alerts1": list(usage_read::list_alerts(&conn, 1).unwrap()),
        "recent": list(usage_read::recent_interactions(&conn, 1).unwrap()),
        "recent0": list(usage_read::recent_interactions(&conn, 0).unwrap()),
        "snapshots": usage_read::quota_snapshots(&conn, NOW - 40 * 86_400).unwrap(),
        "grok": usage_read::measured_usage(&conn, "grok", NOW, day).unwrap(),
        "groq": usage_read::measured_usage(&conn, "groq", NOW, day).unwrap(),
        "guard": usage_read::token_guard_report(&conn, NOW).unwrap(),
        "analytics": usage_read::experiment_analytics(&conn, 7, "", NOW as f64).unwrap().unwrap(),
        "analytics_debug": usage_read::experiment_analytics(&conn, 99, "debugging", NOW as f64).unwrap().unwrap(),
        "ext": usage_read::extension_usage(&conn, "s1", "%1", 7, NOW as f64).unwrap().unwrap(),
        "ext_all": usage_read::extension_usage(&conn, "", "", 0, NOW as f64).unwrap().unwrap(),
    });
    assert_eq!(response_dumps(&got).unwrap(), expected.trim_end());
}

#[test]
fn week_rows_match_cc_dash_queries() {
    let scratch = Scratch::new("week");
    let db = scratch.0.join("comandos-usage.sqlite");
    seed(&db);
    {
        let conn = usage::open_usage_db_at(&db).unwrap();
        exec(
            &conn,
            "insert into usage_spans(id,provider,account,session_id,git_root,started_at,finished_at,source) \
             values('sp1','claude','main','x','/r/a',?,?,'claude_jsonl')",
            params![NOW as f64 - 90.5, NOW as f64 - 30.25],
        );
    }
    // Las dos consultas de `analytics_week_payload` (estrecha del commit y ancha del árbol vivo).
    let script = r#"
import json, os, sys, sqlite3
repo, db, since = sys.argv[1], sys.argv[2], float(sys.argv[3])
con = sqlite3.connect(db)
narrow = [{"provider": r[0], "account": r[1], "git_root": r[2], "pane_pwd": r[3],
           "started": r[4], "finished": r[5], "tokens": r[6]}
          for r in con.execute("select provider, harness_account, git_root, pane_pwd, turn_started_at, turn_finished_at, total_tokens "
                               "from usage_turns where turn_finished_at >= ?", (since,))]
wide = [{"provider": r[0], "account": r[1], "git_root": r[2], "pane_pwd": r[3],
         "started": r[4], "finished": r[5], "tokens": r[6], "cost": r[7],
         "model": r[8], "session": r[9], "agent": r[10]}
        for r in con.execute("select provider, harness_account, git_root, pane_pwd, turn_started_at, turn_finished_at, total_tokens, "
                             "cost_usd, model, tmux_session, agent "
                             "from usage_turns where turn_finished_at >= ?", (since,))]
spans = [{"provider": r[0], "account": r[1], "git_root": r[2], "started": r[3], "finished": r[4]}
         for r in con.execute("select provider, account, git_root, started_at, finished_at from usage_spans where finished_at >= ?",
                              (since,))]
print(json.dumps([narrow, wide, spans]))
"#;
    let since = NOW as f64 - 17.0 * 86_400.0 + 0.5;
    let Some(expected) = run_python(
        script,
        &[db.as_os_str(), OsStr::new(&since.to_string())],
        &scratch.0.join("home"),
    ) else {
        return;
    };
    let conn = usage::open_usage_db_at(&db).unwrap();
    let (narrow, spans) = usage_read::week_rows(&conn, since, false).unwrap();
    let (wide, spans_wide) = usage_read::week_rows(&conn, since, true).unwrap();
    assert_eq!(spans, spans_wide);
    let got = json!([narrow, wide, spans]);
    assert_eq!(response_dumps(&got).unwrap(), expected.trim_end());
}

#[test]
fn writes_match_python_rows() {
    let scratch = Scratch::new("writes");
    let ours = scratch.0.join("ours.sqlite");
    let theirs = scratch.0.join("theirs.sqlite");
    for db in [&ours, &theirs] {
        let conn = usage::open_usage_db_at(db).unwrap();
        usage::ensure_schema(&conn).unwrap();
    }
    let pane = json!({"tmux_session": "s1", "tmux_pane": "%1", "pane_pwd": "/r/a", "git_root": "/r/a",
                      "agent": "claude", "provider": "claude", "agent_pid": 42, "model": "m",
                      "started_at": 10, "last_seen_at": 20, "raw": "{}"});
    let pane2 = json!({"tmux_session": "s1", "tmux_pane": "%1", "pane_pwd": "/r/b", "agent": "codex",
                       "provider": "codex", "started_at": 99, "last_seen_at": 30});
    let snaps = json!([
        {"id": "claude_weekly", "provider": "claude", "window": "7d", "resets_at": NOW + 5400, "percent": 41.5},
        {"id": "claude_5h", "provider": "claude", "account": "relotto", "window": "5h", "resets_at": NOW + 1800,
         "percent": "12", "captured_at": NOW - 3, "scope": "x"},
        {"id": "claude_5h", "provider": "claude", "window": "5h", "resets_at": NOW + 1800, "percent": 13.0,
         "captured_at": NOW - 9},
        {"id": "skip", "provider": "claude", "window": "1d", "resets_at": NOW, "percent": 1},
        {"id": "skip2", "provider": "claude", "window": "5h", "resets_at": 0, "percent": 1},
        {"id": "skip3", "provider": "claude", "window": "5h", "resets_at": NOW, "percent": null}
    ]);
    let script = r#"
import json, os, sys
repo, db, now = sys.argv[1], sys.argv[2], int(sys.argv[3])
sys.path[:0] = [os.path.join(repo, "bin")]
import cc_usage
pane, pane2, snaps = json.loads(sys.argv[4])
cc_usage.record_pane(db, pane)
cc_usage.record_pane(db, pane2)
print(cc_usage.record_quota_snapshots(db, snaps, now=now))
"#;
    let args = response_dumps(&json!([pane, pane2, snaps])).unwrap();
    let Some(count) = run_python(
        script,
        &[
            theirs.as_os_str(),
            OsStr::new(&NOW.to_string()),
            OsStr::new(&args),
        ],
        &scratch.0.join("home"),
    ) else {
        return;
    };
    let conn = usage::open_usage_db_at(&ours).unwrap();
    // `record_panes` (una transacción) con un pane que el Python no podría
    // registrar (`raw` lista: `InterfaceError`, ignorado por su `except`): los
    // otros dos quedan igual que con dos `record_pane` del Python.
    let bad = json!({"tmux_session": "s9", "tmux_pane": "%9", "raw": [1]});
    let panes: Vec<_> = [&pane, &bad, &pane2]
        .iter()
        .map(|p| p.as_object().unwrap().clone())
        .collect();
    assert_eq!(usage_read::record_panes(&conn, &panes).unwrap(), 2);
    let n = usage_read::record_quota_snapshots(&conn, snaps.as_array().unwrap(), NOW).unwrap();
    assert_eq!(n.to_string(), count.trim_end());
    let dump = |db: &Path| {
        let conn = Connection::open(db).unwrap();
        let mut out = Vec::new();
        for sql in [
            "select * from usage_panes order by tmux_session, tmux_pane",
            "select * from usage_quota_snapshots order by limit_id, resets_at",
        ] {
            let mut stmt = conn.prepare(sql).unwrap();
            let columns = stmt.column_count();
            let rows: Vec<String> = stmt
                .query_map([], |r| {
                    Ok((0..columns)
                        .map(|i| format!("{:?}", r.get_ref(i).unwrap()))
                        .collect::<Vec<_>>()
                        .join("|"))
                })
                .unwrap()
                .collect::<Result<_, _>>()
                .unwrap();
            out.push(rows);
        }
        out
    };
    assert_eq!(dump(&ours), dump(&theirs));
}

#[test]
fn extension_usage_validations_are_python_messages() {
    let scratch = Scratch::new("ext");
    let conn = usage::open_usage_db_at(&scratch.0.join("u.sqlite")).unwrap();
    usage::ensure_schema(&conn).unwrap();
    let err = |s: &str, p: &str| {
        usage_read::extension_usage(&conn, s, p, 7, 0.0)
            .unwrap()
            .unwrap_err()
    };
    assert_eq!(err("a b", ""), "sesión inválida");
    assert_eq!(err("", "%1"), "panel inválido");
    assert_eq!(err("s", "1"), "panel inválido");
    assert_eq!(err("s\u{e9}", "%1"), "sesión inválida");
    assert_eq!(usage_read::extension_scope_error("s", "%12"), None);
    // `int(days)` y su mensaje los resuelve la ruta: aquí, el orden de las validaciones.
    assert_eq!(
        usage_read::extension_scope_error("a b", "x"),
        Some("sesión inválida".into())
    );
    assert!(
        usage_read::experiment_analytics(&conn, 7, "nope", 0.0)
            .unwrap()
            .is_err_and(|e| e == "invalid task type")
    );
}

#[test]
fn undecodable_cells_are_errors() {
    let scratch = Scratch::new("blob");
    let conn = usage::open_usage_db_at(&scratch.0.join("u.sqlite")).unwrap();
    usage::ensure_schema(&conn).unwrap();
    exec(
        &conn,
        "insert into usage_alerts(id,kind,level,message,created_at,last_seen_at,raw) values('a','k','l',x'ff',1,1,'{}')",
        [],
    );
    assert!(matches!(
        usage_read::list_alerts(&conn, 12),
        Err(usage_read::ReadError::Undecodable)
    ));
}
