//! Lecturas de los Resúmenes de noticias sobre app-state, portadas de
//! `lib/news_editions.py` y `lib/news_reading.py` con las mismas sentencias
//! SQL y el mismo orden de filas (plan 2f-4, Tarea 1).
//!
//! Cada valor de SQLite se convierte como lo haría `sqlite3` del Python y
//! después `json.dumps`. Lo que el Python haría con certeza y termina en una
//! excepción no capturada es `Fault::Raise` (500 del tablero); lo que no se
//! puede reproducir con certeza (texto que no es UTF-8, BLOB, JSON que el
//! parser portado no clasifica igual) es `Fault::Unsure` y la ruta declina.
use comandos_core::json::{MAX_WORKSPACE_JSON_DEPTH, truthy, workspace_loads};
use comandos_core::text::strip;
use rusqlite::{Connection, OptionalExtension, Row, params, params_from_iter, types::ValueRef};
use serde_json::{Map, Number, Value, json};
use std::{collections::HashMap, path::Path};

/// Lo que no termina en una respuesta normal.
#[derive(Debug)]
pub enum Fault {
    /// El Python lanzaría una excepción no capturada.
    Raise(String),
    /// No se sabe con certeza qué haría el Python.
    Unsure(String),
    Sql(rusqlite::Error),
}

impl From<rusqlite::Error> for Fault {
    fn from(error: rusqlite::Error) -> Self {
        Fault::Sql(error)
    }
}

impl std::fmt::Display for Fault {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Fault::Raise(s) => write!(f, "excepción del Python: {s}"),
            Fault::Unsure(s) => write!(f, "incierto: {s}"),
            Fault::Sql(e) => e.fmt(f),
        }
    }
}

pub type Result<T> = std::result::Result<T, Fault>;

fn unsure<T>(what: &str) -> Result<T> {
    Err(Fault::Unsure(what.to_owned()))
}

/// `READABLE` de `news_editions`.
pub const READABLE: [&str; 2] = ["published", "partial"];

// ---------------------------------------------------------------- valores de SQLite

/// Un valor de columna como lo entrega `sqlite3` y lo escribe `json.dumps`.
fn sql_value(value: ValueRef<'_>) -> Result<Value> {
    match value {
        ValueRef::Null => Ok(Value::Null),
        ValueRef::Integer(n) => Ok(n.into()),
        ValueRef::Real(x) => Number::from_f64(x)
            .map(Value::Number)
            .ok_or_else(|| Fault::Unsure("REAL no finito".into())),
        ValueRef::Text(bytes) => std::str::from_utf8(bytes)
            .map(|s| Value::String(s.to_owned()))
            .map_err(|_| Fault::Unsure("TEXT que no es UTF-8".into())),
        // `bytes` del Python: `json.dumps` lanzaría, pero no se sabe dónde.
        ValueRef::Blob(_) => unsure("BLOB"),
    }
}

fn col(row: &Row<'_>, index: usize) -> Result<Value> {
    sql_value(row.get_ref(index)?)
}

/// `bool(valor)` del Python sobre lo que entrega `sqlite3`.
fn sql_truthy(value: ValueRef<'_>) -> bool {
    match value {
        ValueRef::Null => false,
        ValueRef::Integer(n) => n != 0,
        ValueRef::Real(x) => x != 0.0,
        ValueRef::Text(s) | ValueRef::Blob(s) => !s.is_empty(),
    }
}

/// Texto UTF-8 de una columna, o `None` si no es texto.
fn sql_text<'a>(value: ValueRef<'a>) -> Result<Option<&'a str>> {
    match value {
        ValueRef::Text(bytes) => std::str::from_utf8(bytes)
            .map(Some)
            .map_err(|_| Fault::Unsure("TEXT que no es UTF-8".into())),
        _ => Ok(None),
    }
}

// ---------------------------------------------------------------- JSON del Python

/// `json.loads(texto)`: `Ok(Some(v))`, `Ok(None)` si el Python lanzaría
/// `ValueError` con certeza (JSON roto o BOM), `Unsure` si el `json` del
/// Python podría aceptar lo que el parser portado rechaza.
pub fn py_loads(text: &str) -> Result<Option<Value>> {
    if text.starts_with('\u{feff}') {
        return Ok(None);
    }
    match workspace_loads(text) {
        Ok(value) => Ok(Some(value)),
        Err(_) if has_surrogate_escape(text) || deep(text) => unsure("JSON incierto"),
        Err(_) => Ok(None),
    }
}

/// `\uD800`–`\uDFFF`: el `json` del Python los admite sueltos; el portado no.
fn has_surrogate_escape(text: &str) -> bool {
    text.match_indices("\\u").any(|(i, _)| {
        let hex = text.as_bytes().get(i + 2..i + 4).unwrap_or_default();
        matches!(hex, [b'd' | b'D', b'8'..=b'9' | b'a'..=b'f' | b'A'..=b'F'])
    })
}

