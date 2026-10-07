use comandos_store::{
    migrate::{DbLocation, MoveSpec, demote_db, estimate, move_db, resolve_db, spec_for},
    unified,
};
use rusqlite::Connection;
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};
struct Home(PathBuf);
impl Home {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!(
            "comandos-move-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&p).unwrap();
        fs::set_permissions(&p, fs::Permissions::from_mode(0o700)).unwrap();
        Self(p)
    }
    fn db(&self) -> PathBuf {
        self.0.join(".local/share/comandos/comandos.sqlite3")
    }
    fn spec(&self, domain: &str) -> MoveSpec {
        spec_for(&self.0, domain).unwrap()
    }
    fn source(&self, domain: &str) -> MoveSpec {
        let spec = self.spec(domain);
        fs::create_dir_all(spec.legacy.parent().unwrap()).unwrap();
        let c = Connection::open(&spec.legacy).unwrap();
        c.execute_batch("PRAGMA journal_mode=WAL;PRAGMA foreign_keys=ON")
            .unwrap();
        match domain {
            "db-operator" => {
                c.execute_batch("CREATE TABLE actions(id TEXT PRIMARY KEY,tool TEXT,status TEXT,detail TEXT,created REAL,updated REAL);INSERT INTO actions VALUES('original','browser','confirmed',X'00FF',1.25,NULL)").unwrap();
            }
            "db-news" => {
                c.execute_batch("CREATE TABLE events(url TEXT PRIMARY KEY,source TEXT NOT NULL,kind TEXT NOT NULL,title TEXT NOT NULL,at INTEGER NOT NULL,first_seen INTEGER NOT NULL,last_seen INTEGER NOT NULL,meta TEXT NOT NULL DEFAULT '{}');INSERT INTO events VALUES('url','source','kind','ñ',9223372036854775807,2,3,'{}')").unwrap();
            }
            "db-operations" => {
                c.execute_batch(include_str!(
                    "../migrations/unified/104-session-operations.sql"
                ))
                .unwrap();
                c.execute_batch("INSERT INTO session_operations VALUES('op','pane','hash','{}','confirmed',42,NULL,'{}',1.125);INSERT INTO session_operation_events VALUES('op','confirmed','{}',1.125);INSERT INTO session_operation_events VALUES('op','confirmed','{}',1.125)").unwrap();
            }
            "db-app-state" => {
                c.execute_batch("CREATE TABLE schema_migrations(version INTEGER PRIMARY KEY,name TEXT NOT NULL,applied_at REAL NOT NULL)").unwrap();
                comandos_store::state::migrate(&c, comandos_store::state::MIGRATIONS, 1.0).unwrap();
                c.execute_batch("INSERT INTO workspace_current VALUES(1,7,'{ \"raw\":1.00 }',1.25);INSERT INTO events(sequence,event_id,source,kind,evidence,correlation,occurred_at_ms,received_at_ms) VALUES(40,'event','source','kind','{}','{}',1,2);INSERT INTO event_receipts VALUES('receipt','event','source',2,0);UPDATE sqlite_sequence SET seq=100 WHERE name='events'").unwrap();
            }
            "db-usage" => {
                comandos_store::usage::ensure_schema(&c).unwrap();
                c.execute_batch("INSERT INTO usage_settings VALUES('key',' raw ñ ');INSERT INTO usage_interactions(id,tmux_session,tmux_pane,source,confidence,created_at) VALUES('i','s','p','hook','exact',1);INSERT INTO usage_tool_calls(id,interaction_id,sequence,tool_name,confidence) VALUES('t','i',2,'tool','exact');INSERT INTO usage_ratings VALUES('i',2,'ok',5,'note')").unwrap();
            }
            _ => panic!("fixture"),
        };
        drop(c);
        spec
    }
    fn backup(&self, name: &str) -> PathBuf {
        self.0.join(".local/share/comandos/backups").join(name)
    }
}
impl Drop for Home {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn rows(c: &Connection, table: &str) -> Vec<Vec<rusqlite::types::Value>> {
    let mut s = c
        .prepare(&format!("SELECT * FROM \"{table}\" ORDER BY rowid"))
        .unwrap();
    let n = s.column_count();
    s.query_map([], |r| (0..n).map(|i| r.get(i)).collect())
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap()
}
#[test]
fn move_then_resolve_points_to_unified_and_demote_roundtrips_all_five() {
    for domain in [
        "db-operator",
        "db-news",
        "db-operations",
        "db-app-state",
        "db-usage",
    ] {
        let h = Home::new();
        let spec = h.source(domain);
        let old = Connection::open(&spec.legacy).unwrap();
        let original: Vec<_> = spec
            .tables
            .iter()
            .map(|(from, _)| rows(&old, from))
            .collect();
        drop(old);
        move_db(&spec, &h.db(), &h.backup("first"), 4000).unwrap();
        assert_eq!(
            resolve_db(&spec.legacy, &h.db()).unwrap(),
            DbLocation::Unified(h.db())
        );
        let new = unified::open_unified(&h.db()).unwrap();
        for ((_, to), expected) in spec.tables.iter().zip(&original) {
            assert_eq!(&rows(&new, to), expected, "{domain}/{to}");
        }
        assert_eq!(
            new.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            11
        );
        assert_eq!(
            new.query_row("SELECT count(*) FROM schema_migrations", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            16
        );
        drop(new);
        move_db(&spec, &h.db(), &h.backup("repeat"), 4000).unwrap();
        demote_db(&spec, &h.db()).unwrap();
        assert_eq!(
            resolve_db(&spec.legacy, &h.db()).unwrap(),
            DbLocation::Legacy(spec.legacy.clone())
        );
        let old = Connection::open(&spec.legacy).unwrap();
        for ((from, _), expected) in spec.tables.iter().zip(&original) {
            assert_eq!(&rows(&old, from), expected, "{domain}/{from}");
        }
        if domain == "db-usage" {
            assert_eq!(
                old.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
                    .unwrap(),
                11
            );
        }
        if domain == "db-app-state" {
            assert_eq!(
                old.query_row(
                    "SELECT seq FROM sqlite_sequence WHERE name='events'",
                    [],
                    |r| r.get::<_, i64>(0)
                )
                .unwrap(),
                100
            );
        }
    }
}
#[test]
fn read_only_snapshot_and_resolve_missing_create_no_paths_or_shm() {
    let h = Home::new();
    let missing = h.0.join("missing/tree/db.sqlite3");
    assert_eq!(
        resolve_db(&missing, &h.db()).unwrap(),
        DbLocation::Legacy(missing.clone())
    );
    assert!(!h.0.join("missing").exists());
    let spec = h.source("db-operator");
    let c = Connection::open(&spec.legacy).unwrap();
    c.execute_batch("PRAGMA wal_autocheckpoint=0;INSERT INTO actions VALUES('wal','tool','pending','detail',2,3)").unwrap();
    let clone = h.0.join("clone.sqlite3");
    fs::copy(&spec.legacy, &clone).unwrap();
    fs::copy(
        format!("{}-wal", spec.legacy.display()),
        format!("{}-wal", clone.display()),
    )
    .unwrap();
    let before = fs::read(&clone).unwrap();
    let count = unified::with_readonly_db(&h.0, &clone, |r| {
        Ok(r.query_row("SELECT count(*) FROM actions", [], |r| r.get::<_, i64>(0))?)
    })
    .unwrap();
    assert_eq!(count, 2);
    assert_eq!(fs::read(&clone).unwrap(), before);
    assert!(!PathBuf::from(format!("{}-shm", clone.display())).exists());
    assert!(!h.0.join(".local").exists());
    assert!(unified::with_readonly_db(&h.0, &missing, |_| Ok(())).is_err());
    assert!(!h.0.join("missing").exists());
}

#[test]
fn move_crash_before_sentinel_is_redone() {
    let h = Home::new();
    let spec = h.source("db-app-state");
    let old = Connection::open(&spec.legacy).unwrap();
    old.execute_batch("CREATE TRIGGER fail_marker BEFORE INSERT ON schema_migrations WHEN NEW.version=1000 BEGIN SELECT RAISE(ABORT,'failure after target commit');END").unwrap();
    assert!(move_db(&spec, &h.db(), &h.backup("interrupted"), 4000).is_err());
    assert_eq!(
        resolve_db(&spec.legacy, &h.db()).unwrap(),
        DbLocation::Legacy(spec.legacy.clone())
    );
    let new = unified::open_unified(&h.db()).unwrap();
    assert_eq!(
        new.query_row("SELECT revision FROM workspace_current", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        7
    );
    drop(new);
    old.execute_batch("DROP TRIGGER fail_marker").unwrap();
    drop(old);
    move_db(&spec, &h.db(), &h.backup("interrupted"), 4000).unwrap();
    let new = unified::open_unified(&h.db()).unwrap();
    assert_eq!(
        new.query_row("SELECT count(*) FROM events", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        1
    );
    assert_eq!(
        new.query_row("SELECT count(*) FROM event_receipts", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        1
    );
}
#[test]
fn failed_target_copy_rolls_back_data_and_leaves_legacy_unmarked() {
    let h = Home::new();
    let spec = h.source("db-operator");
    let new = unified::open_unified(&h.db()).unwrap();
    new.execute_batch("INSERT INTO operator_actions(id,detail) VALUES('existing','keep');CREATE TRIGGER fail_copy BEFORE INSERT ON operator_actions WHEN NEW.id='original' BEGIN SELECT RAISE(ABORT,'target failure');END").unwrap();
    assert!(move_db(&spec, &h.db(), &h.backup("failure"), 4000).is_err());
    assert_eq!(rows(&new, "operator_actions").len(), 1);
    assert_eq!(
        new.query_row("SELECT detail FROM operator_actions", [], |r| r
            .get::<_, String>(0))
            .unwrap(),
        "keep"
    );
    assert_eq!(
        new.query_row(
            "SELECT status FROM migration_steps WHERE domain='db-operator'",
            [],
            |r| r.get::<_, String>(0)
        )
        .unwrap(),
        "failed"
    );
    let old = Connection::open(&spec.legacy).unwrap();
    assert_eq!(
        old.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        0
    );
    assert_eq!(rows(&old, "actions").len(), 1);
}
#[test]
fn usage_move_refuses_when_estimate_exceeds_budget() {
    let h = Home::new();
    let spec = h.source("db-usage");
    let old = Connection::open(&spec.legacy).unwrap();
    old.execute_batch("WITH RECURSIVE r(i) AS(SELECT 1 UNION ALL SELECT i+1 FROM r WHERE i<2000) INSERT INTO usage_settings SELECT 'k'||i,hex(randomblob(1000)) FROM r").unwrap();
    drop(old);
    let scratch = Home::new();
    assert!(estimate(&spec, &scratch.0).unwrap().copy_ms > 1);
    let err = move_db(&spec, &h.db(), &h.backup("budget"), 1).unwrap_err();
    assert!(err.to_string().contains("ventana sin agentes"));
    assert!(!h.db().exists());
    comandos_store::migrate::verify_backup(&h.backup("budget").join("manifest.json")).unwrap();
    assert_eq!(
        resolve_db(&spec.legacy, &h.db()).unwrap(),
        DbLocation::Legacy(spec.legacy.clone())
    );
}
#[test]
fn copy_preserves_rowids_storage_classes_and_invalid_utf8_text() {
    let h = Home::new();
    let spec = h.source("db-operator");
    let old = Connection::open(&spec.legacy).unwrap();
    old.execute_batch("INSERT INTO actions(rowid,id,detail) VALUES(400,'text',CAST(X'80FF00' AS TEXT));INSERT INTO actions(rowid,id,detail) VALUES(800,'blob',X'80FF00')").unwrap();
    drop(old);
    move_db(&spec, &h.db(), &h.backup("types"), 4000).unwrap();
    let new = unified::open_unified(&h.db()).unwrap();
    assert_eq!(new.query_row("SELECT rowid||':'||typeof(detail)||':'||hex(detail) FROM operator_actions WHERE id='text'",[],|r|r.get::<_,String>(0)).unwrap(),"400:text:80FF00");
    drop(new);
    demote_db(&spec, &h.db()).unwrap();
    let old = Connection::open(&spec.legacy).unwrap();
    assert_eq!(
        old.query_row(
            "SELECT rowid||':'||typeof(detail)||':'||hex(detail) FROM actions WHERE id='blob'",
            [],
            |r| r.get::<_, String>(0)
        )
        .unwrap(),
        "800:blob:80FF00"
    );
}

#[test]
fn usage_move_and_demote_preserve_optional_session_profiles_exactly() {
    let h = Home::new();
    let spec = h.source("db-usage");
    let old = Connection::open(&spec.legacy).unwrap();
    old.execute_batch("CREATE TABLE session_profiles(id TEXT PRIMARY KEY,name TEXT NOT NULL,payload TEXT NOT NULL,created_at INTEGER NOT NULL,updated_at INTEGER NOT NULL);INSERT INTO session_profiles(rowid,id,name,payload,created_at,updated_at) VALUES(700,'profile','ñ',' { \"name\": \"ñ\" } ',1,9223372036854775807),(900,'opaque','bytes',CAST(X'80FF00' AS TEXT),2,3)").unwrap();
    let profile_rows = |c: &Connection| -> Vec<(i64, String, String, String, String, i64, i64)> {
        c.prepare("SELECT rowid,id,name,typeof(payload),hex(payload),created_at,updated_at FROM session_profiles ORDER BY rowid").unwrap()
            .query_map([], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?,r.get(6)?))).unwrap()
            .collect::<rusqlite::Result<_>>().unwrap()
    };
    let before = profile_rows(&old);
    drop(old);
    move_db(&spec, &h.db(), &h.backup("profiles"), 4000).unwrap();
    let new = unified::open_unified(&h.db()).unwrap();
    assert_eq!(profile_rows(&new), before);
    drop(new);
    demote_db(&spec, &h.db()).unwrap();
    let old = Connection::open(&spec.legacy).unwrap();
    assert_eq!(profile_rows(&old), before);
    assert_eq!(
        old.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        11
    );
    comandos_store::migrate::verify_backup(&h.backup("profiles").join("manifest.json")).unwrap();
}

#[test]
fn usage_without_profiles_demotes_profiles_created_in_unified() {
    let h = Home::new();
    let spec = h.source("db-usage");
    move_db(&spec, &h.db(), &h.backup("absent-profiles"), 4000).unwrap();
    let new = unified::open_unified(&h.db()).unwrap();
    comandos_store::session_profiles::list_profiles(&new).unwrap();
    new.execute_batch("INSERT INTO session_profiles(rowid,id,name,payload,created_at,updated_at) VALUES(1234,'new-profile','New',' { \"kept\": true } ',8,9)").unwrap();
    drop(new);
    demote_db(&spec, &h.db()).unwrap();
    let old = Connection::open(&spec.legacy).unwrap();
    let row = old
        .query_row(
            "SELECT rowid,payload FROM session_profiles WHERE id='new-profile'",
            [],
            |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)),
        )
        .unwrap();
    assert_eq!(row, (1234, " { \"kept\": true } ".to_string()));
    assert_eq!(
        old.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        11
    );
}

#[test]
fn optional_profiles_do_not_allow_unknown_usage_tables() {
    let h = Home::new();
    let spec = h.source("db-usage");
    let old = Connection::open(&spec.legacy).unwrap();
    comandos_store::session_profiles::list_profiles(&old).unwrap();
    old.execute_batch(
        "CREATE TABLE unknown_user_data(value BLOB);INSERT INTO unknown_user_data VALUES(X'80FF')",
    )
    .unwrap();
    drop(old);
    let before = tree(&h.0);
    let error = move_db(&spec, &h.db(), &h.backup("unknown"), 4000).unwrap_err();
    assert!(error.to_string().contains("tablas SQLite no previstas"));
    assert_eq!(tree(&h.0), before);
}

#[test]
fn usage_upgraded_column_layout_roundtrips_by_column_name() {
    let h = Home::new();
    let spec = h.source("db-usage");
    let old = Connection::open(&spec.legacy).unwrap();
    // Released upgrades append columns with ALTER TABLE, unlike a fresh v11
    // schema. These are the two layouts found in the real-data snapshot.
    old.execute_batch(
        "DROP TABLE usage_experiment_runs;
         DROP TABLE usage_experiments;
         CREATE TABLE usage_experiments (
           id TEXT PRIMARY KEY,label TEXT NOT NULL,task_type TEXT NOT NULL,
           status TEXT NOT NULL,design TEXT NOT NULL DEFAULT 'paired',
           created_at INTEGER NOT NULL,updated_at INTEGER NOT NULL);
         ALTER TABLE usage_experiments ADD COLUMN project_id TEXT NOT NULL DEFAULT '';
         ALTER TABLE usage_experiments ADD COLUMN primary_metric TEXT NOT NULL DEFAULT 'outcome';
         ALTER TABLE usage_experiments ADD COLUMN min_pairs INTEGER NOT NULL DEFAULT 10;
         CREATE TABLE usage_experiment_runs (
           id TEXT PRIMARY KEY,experiment_id TEXT NOT NULL,interaction_id TEXT NOT NULL DEFAULT '',
           variant_index INTEGER NOT NULL,harness TEXT NOT NULL,motor TEXT NOT NULL,
           model TEXT NOT NULL,effort TEXT NOT NULL DEFAULT '',route_id TEXT NOT NULL,
           harness_account TEXT NOT NULL DEFAULT 'unknown',motor_account TEXT NOT NULL DEFAULT 'unknown',
           tmux_session TEXT NOT NULL DEFAULT '',tmux_pane TEXT NOT NULL DEFAULT '',
           status TEXT NOT NULL DEFAULT 'planned',started_at INTEGER,finished_at INTEGER,
           FOREIGN KEY(experiment_id) REFERENCES usage_experiments(id) ON DELETE CASCADE);
         ALTER TABLE usage_experiment_runs ADD COLUMN task_id TEXT NOT NULL DEFAULT '';
         ALTER TABLE usage_experiment_runs ADD COLUMN project_id TEXT NOT NULL DEFAULT '';
         ALTER TABLE usage_experiment_runs ADD COLUMN launch_order INTEGER NOT NULL DEFAULT 0;
         INSERT INTO usage_experiments(rowid,id,label,task_type,status,created_at,updated_at,project_id,min_pairs)
           VALUES(5419,'historical','ñ','code','running',7,9,'project',12);
         INSERT INTO usage_experiment_runs(rowid,id,experiment_id,variant_index,harness,motor,model,route_id,task_id,launch_order,started_at)
           VALUES(1048577,'run','historical',0,'codex','openai',CAST(X'80FF00' AS TEXT),'route','task',4,11);",
    ).unwrap();
    let queries = [
        "SELECT rowid,id,label,task_type,status,design,created_at,updated_at,project_id,primary_metric,min_pairs FROM usage_experiments ORDER BY rowid",
        "SELECT rowid,id,experiment_id,interaction_id,variant_index,harness,motor,typeof(model),hex(model),effort,route_id,harness_account,motor_account,tmux_session,tmux_pane,status,started_at,finished_at,task_id,project_id,launch_order FROM usage_experiment_runs ORDER BY rowid",
    ];
    let read = |c: &Connection, query: &str| -> Vec<Vec<rusqlite::types::Value>> {
        let mut stmt = c.prepare(query).unwrap();
        let n = stmt.column_count();
        stmt.query_map([], |row| (0..n).map(|i| row.get(i)).collect())
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap()
    };
    let before: Vec<_> = queries.iter().map(|sql| read(&old, sql)).collect();
    drop(old);
    move_db(&spec, &h.db(), &h.backup("upgraded-layout"), 4000).unwrap();
    let new = unified::open_unified(&h.db()).unwrap();
    assert_eq!(
        queries
            .iter()
            .map(|sql| read(&new, sql))
            .collect::<Vec<_>>(),
        before
    );
    drop(new);
    demote_db(&spec, &h.db()).unwrap();
    let old = Connection::open(&spec.legacy).unwrap();
    assert_eq!(
        queries
            .iter()
            .map(|sql| read(&old, sql))
            .collect::<Vec<_>>(),
        before
    );
    assert_eq!(
        old.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        11
    );
}

#[test]
fn unrecognized_source_columns_are_refused_without_touching_source() {
    let h = Home::new();
    let spec = h.source("db-operator");
    let old = Connection::open(&spec.legacy).unwrap();
    old.execute_batch("ALTER TABLE actions ADD COLUMN unknown_user_data BLOB;UPDATE actions SET unknown_user_data=X'80FF'").unwrap();
    drop(old);
    let before = fs::read(&spec.legacy).unwrap();
    let error = move_db(&spec, &h.db(), &h.backup("unknown-column"), 4000).unwrap_err();
    assert!(error.to_string().contains("columnas incompatibles"));
    assert_eq!(fs::read(&spec.legacy).unwrap(), before);
    assert!(!h.db().exists());
}

#[test]
fn writer_blocked_during_move_resumes_on_unified() {
    use std::{
        sync::mpsc,
        time::{Duration, Instant},
    };
    let h = Home::new();
    let spec = h.source("db-operator");
    let target = h.db();
    let blocker = unified::open_unified(&target).unwrap();
    blocker.execute_batch("BEGIN IMMEDIATE").unwrap();
    let legacy = spec.legacy.clone();
    let backup = h.backup("writer");
    let (moved_tx, moved_rx) = mpsc::channel();
    let dest = target.clone();
    let mover = std::thread::spawn(move || {
        let outcome = move_db(&spec, &dest, &backup, 4000);
        moved_tx.send(outcome).unwrap();
    });
    let probe = Connection::open(&legacy).unwrap();
    probe.busy_timeout(Duration::ZERO).unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        match probe.execute_batch("BEGIN IMMEDIATE") {
            Ok(()) => {
                probe.execute_batch("ROLLBACK").unwrap();
            }
            Err(_) => break,
        }
        assert!(
            Instant::now() < deadline,
            "mover never took source writer lock"
        );
        assert!(moved_rx.try_recv().is_err());
        std::thread::sleep(Duration::from_millis(5));
    }
    let (writer_tx, writer_rx) = mpsc::channel();
    let dest = target.clone();
    let source = legacy.clone();
    let writer = std::thread::spawn(move || {
        let old = Connection::open(&source).unwrap();
        old.busy_timeout(Duration::from_secs(5)).unwrap();
        old.execute_batch("BEGIN IMMEDIATE").unwrap();
        let version: i64 = old
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(version, 1000);
        old.execute_batch("ROLLBACK").unwrap();
        drop(old);
        let DbLocation::Unified(path) = resolve_db(&source, &dest).unwrap() else {
            panic!("writer did not resolve")
        };
        let new = unified::open_unified(&path).unwrap();
        new.execute("INSERT INTO operator_actions(id) VALUES('writer')", [])
            .unwrap();
        writer_tx.send(()).unwrap();
    });
    std::thread::sleep(Duration::from_millis(30));
    assert!(writer_rx.try_recv().is_err());
    blocker.execute_batch("COMMIT").unwrap();
    moved_rx
        .recv_timeout(Duration::from_secs(4))
        .unwrap()
        .unwrap();
    writer_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    mover.join().unwrap();
    writer.join().unwrap();
    assert_eq!(rows(&probe, "actions").len(), 1);
    assert_eq!(rows(&blocker, "operator_actions").len(), 2);
}

