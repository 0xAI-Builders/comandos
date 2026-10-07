//! SQL parity against frozen Python. Replay requires neither Python nor the
//! source checkout. Snapshots preserve schema, SQL types, bytes, REAL bits,
//! row order and user_version rather than SQLite CLI rendering conventions.
use comandos_store::usage;
use rusqlite::{Connection, OpenFlags};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    io::Write,
    os::unix::fs::DirBuilderExt,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{SystemTime, UNIX_EPOCH},
};

const SOURCE: &str = "2674f366bb01b9db42f6f64b728929b3acfe8b83:bin/cc_usage.py";
const SOURCE_SHA256: &str = "eef7368bf9172e29b3116168203e6236a3a2ee1be5348dbf2c78241dd4a2e5f8";

struct TempHome(PathBuf);
impl TempHome {
    fn new(tag: &str) -> Self {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "comandos-usage-{tag}-{}-{nanos}",
            std::process::id()
        ));
        std::fs::DirBuilder::new().mode(0o700).create(&dir).unwrap();
        Self(dir)
    }
    fn db(&self) -> PathBuf {
        self.0.join(".claude/hooks/comandos-usage.sqlite")
    }
}
impl Drop for TempHome {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

// Called exclusively inside try_oracle_at's record/check closure. A source
// removal or Python-free replay environment cannot change recorded expectations.
fn original_script(home: &Path) -> Result<PathBuf, String> {
    let source = Command::new("git")
        .args(["show", SOURCE])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .map_err(|e| format!("usage original source unavailable: {e}"))?;
    if !source.status.success() {
        return Err("usage original source unavailable".into());
    }
    if format!("{:x}", Sha256::digest(&source.stdout)) != SOURCE_SHA256 {
        return Err("usage original source checksum differs".into());
    }
    let directory = home.join(".oracle");
    std::fs::DirBuilder::new()
        .mode(0o700)
        .create(&directory)
        .map_err(|e| e.to_string())?;
    let script = directory.join("cc_usage.py");
    std::fs::write(&script, source.stdout).map_err(|e| e.to_string())?;
    Ok(script)
}

fn oracle(home: &Path, script: &Path, sub: &str, payload: &Value) -> Result<(), String> {
    let mut child = Command::new("python3")
        .arg(script)
        .arg(sub)
        .current_dir(home)
        .env_clear()
        .env("HOME", home)
        .env("PATH", "/usr/bin:/bin")
        .env("LANG", "C.UTF-8")
        .env("TZ", "America/Mexico_City")
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("XDG_DATA_HOME", home.join(".local/share"))
        .env("XDG_STATE_HOME", home.join(".local/state"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("usage Python oracle unavailable: {e}"))?;
    let input_result = child
        .stdin
        .take()
        .ok_or("usage oracle stdin unavailable")?
        .write_all(payload.to_string().as_bytes());
    // Reap even if stdin delivery fails; no owned subprocess is left behind.
    let out = child.wait_with_output().map_err(|e| e.to_string())?;
    input_result.map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(format!(
            "usage oracle {sub} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    Ok(())
}

fn native(home: &Path, sub: &str, payload: &Value) {
    let conn = usage::open_usage_db(home).unwrap();
    match sub {
        "capture-hook" => usage::capture_hook(&conn, payload),
        "lifecycle" => usage::lifecycle(&conn, payload),
        "tool-event" => usage::tool_event(&conn, payload),
        other => panic!("subcomando desconocido {other}"),
    }
    .unwrap();
}

fn execute_sql(db: &Path, sql: &str) -> Result<(), String> {
    // SQL fixture steps only amend an already initialized database. Do not let
    // a missing database become an accidental successful empty SQLite file.
    let conn = Connection::open_with_flags(db, OpenFlags::SQLITE_OPEN_READ_WRITE)
        .map_err(|e| e.to_string())?;
    conn.execute_batch(sql).map_err(|e| e.to_string())
}

fn snapshot(db: &Path) -> Result<Value, String> {
    // No path normalization: SQL TEXT/BLOB bytes must match exactly here.
    comandos_oracle::snapshot_sqlite(db, &[])
}

fn recorded(name: &str, input: &Value, run: impl FnOnce() -> Result<Value, String>) -> Value {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden");
    let bytes =
        comandos_oracle::try_oracle_at(&root, &format!("usage-sql-parity/{name}"), input, || {
            serde_json::to_vec(&run()?).map_err(|e| e.to_string())
        })
        .unwrap_or_else(|e| panic!("{e}"));
    serde_json::from_slice(&bytes).unwrap()
}

fn scenario(name: &str, fixture: &str) {
    let steps: Vec<Value> = serde_json::from_str(fixture).unwrap();
    let input = json!({
        "source": SOURCE,
        "source_sha256": SOURCE_SHA256,
        "fixture": fixture,
    });
    let expected = recorded(name, &input, || {
        let py = TempHome::new(&format!("{name}-py"));
        let script = original_script(&py.0)?;
        for step in &steps {
            if let Some(sql) = step["sql"].as_str() {
                execute_sql(&py.db(), sql)?;
            } else {
                let sub = step["run"].as_str().ok_or("fixture lacks command")?;
                oracle(&py.0, &script, sub, &step["payload"])?;
            }
        }
        snapshot(&py.db())
    });
    let rs = TempHome::new(&format!("{name}-rs"));
    for step in &steps {
        if let Some(sql) = step["sql"].as_str() {
            execute_sql(&rs.db(), sql).unwrap();
        } else {
            native(&rs.0, step["run"].as_str().unwrap(), &step["payload"]);
        }
    }
    assert_eq!(
        expected,
        snapshot(&rs.db()).unwrap(),
        "{name}: schema, SQL types/bytes, REAL bits, rows or user_version differ"
    );
}

#[test]
fn lifecycle_matches_python_oracle() {
    scenario("lifecycle", include_str!("fixtures/usage/lifecycle.json"));
}

#[test]
fn tool_event_matches_python_oracle() {
    scenario("tool-event", include_str!("fixtures/usage/tool_event.json"));
}

#[test]
fn capture_hook_matches_python_oracle() {
    scenario(
        "capture-hook",
        include_str!("fixtures/usage/capture_hook.json"),
    );
}

#[test]
fn capture_hook_without_numbers_creates_no_schema() {
    let payload = json!({"provider": "claude", "agent": "claude",
        "tmux_session": "sess", "tmux_pane": "%1", "input_tokens": 0, "output_tokens": 0,
        "cache_read_tokens": 0, "cache_write_tokens": 0, "cost_usd": 0,
        "turn_started_at": 1700000000, "turn_finished_at": 1700000001});
    let input = json!({
        "source": SOURCE,
        "source_sha256": SOURCE_SHA256,
        "run": "capture-hook",
        "payload": payload,
    });
    let expected = recorded("zero-schema", &input, || {
        let py = TempHome::new("zero-py");
        let script = original_script(&py.0)?;
        oracle(&py.0, &script, "capture-hook", &payload)?;
        let exists = py.db().try_exists().map_err(|e| e.to_string())?;
        let db = if exists {
            snapshot(&py.db())?
        } else {
            Value::Null
        };
        Ok(json!({"exists": exists, "db": db}))
    });
    assert_eq!(
        expected,
        json!({"exists": false, "db": null}),
        "Python no crea la base sin números de uso"
    );
    let rs = TempHome::new("zero-rs");
    native(&rs.0, "capture-hook", &payload);
    let conn = Connection::open_with_flags(rs.db(), OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    let count: i64 = conn
        .query_row("SELECT count(*) FROM sqlite_master", [], |row| row.get(0))
        .unwrap();
    assert_eq!(count, 0, "Rust creates an empty database without a schema");
    assert_eq!(
        snapshot(&rs.db()).unwrap(),
        json!({"schema": [], "tables": {}, "user_version": 0}),
        "The separate native zero case must remain schema-free"
    );
}