/// El límite de anidamiento del parser portado no es el de CPython.
fn deep(text: &str) -> bool {
    text.bytes().filter(|b| matches!(b, b'[' | b'{')).count() >= MAX_WORKSPACE_JSON_DEPTH
}

/// `json.loads(valor or vacío)` de una columna: `Ok(Err(()))` es el
/// `ValueError` del Python (texto roto); un entero verdadero lanza
/// `TypeError`, que nadie captura.
fn loads_column(value: ValueRef<'_>, empty: &str) -> Result<std::result::Result<Value, ()>> {
    let text = if sql_truthy(value) {
        match value {
            ValueRef::Text(_) => sql_text(value)?.unwrap_or_default(),
            // `json.loads(int|float)` → `TypeError`.
            ValueRef::Integer(_) | ValueRef::Real(_) => {
                return Err(Fault::Raise("TypeError".into()));
            }
            // `json.loads(bytes)` decodifica: incierto.
            _ => return unsure("BLOB como JSON"),
        }
    } else {
        empty
    };
    Ok(py_loads(text)?.ok_or(()))
}

/// `json.loads(valor or vacío)` sin `try`: un texto roto lanza.
fn loads_strict(value: ValueRef<'_>, empty: &str) -> Result<Value> {
    loads_column(value, empty)?.map_err(|()| Fault::Raise("JSONDecodeError".into()))
}

/// `json.loads(valor or vacío)` dentro de `try/except ValueError` → `vacío`.
fn loads_or(value: ValueRef<'_>, empty: &str) -> Result<Value> {
    match loads_column(value, empty)? {
        Ok(loaded) => Ok(loaded),
        Err(()) => py_loads(empty)?.ok_or_else(|| Fault::Raise("json.loads".into())),
    }
}

/// `_json_obj(text)`: un objeto, o `{}` si no lo es o no se entiende.
fn json_obj(value: ValueRef<'_>) -> Result<Value> {
    let loaded = loads_or(value, "{}")?;
    Ok(if loaded.is_object() {
        loaded
    } else {
        json!({})
    })
}

/// `meta.get(clave) or defecto` sobre un objeto.
fn get_or(meta: &Value, key: &str, default: &str) -> Value {
    match meta.get(key) {
        Some(v) if truthy(v) => v.clone(),
        _ => Value::from(default),
    }
}

/// `bool(meta.get(clave))`.
fn get_bool(meta: &Value, key: &str) -> bool {
    meta.get(key).is_some_and(truthy)
}

// ---------------------------------------------------------------- redondeo

/// `round(x, n)` de CPython sobre un `float`: decimal correctamente
/// redondeado (mitad al par sobre el valor binario exacto) y vuelta a `float`.
pub fn py_round(x: f64, digits: usize) -> Result<f64> {
    if !x.is_finite() {
        return Ok(x);
    }
    // 1100 decimales escriben exacto cualquier `double` (≤ 1074 fraccionarios).
    let exact = format!("{:.1100}", x.abs());
    let Some((whole, frac)) = exact.split_once('.') else {
        return unsure("redondeo");
    };
    let (keep, rest) = frac.split_at(digits.min(frac.len()));
    let mut digits_out: Vec<u8> = whole.bytes().chain(keep.bytes()).collect();
    let first = rest.as_bytes().first().copied().unwrap_or(b'0');
    let tail_nonzero = rest.bytes().skip(1).any(|b| b != b'0');
    let last_odd = digits_out.last().is_some_and(|d| (d - b'0') % 2 == 1);
    let round_up = first > b'5' || (first == b'5' && (tail_nonzero || last_odd));
    if round_up {
        let mut i = digits_out.len();
        loop {
            if i == 0 {
                digits_out.insert(0, b'1');
                break;
            }
            i -= 1;
            let Some(d) = digits_out.get_mut(i) else {
                return unsure("redondeo");
            };
            if *d == b'9' {
                *d = b'0';
            } else {
                *d += 1;
                break;
            }
        }
    }
    let text = String::from_utf8(digits_out).map_err(|_| Fault::Unsure("redondeo".into()))?;
    let split = text.len() - keep.len();
    let (int_part, frac_part) = text.split_at(split);
    let sign = if x.is_sign_negative() { "-" } else { "" };
    format!("{sign}{int_part}.{frac_part}0")
        .parse::<f64>()
        .map_err(|_| Fault::Unsure("redondeo".into()))
}

