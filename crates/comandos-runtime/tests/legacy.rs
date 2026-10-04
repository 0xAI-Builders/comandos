use comandos_runtime::{legacy::import_legacy, open_state};
use comandos_store::{append_event, list_events};
use serde_json::json;

#[test]
fn imports_history_once_without_targeting_panes_and_keeps_duplicate_lines() {
    let folder = std::env::temp_dir().join(format!("legacy-{}", std::process::id()));
    std::fs::create_dir(&folder).unwrap();
    let conn = open_state(&folder.join("state.sqlite3"), 1500).unwrap();
    let file = folder.join("timeline.jsonl");
    let line = json!({"project":"proj","status":"done","detail":"ok","ts":100}).to_string();
    std::fs::write(&file, format!("{line}\nnot json\n{line}\n")).unwrap();
    assert_eq!(import_legacy(&conn, &file).unwrap(), 2);
    let events = list_events(&conn, 0, 100).unwrap();
    assert_eq!(events.len(), 2);
    assert_eq!(events[0]["occurredAtMs"], 100000);
    assert_eq!(events[0]["destination"], "none");
    assert_eq!(events[0]["evidence"], "historical");
    assert_ne!(events[0]["sourceEventId"], events[1]["sourceEventId"]);
    assert_eq!(import_legacy(&conn, &file).unwrap(), 0);
    conn.close().unwrap();
    std::fs::remove_dir_all(folder).unwrap();
}

#[test]
fn skips_receptions_at_or_after_first_live_event_second_and_respects_rollback() {
    let folder = std::env::temp_dir().join(format!("legacy-cutoff-{}", std::process::id()));
    std::fs::create_dir(&folder).unwrap();
    let conn = open_state(&folder.join("state.sqlite3"), 1500).unwrap();
    append_event(&conn,&json!({"kind":"turn_completed","source":"hook","occurredAtMs":50500,"evidence":"confirmed"}),60000,"first","receipt-first").unwrap();
    let file = folder.join("timeline.jsonl");
    std::fs::write(
        &file,
        (49..=51)
            .map(|ts| format!("{{\"status\":\"done\",\"ts\":{ts}}}\n"))
            .collect::<String>(),
    )
    .unwrap();
    conn.execute_batch("BEGIN IMMEDIATE").unwrap();
    assert_eq!(import_legacy(&conn, &file).unwrap(), 1);
    assert!(!conn.is_autocommit());
    conn.execute_batch("ROLLBACK").unwrap();
    assert_eq!(list_events(&conn, 0, 100).unwrap().len(), 1);
    assert_eq!(import_legacy(&conn, &file).unwrap(), 1);
    conn.close().unwrap();
    std::fs::remove_dir_all(folder).unwrap();
}
