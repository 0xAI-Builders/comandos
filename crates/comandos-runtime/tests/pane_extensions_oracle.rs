#[path = "support/python.rs"]
mod python;
use comandos_runtime::{
    pane_extensions::{self as ext, ExtensionStore},
    session_operations::open_journal,
};
use serde_json::{Value, json};
use std::path::PathBuf;

struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        let p = std::env::temp_dir().join(comandos_runtime::fresh_id("pane-ext").unwrap());
        std::fs::create_dir_all(&p).unwrap();
        Self(p)
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn result(v: ext::Result<Value>) -> Value {
    match v {
        Ok(v) => v,
        Err(e) => json!({"error":e.to_string()}),
    }
}
fn normalized(mut v: Value) -> Value {
    if let Some(a) = v.as_array_mut() {
        for v in a {
            *v = normalized(v.take());
        }
    }
    if let Some(m) = v.as_object_mut()
        && m.contains_key("selection")
    {
        m.insert("id".into(), "<uuid>".into());
    }
    v
}

#[test]
fn drafts_revisions_active_operations_and_templates_match_python() {
    let tmp = Scratch::new();
    let script = r#"
import pathlib,sys,json
sys.path.insert(0,str(pathlib.Path(sys.argv[1])/'lib'))
import pane_extensions as p
p.time.time=lambda:1000.0
s=p.ExtensionStore(pathlib.Path.home()/'py.sqlite')
defaults={'mcps':{'mcp/a':True},'skills':{'skill+1':False}}
desired={'mcps':{'mcp/a':False},'skills':{}}
out=[]
def run(fn,*args):
    try:out.append(fn(*args))
    except ValueError as e:out.append({'error':str(e)})
row=s.state('pid|ñ','conversation','codex',defaults);out.append(row);k=row['key']
run(s.state,'pid|ñ','conversation','codex',desired)
run(s.save,k,0,desired);run(s.save,k,0,defaults);run(s.require_revision,k,True)
run(s.require_revision,'missing',0)
with s.operations.connect() as db:
    db.execute("INSERT INTO session_operations VALUES('active','pid|ñ','f','{}','recovery_required',1,NULL,NULL,0)")
run(s.save,k,1,defaults)
with s.operations.connect() as db:db.execute("UPDATE session_operations SET state='confirmed'")
run(s.save,k,1,defaults)
run(s.save_template,'  Mi plantilla  ',defaults)
run(s.save_template,'\nMala',defaults)
out.append(s.templates())
for i in out:
    if isinstance(i,dict) and 'selection' in i:i['id']='<uuid>'
    elif isinstance(i,list):
        for x in i:x['id']='<uuid>'
print(json.dumps(out,ensure_ascii=False))
"#;
    let Some(oracle) = python::run_python(script, &[], &tmp.0) else {
        return;
    };
    let conn = open_journal(&tmp.0.join("rs.sqlite")).unwrap();
    let store = ExtensionStore::new(&conn, &|| 1000.0).unwrap();
    let defaults = json!({"mcps":{"mcp/a":true},"skills":{"skill+1":false}});
    let desired = json!({"mcps":{"mcp/a":false},"skills":{}});
    let first = store
        .state("pid|ñ", "conversation", "codex", &defaults)
        .unwrap();
    let key = first["key"].as_str().unwrap().to_owned();
    let mut out = vec![first];
    out.push(result(store.state(
        "pid|ñ",
        "conversation",
        "codex",
        &desired,
    )));
    out.push(result(store.save(&key, &json!(0), &desired)));
    out.push(result(store.save(&key, &json!(0), &defaults)));
    out.push(result(store.require_revision(&key, &json!(true))));
    out.push(result(store.require_revision("missing", &json!(0))));
    conn.execute("INSERT INTO session_operations VALUES('active','pid|ñ','f','{}','recovery_required',1,NULL,NULL,0)",[]).unwrap();
    out.push(result(store.save(&key, &json!(1), &defaults)));
    conn.execute("UPDATE session_operations SET state='confirmed'", [])
        .unwrap();
    out.push(result(store.save(&key, &json!(1), &defaults)));
    let template = store
        .save_template(&json!("  Mi plantilla  "), &defaults)
        .unwrap();
    let id = template["id"].as_str().unwrap();
    assert_eq!(id.len(), 32);
    assert!(id.bytes().all(|b| b.is_ascii_hexdigit()));
    out.push(normalized(template));
    out.push(result(store.save_template(&json!("\nMala"), &defaults)));
    out.push(normalized(store.templates().unwrap().into()));
    assert_eq!(json!(out), serde_json::from_str::<Value>(&oracle).unwrap());
}

