//! Persistencia de lib/session_profiles.py. La conexión y el reloj pertenecen
//! al llamador; el inventario de archivos corresponde al adaptador runtime.
use comandos_core::{
    json::{response_dumps_unicode, truthy, workspace_loads},
    text::strip,
};
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::{Map, Value};

#[derive(Debug)]
pub enum Fault {
    Invalid(String),
    Uncertain(String),
    Sql(rusqlite::Error),
    Random(getrandom::Error),
}
impl std::fmt::Display for Fault {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Invalid(s) | Self::Uncertain(s) => f.write_str(s),
            Self::Sql(e) => e.fmt(f),
            Self::Random(e) => e.fmt(f),
        }
    }
}
impl std::error::Error for Fault {}
impl From<rusqlite::Error> for Fault {
    fn from(e: rusqlite::Error) -> Self {
        Self::Sql(e)
    }
}
pub type Result<T> = std::result::Result<T, Fault>;
fn invalid(s: impl Into<String>) -> Fault {
    Fault::Invalid(s.into())
}

const FIELDS: &[&str] = &[
    "id",
    "name",
    "harness",
    "motor",
    "routeId",
    "model",
    "effort",
    "harnessAccount",
    "motorAccount",
    "skills",
    "mcps",
    "updatedAt",
    "createdAt",
];

