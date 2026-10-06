//! Callback-driven quick shell allocation. No environment, process or host-clock access.
use chrono::{DateTime, FixedOffset};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

pub const LEASE_SECONDS: f64 = 30.0;
pub const WAIT_SECONDS: f64 = 15.0;
pub const POLL_SECONDS: f64 = 0.05;

#[derive(Debug)]
pub struct QuickTerminalError {
    pub code: &'static str,
    pub message: String,
    pub retryable: bool,
    pub cwd: Option<String>,
}
#[derive(Debug)]
pub enum Error {
    Quick(QuickTerminalError),
    CallerTransaction,
    Sql {
        source: rusqlite::Error,
        reserved_cwd: Option<PathBuf>,
    },
    Io(std::io::Error),
    NonUnicodePath {
        reserved_cwd: PathBuf,
    },
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Quick(e) => f.write_str(&e.message),
            Self::CallerTransaction => {
                f.write_str("la terminal requiere una transacción propia y duradera")
            }
            Self::Sql { source, .. } => source.fmt(f),
            Self::Io(e) => e.fmt(f),
            Self::NonUnicodePath { reserved_cwd } => write!(
                f,
                "carpeta reservada no representable en SQLite: {:?}",
                reserved_cwd
            ),
        }
    }
}
impl std::error::Error for Error {}
pub type Result<T> = std::result::Result<T, Error>;
/// A callback exception's empty message falls back to its supplied class name.
#[derive(Debug)]
pub struct CallbackError {
    pub message: String,
    pub class_name: String,
}
impl CallbackError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            class_name: "Error".into(),
        }
    }
}
pub struct Callbacks<'a> {
    pub exists: &'a dyn Fn(&str) -> std::result::Result<bool, CallbackError>,
    pub launch: &'a dyn Fn(&str, &str, &str) -> std::result::Result<(), CallbackError>,
    pub register: &'a dyn Fn(&str, &str, &str) -> std::result::Result<(), CallbackError>,
    pub clock: &'a dyn Fn() -> f64,
    pub sleep: &'a dyn Fn(f64),
}
pub struct Options<'a> {
    pub base: &'a Path,
    pub now: DateTime<FixedOffset>,
    pub lease: f64,
    pub wait: f64,
}
impl<'a> Options<'a> {
    pub fn new(base: &'a Path, now: DateTime<FixedOffset>) -> Self {
        Self {
            base,
            now,
            lease: LEASE_SECONDS,
            wait: WAIT_SECONDS,
        }
    }
}

pub fn default_base(home: &Path, override_path: Option<&Path>) -> PathBuf {
    override_path
        .filter(|p| !p.as_os_str().is_empty())
        .map(Path::to_path_buf)
        .unwrap_or_else(|| home.join("codebase/0xJesus/Terminal"))
}
pub fn identity(request_id: &str) -> (String, String) {
    let digest = format!("{:x}", Sha256::digest(request_id.as_bytes()));
    (
        format!("term-q{}", &digest[..12]),
        format!("pane-q{}", &digest[..24]),
    )
}
pub fn valid_request_id(request_id: &Value) -> bool {
    request_id.as_str().is_some_and(|id| {
        !id.is_empty()
            && id.len() <= 128
            && id.as_bytes()[0].is_ascii_alphanumeric()
            && id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b':' | b'-'))
    })
}
/// Only timezone-aware timestamps can be supplied; naive dates do not type-check.
pub fn reserve_directory(base: &Path, now: DateTime<FixedOffset>) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(base)?;
    let stem = directory_stem(now);
    let mut suffix = 1u64;
    loop {
        let path = base.join(if suffix == 1 {
            stem.clone()
        } else {
            format!("{stem}-{suffix}")
        });
        match std::fs::create_dir(&path) {
            Ok(()) => return Ok(path),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                suffix = suffix
                    .checked_add(1)
                    .ok_or_else(|| std::io::Error::other("sufijos de carpeta agotados"))?;
            }
            Err(e) => return Err(e),
        }
    }
}

/// Nombre fechado compartido con la reserva por descriptores del escritorio.
pub fn directory_stem(now: DateTime<FixedOffset>) -> String {
    now.with_timezone(&chrono_tz::America::Mexico_City)
        .format("T-%Y-%m-%d-%H-%M-%S")
        .to_string()
}

