//! `PaneInspector`, `agent_pane_maps` y `account_for_pid` contra el Python
//! sobre un /proc falso.
#[path = "support/python.rs"]
mod python;

use base64::Engine as _;
use comandos_core::json::response_dumps;
use comandos_runtime::{
    agent_procs::{
        AccountCache, AgentProc, PaneRow, agent_pane_maps, agent_procs, parent_pid,
        parse_pane_inventory, proc_cmdline, process_owners, process_start, read_environ,
    },
    pane_snapshot::{PaneInspector, PaneRef},
};
use python::run_python;
use serde_json::{Value, json};
use std::{collections::HashMap, fs, os::unix::fs::symlink, path::Path};

const INSPECTOR: &str = r#"
import json, os, sys
repo, home, proc, panes = sys.argv[1:5]
sys.path.insert(0, os.path.join(repo, "lib"))
import pane_snapshot
inspector = pane_snapshot.PaneInspector(home=home, proc_root=proc)
print(json.dumps([inspector(p) for p in json.load(open(panes))]))
"#;

const LOAD_DASH: &str = r#"
import importlib.machinery, importlib.util, json, os, sys, types
repo = sys.argv[1]
sys.path.insert(0, os.path.join(repo, "bin"))
loader = importlib.machinery.SourceFileLoader("cc_dash_oracle", os.path.join(repo, "bin/cc-dash"))
spec = importlib.util.spec_from_loader(loader.name, loader)
dash = importlib.util.module_from_spec(spec)
loader.exec_module(dash)
"#;

const MAPS: &str = r#"
case = sys.argv[2]
c = json.load(open(case))
dash.parent_pid = lambda pid: c["parents"].get(str(pid), 0)
dash._proc_cmdline = lambda pid: c["cmdlines"].get(str(pid), [])
by_session, by_cwd = dash.agent_pane_maps([tuple(p) for p in c["procs"]], c["panes"])
print(json.dumps({"bySession": by_session, "byCwd": by_cwd}))
"#;

const INVENTORY: &str = r#"
text = open(sys.argv[2]).read()
dash.tmux = lambda *a, **k: types.SimpleNamespace(returncode=0, stdout=text)
# `activity` es 0 (int) tras un `ValueError` y 0.0 con texto vacío; las dos son
# falsas para `read_states` y la fila de Rust es `f64`.
print(json.dumps([dict(r, activity=float(r["activity"])) for r in dash.tmux_pane_inventory()]))
"#;

// Solo se cambia la raíz de /proc de los dos lectores; el resto es el Python.
const ACCOUNTS: &str = r#"
proc, case = sys.argv[2:4]
def start(pid):
    try:
        with open(f"{proc}/{int(pid)}/stat") as fh:
            return fh.read().rsplit(")", 1)[1].split()[19]
    except (OSError, ValueError, IndexError):
        return ""
def environ(pid):
    try:
        with open(f"{proc}/{int(pid)}/environ", "rb") as f:
            raw = f.read()
        return dict(x.split(b"=", 1) for x in raw.split(b"\0") if b"=" in x)
    except Exception:
        return {}
dash._process_start = start
dash._read_environ = environ
print(json.dumps([dash.account_for_pid(pid, agent) for pid, agent in json.load(open(case))]))
"#;

fn process(proc: &Path, pid: i64, ppid: i64, argv: &[&str], environ: &[&str]) {
    let dir = proc.join(pid.to_string());
    fs::create_dir_all(dir.join("fd")).unwrap();
    fs::write(
        dir.join("stat"),
        format!("{pid} (x y) S {ppid} 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 4242 0"),
    )
    .unwrap();
    fs::write(dir.join("cmdline"), argv.join("\0") + "\0").unwrap();
    fs::write(dir.join("environ"), environ.join("\0") + "\0").unwrap();
}

