//! `read_focus_settings` de `bin/cc_usage.py`: `init_db` + `select key,value`.
use comandos_store::usage::{
    SCHEMA_VERSION, ensure_schema, focus_settings_rows, open_usage_db_at, schema_version,
};

#[test]
fn focus_settings_rows_creates_schema_and_reads_table_order() {
    let dir = std::env::temp_dir().join(format!("cmd-usage-focus-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let conn = open_usage_db_at(&dir.join("u.sqlite")).unwrap();
    assert_eq!(schema_version(&conn).unwrap(), 0);
    assert!(focus_settings_rows(&conn).unwrap().is_empty());
    assert_eq!(schema_version(&conn).unwrap(), SCHEMA_VERSION);
    conn.execute_batch(
        "insert into focus_settings(key,value) values('style','\"garden\"'),('cycles','4'),('roto','{')",
    )
    .unwrap();
    assert_eq!(
        focus_settings_rows(&conn).unwrap(),
        vec![
            ("style".to_owned(), Some("\"garden\"".to_owned())),
            ("cycles".to_owned(), Some("4".to_owned())),
            ("roto".to_owned(), Some("{".to_owned())),
        ]
    );
    ensure_schema(&conn).unwrap();
    let _ = std::fs::remove_dir_all(&dir);
}