#[test]
fn selection_validation_and_identity_limits_match_python() {
    let tmp = Scratch::new();
    let cases = json!([{},null,[],{"mcps":{},"skills":{},"extra":1},{"mcps":[],"skills":{}},{"mcps":{"bad key":true},"skills":{}},{"mcps":{"x":1},"skills":{}},{"mcps":{},"skills":{"-+/@._:":true}},{"mcps":{},"skills":{"á":false}}]);
    let input = tmp.0.join("input.json");
    std::fs::write(&input, cases.to_string()).unwrap();
    let script = r#"
import sys,pathlib,json
sys.path.insert(0,str(pathlib.Path(sys.argv[1])/'lib'))
import pane_extensions as p
out=[]
for v in json.loads(pathlib.Path(sys.argv[2]).read_text()):
    try:out.append(p.selection_value(v))
    except ValueError as e:out.append({'error':str(e)})
print(json.dumps(out))
"#;
    let Some(oracle) = python::run_python(script, &[input.as_os_str()], &tmp.0) else {
        return;
    };
    let values: Vec<_> = cases
        .as_array()
        .unwrap()
        .iter()
        .map(|v| result(ext::selection_value(v)))
        .collect();
    assert_eq!(
        json!(values),
        serde_json::from_str::<Value>(&oracle).unwrap()
    );
    let conn = open_journal(&tmp.0.join("rs.sqlite")).unwrap();
    let store = ExtensionStore::new(&conn, &|| 0.0).unwrap();
    let empty = json!({"mcps":{},"skills":{}});
    assert!(store.state("", "c", "codex", &empty).is_err());
    assert!(store.state("i", &"ñ".repeat(257), "codex", &empty).is_err());
    assert!(store.state("i", "c", "gemini", &empty).is_err());
    conn.execute_batch("BEGIN").unwrap();
    assert!(
        store.state("i", "c", "codex", &empty).is_err(),
        "una escritura debe ser durable antes de responder"
    );
    conn.execute_batch("ROLLBACK").unwrap();
}

#[test]
fn concurrent_clients_cannot_overwrite_the_same_revision() {
    let tmp = Scratch::new();
    let path = tmp.0.join("concurrent.sqlite");
    let conn = open_journal(&path).unwrap();
    let store = ExtensionStore::new(&conn, &|| 0.0).unwrap();
    let key = store
        .state(
            "pane",
            "conversation",
            "codex",
            &json!({"mcps":{},"skills":{}}),
        )
        .unwrap()["key"]
        .as_str()
        .unwrap()
        .to_owned();
    let barrier = std::sync::Barrier::new(2);
    let results = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..2)
            .map(|i| {
                let (path, key, barrier) = (&path, &key, &barrier);
                scope.spawn(move || {
                    let conn = open_journal(path).unwrap();
                    let clock = || i as f64;
                    let store = ExtensionStore::new(&conn, &clock).unwrap();
                    barrier.wait();
                    store.save(key, &json!(0), &json!({"mcps":{"s":i==0},"skills":{}}))
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|h| h.join().unwrap())
            .collect::<Vec<_>>()
    });
    assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
    assert_eq!(
        results
            .iter()
            .filter(|r| matches!(r, Err(ext::Fault::Conflict(_))))
            .count(),
        1
    );
    assert_eq!(
        store.require_revision(&key, &json!(1)).unwrap()["revision"],
        1
    );
}
