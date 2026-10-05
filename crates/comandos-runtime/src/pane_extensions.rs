//! Borradores y plantillas por conversación, sobre el journal de operaciones.
//! Ninguna selección guardada se presenta como extensiones cargadas por un CLI.
use crate::session_operations::OperationStore;
use comandos_core::{
    json::{response_dumps, workspace_loads},
    text::strip,
};
use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior, params};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};

#[derive(Debug)]
pub enum Fault {
    Invalid(String),
    Conflict(String),
    Persistence(String),
}
impl std::fmt::Display for Fault {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Invalid(s) | Self::Conflict(s) | Self::Persistence(s) => f.write_str(s),
        }
    }
}
impl std::error::Error for Fault {}
impl From<rusqlite::Error> for Fault {
    fn from(e: rusqlite::Error) -> Self {
        Self::Persistence(e.to_string())
    }
}
pub type Result<T> = std::result::Result<T, Fault>;
const SELECTION: &str = "selección de extensiones inválida";
const MISSING: &str = "el borrador ya no existe; vuelve a abrir el estante";
const STALE: &str = "la selección cambió en otro cliente; vuelve a cargarla";
fn invalid(s: &str) -> Fault {
    Fault::Invalid(s.into())
}
fn conflict(s: &str) -> Fault {
    Fault::Conflict(s.into())
}
fn encode(v: &Value) -> Result<String> {
    response_dumps(v).map_err(Fault::Persistence)
}
fn decode(s: &str) -> Result<Value> {
    workspace_loads(s).map_err(|e| Fault::Persistence(e.to_string()))
}

pub fn selection_value(value: &Value) -> Result<Value> {
    let map = value
        .as_object()
        .filter(|m| m.len() == 2 && m.contains_key("mcps") && m.contains_key("skills"))
        .ok_or_else(|| invalid(SELECTION))?;
    let mut result = Map::new();
    for kind in ["mcps", "skills"] {
        let rows = map[kind]
            .as_object()
            .filter(|m| m.len() <= 3000)
            .ok_or_else(|| invalid(SELECTION))?;
        if rows.iter().any(|(k, v)| {
            k.is_empty()
                || k.len() > 256
                || !k
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"_.:@/+-".contains(&b))
                || !v.is_boolean()
        }) {
            return Err(invalid("identificador o estado de extensión inválido"));
        }
        result.insert(kind.into(), Value::Object(rows.clone()));
    }
    Ok(Value::Object(result))
}
fn atomic<T>(conn: &Connection, run: impl FnOnce() -> Result<T>) -> Result<T> {
    if !conn.is_autocommit() {
        return Err(Fault::Persistence(
            "el diario requiere una transacción propia y duradera".into(),
        ));
    }
    let tx = Transaction::new_unchecked(conn, TransactionBehavior::Immediate)?;
    let out = run()?;
    tx.commit()?;
    Ok(out)
}

