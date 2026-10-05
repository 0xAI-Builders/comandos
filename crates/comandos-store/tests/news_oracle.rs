//! Lecturas de noticias (plan 2f-4, Tarea 1) contra `lib/news_editions.py` y
//! `lib/news_reading.py` sobre la misma base sembrada: cada función, con
//! valores raros (JSON roto, metas que no son objeto, costos que redondear).
//! Ninguna red: solo SQLite y archivos del HOME temporal.
mod support;

use comandos_core::json::response_dumps;
use comandos_store::{
    news::{self, Fault},
    state::{self, MIGRATIONS},
};
use rusqlite::Connection;
use serde_json::{Map, Value, json};
use std::{ffi::OsStr, path::Path};
use support::python::run_python;

const NOW_MS: i64 = 1_791_115_200_000;

/// Filas como las que deja `_store` (más casos raros a mano).
pub const SEED: &str = r#"
INSERT INTO news_editions (id, local_date, slot, timezone, scheduled_at_ms, status, published_at_ms, title,
  story_count, source_count, failed_source_count, cost_usd, model, notes, lead) VALUES
 ('2026-10-03@09:00','2026-10-03','09:00','America/Mexico_City',1791039600000,'published',1791039700000,'Tu edición de IA',2,3,1,0.123456789,'acp:opencode/x','["una nota"]','Lo **importante**'),
 ('2026-10-03@15:00','2026-10-03','15:00','America/Mexico_City',1791061200000,'partial',1791061300000,'Tu edición de IA',1,1,0,0.0078125,NULL,'[]',NULL),
 ('2026-10-03@21:00','2026-10-03','21:00','America/Mexico_City',1791082800000,'empty',NULL,'Tu edición de IA',0,0,2,0,NULL,'["sin novedades","ñandú"]',NULL),
 ('2026-10-04@09:00','2026-10-04','09:00','America/Mexico_City',1791126000000,'scheduled',NULL,NULL,0,0,0,0,NULL,'[]',NULL),
 ('2030-01-01@09:00','2030-01-01','09:00','America/Mexico_City',1893510000000,'scheduled',NULL,NULL,0,0,0,0,NULL,'[]',NULL);
INSERT INTO news_jobs (edition_id, state, attempts, lease_until_ms, started_at_ms, finished_at_ms, error) VALUES
 ('2026-10-03@09:00','done',1,NULL,1791039600000,1791039700000,NULL),
 ('2026-10-03@21:00','failed',2,NULL,1791082800000,1791082900000,'sin fuentes');
INSERT INTO news_sources (id, url, original_url, title, origin, category, published_at_ms, discovered_at_ms,
  verified_at_ms, fetch_status, fetch_error, meta) VALUES
 (1,'https://openai.com/blog/x','https://openai.com/blog/x?utm_source=a','Anuncio','openai.com','oficial',1791000000000,1791030000000,1791039700000,'ok',NULL,'{"official": true, "role": "article", "heat": ""}'),
 (2,'https://news.ycombinator.com/item?id=1','https://news.ycombinator.com/item?id=1','Hilo HN','hn','hot',NULL,1791030000000,NULL,'failed','timeout','{"role": "discussion", "heat": "alto"}'),
 (3,'https://blog.example.com/a','https://blog.example.com/a','Artículo «ñ»','blog','ia',1791000000000,1791030000000,1791039700000,'ok',NULL,'{malo'),
 (4,'https://x.example/b','https://x.example/b','Otro','x','ia',NULL,1791030000000,NULL,'not_fetched',NULL,'[1, 2]'),
 (5,'https://y.example/c','https://y.example/c','Sin meta','y','ia',NULL,1791030000000,NULL,'ok',NULL,'');
INSERT INTO news_captures (source_id, captured_at_ms, final_url, title, byline, lang, blocks, partial) VALUES
 (1,1791039700000,'https://openai.com/blog/x','Anuncio','Equipo','en','[{"type": "h", "text": "Título"}, {"type": "p", "text": "Párrafo con \"comillas\" y 1.50"}, {"type": "img", "src": "/news/media/0123456789abcdef0123456789abcdef.png", "alt": "fig"}]',0),
 (3,1791039700000,'https://blog.example.com/a',NULL,NULL,NULL,'{"no": "lista"}',1),
 (4,1791039700000,'https://x.example/b','Otro',NULL,'es','[roto',0);
