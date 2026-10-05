//! `record_change` contra `cc_usage.record_change`: dos bases nuevas, los
//! mismos eventos con el reloj fijo, y las filas de `usage_changes` iguales.
mod support;

use comandos_store::usage;
use rusqlite::{Connection, types::ValueRef};
use serde_json::{Map, Value, json};
use std::{ffi::OsStr, path::PathBuf};
use support::python::run_python;

const NOW: i64 = 1_791_115_200;

struct Scratch(PathBuf);

impl Scratch {
    fn new(tag: &str) -> Self {
        let dir =
            std::env::temp_dir().join(format!("cmd-usage-change-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("home")).unwrap();
        Self(dir)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn events() -> Vec<Value> {
    vec![
        json!({"origin": "reparto", "kind": "trust_inherited",
               "note": "trust heredado main -> relotto en /home/x/codebase/p"}),
        // Mismo id (mismo segundo, mismos campos de la clave): no se duplica.
        json!({"origin": "reparto", "kind": "trust_inherited", "note": "otra"}),
        json!({"origin": "o".repeat(50), "kind": "", "tmux_session": "s", "tmux_pane": "%1",
               "after_model": "m", "note": "ñ".repeat(250), "status": null}),
    ]
}

fn rows(conn: &Connection) -> Vec<Vec<String>> {
    let mut stmt = conn
        .prepare("select * from usage_changes order by id")
        .unwrap();
    let n = stmt.column_count();
    let mut out = Vec::new();
    let mut q = stmt.query([]).unwrap();
    while let Some(row) = q.next().unwrap() {
        out.push(
            (0..n)
                .map(|i| match row.get_ref(i).unwrap() {
                    ValueRef::Null => "NULL".into(),
                    ValueRef::Integer(v) => format!("i{v}"),
                    ValueRef::Real(v) => format!("r{v}"),
                    ValueRef::Text(t) => format!("t{}", String::from_utf8_lossy(t)),
                    ValueRef::Blob(_) => "blob".into(),
                })
                .collect(),
        );
    }
    out
}

const ORACLE: &str = r#"
import sys, json, time
sys.path.insert(0, sys.argv[1] + "/bin")
import cc_usage
time.time = lambda: float(sys.argv[3])
for event in json.loads(sys.argv[4]):
    print(cc_usage.record_change(sys.argv[2], event)["id"])
"#;

#[test]
fn record_change_matches_python() {
    let scratch = Scratch::new("python");
    let (ours, theirs) = (
        scratch.0.join("ours.sqlite"),
        scratch.0.join("theirs.sqlite"),
    );
    let all = Value::Array(events()).to_string();
    let Some(ids) = run_python(
        ORACLE,
        &[
            theirs.as_os_str(),
            OsStr::new(&NOW.to_string()),
            OsStr::new(&all),
        ],
        &scratch.0.join("home"),
    ) else {
        return;
    };
    let conn = usage::open_usage_db_at(&ours).unwrap();
    usage::ensure_schema(&conn).unwrap();
    let mut got = String::new();
    for event in events() {
        let Value::Object(map) = event else {
            panic!("objeto")
        };
        let map: Map<String, Value> = map;
        got.push_str(&usage::record_change(&conn, &map, NOW).unwrap());
        got.push('\n');
    }
    assert_eq!(got, ids);
    let python = Connection::open(&theirs).unwrap();
    assert_eq!(rows(&conn), rows(&python));
    assert_eq!(rows(&conn).len(), 2);
}

#[test]
fn rejects_what_it_cannot_reproduce() {
    let scratch = Scratch::new("rechazo");
    let conn = usage::open_usage_db_at(&scratch.0.join("x.sqlite")).unwrap();
    usage::ensure_schema(&conn).unwrap();
    for event in [json!({"note": 5}), json!({"created_at": 10})] {
        let Value::Object(map) = event else {
            panic!("objeto")
        };
        assert!(usage::record_change(&conn, &map, NOW).is_err());
    }
}