pub struct ExtensionStore<'a> {
    pub connection: &'a Connection,
    pub clock: &'a dyn Fn() -> f64,
}
impl<'a> ExtensionStore<'a> {
    pub fn new(connection: &'a Connection, clock: &'a dyn Fn() -> f64) -> Result<Self> {
        OperationStore::new(connection, &|| 0, clock)
            .map_err(|e| Fault::Persistence(e.to_string()))?;
        atomic(connection, || {
            connection.execute_batch("CREATE TABLE IF NOT EXISTS pane_extension_drafts (key TEXT PRIMARY KEY,identity TEXT NOT NULL,conversation TEXT NOT NULL,harness TEXT NOT NULL,desired TEXT NOT NULL,revision INTEGER NOT NULL DEFAULT 0,updated REAL NOT NULL);CREATE TABLE IF NOT EXISTS pane_extension_templates (id TEXT PRIMARY KEY,name TEXT NOT NULL,selection TEXT NOT NULL,updated REAL NOT NULL)")?;
            Ok(())
        })?;
        Ok(Self { connection, clock })
    }
    fn row(&self, key: &str) -> Result<Value> {
        let row=self.connection.query_row("SELECT key,identity,conversation,harness,desired,revision,updated FROM pane_extension_drafts WHERE key=?",[key],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?,r.get::<_,String>(3)?,r.get::<_,String>(4)?,r.get::<_,i64>(5)?,r.get::<_,f64>(6)?))).optional()?;
        let (key, identity, conversation, harness, desired, revision, updated) =
            row.ok_or_else(|| conflict(MISSING))?;
        Ok(
            json!({"key":key,"identity":identity,"conversation":conversation,"harness":harness,"desired":decode(&desired)?,"revision":revision,"updated":updated}),
        )
    }
    pub fn state(
        &self,
        identity: &str,
        conversation: &str,
        harness: &str,
        defaults: &Value,
    ) -> Result<Value> {
        let defaults = selection_value(defaults)?;
        if identity.is_empty() || identity.chars().count() > 4096 {
            return Err(invalid("identidad de panel inválida"));
        }
        if conversation.chars().count() > 256 {
            return Err(invalid("conversación inválida"));
        }
        if !["claude", "codex", "grok", "opencode", "agy"].contains(&harness) {
            return Err(invalid("CLI no compatible con el estante"));
        }
        let key = format!(
            "{:x}",
            Sha256::digest(encode(&json!([identity, conversation, harness]))?.as_bytes())
        );
        atomic(self.connection, || {
            self.connection.execute("INSERT OR IGNORE INTO pane_extension_drafts (key,identity,conversation,harness,desired,updated) VALUES (?,?,?,?,?,?)",params![key,identity,conversation,harness,encode(&defaults)?,(self.clock)()])?;
            self.row(&key)
        })
    }
    pub fn require_revision(&self, key: &str, revision: &Value) -> Result<Value> {
        let row = self.row(key)?;
        if !revision.is_i64() || row["revision"].as_i64() != revision.as_i64() {
            return Err(conflict(STALE));
        }
        Ok(row)
    }
    pub fn save(&self, key: &str, revision: &Value, desired: &Value) -> Result<Value> {
        let desired = selection_value(desired)?;
        atomic(self.connection, || {
            let row = self.require_revision(key, revision)?;
            let active:Option<String>=self.connection.query_row("SELECT id FROM session_operations WHERE pane_key=? AND state NOT IN ('confirmed','failed','rolled_back') LIMIT 1",[row["identity"].as_str().unwrap_or("")],|r|r.get(0)).optional()?;
            if active.is_some() {
                return Err(conflict("hay una operación en curso para este panel"));
            }
            self.connection.execute("UPDATE pane_extension_drafts SET desired=?,revision=revision+1,updated=? WHERE key=?",params![encode(&desired)?,(self.clock)(),key])?;
            self.row(key)
        })
    }
    pub fn templates(&self) -> Result<Vec<Value>> {
        let mut stmt = self.connection.prepare(
            "SELECT id,name,selection,updated FROM pane_extension_templates ORDER BY updated,id",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, f64>(3)?,
            ))
        })?;
        rows.map(|r| {
            let (id, name, selection, updated) = r?;
            Ok(json!({"id":id,"name":name,"selection":decode(&selection)?,"updated":updated}))
        })
        .collect()
    }
    pub fn save_template(&self, name: &Value, selection: &Value) -> Result<Value> {
        let name = name
            .as_str()
            .filter(|s| (1..=80).contains(&strip(s).chars().count()) && !s.chars().any(|c| c < ' '))
            .ok_or_else(|| invalid("nombre de plantilla inválido"))?;
        let selection = selection_value(selection)?;
        let generated = crate::fresh_id("").map_err(|e| Fault::Persistence(e.to_string()))?;
        let id = &generated[1..]; // fresh_id("") = '-' + uuid.uuid4().hex.
        let now = (self.clock)();
        let result = json!({"id":id,"name":strip(name),"selection":selection,"updated":now});
        atomic(self.connection, || {
            self.connection.execute(
                "INSERT INTO pane_extension_templates (id,name,selection,updated) VALUES (?,?,?,?)",
                params![id, strip(name), encode(&selection)?, now],
            )?;
            Ok(result)
        })
    }
}