INSERT INTO news_stories (id, edition_id, position, story_key, category, title, summary_md, body_md, opportunity, model, meta) VALUES
 (10,'2026-10-03@09:00',0,'k1','oficial','Lanzan X','Resumen **uno**','Cuerpo uno',NULL,'acp:opencode/x','{"heat": "alto", "kind": "oficial"}'),
 (11,'2026-10-03@09:00',1,'k2','bounty','Recompensa','Resumen dos','Cuerpo dos','{"reward": "1000 USD", "deadline": null}','acp:claude/y','not json'),
 (12,'2026-10-03@15:00',0,'k3','ia','Tercera','Resumen tres','Cuerpo tres','','acp:opencode/x','[]'),
 (13,'2026-10-03@09:00',2,'k4','ia','Cuarta','R4','C4',NULL,'acp:opencode/x','{}');
INSERT INTO news_story_sources (story_id, source_id) VALUES
 (10,2),(10,1),(10,3),(11,4),(11,2),(12,5),(13,1);
INSERT INTO news_translations (source_id, lang, state, title, blocks, model, error, created_at_ms, updated_at_ms) VALUES
 (1,'es','done','Anuncio (es)','[{"type": "h", "text": "Título es"}]','acp:opencode/x',NULL,1791040000000,1791040100000),
 (3,'es','failed',NULL,'{"raro": 1}',NULL,'interrumpida',1791040000000,1791040200000),
 (4,'es','running',NULL,'[roto',NULL,NULL,1791040000000,1791040300000),
 (1,'en','done','Anuncio','[]',NULL,NULL,1791040000000,1791040100000);
INSERT INTO news_chat (id, story_id, edition_id, role, state, text, cite, model, created_at_ms) VALUES
 (1,10,'2026-10-03@09:00','user','done','¿Qué cambia?',NULL,NULL,1791040000000),
 (2,10,'2026-10-03@09:00','assistant','done','Cambia todo','s1',NULL,1791040000001),
 (3,10,'2026-10-03@09:00','user','done','¿Y el precio?',NULL,NULL,1791040000100),
 (4,10,'2026-10-03@09:00','assistant','failed','No pude responder: x',NULL,'acp:opencode/x',1791040000101),
 (5,11,'2026-10-03@09:00','user','done','hola',NULL,NULL,1791040000200);
INSERT INTO news_notes (id, story_id, edition_id, story_title, kind, chat_id, quote, cite, text, created_at_ms, updated_at_ms) VALUES
 (1,10,'2026-10-03@09:00','Lanzan X','libre',NULL,NULL,NULL,'Una PALABRA clave',1791040000000,1791040000000),
 (2,10,'2026-10-03@09:00','Lanzan X','chat',2,'Cambia todo','s1','Respuesta de la IA, guardada desde el chat.',1791040000500,1791040000500),
 (3,11,'2026-10-03@09:00','Recompensa','libre',NULL,'cita con palabra',NULL,'otra',1791040000600,1791040000700),
 (4,12,'2026-10-03@15:00','Árbol 100%_x','libre',NULL,NULL,NULL,'árbol',1791040000800,1791040000800);
INSERT INTO news_saved (story_id, edition_id, saved_at_ms) VALUES
 (10,'2026-10-03@09:00',1791040000000),(12,'2026-10-03@15:00',1791040000900);
"#;

struct Scratch(std::path::PathBuf);

impl Scratch {
    fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("cmd-news-{tag}-{}", std::process::id()));
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

fn seeded(path: &Path, extra: &str) -> Connection {
    let conn = state::connect(path).unwrap();
    state::migrate(&conn, MIGRATIONS, (NOW_MS / 1000) as f64).unwrap();
    conn.execute_batch(SEED).unwrap();
    conn.execute_batch(extra).unwrap();
    conn
}