/// `round(cost or 0.0, 6)` de `_edition_row`.
fn cost_value(value: ValueRef<'_>) -> Result<Value> {
    if !sql_truthy(value) {
        return Ok(json!(0.0));
    }
    match value {
        // `round(int, 6)` devuelve el mismo entero.
        ValueRef::Integer(n) => Ok(n.into()),
        ValueRef::Real(x) => Number::from_f64(py_round(x, 6)?)
            .map(Value::Number)
            .ok_or_else(|| Fault::Unsure("costo no finito".into())),
        // `round(str, 6)` → `TypeError`.
        ValueRef::Text(_) => Err(Fault::Raise("TypeError".into())),
        ValueRef::Null | ValueRef::Blob(_) => unsure("costo"),
    }
}

// ---------------------------------------------------------------- ediciones

const EDITION_COLS: &str = "id, local_date, slot, timezone, scheduled_at_ms, status, published_at_ms, title, \
     story_count, source_count, failed_source_count, cost_usd, model, notes";

/// `_edition_row(row)`.
fn edition_row(row: &Row<'_>) -> Result<Value> {
    let mut out = Map::new();
    for (index, key) in [
        "id",
        "localDate",
        "slot",
        "timezone",
        "scheduledAt",
        "status",
        "publishedAt",
        "title",
        "storyCount",
        "sourceCount",
        "failedSourceCount",
    ]
    .into_iter()
    .enumerate()
    {
        out.insert(key.into(), col(row, index)?);
    }
    out.insert("costUsd".into(), cost_value(row.get_ref(11)?)?);
    out.insert("model".into(), col(row, 12)?);
    out.insert("notes".into(), loads_strict(row.get_ref(13)?, "[]")?);
    Ok(Value::Object(out))
}

/// Filas de ediciones de una consulta con `EDITION_COLS`.
fn edition_rows(conn: &Connection, sql: &str, args: &[&dyn rusqlite::ToSql]) -> Result<Vec<Value>> {
    let mut stmt = conn.prepare(sql)?;
    let mut rows = stmt.query(args)?;
    let mut out = Vec::new();
    while let Some(row) = rows.next()? {
        out.push(edition_row(row)?);
    }
    Ok(out)
}

/// `_enrich(conn, edition)`: modelos que escribieron y el trabajo.
fn enrich(conn: &Connection, mut edition: Value) -> Result<Value> {
    let eid = edition.get("id").cloned().unwrap_or(Value::Null);
    let eid = json_param(&eid)?;
    let mut models = Map::new();
    let mut stmt = conn.prepare(
        "SELECT model, COUNT(*) FROM news_stories WHERE edition_id = ? AND model IS NOT NULL \
         GROUP BY model ORDER BY MIN(position)",
    )?;
    let mut rows = stmt.query(params![eid])?;
    while let Some(row) = rows.next()? {
        // Una clave que no es texto la convierte `json.dumps`: se declina.
        let Some(model) = sql_text(row.get_ref(0)?)? else {
            return unsure("modelo que no es texto");
        };
        models.insert(model.to_owned(), col(row, 1)?);
    }
    let job = conn
        .query_row(
            "SELECT state, attempts, started_at_ms, finished_at_ms, error FROM news_jobs \
             WHERE edition_id = ?",
            params![eid],
            |row| {
                Ok([
                    sql_value(row.get_ref(0)?),
                    sql_value(row.get_ref(1)?),
                    sql_value(row.get_ref(2)?),
                    sql_value(row.get_ref(3)?),
                    sql_value(row.get_ref(4)?),
                ])
            },
        )
        .optional()?;
    let job = match job {
        Some([state, attempts, started, finished, error]) => json!({
            "state": state?, "attempts": attempts?, "startedAt": started?,
            "finishedAt": finished?, "error": error?,
        }),
        None => Value::Null,
    };
    if let Some(obj) = edition.as_object_mut() {
        obj.insert("models".into(), Value::Object(models));
        obj.insert("job".into(), job);
    }
    Ok(edition)
}

/// Un valor de JSON leído de la base de vuelta como parámetro de SQLite.
fn json_param(value: &Value) -> Result<rusqlite::types::Value> {
    use rusqlite::types::Value as Sql;
    Ok(match value {
        Value::Null => Sql::Null,
        Value::String(s) => Sql::Text(s.clone()),
        Value::Number(n) => match (n.as_i64(), n.as_f64()) {
            (Some(i), _) => Sql::Integer(i),
            (None, Some(f)) => Sql::Real(f),
            _ => return unsure("parámetro numérico"),
        },
        _ => return unsure("parámetro"),
    })
}