fn scratch(tag: &str) -> std::path::PathBuf {
    let root = std::env::temp_dir().join(format!("cmd-{tag}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    root
}

#[test]
fn inspector_matches_python_oracle() {
    let root = scratch("insp");
    let (home, proc) = (root.join("home"), root.join("proc"));
    fs::create_dir_all(home.join(".claude/sessions")).unwrap();
    fs::create_dir_all(home.join(".claude-accounts/work/sessions")).unwrap();
    fs::create_dir_all(home.join(".claude/hooks/native-processes")).unwrap();
    fs::create_dir_all(home.join(".codex/sessions")).unwrap();
    fs::create_dir_all(home.join(".grok")).unwrap();
    fs::write(
        home.join(".claude/sessions/a.json"),
        r#"{"pid": 101, "sessionId": "s-main"}"#,
    )
    .unwrap();
    fs::write(
        home.join(".claude-accounts/work/sessions/b.json"),
        r#"{"pid": 102.0, "sessionId": "s-work"}"#,
    )
    .unwrap();
    // Una cadena como pid nunca casa con el entero.
    fs::write(
        home.join(".claude/sessions/c.json"),
        r#"{"pid": "103", "sessionId": "s-str"}"#,
    )
    .unwrap();
    fs::write(
        home.join(".grok/active_sessions.json"),
        r#"[{"pid": 104, "session_id": "g-1"}, 5, {"pid": 0, "session_id": "g-0"}]"#,
    )
    .unwrap();
    fs::write(
        home.join(".claude/hooks/acp-panes.json"),
        r#"{"%9": {"pid": 401, "agent": "codex", "model": "gpt-5", "sessionId": "x"}}"#,
    )
    .unwrap();
    // Registro nativo de opencode que casa con su proceso.
    fs::write(
        home.join(".claude/hooks/native-processes/105.json"),
        r#"{"pid": 105, "start": "4242", "harness": "opencode", "sessionId": "ses_1", "model": "m", "busy": true}"#,
    )
    .unwrap();
    // claude con sesión, claude de otra cuenta, shell, codex con rollout raíz y
    // uno delegado, cc-acp, grok, opencode, agy y un claude con pid de cadena.
    process(&proc, 100, 1, &["zsh"], &[]);
    process(
        &proc,
        101,
        100,
        &[
            "/usr/bin/claude",
            "--model",
            "opus",
            "--dangerously-skip-permissions",
        ],
        &[],
    );
    process(&proc, 102, 1, &["claude", "-m", "--effort"], &[]);
    process(&proc, 103, 1, &["claude", "-c", "x"], &[]);
    process(&proc, 104, 1, &["grok", "-m", "grok-4", "--yolo"], &[]);
    process(
        &proc,
        105,
        1,
        &["opencode", "-s", "ses_2", "--variant", "high"],
        &[],
    );
    process(&proc, 106, 1, &["agy", "--conversation=conv-1"], &[]);
    process(&proc, 107, 1, &["agy"], &[]);
    process(&proc, 200, 1, &["node", "/x/codex.js"], &[]);
    process(
        &proc,
        201,
        200,
        &[
            "codex",
            "--yolo",
            "--no-daemon",
            "-c",
            "model_reasoning_effort=high",
        ],
        &["CODEX_HOME=/c"],
    );
    process(
        &proc,
        202,
        1,
        &["codex", "resume", "0f0f0f0f-0000-4000-8000-00000000000a"],
        &[],
    );
    let root_rollout =
        home.join(".codex/sessions/rollout-2026-0f0f0f0f-0000-4000-8000-000000000001.jsonl");
    fs::write(&root_rollout, "{\"type\":\"session_meta\",\"payload\":{\"id\":\"0f0f0f0f-0000-4000-8000-000000000001\",\"source\":\"cli\"}}\n").unwrap();
    let sub = home.join(".codex/sessions/rollout-2026-0f0f0f0f-0000-4000-8000-000000000002.jsonl");
    fs::write(&sub, "{\"type\":\"session_meta\",\"payload\":{\"id\":\"0f0f0f0f-0000-4000-8000-000000000002\",\"source\":{\"subagent\":{}}}}\n").unwrap();
    symlink(&root_rollout, proc.join("201/fd/7")).unwrap();
    symlink(&sub, proc.join("201/fd/8")).unwrap();
    symlink(
        "/h/.gemini/antigravity-cli/presence/lock-1.lock",
        proc.join("107/fd/3"),
    )
    .unwrap();
    process(&proc, 400, 1, &["cc-acp"], &[]);
    process(&proc, 401, 400, &["codex-acp"], &[]);
    let panes = json!([
        {"id":"%1","pid":100,"command":"node"},
        {"id":"%2","pid":102,"command":"claude"},
        {"id":"%3","pid":100,"command":"zsh"},
        {"id":"%4","pid":200,"command":"node"},
        {"id":"%9","pid":400,"command":"cc-acp"},
        {"id":"%8","pid":999,"command":"vim"},
        {"id":"%10","pid":103,"command":"claude"},
        {"id":"%11","pid":104,"command":"grok"},
        {"id":"%12","pid":105,"command":"opencode"},
        {"id":"%13","pid":106,"command":"agy"},
        {"id":"%14","pid":107,"command":"agy"},
        {"id":"%15","pid":202,"command":"codex"},
        {"id":"%16","pid":400,"command":"cc-acp"}
    ]);
    let file = root.join("panes.json");
    fs::write(&file, panes.to_string()).unwrap();
    let Some(expected) = run_python(
        INSPECTOR,
        &[home.as_os_str(), proc.as_os_str(), file.as_os_str()],
        &home,
    ) else {
        return;
    };
    let inspector = PaneInspector::new(&home, &proc).unwrap();
    let got: Vec<Value> = panes
        .as_array()
        .unwrap()
        .iter()
        .map(|p| {
            Value::Object(
                inspector
                    .inspect(&PaneRef {
                        id: p["id"].as_str().unwrap(),
                        pid: p["pid"].as_i64().unwrap(),
                        command: p["command"].as_str().unwrap(),
                    })
                    .unwrap(),
            )
        })
        .collect();
    assert_eq!(
        response_dumps(&Value::Array(got.clone())).unwrap(),
        expected.trim_end()
    );
    // El heredado vivo guarda `--yolo` de codex como su forma larga y conserva `--no-daemon`.
    assert_eq!(
        got[3]["flags"],
        json!([
            "--dangerously-bypass-approvals-and-sandbox",
            "--no-daemon",
            "-c",
            "model_reasoning_effort=high"
        ])
    );
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn inspector_declines_where_python_raises() {
    let root = scratch("insp-raise");
    let (home, proc) = (root.join("home"), root.join("proc"));
    fs::create_dir_all(home.join(".claude/sessions")).unwrap();
    fs::create_dir_all(home.join(".claude/hooks")).unwrap();
    fs::create_dir_all(&proc).unwrap();
    process(&proc, 400, 1, &["cc-acp"], &[]);
    // `acp-panes.json` que no es un objeto: `AttributeError` en el pane cc-acp.
    fs::write(home.join(".claude/hooks/acp-panes.json"), "[1]").unwrap();
    let inspector = PaneInspector::new(&home, &proc).unwrap();
    let acp = PaneRef {
        id: "%1",
        pid: 400,
        command: "cc-acp",
    };
    assert!(inspector.inspect(&acp).is_err());
    assert!(
        inspector
            .inspect(&PaneRef {
                command: "zsh",
                ..acp
            })
            .is_ok()
    );
    // Un pid de lista no es clave de `dict`: `TypeError` al construir.
    fs::write(
        home.join(".claude/sessions/a.json"),
        r#"{"pid": [1], "sessionId": "s"}"#,
    )
    .unwrap();
    assert!(PaneInspector::new(&home, &proc).is_err());
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn lone_surrogates_only_decline_when_emitted() {
    let root = scratch("insp-surr");
    let (home, proc) = (root.join("home"), root.join("proc"));
    fs::create_dir_all(home.join(".claude/sessions")).unwrap();
    fs::create_dir_all(home.join(".claude/hooks")).unwrap();
    fs::create_dir_all(home.join(".grok")).unwrap();
    // Sustitutos sueltos en campos que nunca llegan a la salida.
    fs::write(
        home.join(".claude/sessions/a.json"),
        r#"{"pid": 101, "sessionId": "s-main", "cwd": "/w/\udc00"}"#,
    )
    .unwrap();
    fs::write(
        home.join(".grok/active_sessions.json"),
        r#"[{"pid": 104, "session_id": "g-1", "x": "\ud800"}]"#,
    )
    .unwrap();
    fs::write(
        home.join(".claude/hooks/acp-panes.json"),
        r#"{"%9": {"pid": 401, "agent": "codex", "note": "\udfff"}, "%7": {"pid": 402, "model": "m\ud800"}}"#,
    )
    .unwrap();
    process(&proc, 101, 1, &["claude"], &[]);
    process(&proc, 104, 1, &["grok"], &[]);
    process(&proc, 400, 1, &["cc-acp"], &[]);
    process(&proc, 401, 400, &["codex-acp"], &[]);
    process(&proc, 410, 1, &["cc-acp"], &[]);
    process(&proc, 402, 410, &["codex-acp"], &[]);
    let panes = json!([
        {"id":"%1","pid":101,"command":"claude"},
        {"id":"%11","pid":104,"command":"grok"},
        {"id":"%9","pid":400,"command":"cc-acp"}
    ]);
    let file = root.join("panes.json");
    fs::write(&file, panes.to_string()).unwrap();
    let Some(expected) = run_python(
        INSPECTOR,
        &[home.as_os_str(), proc.as_os_str(), file.as_os_str()],
        &home,
    ) else {
        return;
    };
    let inspector = PaneInspector::new(&home, &proc).unwrap();
    let got: Vec<Value> = panes
        .as_array()
        .unwrap()
        .iter()
        .map(|p| {
            Value::Object(
                inspector
                    .inspect(&PaneRef {
                        id: p["id"].as_str().unwrap(),
                        pid: p["pid"].as_i64().unwrap(),
                        command: p["command"].as_str().unwrap(),
                    })
                    .unwrap(),
            )
        })
        .collect();
    assert_eq!(
        response_dumps(&Value::Array(got)).unwrap(),
        expected.trim_end()
    );
    // El modelo de `%7` sí se emitiría con su sustituto: declina ese pane.
    let emitted = PaneRef {
        id: "%7",
        pid: 410,
        command: "cc-acp",
    };
    assert!(inspector.inspect(&emitted).is_err());
    // Un `sessionId` con sustituto sería el `resume_id` de la tarjeta.
    fs::write(
        home.join(".claude/sessions/b.json"),
        r#"{"pid": 102, "sessionId": "s\udc00"}"#,
    )
    .unwrap();
    assert!(PaneInspector::new(&home, &proc).is_err());
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn agent_pane_maps_match_python_oracle() {
    let root = scratch("maps");
    // Copia del caso de tests/test_live_pane_inventory.py, con un envoltorio node → codex.
    let case = json!({
        "panes": [
            {"session":"mixed","pane":"%1","pane_pid":100,"command":"node","cwd":"/same","activity":50.0,"paneActive":true},
            {"session":"mixed","pane":"%2","pane_pid":200,"command":"claude","cwd":"/same","activity":50.0,"paneActive":false},
            {"session":"other","pane":"%5","pane_pid":500,"command":"node","cwd":"/same","activity":40.0,"paneActive":false},
            {"session":"terminal","pane":"%6","pane_pid":600,"command":"zsh","cwd":"/shell","activity":30.0,"paneActive":false},
            {"session":"blank","pane":"%7","pane_pid":700,"command":"node","cwd":"/fallback","activity":1.0,"paneActive":false}
        ],
        "procs": [[101,"/same","codex"],[102,"/same","codex"],[201,"/same","claude"],[501,"/same","codex"],[601,"/shell","claude"],[701,"","grok"],[702,"/x","grok"]],
        "parents": {"101":100,"102":101,"201":200,"501":500,"601":600,"701":700,"702":701},
        "cmdlines": {"101":["node","/x/codex.js"],"102":["codex"],"201":["claude"],"501":["codex"],"701":["bun","grok"],"702":["node","grok"]}
    });
    let file = root.join("case.json");
    fs::write(&file, case.to_string()).unwrap();
    let Some(expected) = run_python(&format!("{LOAD_DASH}{MAPS}"), &[file.as_os_str()], &root)
    else {
        return;
    };
    let panes: Vec<PaneRow> = case["panes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| PaneRow {
            session: p["session"].as_str().unwrap().into(),
            pane: p["pane"].as_str().unwrap().into(),
            pane_pid: p["pane_pid"].as_i64().unwrap(),
            command: p["command"].as_str().unwrap().into(),
            cwd: p["cwd"].as_str().unwrap().into(),
            activity: p["activity"].as_f64().unwrap(),
            pane_active: p["paneActive"].as_bool().unwrap(),
        })
        .collect();
    let procs: Vec<AgentProc> = case["procs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| AgentProc {
            pid: p[0].as_i64().unwrap(),
            cwd: p[1].as_str().unwrap().into(),
            agent: p[2].as_str().unwrap().into(),
        })
        .collect();
    let parents: HashMap<i64, i64> = case["parents"]
        .as_object()
        .unwrap()
        .iter()
        .map(|(k, v)| (k.parse().unwrap(), v.as_i64().unwrap()))
        .collect();
    let cmdlines = case["cmdlines"].clone();
    let mut parent = |pid: i64| parents.get(&pid).copied().unwrap_or(0);
    let owners = process_owners(&procs, &panes, &mut parent);
    let mut cmdline = |pid: i64| -> Vec<String> {
        serde_json::from_value(cmdlines[pid.to_string()].clone()).unwrap_or_default()
    };
    let maps = agent_pane_maps(&procs, &panes, &owners, &mut cmdline, &mut parent);
    let object = |rows: Vec<(String, Value)>| Value::Object(rows.into_iter().collect());
    let got = json!({
        "bySession": object(maps.by_session.iter().map(|(k, i)| (k.clone(), Value::Object(i.to_obs()))).collect()),
        "byCwd": object(maps.by_cwd.iter().map(|(k, rows)| (k.clone(), Value::Array(rows.iter().map(|i| Value::Object(i.to_obs())).collect()))).collect()),
    });
    assert_eq!(response_dumps(&got).unwrap(), expected.trim_end());
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn inventory_parses_like_python() {
    let rows = parse_pane_inventory(
        "s|%1|12|zsh|/a|1791115200|1|1\nbad|%x|1|a|b|c|d|e\ns|%2|7|claude|/a|nan?|0|1\ns|%3|8\n",
    );
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[0].activity, 1_791_115_200.0);
    assert!(rows[0].pane_active);
    assert_eq!(rows[1].activity, 0.0);
    assert_eq!((rows[2].command.as_str(), rows[2].cwd.as_str()), ("", ""));
}

#[test]
fn inventory_matches_python_oracle() {
    let root = scratch("inv");
    let text = "s|%1|12|zsh|/a|b|c|1791115200|1|1\nbad|%x|1|a|b|c|d|e\ns.x|%2|7|claude|/a|nan?|0|1\r\n\
                s|%3|8\nt|%44|9|node|/b|1_5|1|0\nu|%5|x|node\n|%6|3\nv|%7|0012|a||  12.5 |1|1\nw|%12345678|4\n";
    let file = root.join("inventory.txt");
    fs::write(&file, text).unwrap();
    let Some(expected) = run_python(
        &format!("{LOAD_DASH}{INVENTORY}"),
        &[file.as_os_str()],
        &root,
    ) else {
        return;
    };
    let got: Vec<Value> = parse_pane_inventory(text)
        .into_iter()
        .map(|r| {
            json!({
                "session": r.session, "pane": r.pane, "pane_pid": r.pane_pid,
                "command": r.command, "cwd": r.cwd, "activity": r.activity,
                "paneActive": r.pane_active,
            })
        })
        .collect();
    assert_eq!(
        response_dumps(&Value::Array(got)).unwrap(),
        expected.trim_end()
    );
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn account_for_pid_matches_python_oracle() {
    let root = scratch("acct");
    let (home, proc) = (root.join("home"), root.join("proc"));
    let h = home.display().to_string();
    fs::create_dir_all(home.join(".claude")).unwrap();
    fs::create_dir_all(home.join(".claude-accounts/work")).unwrap();
    fs::create_dir_all(home.join(".claude-accounts/bare")).unwrap();
    fs::create_dir_all(home.join("codex2")).unwrap();
    fs::create_dir_all(home.join("g2")).unwrap();
    symlink(home.join(".claude"), home.join(".claude-accounts/alias")).unwrap();
    fs::write(
        home.join(".claude.json"),
        r#"{"oauthAccount": {"emailAddress": "main@x"}}"#,
    )
    .unwrap();
    fs::write(
        home.join(".claude-accounts/work/.claude.json"),
        r#"{"oauthAccount": {"emailAddress": "work@x"}}"#,
    )
    .unwrap();
    let claims = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(r#"{"email":"c@x"}"#);
    fs::write(
        home.join("codex2/auth.json"),
        format!(r#"{{"tokens": {{"id_token": "h.{claims}.s"}}}}"#),
    )
    .unwrap();
    fs::write(
        home.join("g2/auth.json"),
        r#"{"a": 1, "b": {"user_id": "u1"}}"#,
    )
    .unwrap();
    let env = |k: &str, v: &str| format!("{k}={v}");
    process(&proc, 10, 1, &["claude"], &["PATH=/bin"]);
    process(
        &proc,
        11,
        1,
        &["claude"],
        &[&env(
            "CLAUDE_CONFIG_DIR",
            &format!("{h}/.claude-accounts/work/"),
        )],
    );
    process(
        &proc,
        12,
        1,
        &["claude"],
        &[&env(
            "CLAUDE_CONFIG_DIR",
            &format!("{h}/.claude-accounts/bare"),
        )],
    );
    process(
        &proc,
        13,
        1,
        &["claude"],
        &[&env(
            "CLAUDE_CONFIG_DIR",
            &format!("{h}/.claude-accounts/alias"),
        )],
    );
    process(
        &proc,
        14,
        1,
        &["codex"],
        &[&env("CODEX_HOME", &format!("{h}/codex2"))],
    );
    process(&proc, 15, 1, &["codex"], &[]);
    process(
        &proc,
        16,
        1,
        &["grok"],
        &[&env("GROK_HOME", &format!("{h}/g2"))],
    );
    process(&proc, 17, 1, &["claude"], &["CLAUDE_CONFIG_DIR="]);
    let case = json!([
        [10, "claude"],
        [11, "claude"],
        [12, "claude"],
        [13, "claude"],
        [14, "codex"],
        [15, "codex"],
        [16, "grok"],
        [17, "claude"],
        [0, "claude"],
        [10, "opencode"],
        [99, "claude"],
        [11, "claude"]
    ]);
    let file = root.join("case.json");
    fs::write(&file, case.to_string()).unwrap();
    let Some(expected) = run_python(
        &format!("{LOAD_DASH}{ACCOUNTS}"),
        &[proc.as_os_str(), file.as_os_str()],
        &home,
    ) else {
        return;
    };
    let mut cache = AccountCache::default();
    let got: Vec<Value> = case
        .as_array()
        .unwrap()
        .iter()
        .map(|c| {
            Value::Object(
                cache
                    .account_for_pid(&home, &proc, c[0].as_i64().unwrap(), c[1].as_str().unwrap())
                    .unwrap(),
            )
        })
        .collect();
    assert_eq!(
        response_dumps(&Value::Array(got)).unwrap(),
        expected.trim_end()
    );
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn agent_procs_reads_proc_like_python() {
    let root = scratch("procs");
    let proc = root.join("proc");
    process(&proc, 300, 1, &["/usr/bin/claude", "--x"], &[]);
    // `argv[:3]` antes de quitar vacíos: el tercero cuenta, el cuarto no.
    process(&proc, 301, 1, &["", "x", "codex"], &[]);
    process(&proc, 302, 1, &["a", "b", "c", "codex"], &[]);
    process(&proc, 303, 1, &["claude"], &[]);
    process(&proc, 304, 1, &["ghost", "claude"], &[]);
    symlink("/w/a", proc.join("300/cwd")).unwrap();
    symlink("/w/b", proc.join("301/cwd")).unwrap();
    symlink("/w/c", proc.join("302/cwd")).unwrap();
    symlink("/w/e", proc.join("304/cwd")).unwrap();
    fs::create_dir_all(proc.join("self")).unwrap();
    let aliases: HashMap<String, String> =
        [("claude", "claude"), ("codex", "codex"), ("ghost", "")]
            .into_iter()
            .map(|(k, v)| (k.to_owned(), v.to_owned()))
            .collect();
    let mut got = agent_procs(&proc, &aliases).unwrap();
    got.sort_by_key(|p| p.pid);
    let want = vec![
        AgentProc {
            pid: 300,
            cwd: "/w/a".into(),
            agent: "claude".into(),
        },
        AgentProc {
            pid: 301,
            cwd: "/w/b".into(),
            agent: "codex".into(),
        },
    ];
    assert_eq!(got, want);
    assert_eq!(parent_pid(&proc, 300), 1);
    assert_eq!(parent_pid(&proc, 999), 0);
    assert_eq!(
        proc_cmdline(&proc, 301),
        vec!["x".to_owned(), "codex".to_owned()]
    );
    assert_eq!(process_start(&proc, 300), "4242");
    assert_eq!(process_start(&proc, 999), "");
    fs::write(proc.join("300/environ"), b"A=1\0B\0A=2=3\0").unwrap();
    let env = read_environ(&proc, 300);
    assert_eq!(
        env.get(b"A".as_slice()).map(Vec::as_slice),
        Some(b"2=3".as_slice())
    );
    assert_eq!(env.len(), 1);
    // Un `cwd` que no es UTF-8: el Python lo guardaría con `surrogateescape`.
    use std::os::unix::ffi::OsStrExt;
    symlink(
        std::ffi::OsStr::from_bytes(b"/w/\xff"),
        proc.join("303/cwd"),
    )
    .unwrap();
    assert!(agent_procs(&proc, &aliases).is_err());
    let _ = fs::remove_dir_all(&root);
}
