//! GET /extension-usage: nativa, byte a byte con `session_profiles.extension_usage`
//! (sin base, el resultado vacío tras las validaciones y sin crearla, D8) y con el
//! cc-dash del repo sobre una base sembrada.
mod support;
use comandos_server::dash::native::wall_clock_ms;
use serde_json::{Value, json};
use std::{process::Command, sync::Arc};
use support::{FakeLegacy, TestHome, dead_port, front, get, oracle::oracle, seed_usage};

/// `lastSeen` depende del segundo de la siembra: se iguala.
fn masked(text: &str) -> String {
    let mut v: Value = serde_json::from_str(text).unwrap();
    if let Some(list) = v.get_mut("extensions").and_then(Value::as_array_mut) {
        for e in list {
            e["lastSeen"] = json!(0);
        }
    }
    comandos_core::json::response_dumps(&v).unwrap()
}

/// `(status, cuerpo)` de `extension_usage` del Python sobre una base que no existe,
/// con el `except (ValueError, OSError)` de la ruta (`bin/cc-dash`).
fn python_without_db(home: &TestHome, cases: &[(&str, &str, &str)]) -> Option<Vec<(u16, String)>> {
    let repo = support::repo();
    let args: Vec<Value> = cases.iter().map(|(s, p, d)| json!([s, p, d])).collect();
    let script = format!(
        "import json, sys\n\
         sys.path[:0] = [{bin:?}, {lib:?}]\n\
         import session_profiles\n\
         for s, p, d in json.loads(sys.argv[1]):\n\
         \x20   try:\n\
         \x20       print(200, json.dumps(session_profiles.extension_usage({db:?}, s, p, days=d)))\n\
         \x20   except (ValueError, OSError) as e:\n\
         \x20       print(400, json.dumps({{'error': str(e)}}))\n",
        bin = repo.join("bin").display().to_string(),
        lib = repo.join("lib").display().to_string(),
        db = home
            .root
            .join("no-existe/comandos-usage.sqlite")
            .display()
            .to_string(),
    );
    let output = Command::new("python3")
        .arg("-c")
        .arg(script)
        .arg(Value::Array(args).to_string())
        .env("HOME", &home.root)
        .env("LANG", "C.UTF-8")
        .env_remove("LC_ALL")
        .env_remove("COMANDOS_USAGE_DB")
        .env_remove("COMANDOS_STATE_DB")
        .output();
    let Ok(output) = output else {
        eprintln!("python3 no está instalado: se salta la comparación con el oráculo");
        return None;
    };
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    Some(
        String::from_utf8(output.stdout)
            .unwrap()
            .lines()
            .map(|line| {
                let (status, body) = line.split_once(' ').unwrap();
                (status.parse().unwrap(), body.to_owned())
            })
            .collect(),
    )
}