/// `"RAISE"` donde el Python lanzaría; las pruebas no esperan dudas.
fn out<T: Into<Value>>(got: news::Result<T>) -> Value {
    match got {
        Ok(v) => v.into(),
        Err(Fault::Raise(_)) => json!("RAISE"),
        Err(other) => panic!("inesperado: {other}"),
    }
}

fn opt(got: news::Result<Option<Value>>) -> Value {
    out(got.map(|v| v.unwrap_or(Value::Null)))
}

const STORIES: [i64; 6] = [10, 11, 12, 13, 99, 0];
const SOURCES: [i64; 6] = [1, 2, 3, 4, 5, 999];
const EDITIONS: [&str; 6] = [
    "2026-10-03@09:00",
    "2026-10-03@15:00",
    "2026-10-03@21:00",
    "2026-10-04@09:00",
    "2030-01-01@09:00",
    "2026-01-01@09:00",
];
const NOTE_QUERIES: [&str; 7] = ["", "palabra", " PaLaBrA ", "%_", "árbol", "ÁRBOL", "100%_x"];

const ORACLE: &str = r#"
import json, sys
sys.path.insert(0, sys.argv[1] + "/lib")
import app_state, news_editions as ne, news_reading as nr
conn = app_state.connect(sys.argv[2])
now = int(sys.argv[3])
spec = json.loads(sys.argv[4])

def safe(f):
    try:
        return f()
    except Exception:
        return "RAISE"

out = {}
out["latest"] = safe(lambda: ne.latest_readable(conn))
out["list"] = safe(lambda: ne.list_editions(conn, 30, until_ms=now))
out["list_all"] = safe(lambda: ne.list_editions(conn, 500))
out["list_one"] = safe(lambda: ne.list_editions(conn, 0, until_ms=now))
out["next"] = safe(lambda: ne.next_scheduled(conn, now))
out["editions"] = [safe(lambda e=e: ne.get_edition(conn, e)) for e in spec["editions"]]
out["sources"] = [safe(lambda s=s: ne.get_source(conn, s)) for s in spec["sources"]]
out["counts"] = safe(lambda: nr.story_counts(conn, spec["stories"]))
out["rows"] = [safe(lambda s=s: nr.story_row(conn, s)) for s in spec["stories"]]
out["chat"] = [safe(lambda s=s: nr.chat_history(conn, s)) for s in spec["stories"]]
out["notes"] = [safe(lambda q=q: nr.list_notes(conn, story_id=None, query=q)) for q in spec["queries"]]
out["notes_story"] = [safe(lambda s=s: nr.list_notes(conn, story_id=s, query="")) for s in spec["stories"]]
out["notes_limit"] = safe(lambda: nr.list_notes(conn, query="", limit=2))
out["saved"] = safe(lambda: nr.saved_stories(conn))
out["translations"] = [safe(lambda s=s: nr.translation(conn, s, "es")) for s in spec["sources"]]
out["translation_en"] = safe(lambda: nr.translation(conn, 1, "en"))
print(json.dumps(out))
"#;

fn rust_side(conn: &Connection) -> Value {
    let counts = news::story_counts(conn, &STORIES).map(|map| {
        let mut obj = Map::new();
        for id in STORIES {
            if let Some(v) = map.get(&id) {
                obj.insert(id.to_string(), v.clone());
            }
        }
        Value::Object(obj)
    });
    json!({
        "latest": opt(news::latest_readable(conn)),
        "list": out(news::list_editions(conn, 30, Some(NOW_MS))),
        "list_all": out(news::list_editions(conn, 500, None)),
        "list_one": out(news::list_editions(conn, 0, Some(NOW_MS))),
        "next": opt(news::next_scheduled(conn, NOW_MS)),
        "editions": EDITIONS.iter().map(|e| opt(news::get_edition(conn, e))).collect::<Vec<_>>(),
        "sources": SOURCES.iter().map(|s| opt(news::get_source(conn, *s))).collect::<Vec<_>>(),
        "counts": out(counts),
        "rows": STORIES.iter().map(|s| opt(news::story_row(conn, *s))).collect::<Vec<_>>(),
        "chat": STORIES.iter().map(|s| out(news::chat_history(conn, *s))).collect::<Vec<_>>(),
        "notes": NOTE_QUERIES.iter().map(|q| out(news::list_notes(conn, None, q, 500))).collect::<Vec<_>>(),
        "notes_story": STORIES.iter().map(|s| out(news::list_notes(conn, Some(*s), "", 500))).collect::<Vec<_>>(),
        "notes_limit": out(news::list_notes(conn, None, "", 2)),
        "saved": out(news::saved_stories(conn)),
        "translations": SOURCES.iter().map(|s| opt(news::translation(conn, *s, "es"))).collect::<Vec<_>>(),
        "translation_en": opt(news::translation(conn, 1, "en")),
    })
}