/// `list_editions(conn, limit, until_ms)`: la más reciente primero.
pub fn list_editions(conn: &Connection, limit: i64, until_ms: Option<i64>) -> Result<Vec<Value>> {
    let limit = limit.clamp(1, 200);
    let rows = match until_ms {
        Some(until) => edition_rows(
            conn,
            &format!(
                "SELECT {EDITION_COLS} FROM news_editions WHERE scheduled_at_ms <= ? \
                 ORDER BY scheduled_at_ms DESC LIMIT ?"
            ),
            &[&until, &limit],
        )?,
        None => edition_rows(
            conn,
            &format!(
                "SELECT {EDITION_COLS} FROM news_editions ORDER BY scheduled_at_ms DESC LIMIT ?"
            ),
            &[&limit],
        )?,
    };
    rows.into_iter().map(|e| enrich(conn, e)).collect()
}

/// `next_scheduled(conn, now_ms)`: sin `_enrich`.
pub fn next_scheduled(conn: &Connection, now_ms: i64) -> Result<Option<Value>> {
    Ok(edition_rows(
        conn,
        &format!(
            "SELECT {EDITION_COLS} FROM news_editions WHERE status = 'scheduled' \
             AND scheduled_at_ms > ? ORDER BY scheduled_at_ms LIMIT 1"
        ),
        &[&now_ms],
    )?
    .into_iter()
    .next())
}

/// `latest_readable(conn)`.
pub fn latest_readable(conn: &Connection) -> Result<Option<Value>> {
    let row = edition_rows(
        conn,
        &format!(
            "SELECT {EDITION_COLS} FROM news_editions WHERE status IN ('published','partial') \
             ORDER BY scheduled_at_ms DESC LIMIT 1"
        ),
        &[],
    )?
    .into_iter()
    .next();
    row.map(|e| enrich(conn, e)).transpose()
}

/// `_edition(conn, eid)`: con `lead`.
fn edition(conn: &Connection, eid: &str) -> Result<Option<Value>> {
    let Some(row) = edition_rows(
        conn,
        &format!("SELECT {EDITION_COLS} FROM news_editions WHERE id = ?"),
        &[&eid],
    )?
    .into_iter()
    .next() else {
        return Ok(None);
    };
    let mut edition = enrich(conn, row)?;
    let lead = conn.query_row("SELECT lead FROM news_editions WHERE id = ?", [eid], |r| {
        Ok(sql_value(r.get_ref(0)?))
    })??;
    if let Some(obj) = edition.as_object_mut() {
        obj.insert("lead".into(), lead);
    }
    Ok(Some(edition))
}

/// `get_edition(conn, eid)`: la edición y sus noticias con sus fuentes.
pub fn get_edition(conn: &Connection, eid: &str) -> Result<Option<Value>> {
    let Some(edition) = edition(conn, eid)? else {
        return Ok(None);
    };
    let mut stories = Vec::new();
    let mut stmt = conn.prepare(
        "SELECT id, position, category, title, summary_md, body_md, opportunity, model, meta \
         FROM news_stories WHERE edition_id = ? ORDER BY position",
    )?;
    let mut rows = stmt.query([eid])?;
    while let Some(row) = rows.next()? {
        let sid = row.get_ref(0)?;
        let sources = story_sources(conn, sid)?;
        let opportunity = row.get_ref(6)?;
        let opportunity = if sql_truthy(opportunity) {
            loads_strict(opportunity, "null")?
        } else {
            Value::Null
        };
        stories.push(json!({
            "id": sql_value(sid)?, "position": col(row, 1)?, "category": col(row, 2)?,
            "title": col(row, 3)?, "summary": col(row, 4)?, "body": col(row, 5)?,
            "opportunity": opportunity, "sources": sources, "model": col(row, 7)?,
            "meta": json_obj(row.get_ref(8)?)?,
        }));
    }
    Ok(Some(json!({"edition": edition, "stories": stories})))
}

/// Las fuentes de una noticia, oficial primero y discusiones al final.
fn story_sources(conn: &Connection, sid: ValueRef<'_>) -> Result<Vec<Value>> {
    use rusqlite::types::Value as Sql;
    let sid = match sid {
        ValueRef::Null => Sql::Null,
        ValueRef::Integer(n) => Sql::Integer(n),
        ValueRef::Real(x) => Sql::Real(x),
        ValueRef::Text(t) => Sql::Text(String::from_utf8_lossy(t).into_owned()),
        ValueRef::Blob(b) => Sql::Blob(b.to_vec()),
    };
    let mut stmt = conn.prepare(
        "SELECT s.id, s.url, s.title, s.origin, s.published_at_ms, s.discovered_at_ms, \
         s.verified_at_ms, s.fetch_status, s.meta, c.source_id IS NOT NULL, c.captured_at_ms \
         FROM news_story_sources l JOIN news_sources s ON s.id = l.source_id \
         LEFT JOIN news_captures c ON c.source_id = s.id WHERE l.story_id = ? ORDER BY s.id",
    )?;
    let mut rows = stmt.query(params![sid])?;
    let mut sources = Vec::new();
    while let Some(r) = rows.next()? {
        let meta = json_obj(r.get_ref(8)?)?;
        let official = get_bool(&meta, "official");
        let role = get_or(&meta, "role", "article");
        let discussion = role.as_str() == Some("discussion");
        let source = json!({
            "id": col(r, 0)?, "url": col(r, 1)?, "title": col(r, 2)?, "origin": col(r, 3)?,
            "publishedAt": col(r, 4)?, "discoveredAt": col(r, 5)?, "verifiedAt": col(r, 6)?,
            "fetchStatus": col(r, 7)?, "official": official, "role": role,
            "heat": get_or(&meta, "heat", ""), "captured": sql_truthy(r.get_ref(9)?),
            "capturedAt": col(r, 10)?,
        });
        sources.push(((!official, discussion), source));
    }
    // `list.sort(key=…)` es estable, como `sort_by_key`.
    sources.sort_by_key(|(key, _)| *key);
    Ok(sources.into_iter().map(|(_, s)| s).collect())
}

