use rusqlite::Connection;
use serde_json::json;
use std::io::Write;
use std::process::{Command, Output, Stdio};

fn invoke(path: &std::path::Path, args: &[&str], input: &str) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_comandos-events"))
        .arg("--state")
        .arg(path)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
}
fn path(label: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!("native-cli-{label}-{}.sqlite3", std::process::id()))
}

#[test]
fn record_creates_schema_and_works_without_dashboard_or_python() {
    let path = path("record");
    let output = invoke(
        &path,
        &["record"],
        r#"{"hookEvent":"Stop","agent":"codex","session":"s","pane":"%1","turnId":"t","conversationId":"c"}"#,
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stdout.is_empty());
    let conn = Connection::open(&path).unwrap();
    assert_eq!(comandos_store::state::schema_version(&conn).unwrap(), 11);
    assert_eq!(comandos_store::latest_sequence(&conn).unwrap(), 1);
    assert_eq!(
        comandos_store::list_events(&conn, 0, 1).unwrap()[0]["kind"],
        "turn_completed"
    );
    conn.close().unwrap();
    std::fs::remove_file(path).unwrap();
}

#[test]
fn malformed_inputs_do_not_create_database_and_usage_has_status_two() {
    let path = path("bad");
    for input in ["[]", "false", "not json"] {
        assert_eq!(invoke(&path, &["record"], input).status.code(), Some(1));
        assert!(!path.exists());
    }
    assert_eq!(invoke(&path, &["unknown"], "").status.code(), Some(2));
    assert!(!path.exists());
}

#[test]
fn sound_is_claimed_once_across_hooks_and_standalone_claim_local() {
    let path = path("sound");
    let payload=json!({"hookEvent":"PermissionRequest","agent":"codex","session":"s","pane":"%1","turnId":"t","conversationId":"c"}).to_string();
    let first = invoke(&path, &["record", "--claim-sound", "desktop"], &payload);
    assert!(first.status.success());
    assert_eq!(first.stdout, b"play\n");
    let duplicate = invoke(&path, &["record", "--claim-sound", "desktop"], &payload);
    assert!(duplicate.status.success());
    assert!(duplicate.stdout.is_empty());
    let conn = Connection::open(&path).unwrap();
    let events = comandos_store::list_events(&conn, 0, 1).unwrap();
    let id = events[0]["eventId"].as_str().unwrap();
    assert_eq!(
        invoke(&path, &["claim-local", id, "desktop"], "")
            .status
            .code(),
        Some(1)
    );
    assert_eq!(
        conn.query_row(
            "SELECT COUNT(*) FROM deliveries WHERE channel='sound'",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        1
    );
    conn.close().unwrap();
    std::fs::remove_file(path).unwrap();
}