/// Sin base: las validaciones van primero (los 400 del Python) y después el
/// resultado vacío; la ruta no crea la base ni abre el carril (D8).
#[tokio::test]
async fn extension_usage_without_db_matches_python_and_never_creates_it() {
    let home = TestHome::new("ext-nodb");
    let legacy = FakeLegacy::start().await;
    let front = front(&home, legacy.port, home.options()).await;
    // (consulta, session, pane, days) ya decodificados como `parse_qs`.
    let cases: &[(&str, (&str, &str, &str))] = &[
        ("/extension-usage", ("", "", "7")),
        ("/extension-usage?session=s1&pane=%251", ("s1", "%1", "7")),
        ("/extension-usage?session=s1&days=30", ("s1", "", "30")),
        ("/extension-usage?days=200", ("", "", "200")),
        ("/extension-usage?days=-5", ("", "", "-5")),
        ("/extension-usage?days=%201_0%20", ("", "", " 1_0 ")),
        ("/extension-usage?days=%2B2", ("", "", "+2")),
        ("/extension-usage?days=x", ("", "", "x")),
        ("/extension-usage?days=7.5", ("", "", "7.5")),
        ("/extension-usage?days=%20", ("", "", " ")),
        ("/extension-usage?days=", ("", "", "7")),
        ("/extension-usage?days=a'b", ("", "", "a'b")),
        ("/extension-usage?pane=%251", ("", "%1", "7")),
        ("/extension-usage?session=s1&pane=1", ("s1", "1", "7")),
        ("/extension-usage?session=a%20b", ("a b", "", "7")),
        ("/extension-usage?session=a%20b&days=x", ("a b", "", "x")),
        (
            "/extension-usage?session=s1&pane=x&days=x",
            ("s1", "x", "x"),
        ),
        ("/extension-usage?session=%C3%B1", ("ñ", "", "7")),
        (
            "/extension-usage?session=s1&session=s2&days=3&days=x",
            ("s1", "", "3"),
        ),
        ("/extension-usage?session=a+b", ("a b", "", "7")),
    ];
    let mut got = Vec::new();
    for (target, _) in cases {
        let wire = get(front.port, target).await;
        assert_eq!(wire.header("content-type"), Some("application/json"));
        assert_eq!(wire.header("cache-control"), Some("no-store"));
        got.push((wire.status, wire.text()));
    }
    // Lo que el port no reproduce con certeza se reenvía: un `int()` fuera de
    // `i64` (el Python lo acota a 90) y dígitos no ASCII.
    for target in [
        "/extension-usage?days=99999999999999999999",
        "/extension-usage?days=%D9%A3",
    ] {
        assert_eq!(get(front.port, target).await.text(), r#"{"legacy": true}"#);
    }
    front.stop().await;
    assert!(
        !home.usage_db().exists(),
        "una ruta de lectura no crea la base"
    );
    assert_eq!(legacy.requests().len(), 2, "{:?}", legacy.requests());
    let inputs: Vec<(&str, &str, &str)> = cases.iter().map(|(_, c)| *c).collect();
    let Some(expected) = python_without_db(&home, &inputs) else {
        return;
    };
    for ((target, _), (got, want)) in cases.iter().zip(got.iter().zip(expected.iter())) {
        assert_eq!(got, want, "{target}");
    }
}

/// Con base: los dos lados sobre el mismo HOME sembrado.
#[tokio::test]
async fn extension_usage_matches_python() {
    let home = TestHome::new("ext-db");
    let now = wall_clock_ms();
    let old = now - 10 * 86_400_000;
    seed_usage(
        &home,
        &format!(
            "insert into usage_interactions(id,tmux_session,tmux_pane,started_at_ms,source,confidence,created_at) values\
             ('i1','s1','%1',{now},'hook:claude','exact',1),('i2','s1','%2',{now},'hook:codex','exact',1),\
             ('i3','s2','%1',{old},'hook:claude','exact',1);\
             insert into usage_tool_calls(id,interaction_id,sequence,tool_name,skill_name,started_at_ms,finished_at_ms,duration_ms,status,confidence) values\
             ('c1','i1',1,'Skill','tdd',{now},{now},20,'ok','exact'),\
             ('c2','i1',2,'mcp__mobbin__search','',{now},{now},null,'failed','exact'),\
             ('c3','i1',3,'Skill','',{now},{now},5,'ok','exact'),\
             ('c4','i2',4,'mcp__mobbin__get','',{now},{now},7,'ok','inferred'),\
             ('c5','i2',5,'skill','tdd',{now},null,3,'ok','exact'),\
             ('c6','i2',6,'Bash','',{now},{now},1,'ok','exact'),\
             ('c7','i2',7,'mcp__x','',{now},{now},1,'ok','exact'),\
             ('c8','i2',8,'Skill','bad name',{now},{now},1,'ok','exact'),\
             ('c9','i3',9,'Skill','vieja',{old},{old},9,'ok','exact');"
        ),
    );
    let Some(py) = oracle(&home).await else {
        return;
    };
    let mut opts = home.options();
    opts.clock = Arc::new(wall_clock_ms);
    let front = front(&home, dead_port(), opts).await;
    for target in [
        "/extension-usage",
        "/extension-usage?session=s1",
        "/extension-usage?session=s1&pane=%251",
        "/extension-usage?session=s1&pane=%252",
        "/extension-usage?session=s2&days=30",
        "/extension-usage?days=90",
        "/extension-usage?days=0",
        "/extension-usage?days=x",
        "/extension-usage?pane=%251",
        "/extension-usage?session=a%20b",
        "/extension-usage?session=zz",
    ] {
        let a = get(front.port, target).await;
        let b = get(py.port, target).await;
        assert_eq!(a.status, b.status, "{target}: {} / {}", a.text(), b.text());
        assert_eq!(
            a.header("content-type"),
            b.header("content-type"),
            "{target}"
        );
        assert_eq!(
            a.header("cache-control"),
            b.header("cache-control"),
            "{target}"
        );
        assert_eq!(masked(&a.text()), masked(&b.text()), "{target}");
    }
    // Hubo extensiones de verdad (no una comparación de vacíos).
    let body: Value =
        serde_json::from_str(&get(front.port, "/extension-usage?session=s1").await.text()).unwrap();
    assert_eq!(body["status"], json!("observed"), "{body}");
    assert!(body["extensions"].as_array().unwrap().len() >= 2, "{body}");
    front.stop().await;
}

#[tokio::test]
async fn extension_usage_prefix_is_still_forwarded() {
    let home = TestHome::new("ext-prefix");
    let legacy = FakeLegacy::start().await;
    let front = front(&home, legacy.port, home.options()).await;
    let wire = get(front.port, "/extension-usageX").await;
    assert_eq!(wire.text(), r#"{"legacy": true}"#);
    front.stop().await;
}

/// Base de uso más nueva: el carril se apaga y la ruta se reenvía (D10), también
/// sus errores de validación; el frente no toca la base.
#[tokio::test]
async fn newer_usage_db_forwards_extension_usage() {
    let home = TestHome::new("ext-newer");
    let conn = rusqlite::Connection::open(home.usage_db()).unwrap();
    conn.execute_batch("pragma user_version=12").unwrap();
    drop(conn);
    let legacy = FakeLegacy::start().await;
    let front = front(&home, legacy.port, home.options()).await;
    for target in [
        "/extension-usage",
        "/extension-usage?days=x",
        "/extension-usage?session=a%20b",
    ] {
        assert_eq!(get(front.port, target).await.text(), r#"{"legacy": true}"#);
    }
    front.stop().await;
    assert_eq!(legacy.requests().len(), 3);
    let conn = rusqlite::Connection::open(home.usage_db()).unwrap();
    let version: i64 = conn
        .query_row("pragma user_version", [], |r| r.get(0))
        .unwrap();
    assert_eq!(version, 12);
}