fn tree(path: &std::path::Path) -> Vec<(PathBuf, String, u64, u32, i64, i64)> {
    use sha2::{Digest, Sha256};
    use std::os::unix::fs::MetadataExt;
    let mut result = Vec::new();
    for entry in fs::read_dir(path).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            result.extend(tree(&path));
        } else {
            let m = fs::metadata(&path).unwrap();
            result.push((
                path.clone(),
                format!("{:x}", Sha256::digest(fs::read(path).unwrap())),
                m.ino(),
                m.mode(),
                m.mtime(),
                m.mtime_nsec(),
            ));
        }
    }
    result.sort();
    result
}
#[test]
fn unknown_source_tables_and_future_wal_are_rejected_unchanged() {
    for future in [false, true] {
        let h = Home::new();
        let mut spec = h.source("db-operator");
        let old = Connection::open(&spec.legacy).unwrap();
        old.execute_batch("PRAGMA wal_autocheckpoint=0").unwrap();
        if future {
            old.pragma_update(None, "user_version", 12).unwrap();
        } else {
            old.execute_batch(
                "CREATE TABLE unknown_data(value BLOB);INSERT INTO unknown_data VALUES(X'80')",
            )
            .unwrap();
        }
        let clone = h.0.join("rejected.sqlite3");
        fs::copy(&spec.legacy, &clone).unwrap();
        fs::copy(
            format!("{}-wal", spec.legacy.display()),
            format!("{}-wal", clone.display()),
        )
        .unwrap();
        spec.legacy = clone;
        let before = tree(&h.0);
        assert!(move_db(&spec, &h.db(), &h.backup("must-not-exist"), 4000).is_err());
        assert_eq!(tree(&h.0), before);
        assert!(!h.db().exists());
        assert!(!h.backup("must-not-exist").exists());
    }
}
#[test]
fn future_unified_wal_rejects_move_without_source_or_target_changes() {
    let h = Home::new();
    let spec = h.source("db-operator");
    let new = unified::open_unified(&h.db()).unwrap();
    new.execute_batch("PRAGMA wal_autocheckpoint=0;PRAGMA user_version=12")
        .unwrap();
    let dir = h.0.join("future");
    fs::create_dir(&dir).unwrap();
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)).unwrap();
    let db = dir.join("unified.sqlite3");
    fs::copy(h.db(), &db).unwrap();
    fs::copy(
        format!("{}-wal", h.db().display()),
        format!("{}-wal", db.display()),
    )
    .unwrap();
    let before = tree(&h.0);
    assert!(move_db(&spec, &db, &h.backup("must-not-exist"), 4000).is_err());
    assert_eq!(tree(&h.0), before);
    assert!(!PathBuf::from(format!("{}-shm", db.display())).exists());
}
#[test]
fn tampered_backup_blocks_retry_and_inverse_copy() {
    let h = Home::new();
    let spec = h.source("db-operator");
    move_db(&spec, &h.db(), &h.backup("first"), 4000).unwrap();
    let manifest =
        comandos_store::migrate::backup::load(&h.backup("first").join("manifest.json")).unwrap();
    fs::write(&manifest.entries.first().unwrap().copy, b"tampered").unwrap();
    let old = Connection::open(&spec.legacy).unwrap();
    let before = rows(&old, "actions");
    assert!(demote_db(&spec, &h.db()).is_err());
    assert_eq!(rows(&old, "actions"), before);
    assert_eq!(
        old.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        1000
    );
}