/// La terminal reservada para un `requestId`: carpeta, sesión tmux y llave del pane.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Terminal {
    pub cwd: String,
    pub session: String,
    pub pane: String,
}
impl Terminal {
    /// `os.path.basename(cwd)`.
    pub fn label(&self) -> &str {
        self.cwd.rsplit('/').next().unwrap_or("")
    }
    /// `_result(row, created)`.
    pub fn result(&self, created: bool) -> Value {
        json!({"tabId":self.session,"paneKey":self.pane,"cwd":self.cwd,"label":self.label(),"created":created})
    }
}
/// Resultado de `_claim`: ya lista, propia (hay que lanzarla) o en curso.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Claim {
    Ready(Terminal),
    Own(Terminal),
    Wait,
}
fn sql(source: rusqlite::Error, reserved_cwd: Option<PathBuf>) -> Error {
    Error::Sql {
        source,
        reserved_cwd,
    }
}
fn quick(code: &'static str, message: String, retryable: bool, cwd: Option<String>) -> Error {
    Error::Quick(QuickTerminalError {
        code,
        message,
        retryable,
        cwd,
    })
}
fn require_own_transaction(conn: &Connection) -> Result<()> {
    if conn.is_autocommit() {
        Ok(())
    } else {
        Err(Error::CallerTransaction)
    }
}
fn io_message(error: &std::io::Error) -> String {
    let message = error.to_string();
    if let Some(code) = error.raw_os_error() {
        message
            .strip_suffix(&format!(" (os error {code})"))
            .unwrap_or(&message)
            .into()
    } else {
        message
    }
}
/// `_claim`: una transacción `BEGIN IMMEDIATE`; la carpeta se reserva dentro.
pub fn claim(
    conn: &Connection,
    id: &str,
    options: &Options<'_>,
    clock: &dyn Fn() -> f64,
) -> Result<Claim> {
    require_own_transaction(conn)?;
    let tx = rusqlite::Transaction::new_unchecked(conn, TransactionBehavior::Immediate)
        .map_err(|e| sql(e, None))?;
    let row=conn.query_row("SELECT state,cwd,session,pane_key,lease_until FROM quick_terminal_requests WHERE request_id=?",[id],|r|{
        Ok((r.get::<_,String>(0)?,Terminal{cwd:r.get(1)?,session:r.get(2)?,pane:r.get(3)?},r.get::<_,f64>(4)?))
    }).optional().map_err(|e|sql(e,None))?;
    let t = clock();
    let mut reserved_cwd = None;
    let outcome = match row {
        None => {
            let path = reserve_directory(options.base, options.now).map_err(|e| {
                quick(
                    "folder",
                    format!(
                        "No se pudo crear la carpeta en {}: {}",
                        options.base.display(),
                        io_message(&e)
                    ),
                    true,
                    None,
                )
            })?;
            let cwd = path
                .to_str()
                .ok_or_else(|| Error::NonUnicodePath {
                    reserved_cwd: path.clone(),
                })?
                .to_owned();
            let (session, pane) = identity(id);
            reserved_cwd = Some(path);
            conn.execute("INSERT INTO quick_terminal_requests VALUES (?, 'launching', ?, ?, ?, NULL, ?, ?, ?)",params![id,cwd,session,pane,t+options.lease,t,t]).map_err(|e|sql(e,reserved_cwd.clone()))?;
            Claim::Own(Terminal { cwd, session, pane })
        }
        Some((state, row, _)) if state == "ready" => Claim::Ready(row),
        Some((state, _, lease_until)) if state == "launching" && lease_until > t => Claim::Wait,
        Some((_, row, _)) => {
            conn.execute("UPDATE quick_terminal_requests SET state='launching',error=NULL,lease_until=?,updated_at=? WHERE request_id=?",params![t+options.lease,t,id]).map_err(|e|sql(e,None))?;
            Claim::Own(row)
        }
    };
    tx.commit().map_err(|e| sql(e, reserved_cwd))?;
    Ok(outcome)
}
/// `_finish`: fija `state`/`error` de la fila en su propia transacción.
pub fn finish(
    conn: &Connection,
    id: &str,
    state: &str,
    error: Option<&str>,
    cwd: &str,
    clock: &dyn Fn() -> f64,
) -> Result<()> {
    require_own_transaction(conn)?;
    let tx = rusqlite::Transaction::new_unchecked(conn, TransactionBehavior::Immediate)
        .map_err(|e| sql(e, Some(cwd.into())))?;
    conn.execute(
        "UPDATE quick_terminal_requests SET state=?,error=?,updated_at=? WHERE request_id=?",
        params![state, error, clock(), id],
    )
    .map_err(|e| sql(e, Some(cwd.into())))?;
    tx.commit().map_err(|e| sql(e, Some(cwd.into())))?;
    Ok(())
}

pub fn open_quick_terminal(
    conn: &Connection,
    request_id: &Value,
    options: &Options<'_>,
    callbacks: &Callbacks<'_>,
) -> Result<Value> {
    if !valid_request_id(request_id) {
        return Err(quick("request", "requestId inválido".into(), false, None));
    }
    require_own_transaction(conn)?;
    let id = request_id.as_str().expect("validated request id");
    let deadline = (callbacks.clock)() + options.wait;
    let row = loop {
        match claim(conn, id, options, callbacks.clock)? {
            Claim::Ready(row) => return Ok(row.result(false)),
            Claim::Own(row) => break row,
            Claim::Wait => {
                if (callbacks.clock)() >= deadline {
                    return Err(quick(
                        "busy",
                        "La terminal se está abriendo; reintenta en unos segundos".into(),
                        true,
                        None,
                    ));
                }
                (callbacks.sleep)(POLL_SECONDS);
            }
        }
    };
    let launch_result = (|| -> std::result::Result<(), CallbackError> {
        std::fs::create_dir_all(&row.cwd).map_err(|e| CallbackError::new(e.to_string()))?;
        if !(callbacks.exists)(&row.session)? {
            (callbacks.launch)(&row.session, &row.cwd, &row.pane)?;
        }
        (callbacks.register)(&row.session, row.label(), &row.cwd)
    })();
    if let Err(error) = launch_result {
        let message = if error.message.is_empty() {
            error.class_name
        } else {
            error.message
        };
        let stored = message.chars().take(500).collect::<String>();
        finish(conn, id, "failed", Some(&stored), &row.cwd, callbacks.clock)?;
        return Err(quick(
            "launch",
            format!("No se pudo abrir la terminal: {message}"),
            true,
            Some(row.cwd),
        ));
    }
    finish(conn, id, "ready", None, &row.cwd, callbacks.clock)?;
    Ok(row.result(true))
}