fn schema(conn: &Connection) -> Result<()> {
    crate::with_transaction(conn, || {
    conn.execute_batch("CREATE TABLE IF NOT EXISTS session_profiles (id TEXT PRIMARY KEY,name TEXT NOT NULL,payload TEXT NOT NULL,created_at INTEGER NOT NULL,updated_at INTEGER NOT NULL)")?;
    Ok(())
    }).map_err(|e| Fault::Uncertain(e.to_string()))
}
fn admit_write(conn: &Connection) -> Result<()> {
    crate::migrate::move_db::admit_write(conn).map_err(|e| Fault::Uncertain(e.to_string()))
}
fn in_write_tx<T>(conn: &Connection, run: impl FnOnce() -> Result<T>) -> Result<T> {
    let tx = if conn.is_autocommit() {
        Some(rusqlite::Transaction::new_unchecked(
            conn,
            rusqlite::TransactionBehavior::Immediate,
        )?)
    } else {
        None
    };
    admit_write(conn)?;
    let out = run()?;
    if let Some(tx) = tx {
        tx.commit()?;
    }
    Ok(out)
}
fn valid_id(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 80
        && s.as_bytes()[0].is_ascii_alphanumeric()
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
}
fn id(v: &Value) -> Result<&str> {
    v.as_str()
        .filter(|s| valid_id(s))
        .ok_or_else(|| invalid("id de perfil inválido"))
}
pub fn valid_extension_name(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 160
        && (s.as_bytes()[0].is_ascii_alphanumeric() || s.as_bytes()[0] == b'_')
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_.:@/-".contains(&b))
}
fn new_id() -> Result<String> {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).map_err(Fault::Random)?;
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}
fn load(raw: &str) -> Result<Value> {
    workspace_loads(raw).map_err(|_| Fault::Uncertain("payload de perfil ilegible".into()))
}
pub fn list_profiles(conn: &Connection) -> Result<Vec<Value>> {
    schema(conn)?;
    let mut stmt =
        conn.prepare("SELECT payload FROM session_profiles ORDER BY name COLLATE NOCASE,id")?;
    let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
    rows.map(|raw| load(&raw?)).collect()
}
pub fn get_profile(conn: &Connection, ident: &Value) -> Result<Value> {
    schema(conn)?;
    let raw: Option<String> = conn
        .query_row(
            "SELECT payload FROM session_profiles WHERE id=?",
            [id(ident)?],
            |r| r.get(0),
        )
        .optional()?;
    load(&raw.ok_or_else(|| invalid("perfil no encontrado"))?)
}
pub fn delete_profile(conn: &Connection, ident: &Value) -> Result<()> {
    in_write_tx(conn, || {
        schema(conn)?;
        conn.execute("DELETE FROM session_profiles WHERE id=?", [id(ident)?])?;
        Ok(())
    })
}
pub fn save_profile(conn: &Connection, data: &Value, now: i64) -> Result<Value> {
    let data = data
        .as_object()
        .filter(|m| m.keys().all(|k| FIELDS.contains(&k.as_str())))
        .ok_or_else(|| invalid("campos de perfil inválidos"))?;
    let ident = match data.get("id").filter(|v| truthy(v)) {
        Some(v) => id(v)?.to_owned(),
        None => new_id()?,
    };
    let name = data
        .get("name")
        .and_then(Value::as_str)
        .filter(|s| !strip(s).is_empty() && s.chars().count() <= 100 && !s.chars().any(|c| c < ' '))
        .ok_or_else(|| invalid("nombre de perfil inválido"))?;
    let mut out = Map::new();
    out.insert("id".into(), ident.clone().into());
    out.insert("name".into(), strip(name).into());
    for key in [
        "harness",
        "motor",
        "routeId",
        "model",
        "effort",
        "harnessAccount",
        "motorAccount",
    ] {
        let default = if key.ends_with("Account") { "main" } else { "" };
        let value = match data.get(key) {
            None => default,
            Some(v) => v
                .as_str()
                .ok_or_else(|| invalid(format!("{key} inválido")))?,
        };
        if value.chars().count() > 160
            || value.chars().any(|c| c < ' ')
            || (key != "model" && !value.is_empty() && !valid_extension_name(value))
        {
            return Err(invalid(format!("{key} inválido")));
        }
        let value = if key.ends_with("Account") {
            if value.is_empty() {
                "main"
            } else {
                if value.len() > 64
                    || !value.as_bytes()[0].is_ascii_alphanumeric()
                    || !value
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
                {
                    return Err(invalid("alias de cuenta invalido"));
                }
                value
            }
        } else {
            value
        };
        out.insert(key.into(), value.into());
    }
    if out["harness"] == "" {
        out.insert("harness".into(), "codex".into());
    }
    if out["motor"] == "" {
        out.insert("motor".into(), out["harness"].clone());
    }
    for key in ["skills", "mcps"] {
        let values = match data.get(key) {
            None => Map::new(),
            Some(v) => v
                .as_object()
                .filter(|m| {
                    m.len() <= 1000
                        && m.iter()
                            .all(|(k, v)| valid_extension_name(k) && v.is_boolean())
                })
                .cloned()
                .ok_or_else(|| invalid(format!("{key} inválidos")))?,
        };
        out.insert(key.into(), values.into());
    }
    // Reservar escritura antes de leer created_at evita que dos guardados
    // iniciales concurrentes devuelvan un createdAt distinto al durable.
    let tx = if conn.is_autocommit() {
        Some(rusqlite::Transaction::new_unchecked(
            conn,
            rusqlite::TransactionBehavior::Immediate,
        )?)
    } else {
        None
    };
    admit_write(conn)?;
    schema(conn)?;
    let previous: Option<i64> = conn
        .query_row(
            "SELECT created_at FROM session_profiles WHERE id=?",
            [&ident],
            |r| r.get(0),
        )
        .optional()?;
    out.insert("createdAt".into(), previous.unwrap_or(now).into());
    out.insert("updatedAt".into(), now.into());
    let out = Value::Object(out);
    let payload = response_dumps_unicode(&out).map_err(Fault::Uncertain)?;
    conn.execute("INSERT INTO session_profiles VALUES(?,?,?,?,?) ON CONFLICT(id) DO UPDATE SET name=excluded.name,payload=excluded.payload,updated_at=excluded.updated_at",params![ident,out["name"].as_str(),payload,previous.unwrap_or(now),now])?;
    if let Some(tx) = tx {
        tx.commit()?;
    }
    Ok(out)
}