fn compare(tag: &str, extra: &str) {
    let scratch = Scratch::new(tag);
    let db = scratch.0.join("app-state.sqlite3");
    let conn = seeded(&db, extra);
    let spec = json!({"editions": EDITIONS, "sources": SOURCES, "stories": STORIES, "queries": NOTE_QUERIES});
    let Some(expected) = run_python(
        ORACLE,
        &[
            db.as_os_str(),
            OsStr::new(&NOW_MS.to_string()),
            OsStr::new(&spec.to_string()),
        ],
        &scratch.0.join("home"),
    ) else {
        return;
    };
    let got = response_dumps(&rust_side(&conn)).unwrap();
    let want: Value = serde_json::from_str(expected.trim_end()).unwrap();
    let got_value: Value = serde_json::from_str(&got).unwrap();
    for (key, value) in want.as_object().unwrap() {
        assert_eq!(
            response_dumps(&got_value[key]).unwrap(),
            response_dumps(value).unwrap(),
            "{tag}: {key}"
        );
    }
    // Además, el texto completo: mismo `json.dumps` byte a byte.
    assert_eq!(got, expected.trim_end(), "{tag}: texto");
}

#[test]
fn reads_match_python() {
    compare("reads", "");
}

#[test]
fn broken_json_and_odd_types_match_python() {
    // `notes` roto → `json.loads` lanza en `_edition_row`; costos enteros y de
    // texto; `opportunity` entero (TypeError); ids de chat REAL en notas.
    compare(
        "odd",
        r#"
        UPDATE news_editions SET notes = '[roto' WHERE id = '2026-10-03@15:00';
        UPDATE news_editions SET cost_usd = 3 WHERE id = '2026-10-03@21:00';
        UPDATE news_stories SET opportunity = 7 WHERE id = 13;
        UPDATE news_stories SET opportunity = '{"x": 1.50, "y": 1e2}' WHERE id = 12;
        UPDATE news_captures SET partial = 'sí' WHERE source_id = 3;
        UPDATE news_sources SET meta = '{"official": 0, "role": null, "heat": 2.50}' WHERE id = 5;
        INSERT INTO news_notes (story_id, edition_id, story_title, kind, chat_id, quote, cite, text, created_at_ms, updated_at_ms)
          VALUES (10,'2026-10-03@09:00','Lanzan X','chat',3.0,NULL,NULL,'real',1791040001000,1791040001000);
        "#,
    );
}

#[test]
fn rounding_matches_python() {
    let scratch = Scratch::new("round");
    let values = [
        0.0078125,
        0.0078135,
        0.123456789,
        1.0000005,
        2.5e-7,
        5e-7,
        1.5e-6,
        0.1 + 0.2,
        123456.7890125,
        1e-300,
        9.999_999_5,
        4503599627370495.5,
        1e22,
    ];
    let script = r#"
import json, sys
print(json.dumps([round(float(x), 6) for x in json.loads(sys.argv[2])]))
"#;
    let arg = serde_json::to_string(&values.to_vec()).unwrap();
    let Some(expected) = run_python(script, &[OsStr::new(&arg)], &scratch.0.join("home")) else {
        return;
    };
    let got: Vec<Value> = values
        .iter()
        .map(|x| json!(news::py_round(*x, 6).unwrap()))
        .collect();
    assert_eq!(response_dumps(&json!(got)).unwrap(), expected.trim_end());
}

