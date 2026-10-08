#[path = "support/python.rs"]
mod python;
use comandos_runtime::extension_observations::conversation_usage;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        let path = std::env::temp_dir().join(comandos_runtime::fresh_id("ext-usage").unwrap());
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn compare(home: &Path, harness: &str, source: &Path, inventory: &Value, limit: usize) -> Value {
    let input = home.join("inventory.json");
    std::fs::write(&input, inventory.to_string()).unwrap();
    let script = r#"
import sys,pathlib,json
sys.path.insert(0,str(pathlib.Path(sys.argv[1])/'lib'))
from extension_observations import conversation_usage
print(json.dumps(conversation_usage(sys.argv[2],'conv',sys.argv[3],json.loads(pathlib.Path(sys.argv[4]).read_text()),max_bytes=int(sys.argv[5]))))
"#;
    let limit_string = limit.to_string();
    let oracle = python::run_python(
        script,
        &[
            harness.as_ref(),
            source.as_os_str(),
            input.as_os_str(),
            limit_string.as_ref(),
        ],
        home,
    );
    let result = conversation_usage(harness, "conv", Some(source), inventory, limit);
    if let Some(oracle) = oracle {
        assert_eq!(
            result,
            serde_json::from_str::<Value>(&oracle).unwrap(),
            "harness={harness}, limit={limit}"
        );
    }
    result
}
fn write_rows(path: &Path, rows: &[Value]) {
    std::fs::write(
        path,
        rows.iter()
            .map(Value::to_string)
            .collect::<Vec<_>>()
            .join("\n")
            + "\n",
    )
    .unwrap();
}
fn inventory() -> Value {
    json!({"mcps":[{"id":"m1","name":"server"},{"id":"m2","name":"unused"}],"skills":[{"id":"s1","name":"research","path":"/skills/research/SKILL.md"}]})
}

#[test]
fn five_harnesses_counts_and_honest_coverage_match_python() {
    let home = Scratch::new();
    let source = home.0.join("history.jsonl");
    let inv = inventory();
    let claude = vec![
        json!({"type":"user"}),
        json!({"type":"assistant","message":{"content":[{"type":"tool_use","id":"same","name":"mcp__server__search"},{"type":"tool_use","id":"same","name":"mcp__server__search"},{"type":"tool_use","id":"skill","name":"Skill","input":{"skill":"research"}}]}}),
        json!({"sessionId":"foreign","type":"assistant","message":{"content":[{"type":"tool_use","name":"mcp__unused__x"}]}}),
    ];
    write_rows(&source, &claude);
    let full = compare(&home.0, "claude", &source, &inv, 2_000_000);
    assert_eq!(full["counts"]["mcps"], json!({"m1":1,"m2":0}));
    assert_eq!(full["complete"], true);
    compare(&home.0, "claude", &source, &inv, 150);
    let codex = vec![
        json!({"type":"session_meta","payload":{"id":"conv"}}),
        json!({"type":"event_msg","payload":{"type":"user_message"}}),
        json!({"type":"response_item","payload":{"type":"function_call","name":"mcp__server__x","arguments":"{}","call_id":"a"}}),
    ];
    write_rows(&source, &codex);
    let r = compare(&home.0, "codex", &source, &inv, 2_000_000);
    assert!(r["counts"]["mcps"]["m2"].is_null());
    write_rows(
        &source,
        &[json!({"type":"session_meta","payload":{"id":"foreign"}})],
    );
    compare(&home.0, "codex", &source, &inv, 2_000_000);
    write_rows(
        &source,
        &[
            json!({"type":"user_message"}),
            json!({"type":"mcp_tool_call_completed","data":{"server_name":"server","tool_name":"search","tool_call_id":"x"}}),
            json!({"type":"tool_call","data":{"name":"read_file","arguments":{"path":"/skills/research/SKILL.md"}}}),
        ],
    );
    compare(&home.0, "grok", &source, &inv, 2_000_000);
    write_rows(
        &source,
        &[
            json!({"source":"USER","tool_calls":[{"name":"mcp__server__x"},{"name":"call_mcp_tool","args":"{\"ServerName\":\"unused\"}"}]}),
        ],
    );
    compare(&home.0, "agy", &source, &inv, 2_000_000);
    let db_path = home.0.join("opencode.db");
    let db = rusqlite::Connection::open(&db_path).unwrap();
    db.execute_batch(
        "PRAGMA journal_mode=WAL;CREATE TABLE part(id TEXT,session_id TEXT,data TEXT)",
    )
    .unwrap();
    for (id, session, data) in [
        (
            "p1",
            "conv",
            json!({"type":"tool","tool":"server_search","callID":"x","state":{"status":"completed"}}),
        ),
        (
            "p2",
            "conv",
            json!({"type":"tool","tool":"server_search","callID":"x","state":{"status":"running"}}),
        ),
        (
            "p3",
            "foreign",
            json!({"type":"tool","tool":"unused_x","state":{"status":"completed"}}),
        ),
    ] {
        db.execute(
            "INSERT INTO part VALUES(?,?,?)",
            rusqlite::params![id, session, data.to_string()],
        )
        .unwrap();
    }
    let r = compare(&home.0, "opencode", &db_path, &inv, 2_000_000);
    assert_eq!(r["counts"]["mcps"]["m1"], 1);
    assert_eq!(r["complete"], true);
    db.execute(
        "INSERT INTO part VALUES('p4','conv',?)",
        [json!({"type":"tool","tool":"unused_x","state":{"status":"pending"}}).to_string()],
    )
    .unwrap();
    assert_eq!(
        compare(&home.0, "opencode", &db_path, &inv, 2_000_000)["complete"],
        false
    );
    compare(&home.0, "opencode", &db_path, &inv, 20);
}

