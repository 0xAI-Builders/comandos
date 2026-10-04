//! Paridad SQL con el oráculo `bin/cc_usage.py`: cada escenario corre igual en
//! dos HOME temporales (uno con Python, otro con Rust) y los volcados de ambas
//! bases deben coincidir línea a línea, incluido `pragma user_version`.
use comandos_store::usage;
use serde_json::Value;
use std::{
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{SystemTime, UNIX_EPOCH},
};

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
        std::fs::create_dir_all(&dir).unwrap();
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

fn oracle(home: &Path, sub: &str, payload: &Value) {
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../bin/cc_usage.py");
    let mut child = Command::new("python3")
        .arg(script)
        .arg(sub)
        .env("HOME", home)
        .env_remove("COMANDOS_USAGE_DB")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(payload.to_string().as_bytes())
        .unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(
        out.status.success(),
        "oráculo {sub} falló: {}",
        String::from_utf8_lossy(&out.stderr)
    );
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

fn sqlite3(db: &Path, sql: &str) -> String {
    let out = Command::new("sqlite3").arg(db).arg(sql).output().unwrap();
    assert!(
        out.status.success(),
        "sqlite3 falló: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap()
}

fn assert_same_dump(py: &Path, rs: &Path) {
    let (a, b) = (sqlite3(py, ".dump"), sqlite3(rs, ".dump"));
    let (a, b): (Vec<_>, Vec<_>) = (a.lines().collect(), b.lines().collect());
    for (i, (x, y)) in a.iter().zip(&b).enumerate() {
        assert_eq!(x, y, "volcado distinto en la línea {}", i + 1);
    }
    assert_eq!(a.len(), b.len(), "volcados de distinto largo");
    assert_eq!(
        sqlite3(py, "pragma user_version"),
        sqlite3(rs, "pragma user_version")
    );
}

fn scenario(name: &str, fixture: &str) {
    let steps: Vec<Value> = serde_json::from_str(fixture).unwrap();
    let (py, rs) = (
        TempHome::new(&format!("{name}-py")),
        TempHome::new(&format!("{name}-rs")),
    );
    for step in &steps {
        if let Some(sql) = step["sql"].as_str() {
            sqlite3(&py.db(), sql);
            sqlite3(&rs.db(), sql);
            continue;
        }
        let sub = step["run"].as_str().unwrap();
        oracle(&py.0, sub, &step["payload"]);
        native(&rs.0, sub, &step["payload"]);
    }
    assert_same_dump(&py.db(), &rs.db());
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
    let (py, rs) = (TempHome::new("zero-py"), TempHome::new("zero-rs"));
    let payload = serde_json::json!({"provider": "claude", "agent": "claude",
        "tmux_session": "sess", "tmux_pane": "%1", "input_tokens": 0, "output_tokens": 0,
        "cache_read_tokens": 0, "cache_write_tokens": 0, "cost_usd": 0,
        "turn_started_at": 1700000000, "turn_finished_at": 1700000001});
    oracle(&py.0, "capture-hook", &payload);
    native(&rs.0, "capture-hook", &payload);
    assert!(
        !py.db().exists(),
        "Python no crea la base sin números de uso"
    );
    assert_eq!(
        sqlite3(&rs.db(), "select count(*) from sqlite_master"),
        "0\n"
    );
}
