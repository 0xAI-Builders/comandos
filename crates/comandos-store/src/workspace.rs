//! SQLite workspace persistence over an explicitly borrowed connection.
use comandos_core::json::{
    PythonLoads, python_eq, python_loads, response_dumps, workspace_dumps, workspace_loads,
};
use comandos_core::workspace::{self, CloseGroupState, WorkspaceError};
use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior, params};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Restoring,
    Ready,
    Failed,
}
impl std::fmt::Display for Phase {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Restoring => "restoring",
            Self::Ready => "ready",
            Self::Failed => "failed",
        })
    }
}
#[derive(Clone, Debug, PartialEq)]
pub struct WorkspaceState {
    pub revision: i64,
    pub document: Value,
    pub recovered: bool,
}
impl WorkspaceState {
    pub fn as_json(&self) -> Value {
        let mut out = json!({"revision":self.revision,"document":self.document});
        if self.recovered {
            out["recovered"] = true.into();
        }
        out
    }
}
#[derive(Debug)]
pub enum Error {
    Invalid(String),
    Conflict {
        current: Option<WorkspaceState>,
        message: &'static str,
    },
    NotReady(Phase),
    EmptyInventory,
    ActiveTransaction,
    Sql(rusqlite::Error),
    Json(serde_json::Error),
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Invalid(message) => f.write_str(message),
            Self::Conflict { message, .. } => f.write_str(message),
            Self::NotReady(phase) => phase.fmt(f),
            Self::EmptyInventory => f.write_str(""),
            Self::ActiveTransaction => {
                f.write_str("workspace requiere una conexión sin transacción activa")
            }
            Self::Sql(error) => error.fmt(f),
            Self::Json(error) => error.fmt(f),
        }
    }
}
impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Sql(error) => Some(error),
            Self::Json(error) => Some(error),
            _ => None,
        }
    }
}
impl From<rusqlite::Error> for Error {
    fn from(error: rusqlite::Error) -> Self {
        Self::Sql(error)
    }
}
impl From<serde_json::Error> for Error {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}
pub type Result<T> = std::result::Result<T, Error>;

