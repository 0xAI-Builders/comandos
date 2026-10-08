use comandos_server::dash::native::lanes::{LaneBackend, UsageBackend};
use comandos_store::{usage, usage_read};
use rusqlite::Connection;
use std::path::PathBuf;

struct Database(PathBuf);
impl Database {
    fn new() -> Self {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        Self(std::env::temp_dir().join(format!(
            "comandos-usage-index-{}-{stamp}.sqlite",
            std::process::id()
        )))
    }
}
impl Drop for Database {
    fn drop(&mut self) {
        for suffix in ["", "-wal", "-shm"] {
            let _ = std::fs::remove_file(format!("{}{suffix}", self.0.display()));
        }
    }
}

#[test]
fn opening_usage_lane_adds_covering_index_without_changing_records_or_version() {
    let db = Database::new();
    let conn = usage::open_usage_db_at(&db.0).unwrap();
    usage::ensure_schema(&conn).unwrap();
    conn.execute_batch("INSERT INTO usage_turns(id,provider,agent,tmux_session,tmux_pane,pane_pwd,git_root,turn_started_at,turn_finished_at,total_tokens,source,confidence,raw) VALUES ('kept-one','openai','codex','session','%1','/project','/project',1000,2000,123,'hook','exact','{\"preserve\":1}'),('kept-two','anthropic','claude','other','%2','/other','/other',1100,2100,456,'hook','exact','{\"preserve\":2}')").unwrap();
    let before = usage_read::measured_usage(&conn, "openai", 3000, 0).unwrap();
    let rows_before: Vec<(String, String, i64)> = conn
        .prepare("SELECT id,raw,total_tokens FROM usage_turns ORDER BY id")
        .unwrap()
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap();
    drop(conn);
    for _ in 0..2 {
        let lane = UsageBackend::open(&db.0).unwrap();
        let plans: Vec<String> = lane.conn.prepare("EXPLAIN QUERY PLAN SELECT COUNT(*),COALESCE(SUM(total_tokens),0),MAX(turn_finished_at) FROM usage_turns WHERE provider=?1 AND turn_finished_at>=?2").unwrap().query_map(rusqlite::params!["openai", 0], |row| row.get(3)).unwrap().collect::<rusqlite::Result<_>>().unwrap();
        assert!(
            plans.iter().any(
                |plan| plan.contains("COVERING INDEX idx_usage_turns_provider_finished_tokens")
            ),
            "usage summary must avoid loading the full history rows: {plans:?}"
        );
        assert_eq!(
            usage::schema_version(&lane.conn).unwrap(),
            usage::SCHEMA_VERSION
        );
        assert_eq!(
            usage_read::measured_usage(&lane.conn, "openai", 3000, 0).unwrap(),
            before
        );
        let rows_after: Vec<(String, String, i64)> = lane
            .conn
            .prepare("SELECT id,raw,total_tokens FROM usage_turns ORDER BY id")
            .unwrap()
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        assert_eq!(rows_after, rows_before);
    }
}

#[test]
fn newer_database_is_refused_without_adding_an_index() {
    let db = Database::new();
    let conn = Connection::open(&db.0).unwrap();
    conn.pragma_update(None, "user_version", usage::SCHEMA_VERSION + 1)
        .unwrap();
    assert!(UsageBackend::open(&db.0).is_err());
    let indexes: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='index'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(indexes, 0);
    assert_eq!(
        usage::schema_version(&conn).unwrap(),
        usage::SCHEMA_VERSION + 1
    );
}

#[test]
fn optional_index_failure_keeps_usage_lane_available() {
    let db = Database::new();
    let conn = usage::open_usage_db_at(&db.0).unwrap();
    usage::ensure_schema(&conn).unwrap();
    // A conflicting user table prevents this optional CREATE INDEX. It must
    // neither be dropped nor cause the otherwise valid lane to be rejected.
    conn.execute_batch("CREATE TABLE idx_usage_turns_provider_finished_tokens(kept TEXT); INSERT INTO idx_usage_turns_provider_finished_tokens VALUES('preserved')").unwrap();
    drop(conn);
    let lane = UsageBackend::open(&db.0).unwrap();
    lane.admit().unwrap();
    assert_eq!(
        lane.conn
            .query_row(
                "SELECT kept FROM idx_usage_turns_provider_finished_tokens",
                [],
                |row| row.get::<_, String>(0)
            )
            .unwrap(),
        "preserved"
    );
    assert_eq!(
        usage_read::measured_usage(&lane.conn, "openai", 3000, 0).unwrap(),
        None
    );
}