/// `get_source(conn, source_id)`: la fuente con su captura completa.
pub fn get_source(conn: &Connection, source_id: i64) -> Result<Option<Value>> {
    let mut stmt = conn.prepare(
        "SELECT s.id, s.url, s.title, s.origin, s.published_at_ms, s.discovered_at_ms, s.meta, \
         c.captured_at_ms, c.final_url, c.title, c.byline, c.lang, c.blocks, c.partial \
         FROM news_sources s LEFT JOIN news_captures c ON c.source_id = s.id WHERE s.id = ?",
    )?;
    let mut rows = stmt.query([source_id])?;
    let Some(row) = rows.next()? else {
        return Ok(None);
    };
    let meta = json_obj(row.get_ref(6)?)?;
    let capture = if matches!(row.get_ref(7)?, ValueRef::Null) {
        Value::Null
    } else {
        let blocks = loads_or(row.get_ref(12)?, "[]")?;
        json!({
            "capturedAt": col(row, 7)?, "finalUrl": col(row, 8)?, "title": col(row, 9)?,
            "byline": col(row, 10)?, "lang": col(row, 11)?,
            "blocks": if blocks.is_array() { blocks } else { json!([]) },
            "partial": sql_truthy(row.get_ref(13)?),
        })
    };
    Ok(Some(json!({
        "id": col(row, 0)?, "url": col(row, 1)?, "title": col(row, 2)?, "origin": col(row, 3)?,
        "publishedAt": col(row, 4)?, "discoveredAt": col(row, 5)?,
        "official": get_bool(&meta, "official"), "role": get_or(&meta, "role", "article"),
        "heat": get_or(&meta, "heat", ""), "capture": capture,
    })))
}

// ---------------------------------------------------------------- lectura de una noticia

/// `story_row(conn, story_id)`.
pub fn story_row(conn: &Connection, story_id: i64) -> Result<Option<Value>> {
    let mut stmt = conn.prepare(
        "SELECT id, edition_id, title, summary_md, body_md FROM news_stories WHERE id = ?",
    )?;
    let mut rows = stmt.query([story_id])?;
    let Some(row) = rows.next()? else {
        return Ok(None);
    };
    Ok(Some(json!({
        "id": col(row, 0)?, "editionId": col(row, 1)?, "title": col(row, 2)?,
        "summary": col(row, 3)?, "body": col(row, 4)?,
    })))
}

/// Una clave entera de un `dict` del Python leída de una columna (`int` o
/// `float` entero comparan igual); otra cosa se declina.
fn int_key(value: ValueRef<'_>) -> Result<i64> {
    match value {
        ValueRef::Integer(n) => Ok(n),
        ValueRef::Real(x) if x.fract() == 0.0 && x.abs() < 9.0e15 => Ok(x as i64),
        _ => unsure("clave que no es entera"),
    }
}

/// `story_counts(conn, ids)`: notas, mensajes y guardada por noticia.
pub fn story_counts(conn: &Connection, story_ids: &[i64]) -> Result<HashMap<i64, Value>> {
    if story_ids.is_empty() {
        return Ok(HashMap::new());
    }
    let marks = vec!["?"; story_ids.len()].join(",");
    let mut out: HashMap<i64, Value> = story_ids
        .iter()
        .map(|i| (*i, json!({"notes": 0, "chat": 0, "saved": false})))
        .collect();
    for (table, field) in [("news_notes", "notes"), ("news_chat", "chat")] {
        let mut stmt = conn.prepare(&format!(
            "SELECT story_id, COUNT(*) FROM {table} WHERE story_id IN ({marks}) GROUP BY story_id"
        ))?;
        let mut rows = stmt.query(params_from_iter(story_ids))?;
        while let Some(row) = rows.next()? {
            let sid = int_key(row.get_ref(0)?)?;
            let count = col(row, 1)?;
            let Some(entry) = out.get_mut(&sid).and_then(Value::as_object_mut) else {
                return Err(Fault::Raise("KeyError".into()));
            };
            entry.insert(field.into(), count);
        }
    }
    let mut stmt = conn.prepare(&format!(
        "SELECT story_id FROM news_saved WHERE story_id IN ({marks})"
    ))?;
    let mut rows = stmt.query(params_from_iter(story_ids))?;
    while let Some(row) = rows.next()? {
        let sid = int_key(row.get_ref(0)?)?;
        let Some(entry) = out.get_mut(&sid).and_then(Value::as_object_mut) else {
            return Err(Fault::Raise("KeyError".into()));
        };
        entry.insert("saved".into(), Value::Bool(true));
    }
    Ok(out)
}