pub struct WorkspaceStore<'a> {
    conn: &'a Connection,
    phase: Phase,
}
impl<'a> WorkspaceStore<'a> {
    pub fn new(conn: &'a Connection) -> Self {
        Self {
            conn,
            phase: Phase::Restoring,
        }
    }
    pub fn phase(&self) -> Phase {
        self.phase
    }
    pub fn set_phase(&mut self, phase: &str) -> Result<()> {
        self.phase = match phase {
            "restoring" => Phase::Restoring,
            "ready" => Phase::Ready,
            "failed" => Phase::Failed,
            _ => return Err(Error::Invalid("Fase desconocida".into())),
        };
        Ok(())
    }
    fn read(&self, table: &str) -> Result<Option<WorkspaceState>> {
        let row: Option<(i64, String)> = self
            .conn
            .query_row(
                &format!("SELECT revision, document FROM {table} WHERE id = 1"),
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        let Some((revision, encoded)) = row else {
            return Ok(None);
        };
        let Ok(document) = workspace_loads(&encoded) else {
            return Ok(None);
        };
        if workspace::validate_document(&document).is_err() {
            return Ok(None);
        }
        Ok(Some(WorkspaceState {
            revision,
            document,
            recovered: false,
        }))
    }
    pub fn current(&self) -> Result<Option<WorkspaceState>> {
        if let Some(current) = self.read("workspace_current")? {
            return Ok(Some(current));
        }
        Ok(self.read("workspace_previous")?.map(|mut state| {
            state.recovered = true;
            state
        }))
    }
    // Every mutator owns a transaction, as does the reference store. Joining a
    // caller's transaction would change the failure and visibility guarantees.
    fn transaction(&self) -> Result<Transaction<'_>> {
        if !self.conn.is_autocommit() {
            return Err(Error::ActiveTransaction);
        }
        Ok(Transaction::new_unchecked(
            self.conn,
            TransactionBehavior::Immediate,
        )?)
    }
    /// `now_seconds` and request ids come from the caller; no clock is read here.
    /// Revision comparison follows Python, including integer/float/bool equality.
    pub fn commit(
        &self,
        expected_revision: &Value,
        document: &Value,
        request_id: &str,
        reason: &str,
        now_seconds: f64,
    ) -> Result<WorkspaceState> {
        ident(request_id, "requestId")?;
        if !matches!(reason, "auto" | "user") {
            return Err(Error::Invalid("Motivo inválido".into()));
        }
        if reason == "auto" && self.phase != Phase::Ready {
            return Err(Error::NotReady(self.phase));
        }
        workspace::validate_document(document).map_err(domain_error)?;
        clock(now_seconds)?;
        let encoded = canonical(document)?;
        let digest = format!("{:x}", Sha256::digest(encoded.as_bytes()));
        let tx = self.transaction()?;
        let seen: Option<(String, i64)> = self
            .conn
            .query_row(
                "SELECT digest, revision FROM workspace_requests WHERE request_id = ?",
                [request_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        if let Some((previous_digest, revision)) = seen {
            tx.rollback()?;
            if previous_digest != digest {
                return Err(Error::Conflict {
                    current: self.current()?,
                    message: "requestId reutilizado con otro contenido",
                });
            }
            return Ok(WorkspaceState {
                revision,
                document: workspace_loads(&encoded)?,
                recovered: false,
            });
        }
        let revision: i64 = self
            .conn
            .query_row(
                "SELECT revision FROM workspace_current WHERE id = 1",
                [],
                |row| row.get(0),
            )
            .optional()?
            .unwrap_or(0);
        if !python_eq(expected_revision, &json!(revision)) {
            tx.rollback()?;
            return Err(Error::Conflict {
                current: self.current()?,
                message: "Revisión desactualizada",
            });
        }
        if reason == "auto"
            && workspace::is_empty(document)
            && revision != 0
            && self
                .read("workspace_current")?
                .is_some_and(|state| !workspace::is_empty(&state.document))
        {
            return Err(Error::EmptyInventory);
        }
        let next = revision
            .checked_add(1)
            .ok_or_else(|| Error::Invalid("Revisión fuera de rango SQLite".into()))?;
        self.conn.execute(
            "INSERT OR REPLACE INTO workspace_previous SELECT 1, revision, document, updated_at FROM workspace_current WHERE id = 1", [],
        )?;
        self.conn.execute(
            "INSERT OR REPLACE INTO workspace_current VALUES (1, ?, ?, ?)",
            params![next, encoded, now_seconds],
        )?;
        self.conn.execute(
            "INSERT INTO workspace_requests VALUES (?, ?, ?, ?)",
            params![request_id, digest, next, now_seconds],
        )?;
        self.conn.execute(
            "DELETE FROM workspace_requests WHERE created_at < ?",
            [now_seconds - 86400.0],
        )?;
        let document = workspace_loads(&encoded)?;
        tx.commit()?;
        Ok(WorkspaceState {
            revision: next,
            document,
            recovered: false,
        })
    }
    /// Merge focus/drafts/anchors under the writer lock, preserving omitted data.
    pub fn save_client(&self, device_id: &str, state: &Value, now_seconds: f64) -> Result<Value> {
        ident(device_id, "deviceId")?;
        if !state.is_object() {
            return Err(Error::Invalid("Estado de cliente inválido".into()));
        }
        clock(now_seconds)?;
        let tx = self.transaction()?;
        let previous = self.client(device_id)?.unwrap_or_else(|| json!({}));
        let clean =
            workspace::clean_client_state(device_id, &previous, state, now_seconds * 1000.0)
                .map_err(domain_error)?;
        let encoded = canonical(&clean)?;
        self.conn.execute(
            "INSERT OR REPLACE INTO workspace_clients VALUES (?, ?, ?)",
            params![device_id, encoded, now_seconds],
        )?;
        tx.commit()?;
        Ok(clean)
    }
    pub fn client(&self, device_id: &str) -> Result<Option<Value>> {
        let encoded: Option<String> = self
            .conn
            .query_row(
                "SELECT state FROM workspace_clients WHERE device_id = ?",
                [device_id],
                |row| row.get(0),
            )
            .optional()?;
        encoded
            .map(|encoded| workspace_loads(&encoded).map_err(Error::from))
            .transpose()
    }
    pub fn meta(&self, key: &str) -> Result<Option<String>> {
        Ok(self
            .conn
            .query_row(
                "SELECT value FROM workspace_meta WHERE key = ?",
                [key],
                |row| row.get(0),
            )
            .optional()?)
    }
    pub fn set_meta(&self, key: &str, value: &str) -> Result<()> {
        let tx = self.transaction()?;
        self.conn.execute(
            "INSERT OR REPLACE INTO workspace_meta VALUES (?, ?)",
            params![key, value],
        )?;
        tx.commit()?;
        Ok(())
    }
}
fn ident(value: &str, what: &str) -> Result<()> {
    if value.is_empty() || value.chars().count() > workspace::MAX_ID {
        return Err(Error::Invalid(format!("{what} inválido")));
    }
    Ok(())
}
fn clock(now_seconds: f64) -> Result<()> {
    if !now_seconds.is_finite() {
        return Err(Error::Invalid("Reloj inválido".into()));
    }
    Ok(())
}
fn canonical(value: &Value) -> Result<String> {
    workspace_dumps(value).map_err(Error::Invalid)
}
fn domain_error(error: WorkspaceError) -> Error {
    Error::Invalid(error.to_string())
}
impl CloseGroupState for WorkspaceStore<'_> {
    fn current(&mut self) -> workspace::Result<Option<Value>> {
        WorkspaceStore::current(self)
            .map(|s| s.map(|s| s.as_json()))
            .map_err(|e| WorkspaceError::Callback(e.to_string()))
    }
    /// `seen = store.meta(key); if seen: return json.loads(seen)`. Un texto
    /// guardado que el Python no decodifica es el `ValueError` de su
    /// `JSONDecodeError` (`Invalid` con el mismo `str(exc)`: la ruta responde
    /// 400); lo que no se reproduce con certeza queda como `Callback`.
    fn meta(&mut self, key: &str) -> workspace::Result<Option<Value>> {
        let raw =
            WorkspaceStore::meta(self, key).map_err(|e| WorkspaceError::Callback(e.to_string()))?;
        let Some(raw) = raw.filter(|v| !v.is_empty()) else {
            return Ok(None);
        };
        match workspace_loads(&raw) {
            Ok(value) => Ok(Some(value)),
            Err(error) => Err(match python_loads(&raw) {
                PythonLoads::Error(message) => WorkspaceError::Invalid(message),
                PythonLoads::Ok | PythonLoads::Unsure => {
                    WorkspaceError::Callback(error.to_string())
                }
            }),
        }
    }
    /// `store.set_meta(key, json.dumps(result))`: el texto del Python (orden de
    /// inserción, ASCII escapado, separadores por omisión), el mismo que lee la
    /// repetición en los dos lados.
    fn set_meta(&mut self, key: &str, value: &Value) -> workspace::Result<()> {
        response_dumps(value)
            .map_err(Error::Invalid)
            .and_then(|encoded| WorkspaceStore::set_meta(self, key, &encoded))
            .map_err(|e| WorkspaceError::Callback(e.to_string()))
    }
}