#[test]
fn capable_usage_state_and_operator_openers_route_and_stale_writer_refuses() {
    for domain in ["db-usage", "db-app-state", "db-operator"] {
        let h = Home::new();
        let spec = h.source(domain);
        let cached = Connection::open(&spec.legacy).unwrap();
        move_db(&spec, &h.db(), &h.backup("first"), 4000).unwrap();
        let tx = cached.unchecked_transaction().unwrap();
        assert!(comandos_store::migrate::move_db::admit_write(&cached).is_err());
        tx.rollback().unwrap();
        let new = match domain {
            "db-usage" => comandos_store::usage::open_usage_db_at(&spec.legacy).unwrap(),
            "db-app-state" => comandos_store::state::connect(&spec.legacy).unwrap(),
            _ => {
                let op = comandos_store::operator::open_operator_db_at(&spec.legacy).unwrap();
                assert_eq!(op.table, "operator_actions");
                op.conn
            }
        };
        assert_eq!(new.path(), h.db().to_str());
        assert_eq!(
            new.query_row("SELECT count(*) FROM schema_migrations", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            16
        );
        assert_eq!(
            new.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            11
        );
        assert!(comandos_store::migrate::move_db::admit_write(&new).is_ok());
    }
}
#[test]
fn missing_moved_target_and_missing_sealed_target_do_not_resurrect_sources() {
    for sealed in [false, true] {
        let h = Home::new();
        let spec = h.source("db-usage");
        move_db(&spec, &h.db(), &h.backup("first"), 4000).unwrap();
        if sealed {
            let new = unified::open_unified(&h.db()).unwrap();
            unified::set_mode(&new, "db-usage", unified::Mode::Sealed, "test", 1).unwrap();
            drop(new);
            fs::remove_file(&spec.legacy).unwrap();
        }
        fs::remove_file(h.db()).unwrap();
        let before = tree(&h.0);
        assert!(comandos_store::usage::open_usage_db_at(&spec.legacy).is_err());
        assert_eq!(tree(&h.0), before);
    }
}
#[test]
fn inverse_failure_rolls_back_data_and_sentinel_together() {
    let h = Home::new();
    let spec = h.source("db-operator");
    move_db(&spec, &h.db(), &h.backup("first"), 4000).unwrap();
    let new = unified::open_unified(&h.db()).unwrap();
    new.execute("INSERT INTO operator_actions(id) VALUES('new')", [])
        .unwrap();
    let old = Connection::open(&spec.legacy).unwrap();
    old.execute_batch("CREATE TRIGGER refuse_inverse BEFORE INSERT ON actions WHEN NEW.id='new' BEGIN SELECT RAISE(ABORT,'inverse failure');END").unwrap();
    let before = rows(&old, "actions");
    assert!(demote_db(&spec, &h.db()).is_err());
    assert_eq!(rows(&old, "actions"), before);
    assert_eq!(
        old.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        1000
    );
    assert_eq!(rows(&new, "operator_actions").len(), 2);
    old.execute_batch("DROP TRIGGER refuse_inverse").unwrap();
    demote_db(&spec, &h.db()).unwrap();
    assert_eq!(rows(&old, "actions").len(), 2);
    assert_eq!(
        new.query_row(
            "SELECT count(*) FROM migration_steps WHERE status='done' AND json_extract(detail,'$.direction')='inverse'",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        1
    );
}
#[test]
fn readonly_callbacks_refuse_writes_and_consume_committed_wal_without_touching_home() {
    let h = Home::new();
    let spec = h.source("db-operator");
    let source = Connection::open(&spec.legacy).unwrap();
    source
        .execute_batch("PRAGMA wal_autocheckpoint=0;INSERT INTO actions(id) VALUES('wal')")
        .unwrap();
    let clone = h.0.join("read-only.sqlite3");
    fs::copy(&spec.legacy, &clone).unwrap();
    fs::copy(
        format!("{}-wal", spec.legacy.display()),
        format!("{}-wal", clone.display()),
    )
    .unwrap();
    let before = tree(&h.0);
    let count = unified::with_readonly_db(&h.0, &clone, |c| {
        assert!(
            c.execute("INSERT INTO actions(id) VALUES('forbidden')", [])
                .is_err()
        );
        Ok(c.query_row("SELECT count(*) FROM actions", [], |r| r.get::<_, i64>(0))?)
    })
    .unwrap();
    assert_eq!(count, 2);
    assert_eq!(tree(&h.0), before);
}
#[test]
fn sqlite_backup_contains_wal_commit_and_verified_0600_snapshot() {
    let h = Home::new();
    let spec = h.source("db-operator");
    let source = Connection::open(&spec.legacy).unwrap();
    source
        .execute_batch("PRAGMA wal_autocheckpoint=0;INSERT INTO actions(id) VALUES('wal')")
        .unwrap();
    move_db(&spec, &h.db(), &h.backup("first"), 4000).unwrap();
    comandos_store::migrate::verify_backup(&h.backup("first").join("manifest.json")).unwrap();
    let manifest =
        comandos_store::migrate::backup::load(&h.backup("first").join("manifest.json")).unwrap();
    let backup = &manifest.entries[0].copy;
    assert_eq!(
        fs::metadata(backup).unwrap().permissions().mode() & 0o7777,
        0o600
    );
    assert_eq!(
        unified::with_readonly_db(&h.0, backup, |c| Ok(c.query_row(
            "SELECT count(*) FROM actions",
            [],
            |r| r.get::<_, i64>(0)
        )?))
        .unwrap(),
        2
    );
}

#[test]
fn authoritative_modes_block_move_and_inverse_without_demotion() {
    for mode in [unified::Mode::Unified, unified::Mode::Sealed] {
        let h = Home::new();
        let spec = h.source("db-operator");
        let new = unified::open_unified(&h.db()).unwrap();
        unified::set_mode(&new, "db-operator", mode, "test", 1).unwrap();
        drop(new);
        let source_before = fs::read(&spec.legacy).unwrap();
        assert!(move_db(&spec, &h.db(), &h.backup("unused"), 4000).is_err());
        assert_eq!(fs::read(&spec.legacy).unwrap(), source_before);
        assert_eq!(
            resolve_db(&spec.legacy, &h.db()).unwrap(),
            DbLocation::Unified(h.db())
        );
        let new = unified::open_unified(&h.db()).unwrap();
        assert_eq!(unified::mode_of(Some(&new), "db-operator").unwrap(), mode);
        drop(new);
        let h = Home::new();
        let spec = h.source("db-operator");
        move_db(&spec, &h.db(), &h.backup("first"), 4000).unwrap();
        let new = unified::open_unified(&h.db()).unwrap();
        unified::set_mode(&new, "db-operator", mode, "test", 1).unwrap();
        drop(new);
        let before = fs::read(&spec.legacy).unwrap();
        assert!(demote_db(&spec, &h.db()).is_err());
        assert_eq!(fs::read(&spec.legacy).unwrap(), before);
    }
}
#[test]
fn file_migration_excludes_usage_named_unified_target_from_estimate() {
    let h = Home::new();
    let db = h.spec("db-usage").legacy;
    fs::create_dir_all(db.parent().unwrap()).unwrap();
    fs::set_permissions(db.parent().unwrap(), fs::Permissions::from_mode(0o700)).unwrap();
    drop(unified::open_unified(&db).unwrap());
    let before = tree(&h.0);
    let report = comandos_store::migrate::migrate(&comandos_store::migrate::MigrateOptions {
        home: h.0.clone(),
        db,
        dry_run: true,
        resume: false,
        domains: None,
        now_ms: 1,
    })
    .unwrap();
    assert_eq!(report.usage_move_estimate_ms, None);
    assert_eq!(tree(&h.0), before);
}

#[test]
fn target_busy_timeout_releases_source_lock_without_marking_or_losing_data() {
    use std::time::{Duration, Instant};
    let h = Home::new();
    let spec = h.source("db-operator");
    let new = unified::open_unified(&h.db()).unwrap();
    new.execute_batch("INSERT INTO operator_actions(id) VALUES('keep');BEGIN IMMEDIATE")
        .unwrap();
    let target = h.db();
    let backup = h.backup("busy");
    let source = spec.legacy.clone();
    let mover = std::thread::spawn(move || move_db(&spec, &target, &backup, 1000));
    let probe = Connection::open(&source).unwrap();
    probe.busy_timeout(Duration::ZERO).unwrap();
    let started = Instant::now();
    let mut saw_lock = false;
    while started.elapsed() < Duration::from_secs(5) {
        match probe.execute_batch("BEGIN IMMEDIATE") {
            Ok(()) => {
                probe.execute_batch("ROLLBACK").unwrap();
                if saw_lock {
                    break;
                }
            }
            Err(_) => saw_lock = true,
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(saw_lock, "mover acquired the original SQLite write lock");
    assert!(
        probe.execute_batch("BEGIN IMMEDIATE").is_ok(),
        "failed target copy must release the source before journaling its failure"
    );
    probe.execute_batch("ROLLBACK").unwrap();
    new.execute_batch("COMMIT").unwrap();
    assert!(mover.join().unwrap().is_err());
    assert_eq!(
        probe
            .query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        0
    );
    assert_eq!(rows(&probe, "actions").len(), 1);
    assert_eq!(rows(&new, "operator_actions").len(), 1);
}

#[test]
fn repeat_after_source_marker_finishes_interrupted_journal_without_copy_or_backup() {
    let h = Home::new();
    let spec = h.source("db-operator");
    move_db(&spec, &h.db(), &h.backup("first"), 4000).unwrap();
    let new = unified::open_unified(&h.db()).unwrap();
    new.execute_batch("UPDATE migration_runs SET status='running',finished_at_ms=NULL;INSERT INTO operator_actions(id) VALUES('newer')").unwrap();
    drop(new);
    move_db(&spec, &h.db(), &h.backup("unused"), 4000).unwrap();
    assert!(!h.backup("unused").exists());
    let new = unified::open_unified(&h.db()).unwrap();
    assert_eq!(rows(&new, "operator_actions").len(), 2);
    assert_eq!(
        new.query_row(
            "SELECT count(*) FROM migration_runs WHERE status='running'",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        0
    );
}

#[test]
fn original_python_consumers_create_sources_that_roundtrip_all_released_tables() {
    let h = Home::new();
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_owned();
    let script = r#"
import os, sys
from pathlib import Path
home, repo = Path(sys.argv[1]), Path(sys.argv[2])
os.umask(0o077)
sys.path[:0] = [str(repo/'lib'), str(repo/'bin')]
import operator_receipts, news_watch, session_operations, app_state, cc_usage
hooks=home/'.claude/hooks';hooks.mkdir(parents=True)
action=operator_receipts.create(hooks/'operator','browser')
operator_receipts.acknowledge(hooks/'operator', {'actionId':action,'status':'confirmed','detail':'ñ raw'})
assert news_watch._history_append(hooks,[{'url':'url','source':'source','kind':'kind','title':'ñ','at':1,'meta':{'raw':1}}],1)==1
ops=session_operations.OperationStore(hooks/'session-operations.sqlite3')
assert ops.claim('operation','pane',{'raw':'ñ'})
ops.stage('operation','confirmed',snapshot={'origin':{}},result={'ok':True})
state=app_state.connect(home/'.local/state/comandos/app-state.sqlite3')
assert app_state.migrate(state)==11
state.execute("INSERT INTO workspace_current VALUES(1,1,?,1)", ('{ "raw":1.00 }',))
state.close()
usage=hooks/'comandos-usage.sqlite';cc_usage.init_db(usage)
with cc_usage.connect(usage) as db:
    db.execute("INSERT INTO usage_settings VALUES('key','ñ raw')")
    assert db.execute('PRAGMA user_version').fetchone()[0]==11
"#;
    let output = std::process::Command::new("/usr/bin/python3")
        .args(["-B", "-c", script])
        .arg(&h.0)
        .arg(&repo)
        .env_clear()
        .env("HOME", &h.0)
        .env("PATH", "/usr/bin:/bin")
        .env("COMANDOS_DB", "")
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    for domain in [
        "db-operator",
        "db-news",
        "db-operations",
        "db-app-state",
        "db-usage",
    ] {
        let spec = h.spec(domain);
        let old = Connection::open(&spec.legacy).unwrap();
        let before: Vec<_> = spec
            .tables
            .iter()
            .map(|(from, _)| rows(&old, from))
            .collect();
        drop(old);
        move_db(&spec, &h.db(), &h.backup(domain), 4000).unwrap();
        demote_db(&spec, &h.db()).unwrap();
        let old = Connection::open(&spec.legacy).unwrap();
        for ((from, _), original) in spec.tables.iter().zip(before) {
            assert_eq!(
                rows(&old, from),
                original,
                "original consumer {domain}/{from}"
            );
        }
    }
}

#[test]
fn missing_target_with_insecure_parent_or_orphan_wal_is_rejected_before_backup() {
    for orphan_wal in [false, true] {
        let h = Home::new();
        let spec = h.source("db-operator");
        let parent = h.db().parent().unwrap().to_owned();
        fs::create_dir_all(&parent).unwrap();
        fs::set_permissions(
            &parent,
            fs::Permissions::from_mode(if orphan_wal { 0o700 } else { 0o755 }),
        )
        .unwrap();
        if orphan_wal {
            fs::write(
                format!("{}-wal", h.db().display()),
                b"orphan WAL must not be ignored",
            )
            .unwrap();
        }
        let before = tree(&h.0);
        assert!(comandos_store::migrate::move_estimate(&h.0, &spec, &h.db()).is_err());
        assert!(move_db(&spec, &h.db(), &h.backup("must-not-exist"), 4000).is_err());
        assert_eq!(tree(&h.0), before);
        assert!(!h.backup("must-not-exist").exists());
    }
}