/// `saved_stories(conn)`: las guardadas, la más reciente primero.
pub fn saved_stories(conn: &Connection) -> Result<Vec<Value>> {
    let mut stmt = conn.prepare(
        "SELECT st.id, st.edition_id, st.title, st.summary_md, st.meta, sv.saved_at_ms \
         FROM news_saved sv JOIN news_stories st ON st.id = sv.story_id \
         ORDER BY sv.saved_at_ms DESC LIMIT 300",
    )?;
    let mut rows = stmt.query([])?;
    let mut out = Vec::new();
    while let Some(r) = rows.next()? {
        out.push(json!({
            "id": col(r, 0)?, "editionId": col(r, 1)?, "title": col(r, 2)?,
            "summary": col(r, 3)?, "meta": json_obj(r.get_ref(4)?)?, "savedAt": col(r, 5)?,
        }));
    }
    Ok(out)
}

const NOTE_COLS: &str = "id, story_id, edition_id, story_title, kind, chat_id, quote, cite, text, created_at_ms, updated_at_ms";

/// `_note(row)`.
fn note(row: &Row<'_>) -> Result<Value> {
    let mut out = Map::new();
    for (index, key) in [
        "id",
        "storyId",
        "editionId",
        "storyTitle",
        "kind",
        "chatId",
        "quote",
        "cite",
        "text",
        "createdAt",
        "updatedAt",
    ]
    .into_iter()
    .enumerate()
    {
        out.insert(key.into(), col(row, index)?);
    }
    Ok(Value::Object(out))
}

/// `list_notes(conn, story_id, query, limit)`: `{"notes": […], "total": n}`.
pub fn list_notes(
    conn: &Connection,
    story_id: Option<i64>,
    query: &str,
    limit: i64,
) -> Result<Value> {
    let mut sql = format!("SELECT {NOTE_COLS} FROM news_notes");
    let mut args: Vec<rusqlite::types::Value> = Vec::new();
    let mut clauses = Vec::new();
    if let Some(story) = story_id {
        clauses.push("story_id = ?");
        args.push(story.into());
    }
    let q = strip(query).to_lowercase();
    if !q.is_empty() {
        clauses.push(
            "(lower(text) LIKE ? OR lower(COALESCE(quote,'')) LIKE ? OR lower(story_title) LIKE ?)",
        );
        let like = format!("%{}%", q.replace(['%', '_'], ""));
        args.extend(std::iter::repeat_n(like.into(), 3));
    }
    if !clauses.is_empty() {
        sql.push_str(" WHERE ");
        sql.push_str(&clauses.join(" AND "));
    }
    sql.push_str(" ORDER BY created_at_ms DESC LIMIT ?");
    args.push(limit.clamp(1, 1000).into());
    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt.query(params_from_iter(args))?;
    let mut notes = Vec::new();
    while let Some(row) = rows.next()? {
        notes.push(note(row)?);
    }
    let total = conn.query_row("SELECT COUNT(*) FROM news_notes", [], |r| {
        Ok(sql_value(r.get_ref(0)?))
    })??;
    Ok(json!({"notes": notes, "total": total}))
}

/// `chat_history(conn, story_id)`.
pub fn chat_history(conn: &Connection, story_id: i64) -> Result<Vec<Value>> {
    // `r[0] in noted`: el id (entero) contra cada `chat_id` anotado; un
    // `float` entero compara igual en el Python, un texto nunca.
    let mut noted = std::collections::HashSet::new();
    let mut stmt =
        conn.prepare("SELECT chat_id FROM news_notes WHERE story_id = ? AND chat_id IS NOT NULL")?;
    let mut rows = stmt.query([story_id])?;
    while let Some(row) = rows.next()? {
        match row.get_ref(0)? {
            ValueRef::Integer(n) => {
                noted.insert(n);
            }
            ValueRef::Real(x) if x.fract() == 0.0 && x.abs() < 9.0e15 => {
                noted.insert(x as i64);
            }
            ValueRef::Real(_) | ValueRef::Text(_) | ValueRef::Null => {}
            ValueRef::Blob(_) => return unsure("chat_id BLOB"),
        }
    }
    let mut stmt = conn.prepare(
        "SELECT id, role, state, text, cite, model, created_at_ms FROM news_chat \
         WHERE story_id = ? ORDER BY id",
    )?;
    let mut rows = stmt.query([story_id])?;
    let mut out = Vec::new();
    while let Some(r) = rows.next()? {
        let is_noted = match r.get_ref(0)? {
            ValueRef::Integer(n) => noted.contains(&n),
            _ => return unsure("id de chat"),
        };
        out.push(json!({
            "id": col(r, 0)?, "role": col(r, 1)?, "state": col(r, 2)?, "text": col(r, 3)?,
            "cite": col(r, 4)?, "model": col(r, 5)?, "createdAt": col(r, 6)?, "noted": is_noted,
        }));
    }
    Ok(out)
}