#[test]
fn config_matches_python() {
    let scratch = Scratch::new("config");
    let files: &[(&str, &str)] = &[
        ("ausente", ""),
        ("roto", "{"),
        ("bom", "\u{feff}{\"enabled\": true}"),
        ("lista", "[1]"),
        ("vacio", "{}"),
        ("apagado", r#"{"enabled": 1}"#),
        (
            "sin-proveedor",
            r#"{"enabled": true, "summarizer": {"kind": "x"}}"#,
        ),
        (
            "cadena",
            r#"{"enabled": true, "summarizer": {"kind": "chain", "steps": [{"agent": "opencode", "model": "m"}, {"agent": "claude"}]}, "policy": {"slots": ["21:00", "09:00", "15:00"], "budgetUsd": 1, "maxSources": 40, "maxStories": 10, "maxSeconds": 300}}"#,
        ),
        (
            "cadena-vacia",
            r#"{"enabled": true, "summarizer": {"kind": "chain", "steps": []}}"#,
        ),
        (
            "cadena-mala",
            r#"{"enabled": true, "summarizer": {"kind": "chain", "steps": [{"agent": ""}]}}"#,
        ),
        (
            "acp",
            r#"{"enabled": true, "summarizer": {"kind": "acp", "agent": "opencode", "model": "free"}, "policy": {"slots": ["09:00", "15:00\n", "23:59"], "budgetUsd": 0, "maxSources": 40.0, "maxSeconds": true}}"#,
        ),
        (
            "acp-sin-modelo",
            r#"{"enabled": true, "summarizer": {"kind": "acp", "agent": "opencode"}}"#,
        ),
        (
            "acp-agente-num",
            r#"{"enabled": true, "summarizer": {"kind": "acp", "agent": 5, "model": "m"}}"#,
        ),
        (
            "anthropic",
            r#"{"enabled": true, "summarizer": {"kind": "anthropic-messages", "model": "m", "inputUsdPerMTok": 3, "outputUsdPerMTok": 15.5, "apiKeyEnv": "MI_CLAVE"}, "policy": {"slots": ["24:00", "09:00", "15:00"], "budgetUsd": 0.5}}"#,
        ),
        (
            "anthropic-sin-clave",
            r#"{"enabled": true, "summarizer": {"kind": "anthropic-messages", "model": "m", "inputUsdPerMTok": 3, "outputUsdPerMTok": 1, "apiKeyEnv": "VACIA"}}"#,
        ),
        (
            "anthropic-precio",
            r#"{"enabled": true, "summarizer": {"kind": "anthropic-messages", "model": "m", "inputUsdPerMTok": -1, "outputUsdPerMTok": 1, "apiKeyEnv": "MI_CLAVE"}}"#,
        ),
        (
            "anthropic-bool",
            r#"{"enabled": true, "summarizer": {"kind": "anthropic-messages", "model": "m", "inputUsdPerMTok": true, "outputUsdPerMTok": 1, "apiKeyEnv": "MI_CLAVE"}}"#,
        ),
        (
            "anthropic-nan",
            r#"{"enabled": true, "summarizer": {"kind": "anthropic-messages", "model": "m", "inputUsdPerMTok": NaN, "outputUsdPerMTok": 1, "apiKeyEnv": "X"}}"#,
        ),
        (
            "openai",
            r#"{"enabled": true, "summarizer": {"kind": "openai-chat", "model": "m", "inputUsdPerMTok": 0, "outputUsdPerMTok": 0, "apiKeyEnv": "MI_CLAVE", "baseUrl": "http://x"}}"#,
        ),
        (
            "openai-https",
            r#"{"enabled": true, "summarizer": {"kind": "openai-chat", "model": "m", "inputUsdPerMTok": 0, "outputUsdPerMTok": 0, "apiKeyEnv": "MI_CLAVE", "baseUrl": "https://x"}}"#,
        ),
        (
            "sin-apikeyenv",
            r#"{"enabled": true, "summarizer": {"kind": "openai-chat", "model": "m", "inputUsdPerMTok": 0, "outputUsdPerMTok": 0}}"#,
        ),
    ];
    let dir = scratch.0.join("cfg");
    std::fs::create_dir_all(&dir).unwrap();
    for (name, text) in files {
        if *name != "ausente" {
            std::fs::write(dir.join(name), text).unwrap();
        }
    }
    let names: Vec<&str> = files.iter().map(|(n, _)| *n).collect();
    let script = r#"
import json, os, sys
sys.path.insert(0, sys.argv[1] + "/lib")
import news_editions as ne
env = {"MI_CLAVE": "x", "VACIA": ""}
out = []
for name in json.loads(sys.argv[3]):
    config = ne.load_config(os.path.join(sys.argv[2], name))
    out.append([config, ne.config_status(config, env), ne.policy_from_config(config)])
print(json.dumps(out))
"#;
    let Some(expected) = run_python(
        script,
        &[dir.as_os_str(), OsStr::new(&json!(names).to_string())],
        &scratch.0.join("home"),
    ) else {
        return;
    };
    let env = |key: &str| key == "MI_CLAVE";
    let got: Vec<Value> = names
        .iter()
        .map(|name| {
            let config = news::load_config(&dir.join(name)).unwrap();
            let status = news::config_status(config.as_ref(), &env);
            let policy = news::policy_from_config(config.as_ref()).unwrap();
            // Sin `json!`: un `NaN` leído no se puede volver a serializar con serde.
            Value::Array(vec![
                config.map_or(Value::Null, Value::Object),
                status,
                Value::Object(policy),
            ])
        })
        .collect();
    assert_eq!(
        response_dumps(&Value::Array(got)).unwrap(),
        expected.trim_end()
    );
}

// ---------------------------------------------------------------- escrituras (Tarea 2)

/// Filas extra para `toggle_chat_note`: un chat de una noticia que no existe,
/// uno vacío, uno de solo blancos, uno con cita larga y uno con texto numérico.
const WRITE_EXTRA: &str = r#"
INSERT INTO news_chat (id, story_id, edition_id, role, state, text, cite, model, created_at_ms) VALUES
 (6,77,'2026-10-03@09:00','assistant','done','huérfano',NULL,NULL,1791040000300),
 (7,10,'2026-10-03@09:00','assistant','done','',NULL,NULL,1791040000400),
 (8,10,'2026-10-03@09:00','assistant','done','   ',NULL,NULL,1791040000500),
 (9,11,'2026-10-03@09:00','assistant','done','  respuesta  ','  cita larga cita larga cita larga cita larga cita larga cita larga cita larga cita larga cita larga cita larga cita larga cita larga cita larga cita larga cita larga cita larga cita larga cita larga cita larga cita larga  ',NULL,1791040000600),
 (10,12,'2026-10-03@15:00','user','done',42,NULL,NULL,1791040000700);
"#;

/// Cada operación: `[función, id, argumento]` con los tipos del Python.
fn write_ops() -> Value {
    let long = "x".repeat(9000);
    json!([
        ["saved", 11, true],
        ["saved", 11, true],
        ["saved", 10, false],
        ["saved", 10, false],
        ["saved", 99, true],
        ["add", 10, "  Una nota nueva  "],
        ["add", 10, ""],
        ["add", 10, "   "],
        ["add", 10, null],
        ["add", 10, 0],
        ["add", 10, false],
        ["add", 99, "x"],
        ["add", null, "x"],
        ["add", 12, 5],
        ["add", 12, true],
        ["add", 12, -7],
        ["add", 11, long],
        ["add", 11, "\u{a0}\tñandú\u{2003}\n"],
        ["update", 1, "editada"],
        ["update", 1, "editada"],
        ["update", 999, "x"],
        ["update", null, "x"],
        ["update", 3, ""],
        ["update", 3, null],
        ["update", 4, 12],
        ["delete", 4],
        ["delete", 4],
        ["delete", null],
        ["toggle", 2],
        ["toggle", 2],
        ["toggle", 1],
        ["toggle", 4],
        ["toggle", 99],
        ["toggle", null],
        ["toggle", 6],
        ["toggle", 7],
        ["toggle", 8],
        ["toggle", 9],
        ["toggle", 10],
        ["toggle", 9],
    ])
}

const WRITE_ORACLE: &str = r#"
import json, sys
sys.path.insert(0, sys.argv[1] + "/lib")
import app_state, news_reading as nr
conn = app_state.connect(sys.argv[2])
now = int(sys.argv[3])

def run(op):
    kind, ident, arg = (op + [None])[:3]
    try:
        if kind == "saved":
            return nr.set_saved(conn, ident, arg, now=now)
        if kind == "add":
            return nr.add_note(conn, ident, arg, now=now)
        if kind == "update":
            return nr.update_note(conn, ident, arg, now=now)
        if kind == "delete":
            return nr.delete_note(conn, ident)
        return nr.toggle_chat_note(conn, ident, now=now)
    except LookupError as exc:
        return {"E": ["lookup", str(exc).strip("'")]}
    except ValueError as exc:
        return {"E": ["value", str(exc)]}
    except RuntimeError as exc:
        return {"E": ["runtime", str(exc)]}

print(json.dumps([run(op) for op in json.loads(sys.argv[4])]))
"#;

/// Las filas de las tablas que tocan las escrituras, volcadas por el Python.
const DUMP_ORACLE: &str = r#"
import json, sqlite3, sys
conn = sqlite3.connect(sys.argv[2])
print(json.dumps({t: [list(r) for r in conn.execute(f"SELECT * FROM {t} ORDER BY rowid")]
                  for t in ("news_notes", "news_saved", "news_chat")}))
"#;

fn write_out<T: Into<Value>>(got: std::result::Result<T, news::NewsError>) -> Value {
    match got {
        Ok(v) => v.into(),
        Err(news::NewsError::Lookup(m)) => json!({"E": ["lookup", m.trim_matches('\'')]}),
        Err(news::NewsError::Value(m)) => json!({"E": ["value", m]}),
        Err(news::NewsError::Runtime(m)) => json!({"E": ["runtime", m]}),
        Err(other) => panic!("inesperado: {other:?}"),
    }
}

fn rust_write(conn: &Connection, op: &Value) -> Value {
    let kind = op[0].as_str().unwrap();
    let ident = op[1].as_i64();
    let arg = &op[2];
    match kind {
        "saved" => write_out(
            news::set_saved(conn, ident.unwrap(), arg.as_bool().unwrap(), NOW_MS)
                .map(|v| v.map_or(Value::Null, Value::Bool)),
        ),
        "add" => write_out(news::add_note(conn, ident, arg, NOW_MS)),
        "update" => write_out(news::update_note(conn, ident, arg, NOW_MS)),
        "delete" => write_out(news::delete_note(conn, ident)),
        _ => write_out(news::toggle_chat_note(conn, ident, NOW_MS)),
    }
}

#[test]
fn writes_match_python() {
    let scratch = Scratch::new("writes");
    let (py_db, rs_db) = (scratch.0.join("py.sqlite3"), scratch.0.join("rs.sqlite3"));
    drop(seeded(&py_db, WRITE_EXTRA));
    let conn = seeded(&rs_db, WRITE_EXTRA);
    let ops = write_ops();
    let home = scratch.0.join("home");
    let now = OsStr::new(&NOW_MS.to_string()).to_owned();
    let ops_arg = ops.to_string();
    let Some(expected) = run_python(
        WRITE_ORACLE,
        &[py_db.as_os_str(), &now, OsStr::new(&ops_arg)],
        &home,
    ) else {
        return;
    };
    let got: Vec<Value> = ops
        .as_array()
        .unwrap()
        .iter()
        .map(|op| rust_write(&conn, op))
        .collect();
    let want: Vec<Value> = serde_json::from_str(expected.trim_end()).unwrap();
    for (i, (g, w)) in got.iter().zip(&want).enumerate() {
        assert_eq!(
            response_dumps(g).unwrap(),
            response_dumps(w).unwrap(),
            "operación {i}: {}",
            ops[i].to_string().chars().take(80).collect::<String>()
        );
    }
    assert_eq!(
        response_dumps(&Value::Array(got)).unwrap(),
        expected.trim_end()
    );
    drop(conn);
    // Las mismas filas en las dos bases.
    let dump = |db: &Path| run_python(DUMP_ORACLE, &[db.as_os_str()], &home).unwrap();
    assert_eq!(dump(&rs_db), dump(&py_db), "filas");
}
