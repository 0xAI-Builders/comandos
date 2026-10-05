//! Importación local de uso contra `bin/cc_usage.py`: el mismo HOME de fixture
//! (solo se lee) y dos copias de la misma base sembrada; el Python importa en una
//! y `usage_import` en la otra, en el mismo orden. Se comparan las tablas fila a
//! fila (con el tipo de SQLite de cada celda) tras una pasada y tras una segunda
//! con el mismo `seen`, en la que Rust no vuelve a abrir ningún transcript.
mod support;

use comandos_store::{usage, usage_import};
use rusqlite::{Connection, params, types::ValueRef};
use serde_json::{Map, Value, json};
use std::{
    collections::HashMap,
    ffi::OsStr,
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use support::python::run_python;

const NOW: i64 = 1_791_115_200;

struct Scratch(PathBuf);

impl Scratch {
    fn new(tag: &str) -> Self {
        let dir =
            std::env::temp_dir().join(format!("cmd-usage-import-{tag}-{}", std::process::id()));
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

/// Escribe `bytes` en `path` con `mtime` fijo (segundos con fracción).
fn put(path: &Path, bytes: &[u8], mtime: f64) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, bytes).unwrap();
    let when = UNIX_EPOCH + Duration::from_secs_f64(mtime);
    std::fs::File::options()
        .write(true)
        .open(path)
        .unwrap()
        .set_modified(when)
        .unwrap();
}

fn iso(ts: i64) -> String {
    chrono::DateTime::from_timestamp(ts, 0)
        .unwrap()
        .format("%Y-%m-%dT%H:%M:%S%.3fZ")
        .to_string()
}

fn lines(rows: &[Value]) -> Vec<u8> {
    let mut out = Vec::new();
    for row in rows {
        out.extend(serde_json::to_vec(row).unwrap());
        out.push(b'\n');
    }
    out
}

/// El HOME de fixture: Codex (dos cuentas), Claude (dos cuentas y carpetas que se
/// ignoran), Grok y OpenCode.
fn seed_home(home: &Path) {
    let day = 86_400;
    // --- Codex, cuenta main.
    let mut codex = lines(&[
        json!({"type": "session_meta", "payload": {"id": "thread-1", "cwd": "/r/codex"}}),
        json!({"type": "turn_context", "payload": {"turn_id": "t1", "cwd": "/r/codex/sub", "model": "gpt-5.5", "effort": "high"}}),
        json!({"type": "token_usage_record", "timestamp": iso(NOW - 3600), "payload": {"response_id": "r1", "turn_id": "t1",
              "usage": {"input_tokens": 100, "cached_input_tokens": 40, "output_tokens": 20, "reasoning_output_tokens": 5}}}),
        json!({"type": "token_usage_record", "timestamp": iso(NOW - 3500), "payload": {"response_id": "r2", "turn_id": "t2",
              "thread_id": "thread-x", "usage": {"input_tokens": 7, "output_tokens": 3, "total_tokens": 0}}}),
        json!({"type": "token_usage_record", "timestamp": iso(NOW - 30 * day), "payload": {"response_id": "old", "usage": {"input_tokens": 1}}}),
        json!({"type": "token_usage_record", "timestamp": iso(NOW - 3400), "payload": {"response_id": "zero", "usage": {}}}),
        json!({"type": "event_msg", "payload": {"type": "task_complete", "turn_id": "t1", "started_at": NOW - 3700, "completed_at": NOW - 3600}}),
        json!({"type": "event_msg", "payload": {"type": "agent_message", "turn_id": "t9"}}),
        json!({"type": "response_item", "payload": {"type": "message", "content": [{"text": "x".repeat(4000)}]}}),
        json!([1, 2, 3]),
    ]);
    // Bytes no UTF-8 dentro de un texto (U+FFFD en la carpeta) y un `\r` que
    // parte la línea en dos; `\r\n` como fin de línea; NaN que serde no acepta.
    codex.extend(b"{\"type\": \"turn_context\", \"payload\": {\"turn_id\": \"t2\", \"cwd\": \"/r/c\xff\xfeodex\", \"model\": \"gpt-5.4\"}}\r");
    codex.extend(format!("{{\"type\": \"token_usage_record\", \"timestamp\": {}, \"payload\": {{\"response_id\": \"r3\", \"turn_id\": \"t2\", \"usage\": {{\"input_tokens\": 9, \"output_tokens\": 1, \"x\": NaN}}}}}}\r\n", NOW - 3000).as_bytes());
    codex.extend(b"not json\n");
    codex.extend(format!("{{\"type\": \"event_msg\", \"payload\": {{\"type\": \"task_complete\", \"turn_id\": \"t2\", \"started_at\": {}, \"completed_at\": {}}}}}", NOW - 3100, NOW - 2990).as_bytes());
    put(
        &home.join(".codex/sessions/2026/10/04/rollout-a.jsonl"),
        &codex,
        (NOW - 100) as f64 + 0.25,
    );
    put(
        &home.join(".codex/sessions/2026/10/04/notes.jsonl"),
        b"{}\n",
        (NOW - 100) as f64,
    );
    put(
        &home.join(".codex/sessions/2026/09/01/rollout-old.jsonl"),
        &lines(&[json!({"type": "session_meta"})]),
        (NOW - 40 * day) as f64,
    );
    // --- Codex, cuenta work: un fork que repite r1 con otra cuenta.
    put(
        &home.join(".codex-accounts/work/sessions/2026/10/04/rollout-fork.jsonl"),
        &lines(&[
            json!({"type": "session_meta", "payload": {"id": "thread-2", "cwd": "/r/fork"}}),
            json!({"type": "token_usage_record", "timestamp": NOW - 3600, "payload": {"response_id": "r1",
                  "usage": {"input_tokens": 100, "cached_input_tokens": 40, "output_tokens": 20}}}),
        ]),
        (NOW - 50) as f64,
    );
    std::fs::create_dir_all(home.join(".codex-accounts/.hidden/sessions")).unwrap();
    // --- Claude, cuenta main.
    let a = json!({"type": "assistant", "timestamp": iso(NOW - 1000), "cwd": "/r/a", "sessionId": "s-a", "requestId": "req1",
                   "message": {"id": "msg1", "model": "claude-fable-5", "content": [{"type": "text", "text": "hola"}],
                               "usage": {"input_tokens": 5, "output_tokens": 7, "cache_read_input_tokens": 3, "z": 1, "a": {"b": 2, "a": 1}}}});
    let mut a2 = a.clone();
    a2["message"]["content"] = json!([{"type": "tool_use", "input": {"x": "y".repeat(10_000)}}]);
    let claude = lines(&[
        a,
        a2,
        json!({"type": "assistant", "cwd": "/r/b", "uuid": "u-sin-fecha",
               "message": {"model": "<synthetic>", "usage": {"input_tokens": 1}}}),
        json!({"type": "assistant", "timestamp": iso(NOW - 900), "cwd": "/r/a",
               "message": {"model": "  claude-opus-5  ", "usage": {"output_tokens": "4"}}}),
        json!({"type": "user", "timestamp": iso(NOW - 800), "message": {"usage": {"input_tokens": 50}}}),
        json!({"type": "assistant", "timestamp": iso(NOW - 700), "message": {"id": "msg2", "usage": {"input_tokens": 0}}}),
        json!({"type": "system", "subtype": "turn_duration", "timestamp": iso(NOW - 600), "durationMs": 61500,
               "cwd": "/r/a", "sessionId": "s-a", "uuid": "dur-1"}),
        json!({"type": "system", "subtype": "turn_duration", "timestamp": iso(NOW - 500), "durationMs": 0}),
        json!({"type": "system", "subtype": "turn_duration", "timestamp": iso(NOW - 400), "durationMs": 1000, "cwd": "/r/zzz"}),
    ]);
    put(
        &home.join(".claude/projects/-r-a/s1.jsonl"),
        &claude,
        (NOW - 10) as f64 + 0.999_999_9,
    );
    put(
        &home.join(".claude/projects/-r-a/tool-results/t.jsonl"),
        &lines(&[
            json!({"type": "assistant", "timestamp": iso(NOW), "message": {"id": "skip", "usage": {"input_tokens": 9}}}),
        ]),
        (NOW - 10) as f64,
    );
    put(
        &home.join(".claude/projects/-r-a/memory/m.jsonl"),
        &lines(&[
            json!({"type": "assistant", "timestamp": iso(NOW), "message": {"id": "skip2", "usage": {"input_tokens": 9}}}),
        ]),
        (NOW - 10) as f64,
    );
    // --- Claude, cuenta relotto; carpetas `-x` y `y.lock` no son cuentas.
    let relotto = lines(&[
        json!({"type": "assistant", "timestamp": iso(NOW - 200), "cwd": "/r/rel", "sessionId": "s-rel",
                                 "message": {"id": "msg1", "model": "claude-fable-5", "usage": {"input_tokens": 2}}}),
    ]);
    put(
        &home.join(".claude-accounts/relotto/projects/-r-rel/s.jsonl"),
        &relotto,
        (NOW - 20) as f64,
    );
    put(
        &home.join(".claude-accounts/-x/projects/p/s.jsonl"),
        &relotto,
        (NOW - 20) as f64,
    );
    put(
        &home.join(".claude-accounts/y.lock/projects/p/s.jsonl"),
        &relotto,
        (NOW - 20) as f64,
    );
    // --- Grok.
    put(
        &home.join(".grok/sessions/g1/summary.json"),
        serde_json::to_string(&json!({"info": {"id": "grok-s1", "cwd": "/r/grok"}, "current_model_id": "grok-5", "reasoning_effort": "low"})).unwrap().as_bytes(),
        (NOW - 30) as f64,
    );
    put(
        &home.join(".grok/sessions/g1/updates.jsonl"),
        &lines(&[
            json!({"timestamp": (NOW - 300) * 1000, "params": {"sessionId": "ext-1", "update": {"sessionUpdate": "turn_completed", "prompt_id": "p1",
                   "stop_reason": "end_turn", "usage": {"inputTokens": 10, "outputTokens": 4, "cachedReadTokens": 2, "apiDurationMs": 4500,
                   "modelUsage": {"grok-5-fast": {}, "grok-5": {}}, "modelCalls": 2, "numTurns": 1, "reasoningTokens": 3}}}}),
            json!({"timestamp": NOW - 200, "params": {"_meta": {"eventId": "ev-2"}, "update": {"sessionUpdate": "turn_completed",
                   "usage": {"inputTokens": 1, "outputTokens": 1, "totalTokens": 9}}}}),
            json!({"timestamp": NOW - 100, "params": {"update": {"sessionUpdate": "agent_message_chunk"}}}),
            json!({"timestamp": (NOW - 30 * day) * 1000, "params": {"update": {"sessionUpdate": "turn_completed", "usage": {"inputTokens": 1}}}}),
        ]),
        (NOW - 30) as f64,
    );
    put(
        &home.join(".grok/sessions/updates.jsonl"),
        &lines(&[
            json!({"timestamp": NOW - 50, "params": {"sessionId": "", "update": {"sessionUpdate": "turn_completed",
                        "prompt_id": "p9", "usage": {"inputTokens": 3, "outputTokens": 3, "reasoningTokens": "x"}}}}),
        ]),
        (NOW - 40) as f64,
    );
    put(
        &home.join(".grok/sessions/.hidden/updates.jsonl"),
        &lines(&[
            json!({"timestamp": NOW - 50, "params": {"update": {"sessionUpdate": "turn_completed", "usage": {"inputTokens": 3}}}}),
        ]),
        (NOW - 40) as f64,
    );
    // --- OpenCode.
    let oc = Connection::open(home.join("opencode.db")).unwrap();
    oc.execute_batch(
        "create table session (id text primary key, directory text);
         create table message (id text primary key, session_id text, time_created integer, data text);",
    )
    .unwrap();
    oc.execute("insert into session values ('ses1', '/r/oc')", [])
        .unwrap();
    let msgs = [
        ("m1", "ses1", (NOW - 500) * 1000 + 999, json!({"role": "assistant", "providerID": "groq", "modelID": "llama",
          "tokens": {"input": 10, "output": 5, "reasoning": 2, "cache": {"read": 1, "write": 0}}, "cost": 0.0021}).to_string()),
        ("m2", "ses1", (NOW - 400) * 1000, json!({"role": "user", "tokens": {"input": 3}}).to_string()),
        ("m3", "nosession", (NOW - 300) * 1000, json!({"role": "assistant", "modelID": "solo", "tokens": {}, "cost": 0.5}).to_string()),
        ("m4", "ses1", (NOW - 200) * 1000, "{roto".to_owned()),
        ("m5", "ses1", (NOW - 30 * day) * 1000, json!({"role": "assistant", "tokens": {"input": 3}}).to_string()),
        ("m6", "ses1", (NOW - 100) * 1000, json!({"role": "assistant", "tokens": {"input": 0}, "cost": 0}).to_string()),
    ];
    for (id, session, at, data) in msgs {
        oc.execute(
            "insert into message values (?,?,?,?)",
            params![id, session, at, data],
        )
        .unwrap();
    }
    drop(oc);
    let mut roots = Map::new();
    for (k, v) in [
        ("/r/codex/sub", "/r/codex"),
        ("/r/a", "/r/a-root"),
        ("/r/grok", "/r/grok-root"),
        ("/r/oc", "/r/oc-root"),
        ("/r/rel", ""),
    ] {
        roots.insert(k.into(), v.into());
    }
    std::fs::write(home.join("roots.json"), Value::Object(roots).to_string()).unwrap();
}

fn seed_db(db: &Path) {
    let conn = usage::open_usage_db_at(db).unwrap();
    usage::ensure_schema(&conn).unwrap();
    let day = 86_400;
    let turn = |id: &str,
                source: &str,
                account: &str,
                finished: i64,
                interaction: &str,
                model: &str| {
        conn.execute(
            "insert into usage_turns(id,provider,agent,tmux_session,tmux_pane,pane_pwd,git_root,model,turn_started_at,\
             turn_finished_at,source,confidence,harness_account,interaction_id,motor,route_id,harness) \
             values(?,'codex','codex','s','%1','/p','/p',?,?,?,?,'exact',?,?,'codex','codex:codex','codex')",
            params![id, model, finished, finished, source, account, interaction],
        )
        .unwrap();
    };
    turn("old", "hook", "main", NOW - 30 * day, "", "m");
    turn(
        "claude-unknown",
        "claude_jsonl",
        "unknown",
        NOW - day,
        "",
        "m",
    );
    turn(
        "codex-resp-r1",
        "codex_rollout",
        "main",
        NOW - 3600,
        "",
        "m",
    );
    turn("t-int3", "hook", "work", NOW - 2000, "int3", "gpt-5.5");
    conn.execute(
        "insert into usage_spans values('span-old','claude','main','','',?,?,'claude_jsonl')",
        params![(NOW - 30 * day) as f64, (NOW - 30 * day) as f64],
    )
    .unwrap();
    conn.execute(
        "insert into usage_panes(tmux_session,tmux_pane,pane_pwd,git_root,agent,provider,started_at,last_seen_at) \
         values('s1','%1','/r/pwd','',?,?,?,?)",
        params!["claude", "claude", NOW - 900, NOW - 10],
    )
    .unwrap();
    let cfg = |id: &str, pane: &str, at: i64| {
        conn.execute(
            "insert into usage_session_configs values(?, 's1', ?, ?, 'claude','claude','claude-fable-5','high','main','main','claude:claude','runtime','exact')",
            params![id, pane, at],
        )
        .unwrap();
    };
    cfg("cfg-race", "%1", NOW - 5000 + 30);
    cfg("cfg-far", "%4", NOW - 10 * day);
    let interaction = |id: &str,
                       pane: &str,
                       at_ms: i64,
                       source: &str,
                       session: &str,
                       prompt: &str| {
        conn.execute(
            "insert into usage_interactions(id,tmux_session,tmux_pane,started_at_ms,source,confidence,created_at,\
             agent_session_id,prompt_id) values(?,'s1',?,?,?,'exact',?,?,?)",
            params![id, pane, at_ms, source, NOW, session, prompt],
        )
        .unwrap();
    };
    interaction("int1", "%1", (NOW - 5000) * 1000, "hook:claude", "", "");
    interaction("int2", "%2", (NOW - 4000) * 1000, "hook:codex", "", "");
    interaction("int3", "%3", (NOW - 3000) * 1000 + 1, "hook", "", "");
    interaction("int4", "%4", (NOW - 2000) * 1000, "hook", "", "");
    interaction("int5", "%5", (NOW - 1000) * 1000, "hook", "", "");
    interaction(
        "int-grok",
        "%7",
        (NOW - 400) * 1000,
        "hook:grok",
        "ext-1",
        "p1",
    );
    interaction(
        "int-old",
        "%1",
        (NOW - 30 * day) * 1000,
        "hook:claude",
        "",
        "",
    );
}

/// Volcado ordenado de las tablas que toca la importación, celda a celda con su tipo.
fn dump(db: &Path) -> Vec<Vec<String>> {
    let conn = Connection::open(db).unwrap();
    let mut out = Vec::new();
    for table in [
        "usage_turns",
        "usage_spans",
        "usage_session_configs",
        "usage_interactions",
    ] {
        let mut stmt = conn
            .prepare(&format!("select * from {table} order by 1"))
            .unwrap();
        let columns = stmt.column_count();
        let rows: Vec<String> = stmt
            .query_map([], |r| {
                Ok((0..columns)
                    .map(|i| match r.get_ref(i).unwrap() {
                        ValueRef::Null => "NULL".to_owned(),
                        ValueRef::Integer(n) => format!("i:{n}"),
                        ValueRef::Real(x) => format!("r:{x:?}"),
                        ValueRef::Text(t) => format!("t:{}", String::from_utf8_lossy(t)),
                        ValueRef::Blob(b) => format!("b:{b:?}"),
                    })
                    .collect::<Vec<_>>()
                    .join("|"))
            })
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        out.push(rows);
    }
    out
}

struct MapRoots(HashMap<String, String>);

impl usage_import::GitRoots for MapRoots {
    fn root(&self, path: &str) -> String {
        self.0.get(path).cloned().unwrap_or_else(|| path.to_owned())
    }
}

const ORACLE: &str = r#"
import json, os, sqlite3, sys
repo, home, db, now = sys.argv[1], sys.argv[2], sys.argv[3], int(sys.argv[4])
sys.path[:0] = [os.path.join(repo, "bin"), os.path.join(repo, "lib")]
os.environ["HOME"] = home
import cc_usage
roots = json.load(open(os.path.join(home, "roots.json")))
cc_usage.git_root_for_path = lambda p, run=None: roots.get(p, p)
seen = {}
def cycle():
    cc_usage.reconcile_orphan_interactions(db, now=now)
    cc_usage.prune_old_turns(db, max_age_days=21, now=now)
    cc_usage.record_local_codex_rollouts(db, now=now, max_age_days=21, max_files=None, seen=seen.setdefault("codex", {}))
    cc_usage.record_local_grok_updates(db, [os.path.join(home, ".grok")], now=now, max_age_days=21)
    for alias, h in cc_usage.account_homes("~/.claude", "~/.claude-accounts"):
        cc_usage.record_local_claude_jsonl(db, os.path.join(h, "projects"), now=now, max_age_days=21,
            max_files=None, account=alias, seen=seen.setdefault("claude:" + alias, {}))
    cc_usage.record_local_opencode_db(db, os.path.join(home, "opencode.db"), now=now, max_age_days=21)
cycle()
src, dst = sqlite3.connect(db), sqlite3.connect(db + ".1")
src.backup(dst)
dst.close()
src.close()
cycle()
"#;

fn rust_cycle(home: &Path, db: &Path, seen: &mut usage_import::ImportSeen, roots: &MapRoots) {
    let conn = usage::open_usage_db_at(db).unwrap();
    let zone = chrono_tz::America::Mexico_City;
    let admit = |_: &Connection| true;
    let plan = usage_import::ImportPlan {
        now: NOW,
        max_age_days: 21,
        claude_max_files: None,
        codex_max_files: None,
        home,
        claude_projects_main: None,
        opencode_db: home.join("opencode.db"),
        zone: &zone,
        admit: &admit,
        cancelled: &|| false,
    };
    usage_import::reconcile_orphan_interactions(&conn, NOW, 14).unwrap();
    usage_import::prune_old_turns(&conn, NOW, 21).unwrap();
    usage_import::record_local_codex_rollouts(&conn, &plan, seen, roots).unwrap();
    usage_import::record_local_grok_updates(&conn, &plan, &[home.join(".grok")], roots).unwrap();
    for (alias, h) in usage_import::account_homes(home, ".claude", ".claude-accounts") {
        usage_import::record_local_claude_jsonl(
            &conn,
            &plan,
            &h.join("projects"),
            &alias,
            seen,
            roots,
        )
        .unwrap();
    }
    usage_import::record_local_opencode_db(&conn, &plan, roots).unwrap();
}

#[test]
fn importers_match_python_row_by_row() {
    let scratch = Scratch::new("oracle");
    let home = scratch.0.join("home");
    seed_home(&home);
    let ours = scratch.0.join("ours.sqlite");
    let theirs = scratch.0.join("theirs.sqlite");
    seed_db(&ours);
    seed_db(&theirs);
    let Some(_) = run_python(
        ORACLE,
        &[
            home.as_os_str(),
            theirs.as_os_str(),
            OsStr::new(&NOW.to_string()),
        ],
        &home,
    ) else {
        return;
    };
    let roots: HashMap<String, String> =
        serde_json::from_str(&std::fs::read_to_string(home.join("roots.json")).unwrap()).unwrap();
    let roots = MapRoots(roots);
    let mut seen = usage_import::ImportSeen::default();
    rust_cycle(&home, &ours, &mut seen, &roots);
    let first = dump(&ours);
    assert!(seen.files_read() >= 4, "{}", seen.files_read());
    // Lo importado no es trivial: turnos de las cuatro fuentes y tramos.
    let turns = first.first().unwrap().join("\n");
    for id in [
        "codex-resp-r3",
        "claude-jsonl-msg1:req1",
        "grok-update-ev-2",
        "opencode-db-m1",
        "claude-jsonl-u-sin-fecha",
    ] {
        assert!(turns.contains(id), "falta {id}:\n{turns}");
    }
    let spans = first.get(1).unwrap().join("\n");
    for id in ["claude-turn-dur-1", "codex-turn-t1", "codex-turn-t2"] {
        assert!(spans.contains(id), "falta {id}:\n{spans}");
    }
    assert!(
        !spans.contains("span-old") && !turns.contains("t:old|"),
        "poda"
    );
    let configs = first.get(2).unwrap().join("\n");
    for via in ["backfill:turn", "backfill:provider"] {
        assert!(configs.contains(via), "falta {via}:\n{configs}");
    }
    assert_eq!(
        first,
        dump(&PathBuf::from(format!("{}.1", theirs.display()))),
        "primera pasada"
    );
    let read = seen.files_read();
    rust_cycle(&home, &ours, &mut seen, &roots);
    assert_eq!(
        seen.files_read(),
        read,
        "la segunda pasada no relee transcripts"
    );
    assert_eq!(dump(&ours), dump(&theirs), "segunda pasada");
    assert!(seen.len() <= 6, "seen acotado al corte: {}", seen.len());
    let _ = SystemTime::now();
}

const ORACLE_EDGES: &str = r#"
import json, os, sys
repo, home, db, now = sys.argv[1], sys.argv[2], sys.argv[3], int(sys.argv[4])
sys.path[:0] = [os.path.join(repo, "bin"), os.path.join(repo, "lib")]
os.environ["HOME"] = home
import cc_usage
cc_usage.git_root_for_path = lambda p, run=None: p
out = {}
def attempt(name, fn):
    try:
        out[name] = fn()
    except Exception as e:
        out[name] = "raises:" + type(e).__name__
attempt("codex", lambda: cc_usage.record_local_codex_rollouts(db, now=now, max_age_days=21, max_files=1, seen={}))
attempt("claude", lambda: cc_usage.record_local_claude_jsonl(db, os.path.join(home, ".claude", "projects"), now=now,
        max_age_days=21, max_files=-1, account="main", seen={}))
attempt("grok", lambda: cc_usage.record_local_grok_updates(db, [os.path.join(home, ".grok")], now=now, max_age_days=21))
attempt("grok_meta", lambda: cc_usage.record_local_grok_updates(db, [os.path.join(home, ".grok2")], now=now, max_age_days=21))
attempt("codex_inf", lambda: cc_usage.record_local_codex_rollouts(db, homes=[("x", os.path.join(home, ".codex-inf"))],
        now=now, max_age_days=21, max_files=None, seen={}))
print(json.dumps(out, sort_keys=True))
"#;

#[test]
fn cuts_and_uncaught_errors_match_python() {
    let scratch = Scratch::new("edges");
    let home = scratch.0.join("home");
    let usage = |n: i64| {
        lines(&[
            json!({"type": "token_usage_record", "timestamp": NOW - 100 * n,
                       "payload": {"response_id": format!("e{n}"), "usage": {"input_tokens": n}}}),
        ])
    };
    // Corte por mtime: solo el más nuevo de Codex; Claude con `-1` deja fuera el más nuevo.
    for n in 1..=3 {
        put(
            &home.join(format!(".codex/sessions/rollout-{n}.jsonl")),
            &usage(n),
            (NOW - 100 * n) as f64,
        );
        put(
            &home.join(format!(".claude/projects/p/{n}.jsonl")),
            &lines(&[
                json!({"type": "assistant", "timestamp": NOW - 100 * n, "message": {"id": format!("c{n}"),
                            "usage": {"input_tokens": n}}}),
            ]),
            (NOW - 100 * n) as f64,
        );
    }
    // `summary.json` que no es objeto: `summary.get` lanza.
    put(
        &home.join(".grok/sessions/a/summary.json"),
        b"[1]",
        (NOW - 10) as f64,
    );
    put(
        &home.join(".grok/sessions/a/updates.jsonl"),
        b"{}\n",
        (NOW - 10) as f64,
    );
    // `_meta` verdadero que no es objeto: `.get` lanza.
    put(
        &home.join(".grok2/sessions/a/updates.jsonl"),
        &lines(&[
            json!({"timestamp": NOW - 10, "params": {"_meta": "x", "update": {"sessionUpdate": "turn_completed",
                        "usage": {"inputTokens": 1}}}}),
        ]),
        (NOW - 10) as f64,
    );
    // `Infinity` en un contador: `_as_int` lanza `OverflowError`.
    put(
        &home.join(".codex-inf/sessions/rollout-i.jsonl"),
        format!("{{\"type\": \"token_usage_record\", \"timestamp\": {}, \"payload\": {{\"response_id\": \"i\", \"usage\": {{\"input_tokens\": Infinity}}}}}}\n", NOW - 10).as_bytes(),
        (NOW - 10) as f64,
    );
    let ours = scratch.0.join("ours.sqlite");
    let theirs = scratch.0.join("theirs.sqlite");
    seed_db(&ours);
    seed_db(&theirs);
    let Some(out) = run_python(
        ORACLE_EDGES,
        &[
            home.as_os_str(),
            theirs.as_os_str(),
            OsStr::new(&NOW.to_string()),
        ],
        &home,
    ) else {
        return;
    };
    let conn = usage::open_usage_db_at(&ours).unwrap();
    let zone = chrono_tz::America::Mexico_City;
    let admit = |_: &Connection| true;
    let plan = |codex: Option<i64>, claude: Option<i64>, home: PathBuf| usage_import::ImportPlan {
        now: NOW,
        max_age_days: 21,
        claude_max_files: claude,
        codex_max_files: codex,
        home: Box::leak(home.into_boxed_path()),
        claude_projects_main: None,
        opencode_db: PathBuf::from("/no-existe"),
        zone: &zone,
        admit: &admit,
        cancelled: &|| false,
    };
    let roots = MapRoots(HashMap::new());
    let show = |r: usage_import::Result<usize>| match r {
        Ok(n) => json!(n),
        Err(usage_import::ImportError::Raises) => json!("raises"),
        Err(e) => panic!("{e}"),
    };
    let mut seen = usage_import::ImportSeen::default();
    let mut ours_out = Map::new();
    ours_out.insert(
        "codex".into(),
        show(usage_import::record_local_codex_rollouts(
            &conn,
            &plan(Some(1), None, home.clone()),
            &mut seen,
            &roots,
        )),
    );
    ours_out.insert(
        "claude".into(),
        show(usage_import::record_local_claude_jsonl(
            &conn,
            &plan(None, Some(-1), home.clone()),
            &home.join(".claude/projects"),
            "main",
            &mut seen,
            &roots,
        )),
    );
    let p = plan(None, None, home.clone());
    ours_out.insert(
        "grok".into(),
        show(usage_import::record_local_grok_updates(
            &conn,
            &p,
            &[home.join(".grok")],
            &roots,
        )),
    );
    ours_out.insert(
        "grok_meta".into(),
        show(usage_import::record_local_grok_updates(
            &conn,
            &p,
            &[home.join(".grok2")],
            &roots,
        )),
    );
    // `homes=[("x", .codex-inf)]`: un HOME cuyo `.codex` es esa carpeta.
    let inf_home = scratch.0.join("inf-home");
    std::fs::create_dir_all(&inf_home).unwrap();
    std::os::unix::fs::symlink(home.join(".codex-inf"), inf_home.join(".codex")).unwrap();
    let inf = usage_import::record_local_codex_rollouts(
        &conn,
        &plan(None, None, inf_home),
        &mut seen,
        &roots,
    );
    ours_out.insert("codex_inf".into(), show(inf));
    let mut theirs_out: Map<String, Value> = serde_json::from_str(out.trim()).unwrap();
    for v in theirs_out.values_mut() {
        if v.as_str().is_some_and(|s| s.starts_with("raises:")) {
            *v = json!("raises");
        }
    }
    assert_eq!(Value::Object(ours_out), Value::Object(theirs_out));
    assert_eq!(dump(&ours), dump(&theirs));
}