/// `translation(conn, source_id, lang)`.
pub fn translation(conn: &Connection, source_id: i64, lang: &str) -> Result<Option<Value>> {
    let mut stmt = conn.prepare(
        "SELECT state, title, blocks, model, error, updated_at_ms FROM news_translations \
         WHERE source_id = ? AND lang = ?",
    )?;
    let mut rows = stmt.query(params![source_id, lang])?;
    let Some(row) = rows.next()? else {
        return Ok(None);
    };
    Ok(Some(json!({
        "state": col(row, 0)?, "title": col(row, 1)?, "blocks": loads_or(row.get_ref(2)?, "[]")?,
        "model": col(row, 3)?, "error": col(row, 4)?, "updatedAt": col(row, 5)?,
    })))
}

// ---------------------------------------------------------------- configuración

/// `load_config(path)`: el objeto de `news-editions.json` o `None`
/// (`OSError`/`ValueError` o algo que no es objeto). Bloquea si la ruta es
/// un FIFO, como el Python: el frente lee el archivo con su propio lector no
/// bloqueante y pasa los bytes a `config_from_bytes`.
pub fn load_config(path: &Path) -> Result<Option<Map<String, Value>>> {
    match std::fs::read(path) {
        Ok(bytes) => config_from_bytes(&bytes),
        // Todo `OSError` (ausente, directorio, permisos) → `None`.
        Err(_) => Ok(None),
    }
}

/// La mitad de `load_config` que sigue a la lectura: `json.load` del texto.
pub fn config_from_bytes(bytes: &[u8]) -> Result<Option<Map<String, Value>>> {
    // `open()` en modo texto: lo que no es UTF-8 depende del locale.
    let Ok(text) = std::str::from_utf8(bytes) else {
        return unsure("configuración que no es UTF-8");
    };
    Ok(match py_loads(text)? {
        Some(Value::Object(map)) => Some(map),
        _ => None,
    })
}

/// `config_status(config, env)`: `env(clave)` dice si la variable está
/// definida y no vacía (`env.get(clave)` verdadero).
pub fn config_status(config: Option<&Map<String, Value>>, env: &dyn Fn(&str) -> bool) -> Value {
    let no = |reason: &str| json!({"configured": false, "reason": reason});
    let Some(config) = config.filter(|c| !c.is_empty()) else {
        return no("sin configurar");
    };
    if config.get("enabled") != Some(&Value::Bool(true)) {
        return no("desactivado");
    }
    let empty = Map::new();
    let s = config
        .get("summarizer")
        .and_then(Value::as_object)
        .unwrap_or(&empty);
    let kind = s.get("kind").and_then(Value::as_str);
    if !matches!(
        kind,
        Some("anthropic-messages" | "openai-chat" | "acp" | "chain")
    ) {
        return no("proveedor no soportado");
    }
    let falsy = |key: &str| !s.get(key).is_some_and(truthy);
    if kind == Some("chain") {
        let Some(steps) = s
            .get("steps")
            .and_then(Value::as_array)
            .filter(|a| !a.is_empty())
        else {
            return no("la cadena no tiene agentes");
        };
        for step in steps {
            let agent = step.as_object().and_then(|o| o.get("agent"));
            if !matches!(agent, Some(Value::String(a)) if !a.is_empty()) {
                return no("falta el agente ACP");
            }
        }
        return json!({"configured": true, "reason": ""});
    }
    if kind == Some("acp") {
        if falsy("agent") || !s.get("agent").is_some_and(Value::is_string) {
            return no("falta el agente ACP");
        }
        if falsy("model") {
            return no("falta el modelo");
        }
        return json!({"configured": true, "reason": ""});
    }
    if falsy("model") {
        return no("falta el modelo");
    }
    for key in ["inputUsdPerMTok", "outputUsdPerMTok"] {
        // `isinstance(v, (int, float))` sin `bool` y `v >= 0` (NaN pasa).
        let ok = match s.get(key) {
            Some(Value::Number(n)) => !n.as_f64().is_some_and(|x| x < 0.0),
            _ => false,
        };
        if !ok {
            return no("faltan precios del modelo para controlar el gasto");
        }
    }
    let key_env = match s.get("apiKeyEnv") {
        Some(Value::String(k)) if !k.is_empty() => k,
        _ => return no("falta apiKeyEnv"),
    };
    if !env(key_env) {
        return no(&format!("falta la clave {key_env}"));
    }
    if kind == Some("openai-chat")
        && !s
            .get("baseUrl")
            .and_then(Value::as_str)
            .is_some_and(|u| u.starts_with("https://"))
    {
        return no("baseUrl debe ser https");
    }
    json!({"configured": true, "reason": ""})
}

