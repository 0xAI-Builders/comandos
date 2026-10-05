//! Perfiles persistidos contra Python; bases y HOME privados, sin procesos de agentes.
mod support;

use comandos_store::session_profiles as profiles;
use rusqlite::Connection;
use serde_json::{Value, json};
use std::path::PathBuf;

struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path =
            std::env::temp_dir().join(format!("comandos-profiles-{}-{stamp}", std::process::id()));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn cases() -> Value {
    json!([
        {"id":"b","name":" beta ","skills":{"safe":false},"mcps":{"server":true}},
        {"id":"a","name":"Árbol","harness":"claude","harnessAccount":"work","model":"Modelo libre 💻"},
        {"id":"c","name":"Alpha","harness":"grok","motor":"grok","effort":"high"},
        {"id":"b","name":"Beta nueva","model":"m2","createdAt":1,"updatedAt":2},
        {"id":"bad","name":""},
        {"id":"bad","name":"hola\n"},
        {"id":"bad","name":"ok","extra":1},
        {"id":"-bad","name":"ok"},
        {"id":"bad","name":"ok","harness":null},
        {"id":"bad","name":"ok","effort":"con espacio"},
        {"id":"bad","name":"ok","harnessAccount":"_work"},
        {"id":"bad","name":"ok","motorAccount":""},
        {"id":"bad","name":"ok","skills":{"s":1}},
        {"id":"bad","name":"ok","mcps":{"bad key":true}},
        {"id":"bad","name":"ok","skills":[]},
        [], null
    ])
}

#[test]
fn save_list_get_delete_and_rows_match_python() {
    let tmp = Scratch::new();
    let input = tmp.0.join("cases.json");
    let cases = cases();
    std::fs::write(&input, cases.to_string()).unwrap();
    let script = r#"
import sys,json,pathlib
sys.path.insert(0,str(pathlib.Path(sys.argv[1])/'lib'))
sys.path.insert(0,str(pathlib.Path(sys.argv[1])/'bin'))
import session_profiles as p
p.time.time=lambda:1700000000
db=str(pathlib.Path.home()/'oracle.sqlite3')
out=[]
for data in json.loads(pathlib.Path(sys.argv[2]).read_text()):
    try:out.append({'ok':p.save_profile(db,data)})
    except ValueError as e:out.append({'error':str(e)})
out.append(p.list_profiles(db))
out.append(p.get_profile(db,'b'))
for ident in ('missing',None,'bad/id'):
    try:out.append(p.get_profile(db,ident))
    except ValueError as e:out.append({'error':str(e)})
p.delete_profile(db,'b');p.delete_profile(db,'missing')
out.append(p.list_profiles(db))
with p._connect(db) as con:
    out.append([list(r) for r in con.execute('select id,name,payload,created_at,updated_at from session_profiles order by id')])
print(json.dumps(out,ensure_ascii=False))
"#;
    let Some(oracle) = support::python::run_python(script, &[input.as_os_str()], &tmp.0) else {
        return;
    };
    let conn = Connection::open_in_memory().unwrap();
    let mut out = Vec::new();
    for data in cases.as_array().unwrap() {
        match profiles::save_profile(&conn, data, 1_700_000_000) {
            Ok(v) => out.push(json!({"ok":v})),
            Err(e) => out.push(json!({"error":e.to_string()})),
        }
    }
    out.push(profiles::list_profiles(&conn).unwrap().into());
    out.push(profiles::get_profile(&conn, &json!("b")).unwrap());
    for id in [json!("missing"), Value::Null, json!("bad/id")] {
        out.push(json!({"error":profiles::get_profile(&conn,&id).unwrap_err().to_string()}));
    }
    profiles::delete_profile(&conn, &json!("b")).unwrap();
    profiles::delete_profile(&conn, &json!("missing")).unwrap();
    out.push(profiles::list_profiles(&conn).unwrap().into());
    let mut stmt = conn
        .prepare("select id,name,payload,created_at,updated_at from session_profiles order by id")
        .unwrap();
    let rows: Vec<Value> = stmt
        .query_map([], |r| {
            Ok(json!([
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, i64>(3)?,
                r.get::<_, i64>(4)?
            ]))
        })
        .unwrap()
        .map(Result::unwrap)
        .collect();
    out.push(rows.into());
    assert_eq!(
        Value::Array(out),
        serde_json::from_str::<Value>(&oracle).unwrap()
    );
}

#[test]
fn generated_ids_and_updates_keep_creation_and_transactions() {
    let conn = Connection::open_in_memory().unwrap();
    let one = profiles::save_profile(&conn, &json!({"name":"Uno"}), 10).unwrap();
    let two = profiles::save_profile(&conn, &json!({"name":"Dos"}), 11).unwrap();
    let id = one["id"].as_str().unwrap();
    assert_eq!(id.len(), 32);
    assert!(id.bytes().all(|b| b.is_ascii_hexdigit()));
    assert_ne!(one["id"], two["id"]);
    let updated = profiles::save_profile(&conn, &json!({"id":id,"name":"Nuevo"}), 20).unwrap();
    assert_eq!(updated["createdAt"], 10);
    assert_eq!(updated["updatedAt"], 20);
    conn.execute_batch("BEGIN").unwrap();
    profiles::delete_profile(&conn, &one["id"]).unwrap();
    profiles::save_profile(&conn, &json!({"id":"nested","name":"Temporal"}), 30).unwrap();
    conn.execute_batch("ROLLBACK").unwrap();
    assert_eq!(profiles::get_profile(&conn, &one["id"]).unwrap(), updated);
    assert!(profiles::get_profile(&conn, &json!("nested")).is_err());
}

#[test]
fn unicode_name_limits_count_characters_and_python_whitespace() {
    let conn = Connection::open_in_memory().unwrap();
    assert!(profiles::save_profile(&conn, &json!({"id":"ok","name":"ñ".repeat(100)}), 0).is_ok());
    assert!(profiles::save_profile(&conn, &json!({"id":"bad","name":"ñ".repeat(101)}), 0).is_err());
    assert!(profiles::save_profile(&conn, &json!({"id":"bad","name":"\u{2003}"}), 0).is_err());
    let value =
        profiles::save_profile(&conn, &json!({"id":"ok","name":"\u{2003}Hola\u{a0}"}), 0).unwrap();
    assert_eq!(value["name"], "Hola");
}