#[test]
fn malformed_ambiguous_and_missing_sources_never_claim_unused() {
    let home = Scratch::new();
    let source = home.0.join("history.jsonl");
    let inv = inventory();
    for body in [
        "{\"type\":\"user\"}\nnot-json\n",
        "{\"type\":\"user\"}\n[]\n",
        "{\"type\":\"user\"}\n{\"type\":\"assistant\",\"message\":{\"content\":[{\"type\":\"tool_use\",\"name\":\"mcp__broken\"}]}}\n",
    ] {
        std::fs::write(&source, body).unwrap();
        assert_eq!(
            compare(&home.0, "claude", &source, &inv, 2_000_000)["complete"],
            false
        );
    }
    write_rows(
        &source,
        &[
            json!({"type":"user"}),
            json!({"type":"assistant","message":{"content":[{"type":"tool_use","name":"mcp__server__x"}]}}),
        ],
    );
    let dup = json!({"mcps":[{"id":"a","name":"server"},{"id":"b","name":"server"}],"skills":[]});
    assert_eq!(
        compare(&home.0, "claude", &source, &dup, 2_000_000)["complete"],
        false
    );
    compare(&home.0, "claude", &home.0.join("missing"), &inv, 2_000_000);
    compare(&home.0, "unsupported", &source, &inv, 2_000_000);
    std::fs::write(&source, b"{\"type\":\"user\"}\n\xff\n").unwrap();
    compare(&home.0, "claude", &source, &inv, 2_000_000);
}

#[test]
fn malformed_nested_records_and_line_endings_match_python() {
    let home = Scratch::new();
    let source = home.0.join("history.jsonl");
    let inv = inventory();
    for content in [
        json!(false),
        json!(0),
        json!(true),
        json!(1),
        json!("ignored"),
        json!({}),
        json!([]),
        Value::Null,
    ] {
        write_rows(
            &source,
            &[
                json!({"type":"user"}),
                json!({"type":"assistant","message":{"content":content}}),
                json!({"type":"assistant","message":{"content":[{"type":"tool_use","name":"mcp__server__search","id":"a"}]}}),
            ],
        );
        compare(&home.0, "claude", &source, &inv, 2_000_000);
    }
    for eol in ["\r", "\r\n", "\n"] {
        let body=[json!({"type":"user"}),json!({"type":"assistant","message":{"content":[{"type":"tool_use","name":"mcp__server__search","id":"a"}]}})].iter().map(Value::to_string).collect::<Vec<_>>().join(eol)+eol;
        std::fs::write(&source, body).unwrap();
        compare(&home.0, "claude", &source, &inv, 2_000_000);
    }
    let db_path = home.0.join("opencode.db");
    let db = rusqlite::Connection::open(&db_path).unwrap();
    db.execute_batch("CREATE TABLE part(id TEXT,session_id TEXT,data TEXT)")
        .unwrap();
    db.execute(
        "INSERT INTO part VALUES('p1','conv',?)",
        [
            json!({"type":"tool","tool":"foo_bar_search","state":{"status":"completed"}})
                .to_string(),
        ],
    )
    .unwrap();
    let ambiguous =
        json!({"mcps":[{"id":"a","name":"foo"},{"id":"b","name":"foo.bar"}],"skills":[]});
    assert_eq!(
        compare(&home.0, "opencode", &db_path, &ambiguous, 2_000_000)["complete"],
        false
    );
}