/// `default_policy()`.
pub fn default_policy() -> Map<String, Value> {
    let Value::Object(map) = json!({
        "timezone": "America/Mexico_City", "slots": ["09:00", "15:00", "21:00"],
        "maxSources": 25, "maxStories": 6, "budgetUsd": 0.25, "reserveUsdPerCall": 0.01,
        "maxSeconds": 1500, "graceMinutes": 30,
        "sourceScope": ["oficial", "hot", "ia", "modelo", "mcp", "skill", "bounty", "hackathon"],
    }) else {
        return Map::new();
    };
    map
}

/// `_SLOT.match(x)`: `^([01]\d|2[0-3]):[0-5]\d$` con el `$` del Python (antes
/// de un `\n` final). Un dígito Unicode (que `\d` admite) se declina.
fn slot_matches(slot: &str) -> Result<bool> {
    if slot.chars().any(|c| !c.is_ascii() && c.is_numeric()) {
        return unsure("dígito Unicode en la franja");
    }
    let b = slot.strip_suffix('\n').unwrap_or(slot).as_bytes();
    Ok(match b {
        [h, m, b':', t, u] => {
            let hour = matches!((h, m), (b'0' | b'1', b'0'..=b'9') | (b'2', b'0'..=b'3'));
            hour && matches!(t, b'0'..=b'5') && u.is_ascii_digit()
        }
        _ => false,
    })
}

/// Un entero JSON de verdad (`isinstance(x, int)` sin `bool`).
fn json_int(value: Option<&Value>) -> Option<i64> {
    match value {
        Some(Value::Number(n)) if !n.as_str().contains(['.', 'e', 'E', 'N', 'I']) => n.as_i64(),
        _ => None,
    }
}

/// `policy_from_config(config)`.
pub fn policy_from_config(config: Option<&Map<String, Value>>) -> Result<Map<String, Value>> {
    let mut policy = default_policy();
    let Some(over) = config
        .and_then(|c| c.get("policy"))
        .and_then(Value::as_object)
    else {
        return Ok(policy);
    };
    if let Some(slots) = over.get("slots").and_then(Value::as_array)
        && slots.len() == 3
    {
        let mut texts = Vec::new();
        for slot in slots {
            match slot {
                Value::String(s) if slot_matches(s)? => texts.push(s.clone()),
                _ => break,
            }
        }
        if texts.len() == 3 {
            texts.sort();
            policy.insert("slots".into(), json!(texts));
        }
    }
    if let Some(Value::Number(n)) = over.get("budgetUsd")
        && let Some(x) = n.as_f64()
        && 0.0 < x
        && x <= 1.0
    {
        policy.insert(
            "budgetUsd".into(),
            Number::from_f64(x).map_or(Value::Null, Value::Number),
        );
    }
    for (key, lo, hi) in [
        ("maxSources", 1, 40),
        ("maxStories", 3, 10),
        ("maxSeconds", 300, 3600),
    ] {
        if let Some(n) = json_int(over.get(key))
            && (lo..=hi).contains(&n)
        {
            policy.insert(key.into(), n.into());
        }
    }
    Ok(policy)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slots_follow_python_dollar() {
        assert!(slot_matches("09:00").unwrap());
        assert!(slot_matches("23:59\n").unwrap());
        assert!(!slot_matches("24:00").unwrap());
        assert!(!slot_matches("9:00").unwrap());
        assert!(!slot_matches("09:00\n\n").unwrap());
        assert!(slot_matches("0\u{663}:00").is_err());
    }

    #[test]
    fn round_is_half_even_on_exact_value() {
        assert_eq!(py_round(0.0078125, 6).unwrap(), 0.007812);
        assert_eq!(py_round(0.0078135, 6).unwrap(), 0.007813);
        assert_eq!(py_round(1.23456789, 6).unwrap(), 1.234568);
        assert_eq!(py_round(-0.9999999, 6).unwrap(), -1.0);
        assert_eq!(py_round(12.0, 6).unwrap(), 12.0);
    }
}
