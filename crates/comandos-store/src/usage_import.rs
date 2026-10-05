//! Importación local de uso de `bin/cc_usage.py`: rollouts de Codex
//! (`record_local_codex_rollouts` 1418), actualizaciones de Grok
//! (`record_local_grok_updates` 1611), transcripts de Claude
//! (`record_local_claude_jsonl` 2055) y la base de OpenCode
//! (`record_local_opencode_db` 1539), más la poda (`prune_old_turns` 1521), la
//! reconciliación de interacciones huérfanas (`reconcile_orphan_interactions`
//! 2395) y las configuraciones de sesión (`record_session_config` 2349,
//! `latest_session_config` 2370).
//!
//! Todo es síncrono y corre en el hilo del carril de importación. Los archivos se
//! leen línea a línea (nuevas líneas universales y `errors="replace"`, como el
//! `open()` del Python) y de cada línea solo se decodifican las claves que se usan:
//! una línea de varios MiB (resultados de herramientas) se valida y se descarta
//! sin materializarla. Los turnos y tramos se escriben por lotes acotados, cada uno
//! en su transacción, en el orden del Python; el Python los escribía todos al final
//! (dos transacciones). Diferencia solo visible si un importador falla a mitad: el
//! Python no escribía nada de ese importador y aquí quedan los lotes ya escritos.
//!
//! Lo que el Python lanzaría sin capturar (`OverflowError` de `int(float('inf'))`,
//! `AttributeError` de un `summary.json` que no es objeto, un error de SQLite) es
//! `ImportError` y termina la vuelta; lo que no se reproduce con certeza en una
//! línea (`repr` de un contenedor, un entero fuera de `i64`) salta esa línea.
use crate::usage::{TURN_INSERT_SQL, stable_id};
use comandos_core::{
    json::{object_fields, python_eq, truthy, workspace_dumps_with_options, workspace_loads},
    text::{self, NumError},
    usage_state::{self, LocalZone, PyNum, UsageError},
};
use rusqlite::{
    Connection, OpenFlags, OptionalExtension, ToSql, Transaction, TransactionBehavior, params,
    types::{Value as Sql, ValueRef},
};
use serde_json::{Map, Value, value::RawValue};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    fs,
    io::{BufRead, BufReader},
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
};

type Object = Map<String, Value>;

/// Turnos o tramos pendientes antes de escribirlos en una transacción. Un
/// turno son 31 celdas (≈ 1 KB): 128 caben en < 128 KiB, bajo el umbral de
/// `mmap` de glibc. Un lote de 2 MB lo servía glibc con `mmap` y, al soltarlo,
/// subía su umbral de `mmap` y el de recorte: las arenas dejaban de recortarse.
const BATCH: usize = 128;
/// Un búfer de línea que creció por encima de esto se suelta tras la línea.
const LINE_KEEP: usize = 1 << 20;
/// `record_local_grok_updates(..., max_files=200)`.
const GROK_MAX_FILES: i64 = 200;
/// `CONFIG_RACE_WINDOW` (cc_usage.py:2379).
const CONFIG_RACE_WINDOW: i64 = 900;

#[derive(Debug)]
pub enum ImportError {
    Sql(rusqlite::Error),
    /// Excepción que el Python no captura: el hilo de importación moría ahí.
    Raises,
    /// La puerta de esquema del carril rechazó la base a mitad: no se escribe más.
    Refused,
}

impl std::fmt::Display for ImportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Sql(e) => e.fmt(f),
            Self::Raises => f.write_str("el Python lanzaría una excepción"),
            Self::Refused => f.write_str("la base de uso dejó de ser compatible"),
        }
    }
}

impl std::error::Error for ImportError {}

impl From<rusqlite::Error> for ImportError {
    fn from(e: rusqlite::Error) -> Self {
        Self::Sql(e)
    }
}

pub type Result<T> = std::result::Result<T, ImportError>;

/// Resultado de una conversión de línea: `Ok(None)` salta la línea (`Unsure`).
type Line<T> = Result<Option<T>>;

/// `Overflow`/`Raises` terminan la vuelta; `Unsure` salta la línea.
fn line<T>(r: usage_state::Result<T>) -> Line<T> {
    match r {
        Ok(v) => Ok(Some(v)),
        Err(UsageError::Unsure) => Ok(None),
        Err(UsageError::Overflow | UsageError::Raises) => Err(ImportError::Raises),
    }
}

macro_rules! take {
    ($e:expr) => {
        match line($e)? {
            Some(v) => v,
            None => return Ok(None),
        }
    };
}

/// `git_root_for_path`: el carril de importación lo implementa con
/// `Handle::block_on(git_root_for_path(path))` (puente de la 2c); las pruebas, con un mapa.
pub trait GitRoots {
    fn root(&self, path: &str) -> String;
}

pub struct ImportPlan<'a> {
    pub now: i64,
    pub max_age_days: i64,
    /// `int(env or 0) or None`: `None` sin tope; negativo corta desde el final.
    pub claude_max_files: Option<i64>,
    pub codex_max_files: Option<i64>,
    pub home: &'a Path,
    /// `COMANDOS_CLAUDE_PROJECTS_DIR` (solo cuenta `main`) y `COMANDOS_OPENCODE_DB`.
    pub claude_projects_main: Option<PathBuf>,
    pub opencode_db: PathBuf,
    pub zone: &'a dyn LocalZone,
    /// La puerta de esquema del carril antes de cada escritura.
    pub admit: &'a dyn Fn(&Connection) -> bool,
    /// Verdadero si hay que dejar de importar (el carril se apaga): se mira
    /// antes de cada archivo y de cada lote, y la vuelta termina en `Refused`.
    pub cancelled: &'a dyn Fn() -> bool,
}

impl ImportPlan<'_> {
    fn check(&self) -> Result<()> {
        if (self.cancelled)() {
            Err(ImportError::Refused)
        } else {
            Ok(())
        }
    }

    /// `ts - int(max_age_days) * 24 * 3600`.
    fn cutoff(&self) -> Result<i64> {
        self.max_age_days
            .checked_mul(86_400)
            .and_then(|s| self.now.checked_sub(s))
            .ok_or(ImportError::Raises)
    }
}

/// `seen` acotado (rul. 6): por fuente, solo las rutas del corte de esta vuelta.
#[derive(Default, Clone)]
pub struct ImportSeen {
    sources: BTreeMap<String, BTreeMap<PathBuf, f64>>,
    reads: usize,
}

impl ImportSeen {
    /// Cuántos archivos de Codex y Claude se abrieron desde que existe.
    pub fn files_read(&self) -> usize {
        self.reads
    }

    /// Rutas recordadas en total (para la cota).
    pub fn len(&self) -> usize {
        self.sources.values().map(BTreeMap::len).sum()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// `_changed_files` (2045): las rutas del corte cuyo `mtime` cambió. El `seen`
    /// de la fuente se actualiza ANTES de leerlas (como el Python) y se reconstruye
    /// con exactamente las rutas del corte.
    fn changed(&mut self, source: &str, files: &[(f64, PathBuf)]) -> Vec<(f64, PathBuf)> {
        let prev = self.sources.remove(source).unwrap_or_default();
        let out = files
            .iter()
            .filter(|(m, p)| prev.get(p).is_none_or(|old| old != m))
            .cloned()
            .collect();
        self.sources.insert(
            source.to_owned(),
            files.iter().map(|(m, p)| (p.clone(), *m)).collect(),
        );
        out
    }

    fn retain_sources(&mut self, prefix: &str, keep: &HashSet<String>) {
        self.sources
            .retain(|k, _| !k.starts_with(prefix) || keep.contains(k));
    }
}

// ---------------------------------------------------------------- JSON a medias

/// Un objeto JSON leído a medias: las claves pedidas como texto crudo (sin copiar el
/// resto) o, si serde no lo acepta (`NaN`, más de 128 niveles), el objeto entero de
/// `workspace_loads`, que admite lo mismo que `json.loads`.
enum Doc<'a> {
    Raw(Vec<(&'static str, Option<&'a RawValue>)>),
    Full(Object),
}

impl<'a> Doc<'a> {
    /// `json.loads(text)` que es un `dict`; `None` si no lo es o no es JSON.
    fn parse(text: &'a str, keys: &[&'static str]) -> Option<Self> {
        match object_fields(text, keys) {
            Ok(raws) => Some(Doc::Raw(keys.iter().copied().zip(raws).collect())),
            Err(_) => match workspace_loads(text) {
                Ok(Value::Object(map)) => Some(Doc::Full(map)),
                _ => None,
            },
        }
    }

    fn empty() -> Self {
        Doc::Full(Object::new())
    }

    fn raw(&self, key: &str) -> Option<&'a RawValue> {
        match self {
            Doc::Raw(fields) => fields.iter().find(|(k, _)| *k == key).and_then(|(_, r)| *r),
            Doc::Full(_) => None,
        }
    }

    /// `obj.get(key)` (`null` si falta).
    fn get(&self, key: &str) -> Value {
        match self {
            Doc::Raw(_) => self
                .raw(key)
                .and_then(|r| workspace_loads(r.get()).ok())
                .unwrap_or(Value::Null),
            Doc::Full(map) => map.get(key).cloned().unwrap_or(Value::Null),
        }
    }

    /// `obj.get(key) == want` con un texto.
    fn is(&self, key: &str, want: &str) -> bool {
        matches!(self.get(key), Value::String(s) if s == want)
    }

    /// `obj.get(key)` si es un `dict`, leído con sus propias claves.
    fn object(&self, key: &str, keys: &[&'static str]) -> Option<Doc<'a>> {
        match self {
            Doc::Raw(_) => Doc::parse(self.raw(key)?.get(), keys),
            Doc::Full(map) => match map.get(key) {
                Some(Value::Object(inner)) => Some(Doc::Full(inner.clone())),
                _ => None,
            },
        }
    }
}

fn get<'v>(obj: &'v Object, key: &str) -> &'v Value {
    static NULL: Value = Value::Null;
    obj.get(key).unwrap_or(&NULL)
}

/// `x if isinstance(x, dict) else {}`.
fn dict(value: Value) -> Object {
    match value {
        Value::Object(map) => map,
        _ => Object::new(),
    }
}

/// `a or b` de Python.
fn or(a: Value, b: Value) -> Value {
    if truthy(&a) { a } else { b }
}

/// `_text(x) or fallback`.
fn text_or(value: &Value, fallback: &str) -> usage_state::Result<String> {
    let s = usage_state::text(value)?;
    Ok(if s.is_empty() { fallback.to_owned() } else { s })
}

/// `_as_int(value, None)`.
fn as_int_opt(value: &Value) -> usage_state::Result<Option<i64>> {
    match value {
        Value::Null => Ok(None),
        Value::String(s) if s.is_empty() => Ok(None),
        Value::String(s) => match text::int(s) {
            Ok(n) => Ok(Some(n)),
            Err(NumError::Invalid) => Ok(None),
            Err(NumError::Exotic) => Err(UsageError::Unsure),
        },
        Value::Array(_) | Value::Object(_) => Ok(None),
        _ => match PyNum::of(value)? {
            Some(PyNum::Float(x)) if x.is_nan() => Ok(None),
            Some(_) => usage_state::as_int(value, 0).map(Some),
            None => Ok(None),
        },
    }
}

fn int(value: &Value) -> usage_state::Result<i64> {
    usage_state::as_int(value, 0)
}

fn add(a: i64, b: i64) -> usage_state::Result<i64> {
    a.checked_add(b).ok_or(UsageError::Unsure)
}

/// `json.dumps(obj, sort_keys=True)`.
fn dumps(obj: Object) -> usage_state::Result<String> {
    workspace_dumps_with_options(&Value::Object(obj), true, false).map_err(|_| UsageError::Unsure)
}

/// Una ruta como `str` de Python; una que no es UTF-8 no se importa (el Python la
/// llevaría con escapes `\udcXX`).
fn path_text(path: &Path) -> Option<&str> {
    path.to_str()
}

// ---------------------------------------------------------------- archivos

/// `st_mtime` de `os.stat` (sigue enlaces): `sec + nsec * 1e-9` en `double`,
/// como `fill_time` de CPython 3.10.
fn mtime(path: &Path) -> Option<f64> {
    let meta = fs::metadata(path).ok()?;
    Some(meta.mtime() as f64 + meta.mtime_nsec() as f64 * 1e-9)
}

/// `os.walk(top)` sin seguir enlaces a directorios, con las carpetas de `prune`
/// podadas: llama a `found` con cada archivo (lo que `os.walk` pone en `names`,
/// también enlaces rotos). Errores de lectura de una carpeta se ignoran.
fn walk(top: &Path, prune: &[&str], found: &mut dyn FnMut(PathBuf, &str)) {
    let mut stack = vec![top.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let name = entry.file_name();
            let path = entry.path();
            let is_dir = match entry.file_type() {
                Ok(t) if t.is_dir() => {
                    if !name.to_str().is_some_and(|n| prune.contains(&n)) {
                        stack.push(path);
                    }
                    continue;
                }
                // Un enlace a un directorio va a `dirs` pero no se recorre.
                Ok(t) if t.is_symlink() => fs::metadata(&path).is_ok_and(|m| m.is_dir()),
                Ok(_) => false,
                Err(_) => false,
            };
            if is_dir {
                continue;
            }
            if let Some(name) = name.to_str() {
                found(path, name);
            }
        }
    }
}

/// `files.sort(reverse=True)` sobre `(mtime, ruta)` y `files[:max_files]`.
fn sort_and_cut(mut files: Vec<(f64, PathBuf)>, max_files: Option<i64>) -> Vec<(f64, PathBuf)> {
    files.sort_by(|a, b| {
        b.0.partial_cmp(&a.0)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| b.1.as_os_str().cmp(a.1.as_os_str()))
    });
    if let Some(n) = max_files {
        let len = files.len();
        let keep = if n >= 0 {
            usize::try_from(n).unwrap_or(usize::MAX).min(len)
        } else {
            len.saturating_sub(usize::try_from(n.unsigned_abs()).unwrap_or(usize::MAX))
        };
        files.truncate(keep);
    }
    files
}

/// `for line_no, line in enumerate(open(path, errors="replace"), 1)`: líneas
/// partidas por `\n`, `\r` o `\r\n` (nuevas líneas universales), bytes no UTF-8
/// como U+FFFD. El búfer se reutiliza y se suelta si una línea lo hizo crecer.
/// Un error de lectura a mitad corta el archivo (el `except OSError: continue`)
/// conservando lo ya visto.
fn each_line(file: fs::File, mut visit: impl FnMut(usize, &str) -> Result<()>) -> Result<()> {
    let mut reader = BufReader::with_capacity(64 * 1024, file);
    let mut buf: Vec<u8> = Vec::new();
    let mut skip_lf = false;
    let mut no = 0usize;
    loop {
        let chunk = match reader.fill_buf() {
            Ok(chunk) => chunk,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => return Ok(()),
        };
        if chunk.is_empty() {
            if !buf.is_empty() {
                no += 1;
                visit(no, &String::from_utf8_lossy(&buf))?;
            }
            return Ok(());
        }
        if skip_lf {
            skip_lf = false;
            if chunk.first() == Some(&b'\n') {
                reader.consume(1);
                continue;
            }
        }
        match chunk.iter().position(|b| *b == b'\n' || *b == b'\r') {
            Some(at) => {
                let end = chunk.get(at).copied();
                buf.extend_from_slice(chunk.get(..at).unwrap_or_default());
                // `\r\n` en el mismo trozo: se consume entero.
                let mut used = at + 1;
                if end == Some(b'\r') {
                    if chunk.get(at + 1) == Some(&b'\n') {
                        used += 1;
                    } else if chunk.len() == at + 1 {
                        skip_lf = true;
                    }
                }
                reader.consume(used);
                no += 1;
                visit(no, &String::from_utf8_lossy(&buf))?;
                buf.clear();
                if buf.capacity() > LINE_KEEP {
                    buf = Vec::new();
                }
            }
            None => {
                let len = chunk.len();
                buf.extend_from_slice(chunk);
                reader.consume(len);
            }
        }
    }
}

/// `open(path)` de un archivo regular (un directorio, FIFO o socket no se lee).
fn open_regular(path: &Path) -> Option<fs::File> {
    let file = fs::File::open(path).ok()?;
    file.metadata().ok()?.is_file().then_some(file)
}

/// `account_homes` (cc_usage.py:1977): `main` = `~/<default_rel>` y cada subcarpeta
/// de `~/<accounts_rel>` (orden por nombre; sin `.`/`-` al principio, sin `.lock` al
/// final, solo directorios). Nombres que no son UTF-8 se omiten.
pub fn account_homes(home: &Path, default_rel: &str, accounts_rel: &str) -> Vec<(String, PathBuf)> {
    let mut out = vec![("main".to_owned(), home.join(default_rel))];
    let root = home.join(accounts_rel);
    let mut names: Vec<String> = match fs::read_dir(&root) {
        Ok(entries) => entries
            .flatten()
            .filter_map(|e| e.file_name().into_string().ok())
            .collect(),
        Err(_) => Vec::new(),
    };
    names.sort();
    for name in names {
        let path = root.join(&name);
        if name.starts_with(['.', '-']) || name.ends_with(".lock") || !path.is_dir() {
            continue;
        }
        out.push((name, path));
    }
    out
}

// ---------------------------------------------------------------- escritura

/// Un turno de `_turn_values` (618) con las claves que da cada importador; las que
/// faltan toman el valor por omisión del Python en `values`.
struct Turn {
    id: String,
    provider: String,
    agent: &'static str,
    tmux_session: Sql,
    tmux_pane: Sql,
    pane_pwd: String,
    git_root: String,
    model: String,
    reasoning_effort: String,
    started: i64,
    finished: i64,
    input: i64,
    output: i64,
    cache_read: i64,
    cache_write: i64,
    total: i64,
    cost: f64,
    source: &'static str,
    confidence: &'static str,
    raw: String,
    harness: Option<&'static str>,
    motor: Option<&'static str>,
    route_id: Option<&'static str>,
    harness_account: Sql,
    motor_account: Sql,
    interaction_id: Sql,
    /// `None`: la clave falta (por omisión `max(0, (fin - inicio) * 1000)`).
    duration_ms: Option<Option<i64>>,
    reasoning_tokens: Option<i64>,
}

const TURN_PARAMS: [&str; 31] = [
    ":id",
    ":provider",
    ":agent",
    ":tmux_session",
    ":tmux_pane",
    ":pane_pwd",
    ":git_root",
    ":model",
    ":reasoning_effort",
    ":turn_started_at",
    ":turn_finished_at",
    ":input_tokens",
    ":output_tokens",
    ":cache_read_tokens",
    ":cache_write_tokens",
    ":total_tokens",
    ":cost_usd",
    ":source",
    ":confidence",
    ":raw",
    ":harness",
    ":motor",
    ":route_id",
    ":harness_account",
    ":motor_account",
    ":interaction_id",
    ":experiment_run_id",
    ":tool_profile",
    ":duration_ms",
    ":outcome",
    ":reasoning_tokens",
];

fn txt(s: impl Into<String>) -> Sql {
    Sql::Text(s.into())
}

impl Turn {
    /// `_turn_values` (618) en el orden de `TURN_FIELDS`.
    fn values(self) -> usage_state::Result<[Sql; 31]> {
        let harness = self.harness.unwrap_or(self.agent).to_owned();
        let motor = match self.motor {
            Some(m) => m.to_owned(),
            None => {
                let lower = self.model.to_lowercase();
                if lower.starts_with("grok-") {
                    "grok".to_owned()
                } else if lower.starts_with("gpt-") || lower.starts_with("codex") {
                    "codex".to_owned()
                } else if harness == "codex" || harness == "grok" {
                    harness.clone()
                } else {
                    "claude".to_owned()
                }
            }
        };
        let route = match self.route_id {
            Some(r) => r.to_owned(),
            None => format!("{harness}:{motor}"),
        };
        let duration = match self.duration_ms {
            Some(d) => d.map_or(Sql::Null, Sql::Integer),
            None => {
                let span = self
                    .finished
                    .checked_sub(self.started)
                    .and_then(|s| s.checked_mul(1000))
                    .ok_or(UsageError::Unsure)?;
                Sql::Integer(span.max(0))
            }
        };
        Ok([
            txt(self.id),
            txt(self.provider),
            txt(self.agent),
            self.tmux_session,
            self.tmux_pane,
            txt(self.pane_pwd),
            txt(self.git_root),
            txt(self.model),
            txt(self.reasoning_effort),
            Sql::Integer(self.started),
            Sql::Integer(self.finished),
            Sql::Integer(self.input),
            Sql::Integer(self.output),
            Sql::Integer(self.cache_read),
            Sql::Integer(self.cache_write),
            Sql::Integer(self.total),
            Sql::Real(self.cost),
            txt(self.source),
            txt(self.confidence),
            txt(self.raw),
            txt(harness),
            txt(motor),
            txt(route),
            self.harness_account,
            self.motor_account,
            self.interaction_id,
            txt(""),
            txt(""),
            duration,
            txt("unknown"),
            self.reasoning_tokens.map_or(Sql::Null, Sql::Integer),
        ])
    }
}

/// `record_spans` (1994): `(id, provider, account, session_id, git_root, started, finished, source)`.
struct Span {
    id: String,
    provider: &'static str,
    account: String,
    session_id: String,
    git_root: String,
    started: f64,
    finished: f64,
    source: &'static str,
}

const SPAN_SQL: &str = "insert into usage_spans (id, provider, account, session_id, git_root, started_at, finished_at, source) \
     values (?,?,?,?,?,?,?,?) on conflict(id) do update set session_id=excluded.session_id, \
     git_root=excluded.git_root, started_at=excluded.started_at, finished_at=excluded.finished_at, \
     source=excluded.source";

/// Lo que un importador va a escribir, por lotes y en el orden del Python.
struct Sink<'p> {
    admit: &'p dyn Fn(&Connection) -> bool,
    cancelled: &'p dyn Fn() -> bool,
    turns: Vec<[Sql; 31]>,
    spans: Vec<Span>,
    count: usize,
}

impl<'p> Sink<'p> {
    fn new(plan: &ImportPlan<'p>) -> Self {
        Self {
            admit: plan.admit,
            cancelled: plan.cancelled,
            turns: Vec::new(),
            spans: Vec::new(),
            count: 0,
        }
    }

    fn begin<'c>(&self, conn: &'c Connection) -> Result<Transaction<'c>> {
        if (self.cancelled)() || !(self.admit)(conn) {
            return Err(ImportError::Refused);
        }
        Ok(Transaction::new_unchecked(
            conn,
            TransactionBehavior::Deferred,
        )?)
    }

    fn turn(&mut self, conn: &Connection, turn: Turn) -> Line<()> {
        let row = take!(turn.values());
        self.turns.push(row);
        self.count += 1;
        if self.turns.len() >= BATCH {
            self.flush_turns(conn)?;
        }
        Ok(Some(()))
    }

    fn span(&mut self, conn: &Connection, span: Span) -> Result<()> {
        self.spans.push(span);
        if self.spans.len() >= BATCH {
            self.flush_spans(conn)?;
        }
        Ok(())
    }

    fn flush_turns(&mut self, conn: &Connection) -> Result<()> {
        if self.turns.is_empty() {
            return Ok(());
        }
        let tx = self.begin(conn)?;
        {
            let mut stmt = tx.prepare(TURN_INSERT_SQL)?;
            for row in self.turns.drain(..) {
                let named: Vec<(&str, &dyn ToSql)> = TURN_PARAMS
                    .iter()
                    .copied()
                    .zip(row.iter().map(|v| v as &dyn ToSql))
                    .collect();
                stmt.execute(named.as_slice())?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    fn flush_spans(&mut self, conn: &Connection) -> Result<()> {
        if self.spans.is_empty() {
            return Ok(());
        }
        let tx = self.begin(conn)?;
        {
            let mut stmt = tx.prepare(SPAN_SQL)?;
            for s in self.spans.drain(..) {
                stmt.execute(params![
                    s.id,
                    s.provider,
                    s.account,
                    s.session_id,
                    s.git_root,
                    s.started,
                    s.finished,
                    s.source
                ])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// `record_spans(db, spans)` y luego `return record_turns(db, events)`.
    fn finish(mut self, conn: &Connection) -> Result<usize> {
        self.flush_spans(conn)?;
        self.flush_turns(conn)?;
        Ok(self.count)
    }
}

/// Raíces git de un importador: `roots[cwd] = git_root_for_path(cwd)`.
fn root(roots: &dyn GitRoots, cwd: &str) -> String {
    if cwd.is_empty() {
        String::new()
    } else {
        roots.root(cwd)
    }
}

// ---------------------------------------------------------------- Codex

const CODEX_LINE: [&str; 3] = ["type", "payload", "timestamp"];
const CODEX_PAYLOAD: [&str; 11] = [
    "cwd",
    "id",
    "turn_id",
    "model",
    "effort",
    "response_id",
    "usage",
    "thread_id",
    "type",
    "started_at",
    "completed_at",
];

/// Lo que se usa de un `turn_context` (el Python guarda el `payload` entero).
#[derive(Default)]
struct TurnContext {
    cwd: Value,
    model: Value,
    effort: Value,
}

struct CodexFile<'x> {
    account: &'x str,
    path: &'x str,
    cutoff: i64,
    cwd: String,
    thread: String,
    turns: HashMap<String, TurnContext>,
}

/// `record_local_codex_rollouts` (1418) sobre `~/.codex` y `~/.codex-accounts/*`.
pub fn record_local_codex_rollouts(
    conn: &Connection,
    plan: &ImportPlan<'_>,
    seen: &mut ImportSeen,
    roots: &dyn GitRoots,
) -> Result<usize> {
    let homes = account_homes(plan.home, ".codex", ".codex-accounts");
    let cutoff = plan.cutoff()?;
    let mut sink = Sink::new(plan);
    let keep: HashSet<String> = homes.iter().map(|(a, _)| format!("codex:{a}")).collect();
    seen.retain_sources("codex:", &keep);
    for (account, home) in &homes {
        let mut files = Vec::new();
        walk(&home.join("sessions"), &[], &mut |path, name| {
            if !(name.starts_with("rollout-") && name.ends_with(".jsonl")) {
                return;
            }
            if let Some(m) = mtime(&path)
                && m >= cutoff as f64
            {
                files.push((m, path));
            }
        });
        let files = sort_and_cut(files, plan.codex_max_files);
        for (_mtime, path) in seen.changed(&format!("codex:{account}"), &files) {
            plan.check()?;
            let Some(path_str) = path_text(&path) else {
                continue;
            };
            let Some(file) = open_regular(&path) else {
                continue;
            };
            seen.reads += 1;
            let mut state = CodexFile {
                account,
                path: path_str,
                cutoff,
                cwd: String::new(),
                thread: String::new(),
                turns: HashMap::new(),
            };
            each_line(file, |_, text| {
                codex_line(conn, &mut sink, &mut state, text, plan.zone, roots).map(|_| ())
            })?;
        }
    }
    sink.finish(conn)
}

fn codex_line(
    conn: &Connection,
    sink: &mut Sink<'_>,
    st: &mut CodexFile<'_>,
    text: &str,
    zone: &dyn LocalZone,
    roots: &dyn GitRoots,
) -> Line<()> {
    let Some(data) = Doc::parse(text, &CODEX_LINE) else {
        return Ok(None);
    };
    let kind = data.get("type");
    let payload = data
        .object("payload", &CODEX_PAYLOAD)
        .unwrap_or_else(Doc::empty);
    let Value::String(kind) = kind else {
        return Ok(None);
    };
    match kind.as_str() {
        "session_meta" => {
            let cwd = take!(text_or(&payload.get("cwd"), &st.cwd));
            let thread = take!(text_or(&payload.get("id"), &st.thread));
            st.cwd = cwd;
            st.thread = thread;
        }
        "turn_context" => {
            let id = take!(usage_state::text(&payload.get("turn_id")));
            st.turns.insert(
                id,
                TurnContext {
                    cwd: payload.get("cwd"),
                    model: payload.get("model"),
                    effort: payload.get("effort"),
                },
            );
        }
        "token_usage_record" => return codex_usage(conn, sink, st, &data, &payload, zone, roots),
        "event_msg" if payload.is("type", "task_complete") => {
            let turn_id = take!(usage_state::text(&payload.get("turn_id")));
            let started = take!(int(&payload.get("started_at")));
            let finished = take!(int(&payload.get("completed_at")));
            if turn_id.is_empty() || started == 0 || finished < started.max(st.cutoff) {
                return Ok(None);
            }
            let ctx_cwd = st
                .turns
                .get(&turn_id)
                .map_or(Value::Null, |c| c.cwd.clone());
            let here = take!(text_or(&ctx_cwd, &st.cwd));
            let git_root = or_str(root(roots, &here), &here);
            sink.span(
                conn,
                Span {
                    id: format!("codex-turn-{turn_id}"),
                    provider: "codex",
                    account: st.account.to_owned(),
                    session_id: st.thread.clone(),
                    git_root,
                    started: started as f64,
                    finished: finished as f64,
                    source: "codex_rollout",
                },
            )?;
        }
        _ => {}
    }
    Ok(Some(()))
}

/// `x or fallback` sobre textos.
fn or_str(x: String, fallback: &str) -> String {
    if x.is_empty() { fallback.to_owned() } else { x }
}

fn codex_usage(
    conn: &Connection,
    sink: &mut Sink<'_>,
    st: &CodexFile<'_>,
    data: &Doc<'_>,
    payload: &Doc<'_>,
    zone: &dyn LocalZone,
    roots: &dyn GitRoots,
) -> Line<()> {
    let rid = take!(usage_state::text(&payload.get("response_id")));
    let usage = dict(payload.get("usage"));
    let finished = take!(usage_state::as_epoch(&data.get("timestamp"), zone));
    if rid.is_empty() || finished < st.cutoff {
        return Ok(None);
    }
    let turn_id = take!(usage_state::text(&payload.get("turn_id")));
    let empty = TurnContext::default();
    let ctx = st.turns.get(&turn_id).unwrap_or(&empty);
    let here = take!(text_or(&ctx.cwd, &st.cwd));
    let inp = take!(int(get(&usage, "input_tokens")));
    let cached = take!(int(get(&usage, "cached_input_tokens")));
    let out = take!(int(get(&usage, "output_tokens")));
    let total = match take!(int(get(&usage, "total_tokens"))) {
        0 => take!(add(inp, out)),
        n => n,
    };
    if total <= 0 {
        return Ok(None);
    }
    let session = take!(text_or(&payload.get("thread_id"), &st.thread));
    let git_root = or_str(root(roots, &here), &here);
    let model = take!(usage_state::real_model(&ctx.model));
    let effort = take!(usage_state::text(&ctx.effort));
    let input = take!(inp.checked_sub(cached).ok_or(UsageError::Unsure)).max(0);
    let cache_write = take!(int(get(&usage, "cache_write_input_tokens")));
    let reasoning = take!(int(get(&usage, "reasoning_output_tokens")));
    let mut raw = Object::new();
    raw.insert("path".into(), st.path.into());
    raw.insert("turn_id".into(), turn_id.into());
    raw.insert("response_id".into(), rid.clone().into());
    let raw = take!(dumps(raw));
    sink.turn(
        conn,
        Turn {
            id: format!("codex-resp-{rid}"),
            provider: "codex".into(),
            agent: "codex",
            tmux_session: txt(session),
            tmux_pane: txt(""),
            pane_pwd: here,
            git_root,
            model,
            reasoning_effort: effort,
            started: finished,
            finished,
            input,
            output: out,
            cache_read: cached,
            cache_write,
            total,
            cost: 0.0,
            source: "codex_rollout",
            confidence: "local",
            raw,
            harness: None,
            motor: None,
            route_id: None,
            harness_account: txt(st.account),
            motor_account: txt(st.account),
            interaction_id: txt(""),
            duration_ms: None,
            reasoning_tokens: Some(reasoning),
        },
    )
}

// ---------------------------------------------------------------- Claude

const CLAUDE_LINE: [&str; 9] = [
    "type",
    "subtype",
    "timestamp",
    "durationMs",
    "cwd",
    "uuid",
    "sessionId",
    "message",
    "requestId",
];
const CLAUDE_MESSAGE: [&str; 3] = ["id", "model", "usage"];

/// `record_local_claude_jsonl` (2055) de una cuenta.
pub fn record_local_claude_jsonl(
    conn: &Connection,
    plan: &ImportPlan<'_>,
    projects_root: &Path,
    account: &str,
    seen: &mut ImportSeen,
    roots: &dyn GitRoots,
) -> Result<usize> {
    if !projects_root.is_dir() {
        return Ok(0);
    }
    let cutoff = plan.cutoff()?;
    let mut files = Vec::new();
    walk(
        projects_root,
        &["tool-results", "memory"],
        &mut |path, name| {
            if !name.ends_with(".jsonl") {
                return;
            }
            if let Some(m) = mtime(&path)
                && m >= cutoff as f64
            {
                files.push((m, path));
            }
        },
    );
    let files = sort_and_cut(files, plan.claude_max_files);
    let mut sink = Sink::new(plan);
    for (file_mtime, path) in seen.changed(&format!("claude:{account}"), &files) {
        plan.check()?;
        let Some(path_str) = path_text(&path) else {
            continue;
        };
        let Some(handle) = open_regular(&path) else {
            continue;
        };
        seen.reads += 1;
        let info = ClaudeFile {
            account,
            path: path_str,
            mtime: file_mtime,
            cutoff,
        };
        each_line(handle, |no, text| {
            claude_line(conn, &mut sink, &info, no, text, plan.zone, roots).map(|_| ())
        })?;
    }
    sink.finish(conn)
}

struct ClaudeFile<'x> {
    account: &'x str,
    path: &'x str,
    /// `_mtime` del bucle: el `int(_mtime)` de un turno sin fecha.
    mtime: f64,
    cutoff: i64,
}

fn claude_line(
    conn: &Connection,
    sink: &mut Sink<'_>,
    f: &ClaudeFile<'_>,
    no: usize,
    text: &str,
    zone: &dyn LocalZone,
    roots: &dyn GitRoots,
) -> Line<()> {
    let Some(data) = Doc::parse(text, &CLAUDE_LINE) else {
        return Ok(None);
    };
    let here = || format!("{}:{no}", f.path);
    if data.is("type", "system") && data.is("subtype", "turn_duration") {
        let end = take!(usage_state::as_epoch(&data.get("timestamp"), zone));
        let ms = take!(int(&data.get("durationMs")));
        if end >= f.cutoff && ms > 0 {
            let cwd = take!(usage_state::text(&data.get("cwd")));
            // `_text(uuid or f"{path}:{no}")`: la verdad del uuid, no su texto.
            let uuid = take!(usage_state::text(&or(
                data.get("uuid"),
                Value::String(here())
            )));
            let session = take!(usage_state::text(&data.get("sessionId")));
            let git_root = or_str(root(roots, &cwd), &cwd);
            sink.span(
                conn,
                Span {
                    id: format!("claude-turn-{uuid}"),
                    provider: "claude",
                    account: f.account.to_owned(),
                    session_id: session,
                    git_root,
                    started: end as f64 - ms as f64 / 1000.0,
                    finished: end as f64,
                    source: "claude_jsonl",
                },
            )?;
        }
        return Ok(Some(()));
    }
    if !data.is("type", "assistant") {
        return Ok(None);
    }
    let Some(msg) = data.object("message", &CLAUDE_MESSAGE) else {
        return Ok(None);
    };
    let Value::Object(usage) = msg.get("usage") else {
        return Ok(None);
    };
    let finished = match take!(usage_state::as_epoch(&data.get("timestamp"), zone)) {
        0 => take!(trunc(f.mtime)),
        n => n,
    };
    if finished < f.cutoff {
        return Ok(None);
    }
    let input = take!(int(get(&usage, "input_tokens")));
    let output = take!(int(get(&usage, "output_tokens")));
    let cache_read = take!(int(get(&usage, "cache_read_input_tokens")));
    let cache_write = take!(int(get(&usage, "cache_creation_input_tokens")));
    let total = take!(
        add(input, output)
            .and_then(|t| add(t, cache_read))
            .and_then(|t| add(t, cache_write))
    );
    if total <= 0 {
        return Ok(None);
    }
    let cwd = take!(usage_state::text(&data.get("cwd")));
    let git_root = root(roots, &cwd);
    // Una línea por bloque de contenido con el mismo `usage`: la respuesta es
    // `(message.id, requestId)`, no la línea.
    let msg_id = take!(usage_state::text(&msg.get("id")));
    let stable = if msg_id.is_empty() {
        let key = or(
            or(data.get("uuid"), data.get("requestId")),
            Value::String(here()),
        );
        take!(usage_state::text(&key))
    } else {
        let request = take!(usage_state::text(&data.get("requestId")));
        format!("{msg_id}:{request}")
    };
    let session = take!(usage_state::text(&data.get("sessionId")));
    let model = take!(usage_state::real_model(&msg.get("model")));
    let mut raw = Object::new();
    raw.insert("path".into(), f.path.into());
    raw.insert("line".into(), no.into());
    raw.insert("uuid".into(), stable.clone().into());
    raw.insert("usage".into(), Value::Object(usage));
    let raw = take!(dumps(raw));
    sink.turn(
        conn,
        Turn {
            id: format!("claude-jsonl-{stable}"),
            provider: "claude".into(),
            agent: "claude",
            tmux_session: txt(session),
            tmux_pane: txt(""),
            pane_pwd: cwd,
            git_root,
            model,
            reasoning_effort: String::new(),
            started: finished,
            finished,
            input,
            output,
            cache_read,
            cache_write,
            total,
            cost: 0.0,
            source: "claude_jsonl",
            confidence: "local",
            raw,
            harness: None,
            motor: None,
            route_id: None,
            harness_account: txt(f.account),
            motor_account: txt(f.account),
            interaction_id: txt(""),
            duration_ms: None,
            reasoning_tokens: None,
        },
    )
}

/// `int(x)` de un `float` finito.
fn trunc(x: f64) -> usage_state::Result<i64> {
    let t = x.trunc();
    if !t.is_finite() {
        return Err(UsageError::Overflow);
    }
    if (-9.223_372_036_854_776e18..9.223_372_036_854_776e18).contains(&t) {
        Ok(t as i64)
    } else {
        Err(UsageError::Unsure)
    }
}

// ---------------------------------------------------------------- Grok

const GROK_LINE: [&str; 2] = ["params", "timestamp"];
const GROK_PARAMS: [&str; 3] = ["update", "sessionId", "_meta"];
const GROK_UPDATE: [&str; 4] = ["sessionUpdate", "usage", "prompt_id", "stop_reason"];
const GROK_INTERACTION: &str = "select i.id,i.tmux_session,i.tmux_pane,c.harness_account
                      from usage_interactions i left join usage_session_configs c on c.id=i.config_id
                      where i.agent_session_id=? and (?='' or i.prompt_id=?)
                      order by i.started_at_ms desc limit 1";

/// `glob(<home>/sessions/**/updates.jsonl, recursive=True)`: la propia carpeta
/// `sessions` y cada subcarpeta (sigue enlaces, sin las que empiezan por `.`),
/// con `lexists`. Un ciclo de enlaces se corta (el Python seguiría hasta `ELOOP`).
fn grok_candidates(dir: &Path, ancestors: &mut Vec<(u64, u64)>, out: &mut Vec<PathBuf>) {
    let candidate = dir.join("updates.jsonl");
    if fs::symlink_metadata(&candidate).is_ok() {
        out.push(candidate);
    }
    let Ok(meta) = fs::metadata(dir) else {
        return;
    };
    let id = (meta.dev(), meta.ino());
    if ancestors.contains(&id) {
        return;
    }
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    ancestors.push(id);
    for entry in entries.flatten() {
        let name = entry.file_name();
        if name.as_encoded_bytes().starts_with(b".") {
            continue;
        }
        let path = entry.path();
        if fs::metadata(&path).is_ok_and(|m| m.is_dir()) {
            grok_candidates(&path, ancestors, out);
        }
    }
    ancestors.pop();
}

/// `json.load(open(summary.json, errors="replace"))`: cualquier fallo → `{}`; un
/// valor que no es objeto hace lanzar a `summary.get` (`AttributeError`).
fn grok_summary(path: &Path) -> Result<Object> {
    let Ok(bytes) = fs::read(path) else {
        return Ok(Object::new());
    };
    match workspace_loads(&String::from_utf8_lossy(&bytes)) {
        Ok(Value::Object(map)) => Ok(map),
        Ok(_) => Err(ImportError::Raises),
        Err(_) => Ok(Object::new()),
    }
}

struct GrokFile<'x> {
    path: &'x str,
    cutoff_ms: i64,
    cwd: String,
    git_root: String,
    session_id: String,
    model: String,
    effort: String,
}

/// `record_local_grok_updates` (1611) sobre `homes` (las de `grok_state.account_homes`).
pub fn record_local_grok_updates(
    conn: &Connection,
    plan: &ImportPlan<'_>,
    homes: &[PathBuf],
    roots: &dyn GitRoots,
) -> Result<usize> {
    let cutoff_ms = plan
        .cutoff()?
        .checked_mul(1000)
        .ok_or(ImportError::Raises)?;
    let mut files = Vec::new();
    for home in homes {
        let mut found = Vec::new();
        grok_candidates(&home.join("sessions"), &mut Vec::new(), &mut found);
        for path in found {
            if let Some(m) = mtime(&path)
                && m * 1000.0 >= cutoff_ms as f64
            {
                files.push((m, path));
            }
        }
    }
    let files = sort_and_cut(files, Some(GROK_MAX_FILES));
    let mut sink = Sink::new(plan);
    let mut lookup = None;
    for (_mtime, path) in files {
        plan.check()?;
        let Some(path_str) = path_text(&path) else {
            continue;
        };
        let dir = path.parent().unwrap_or(Path::new(""));
        let summary = grok_summary(&dir.join("summary.json"))?;
        let info = dict(get(&summary, "info").clone());
        let Some(cwd) = line(usage_state::text(get(&info, "cwd")))? else {
            continue;
        };
        let git_root = root(roots, &cwd);
        let base = dir.file_name().and_then(|n| n.to_str()).unwrap_or_default();
        let id = or(get(&info, "id").clone(), Value::String(base.to_owned()));
        let (Some(session_id), Some(model), Some(effort)) = (
            line(usage_state::text(&id))?,
            line(usage_state::text(get(&summary, "current_model_id")))?,
            line(usage_state::text(get(&summary, "reasoning_effort")))?,
        ) else {
            continue;
        };
        let Some(handle) = open_regular(&path) else {
            continue;
        };
        let file = GrokFile {
            path: path_str,
            cutoff_ms,
            cwd,
            git_root,
            session_id,
            model,
            effort,
        };
        each_line(handle, |no, text| {
            grok_line(conn, &mut lookup, &mut sink, &file, no, text).map(|_| ())
        })?;
    }
    sink.finish(conn)
}

fn sql_truthy(v: &Sql) -> bool {
    match v {
        Sql::Null => false,
        Sql::Integer(n) => *n != 0,
        Sql::Real(x) => *x != 0.0,
        Sql::Text(s) => !s.is_empty(),
        Sql::Blob(b) => !b.is_empty(),
    }
}

fn grok_line<'c>(
    conn: &'c Connection,
    lookup: &mut Option<rusqlite::Statement<'c>>,
    sink: &mut Sink<'_>,
    f: &GrokFile<'_>,
    no: usize,
    text: &str,
) -> Line<()> {
    let Some(row) = Doc::parse(text, &GROK_LINE) else {
        return Ok(None);
    };
    let Some(params) = row.object("params", &GROK_PARAMS) else {
        return Ok(None);
    };
    let Some(update) = params.object("update", &GROK_UPDATE) else {
        return Ok(None);
    };
    if !update.is("sessionUpdate", "turn_completed") {
        return Ok(None);
    }
    let Value::Object(usage) = update.get("usage") else {
        return Ok(None);
    };
    let mut finished_ms = take!(int(&row.get("timestamp")));
    // El CLI escribe SEGUNDOS.
    if 0 < finished_ms && finished_ms < 100_000_000_000 {
        finished_ms *= 1000;
    }
    if finished_ms < f.cutoff_ms {
        return Ok(None);
    }
    let prompt_id = take!(usage_state::text(&update.get("prompt_id")));
    let external = take!(text_or(&params.get("sessionId"), &f.session_id));
    // La consulta se prepara una vez por llamada, no por línea.
    let stmt = match lookup {
        Some(stmt) => stmt,
        None => lookup.insert(conn.prepare(GROK_INTERACTION)?),
    };
    let interaction: Option<[Sql; 4]> = stmt
        .query_row(params![external, prompt_id, prompt_id], |r| {
            Ok([r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?])
        })
        .optional()?;
    let duration = match take!(int(get(&usage, "apiDurationMs"))) {
        0 => None,
        n => Some(n),
    };
    let finished = finished_ms.div_euclid(1000);
    let model_usage = dict(get(&usage, "modelUsage").clone());
    let actual = model_usage
        .keys()
        .next()
        .filter(|k| !k.is_empty())
        .cloned()
        .unwrap_or_else(|| f.model.clone());
    let input = take!(int(get(&usage, "inputTokens")));
    let output = take!(int(get(&usage, "outputTokens")));
    let cache_read = take!(int(get(&usage, "cachedReadTokens")));
    let cache_write = take!(int(get(&usage, "cacheCreationTokens")));
    // `(params.get("_meta") or {}).get("eventId")`: un `_meta` verdadero que no es
    // objeto lanza `AttributeError`.
    let meta = params.get("_meta");
    let event_id = if truthy(&meta) {
        match meta {
            Value::Object(m) => take!(usage_state::text(get(&m, "eventId"))),
            _ => return Err(ImportError::Raises),
        }
    } else {
        String::new()
    };
    let id = if event_id.is_empty() {
        stable_id(&[f.path.to_owned(), no.to_string(), prompt_id.clone()])
    } else {
        event_id
    };
    let total = match take!(int(get(&usage, "totalTokens"))) {
        0 => take!(add(input, output)),
        n => n,
    };
    let reasoning = take!(as_int_opt(get(&usage, "reasoningTokens")));
    let started = take!(
        finished
            .checked_sub(duration.unwrap_or(0).div_euclid(1000))
            .ok_or(UsageError::Unsure)
    );
    let mut raw = Object::new();
    raw.insert("session_id".into(), external.clone().into());
    raw.insert("prompt_id".into(), prompt_id.into());
    raw.insert(
        "stop_reason".into(),
        take!(usage_state::text(&update.get("stop_reason"))).into(),
    );
    raw.insert(
        "model_calls".into(),
        take!(int(get(&usage, "modelCalls"))).into(),
    );
    raw.insert(
        "num_turns".into(),
        take!(int(get(&usage, "numTurns"))).into(),
    );
    let raw = take!(dumps(raw));
    let (session, pane, interaction_id, account) = match interaction {
        Some([id, session, pane, account]) => {
            let account = if sql_truthy(&account) {
                account
            } else {
                txt("unknown")
            };
            (session, pane, id, account)
        }
        None => (txt(external), txt(""), txt(""), txt("unknown")),
    };
    sink.turn(
        conn,
        Turn {
            id: format!("grok-update-{id}"),
            provider: "grok".into(),
            agent: "grok",
            tmux_session: session,
            tmux_pane: pane,
            pane_pwd: f.cwd.clone(),
            git_root: f.git_root.clone(),
            model: actual,
            reasoning_effort: f.effort.clone(),
            started,
            finished,
            input,
            output,
            cache_read,
            cache_write,
            total,
            cost: 0.0,
            source: "grok_updates",
            confidence: "exact",
            raw,
            harness: Some("grok"),
            motor: Some("grok"),
            route_id: Some("grok:grok"),
            harness_account: account,
            motor_account: txt("main"),
            interaction_id,
            duration_ms: Some(duration),
            reasoning_tokens: reasoning,
        },
    )
}

// ---------------------------------------------------------------- OpenCode

/// Un valor de SQLite como lo vería el Python; `None` si no es UTF-8 (el
/// `OperationalError` al decodificar, que aborta la lectura entera).
fn sql_value(v: ValueRef<'_>) -> Option<Value> {
    Some(match v {
        ValueRef::Null => Value::Null,
        ValueRef::Integer(n) => Value::from(n),
        ValueRef::Real(x) => serde_json::Number::from_f64(x).map_or(Value::Null, Value::Number),
        ValueRef::Text(t) => Value::String(std::str::from_utf8(t).ok()?.to_owned()),
        // `bytes` no tiene equivalente JSON: los usos lo tratan como incierto.
        ValueRef::Blob(_) => Value::Array(Vec::new()),
    })
}

/// `json.loads(data)` del mensaje: texto o bytes UTF-8 (con BOM opcional en bytes);
/// cualquier otra cosa es el `TypeError`/`ValueError` capturado.
fn opencode_message(v: ValueRef<'_>) -> Option<Object> {
    let parsed = match v {
        ValueRef::Text(t) => workspace_loads(std::str::from_utf8(t).ok()?),
        ValueRef::Blob(b) => {
            let b = b.strip_prefix(b"\xef\xbb\xbf").unwrap_or(b);
            workspace_loads(std::str::from_utf8(b).ok()?)
        }
        _ => return None,
    };
    match parsed {
        Ok(Value::Object(map)) => Some(map),
        _ => None,
    }
}

/// `str(x)` de un valor de SQLite (`bytes` y contenedores: inciertos).
fn sql_text(v: &Value) -> usage_state::Result<String> {
    match v {
        Value::Array(_) => Err(UsageError::Unsure),
        other => usage_state::text(other),
    }
}

const OPENCODE_SQL: &str = "
            select m.id, m.session_id, m.time_created, m.data, s.directory
            from message m left join session s on s.id = m.session_id
            where m.time_created >= ?
            ";

/// `record_local_opencode_db` (1539): la base de OpenCode de solo lectura;
/// cualquier fallo al leerla → 0 sin escribir.
pub fn record_local_opencode_db(
    conn: &Connection,
    plan: &ImportPlan<'_>,
    roots: &dyn GitRoots,
) -> Result<usize> {
    if !plan.opencode_db.exists() {
        return Ok(0);
    }
    let cutoff_ms = plan
        .cutoff()?
        .checked_mul(1000)
        .ok_or(ImportError::Raises)?;
    let Some(path) = path_text(&plan.opencode_db) else {
        return Ok(0);
    };
    plan.check()?;
    let uri = format!("file:{path}?mode=ro");
    let turns = match opencode_turns(&uri, cutoff_ms, roots) {
        Ok(OpenCodeRead::Turns(turns)) => turns,
        // El bucle del Python lanza antes de escribir nada.
        Ok(OpenCodeRead::Raises) => return Err(ImportError::Raises),
        Ok(OpenCodeRead::Unread) | Err(_) => return Ok(0),
    };
    let mut sink = Sink::new(plan);
    for turn in turns {
        sink.turn(conn, turn)?;
    }
    sink.finish(conn)
}

/// Lo que da la lectura de la base de OpenCode.
enum OpenCodeRead {
    /// La lectura falló (el `try` del `fetchall`): cero filas.
    Unread,
    Turns(Vec<Turn>),
    /// Una fila lanza en el bucle del Python, que corre tras leerlas todas.
    Raises,
}

/// Los turnos de la base de OpenCode. El Python lee todas las filas antes de
/// convertirlas: una fila que lanza no gana a un fallo de lectura posterior,
/// así que tras ella se sigue leyendo sin convertir.
fn opencode_turns(
    uri: &str,
    cutoff_ms: i64,
    roots: &dyn GitRoots,
) -> rusqlite::Result<OpenCodeRead> {
    let oc = Connection::open_with_flags(
        uri,
        OpenFlags::SQLITE_OPEN_READ_ONLY
            | OpenFlags::SQLITE_OPEN_URI
            | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    let mut stmt = oc.prepare(OPENCODE_SQL)?;
    let mut cursor = stmt.query(params![cutoff_ms])?;
    let mut turns = Vec::new();
    let mut raised = false;
    let mut cache: HashMap<String, String> = HashMap::new();
    while let Some(row) = cursor.next()? {
        let mut cells = Vec::with_capacity(5);
        for i in [0usize, 1, 2, 4] {
            match sql_value(row.get_ref(i)?) {
                Some(v) => cells.push(v),
                None => return Ok(OpenCodeRead::Unread),
            }
        }
        let data = row.get_ref(3)?;
        if let ValueRef::Text(t) = data
            && std::str::from_utf8(t).is_err()
        {
            return Ok(OpenCodeRead::Unread);
        }
        if raised {
            continue;
        }
        let [mid, session, created, directory] = <[Value; 4]>::try_from(cells).unwrap_or_default();
        let Some(msg) = opencode_message(data) else {
            continue;
        };
        match opencode_turn(
            &msg,
            &mid,
            &session,
            &created,
            &directory,
            &mut |cwd: &str| {
                cache
                    .entry(cwd.to_owned())
                    .or_insert_with(|| root(roots, cwd))
                    .clone()
            },
        ) {
            Ok(Some(turn)) => turns.push(turn),
            Ok(None) => {}
            Err(_) => {
                raised = true;
                turns = Vec::new();
            }
        }
    }
    Ok(if raised {
        OpenCodeRead::Raises
    } else {
        OpenCodeRead::Turns(turns)
    })
}

fn opencode_turn(
    msg: &Object,
    mid: &Value,
    session: &Value,
    created: &Value,
    directory: &Value,
    root_of: &mut dyn FnMut(&str) -> String,
) -> Line<Turn> {
    if !matches!(get(msg, "role"), Value::String(r) if r == "assistant") {
        return Ok(None);
    }
    let tokens = dict(get(msg, "tokens").clone());
    let cache = dict(get(&tokens, "cache").clone());
    let input = take!(int(get(&tokens, "input")));
    let output = take!(add(
        take!(int(get(&tokens, "output"))),
        take!(int(get(&tokens, "reasoning")))
    ));
    let cache_read = take!(int(get(&cache, "read")));
    let cache_write = take!(int(get(&cache, "write")));
    let cost = take!(usage_state::as_float(get(msg, "cost"), 0.0));
    let total = take!(
        add(input, output)
            .and_then(|t| add(t, cache_read))
            .and_then(|t| add(t, cache_write))
    );
    if total <= 0 && cost <= 0.0 {
        return Ok(None);
    }
    let cwd = take!(sql_text(directory));
    let git_root = root_of(&cwd);
    let provider_id = take!(usage_state::text(get(msg, "providerID")));
    let model_id = take!(usage_state::text(get(msg, "modelID")));
    let bucket = if provider_id == "groq" {
        "groq"
    } else {
        "opencode"
    };
    let model = if !provider_id.is_empty() && !model_id.is_empty() {
        format!("{provider_id}/{model_id}")
    } else {
        model_id
    };
    let created = match created {
        Value::Array(_) => return Ok(None),
        other => take!(int(other)).div_euclid(1000),
    };
    let id = take!(sql_text(mid));
    let session = take!(sql_text(session));
    Ok(Some(Turn {
        id: format!("opencode-db-{id}"),
        provider: bucket.into(),
        agent: "opencode",
        tmux_session: txt(session),
        tmux_pane: txt(""),
        pane_pwd: cwd,
        git_root,
        model,
        reasoning_effort: String::new(),
        started: created,
        finished: created,
        input,
        output,
        cache_read,
        cache_write,
        total,
        cost,
        source: "opencode_db",
        confidence: "local",
        raw: "{}".into(),
        harness: None,
        motor: None,
        route_id: None,
        harness_account: txt("unknown"),
        motor_account: txt("unknown"),
        interaction_id: txt(""),
        duration_ms: None,
        reasoning_tokens: None,
    }))
}

// ---------------------------------------------------------------- poda y configuraciones

/// `prune_old_turns` (1521): turnos y tramos fuera de la ventana y las filas del
/// import viejo; devuelve los turnos borrados por fecha.
pub fn prune_old_turns(conn: &Connection, now: i64, max_age_days: i64) -> Result<usize> {
    let cutoff = max_age_days
        .checked_mul(86_400)
        .and_then(|s| now.checked_sub(s))
        .ok_or(ImportError::Raises)?;
    let tx = Transaction::new_unchecked(conn, TransactionBehavior::Deferred)?;
    let n = tx.execute(
        "delete from usage_turns where turn_finished_at < ?",
        params![cutoff],
    )?;
    tx.execute(
        "delete from usage_spans where finished_at < ?",
        params![cutoff],
    )?;
    tx.execute(
        "delete from usage_turns where source='codex_state_db' \
         or (source='claude_jsonl' and coalesce(harness_account,'') in ('','unknown'))",
        [],
    )?;
    tx.commit()?;
    Ok(n)
}

/// Un valor de JSON para enlazarlo como lo enlazaría `sqlite3` de Python.
fn bind(v: &Value) -> Result<Sql> {
    Ok(match v {
        Value::Null => Sql::Null,
        Value::Bool(b) => Sql::Integer(i64::from(*b)),
        Value::String(s) => Sql::Text(s.clone()),
        Value::Number(_) => match PyNum::of(v).map_err(|_| ImportError::Raises)? {
            Some(PyNum::Int(n)) => Sql::Integer(n),
            Some(PyNum::Float(x)) => Sql::Real(x),
            None => return Err(ImportError::Raises),
        },
        // `InterfaceError`: tipo no admitido.
        Value::Array(_) | Value::Object(_) => return Err(ImportError::Raises),
    })
}

/// Una fila como `dict(sqlite3.Row)`. Un `BLOB` se representa con una lista (de
/// bytes): verdadera si no está vacía y nunca igual a un texto.
fn row_dict(row: &rusqlite::Row<'_>) -> Result<Object> {
    let mut out = Object::new();
    for (i, name) in row.as_ref().column_names().into_iter().enumerate() {
        let value = match row.get_ref(i)? {
            ValueRef::Blob(b) => Value::Array(b.iter().map(|x| Value::from(*x)).collect()),
            other => sql_value(other).ok_or(ImportError::Raises)?,
        };
        out.insert(name.to_owned(), value);
    }
    Ok(out)
}

/// `latest_session_config` (2370): la configuración más reciente de un pane, o `{}`.
pub fn latest_session_config(conn: &Connection, session: &Value, pane: &Value) -> Result<Object> {
    let (session, pane) = (bind(session)?, bind(pane)?);
    let mut stmt = conn.prepare(
        "select * from usage_session_configs
          where tmux_session=? and (tmux_pane=? or (?='' and tmux_pane=''))
          order by effective_at desc limit 1",
    )?;
    let mut rows = stmt.query(params![session, pane, pane])?;
    match rows.next()? {
        Some(row) => row_dict(row),
        None => Ok(Object::new()),
    }
}

/// `record_session_config` (2349) con `data` como el `dict` del Python.
pub fn record_session_config(conn: &Connection, data: &Object, now: i64) -> Result<()> {
    let t = |key: &str| line(usage_state::text(get(data, key)));
    let (Some(session), Some(pane), Some(harness), Some(motor), Some(model), Some(effort)) = (
        t("tmux_session")?,
        t("tmux_pane")?,
        t("harness")?,
        t("motor")?,
        t("model")?,
        t("effort")?,
    ) else {
        return Err(ImportError::Raises);
    };
    let or_text = |key: &str, fallback: String| -> Result<String> {
        let s = t(key)?.ok_or(ImportError::Raises)?;
        Ok(if s.is_empty() { fallback } else { s })
    };
    let route_id = or_text("route_id", format!("{harness}:{motor}"))?;
    let effective = get(data, "effective_at");
    let at = if truthy(effective) {
        line(usage_state::as_int(effective, 0))?.ok_or(ImportError::Raises)?
    } else {
        now
    };
    let ident = stable_id(&[
        session.clone(),
        pane.clone(),
        at.to_string(),
        route_id.clone(),
        model.clone(),
        effort.clone(),
    ]);
    let harness_account = or_text("harness_account", "unknown".into())?;
    let motor_account = or_text("motor_account", "unknown".into())?;
    let source = or_text("source", "runtime".into())?;
    let confidence = or_text("confidence", "exact".into())?;
    let tx = Transaction::new_unchecked(conn, TransactionBehavior::Deferred)?;
    tx.execute(
        "insert into usage_session_configs
          (id,tmux_session,tmux_pane,effective_at,harness,motor,model,effort,harness_account,motor_account,route_id,source,confidence)
          values(?,?,?,?,?,?,?,?,?,?,?,?,?) on conflict(id) do update set
          model=excluded.model,effort=excluded.effort,route_id=excluded.route_id",
        params![
            ident,
            session,
            pane,
            at,
            harness,
            motor,
            model,
            effort,
            harness_account,
            motor_account,
            route_id,
            source,
            confidence
        ],
    )?;
    tx.commit()?;
    Ok(())
}

/// `ensure_observed_configs` (cc-dash:315) sobre las tarjetas de `read_states_cached()`:
/// la configuración observada de cada pane vivo que no la tenga o que cambió, con
/// `record_runtime_config(..., source="observed")` (cuyos errores se ignoran).
/// `Err`: lo que el Python lanzaría fuera de ese `try` (una tarjeta que no es
/// objeto, `latest_session_config` que falla), que omite también la reconciliación.
pub fn ensure_observed_configs(conn: &Connection, cards: &[Value], now: i64) -> Result<usize> {
    let mut n = 0;
    for card in cards {
        let Value::Object(s) = card else {
            return Err(ImportError::Raises);
        };
        let field = |key: &str| get(s, key).clone();
        let empty = || Value::String(String::new());
        if !truthy(get(s, "alive")) || !truthy(get(s, "model")) {
            continue;
        }
        let (sess, pane) = (or(field("session"), empty()), or(field("pane"), empty()));
        if !truthy(&sess) || !truthy(&pane) {
            continue;
        }
        let harness = or(or(field("harness"), field("agent")), empty());
        let motor = or(or(field("motor"), field("agent")), empty());
        if !truthy(&harness) || !truthy(&motor) {
            continue;
        }
        let model = or(field("model"), empty());
        let effort = or(field("effort"), empty());
        let route = if truthy(get(s, "routeId")) {
            field("routeId")
        } else {
            // `f"{harness}:{motor}"`: el `repr` de un contenedor no se reproduce.
            match (usage_state::text(&harness), usage_state::text(&motor)) {
                (Ok(h), Ok(m)) => Value::String(format!("{h}:{m}")),
                _ => continue,
            }
        };
        let prev = latest_session_config(conn, &sess, &pane)?;
        if !prev.is_empty()
            && python_eq(get(&prev, "model"), &model)
            && python_eq(&or(field_of(&prev, "effort"), empty()), &effort)
            && python_eq(&or(field_of(&prev, "route_id"), empty()), &route)
        {
            continue;
        }
        let mut data = Object::new();
        data.insert("tmux_session".into(), sess);
        data.insert("tmux_pane".into(), pane);
        data.insert("harness".into(), harness);
        data.insert("motor".into(), motor);
        data.insert("model".into(), model);
        data.insert("effort".into(), effort);
        let account = or(
            or(field("harnessAccount"), field("account")),
            "unknown".into(),
        );
        data.insert("harness_account".into(), or(account, "unknown".into()));
        data.insert(
            "motor_account".into(),
            or(field("motorAccount"), "unknown".into()),
        );
        data.insert("route_id".into(), route);
        data.insert("effective_at".into(), now.into());
        data.insert("source".into(), "observed".into());
        data.insert("confidence".into(), "exact".into());
        // `record_runtime_config`: `except Exception: return None`.
        let _ = record_session_config(conn, &data, now);
        n += 1;
    }
    Ok(n)
}

fn field_of(obj: &Object, key: &str) -> Value {
    get(obj, key).clone()
}

/// Una configuración de `usage_session_configs` (o la recién creada).
struct Config {
    id: Sql,
    source: Sql,
    confidence: Sql,
}

fn config_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Config> {
    Ok(Config {
        id: row.get("id")?,
        source: row.get("source")?,
        confidence: row.get("confidence")?,
    })
}

/// `str(x)` de un valor de SQLite para claves e ids; `bytes` → `Raises` (el `repr`
/// de `bytes` no se reproduce; la transacción se deshace).
fn sql_str(v: &Sql) -> Result<String> {
    match v {
        Sql::Null => Ok(String::new()),
        Sql::Integer(n) => Ok(n.to_string()),
        Sql::Real(x) => Ok(comandos_core::json::float_repr(*x)),
        Sql::Text(s) => Ok(s.clone()),
        Sql::Blob(_) => Err(ImportError::Raises),
    }
}

/// `_text(x)`: `None` → `""`.
fn sql_text_or(v: &Sql, fallback: &str) -> Result<String> {
    let s = sql_str(v)?;
    Ok(if s.is_empty() { fallback.to_owned() } else { s })
}

/// `int(x or 0)` de `started_at_ms`.
fn sql_int(v: &Sql) -> Result<i64> {
    match v {
        Sql::Null => Ok(0),
        Sql::Integer(n) => Ok(*n),
        Sql::Real(x) => trunc(*x).map_err(|_| ImportError::Raises),
        Sql::Text(s) if s.is_empty() => Ok(0),
        Sql::Text(s) => text::int(s).map_err(|_| ImportError::Raises),
        Sql::Blob(_) => Err(ImportError::Raises),
    }
}

/// `_latest_config` (2382): la del pane con fecha ≤ `at` o, por la carrera, la
/// primera hasta `at + CONFIG_RACE_WINDOW`.
fn latest_config(conn: &Connection, session: &Sql, pane: &Sql, at: i64) -> Result<Option<Config>> {
    let found = conn
        .prepare(
            "select * from usage_session_configs where tmux_session=? and tmux_pane=? and effective_at<=?
                         order by effective_at desc limit 1",
        )?
        .query_row(params![session, pane, at], config_row)
        .optional()?;
    if found.is_some() {
        return Ok(found);
    }
    let raced = at
        .checked_add(CONFIG_RACE_WINDOW)
        .ok_or(ImportError::Raises)?;
    Ok(conn
        .prepare(
            "select * from usage_session_configs where tmux_session=? and tmux_pane=? and effective_at<=?
                         order by effective_at asc limit 1",
        )?
        .query_row(params![session, pane, raced], config_row)
        .optional()?)
}

/// `_upsert_config` (2470).
#[allow(clippy::too_many_arguments)]
fn upsert_config(
    conn: &Connection,
    session: &Sql,
    pane: &Sql,
    at: i64,
    route: (&str, &str, &str),
    model: &str,
    effort: &str,
    accounts: (&str, &str),
    source: &'static str,
    confidence: &'static str,
) -> Result<Config> {
    let (harness, motor, route_id) = route;
    let ident = stable_id(&[
        sql_str(session)?,
        sql_str(pane)?,
        at.to_string(),
        route_id.to_owned(),
        model.to_owned(),
        effort.to_owned(),
    ]);
    conn.execute(
        "insert into usage_session_configs
      (id,tmux_session,tmux_pane,effective_at,harness,motor,model,effort,harness_account,motor_account,route_id,source,confidence)
      values(?,?,?,?,?,?,?,?,?,?,?,?,?) on conflict(id) do nothing",
        params![
            ident, session, pane, at, harness, motor, model, effort, accounts.0, accounts.1, route_id,
            source, confidence
        ],
    )?;
    Ok(Config {
        id: Sql::Text(ident),
        source: txt(source),
        confidence: txt(confidence),
    })
}

/// `reconcile_orphan_interactions` (2395): da configuración a las interacciones
/// huérfanas de los últimos `max_age_days` días, en una transacción.
pub fn reconcile_orphan_interactions(conn: &Connection, now: i64, max_age_days: i64) -> Result<()> {
    let since_ms = max_age_days
        .checked_mul(86_400)
        .and_then(|s| now.checked_sub(s))
        .and_then(|s| s.checked_mul(1000))
        .ok_or(ImportError::Raises)?;
    let tx = Transaction::new_unchecked(conn, TransactionBehavior::Deferred)?;
    tx.execute(
        "update usage_interactions set git_root=(
                         select coalesce(nullif(p.git_root,''), p.pane_pwd) from usage_panes p
                         where p.tmux_session=usage_interactions.tmux_session and p.tmux_pane=usage_interactions.tmux_pane
                         order by p.last_seen_at desc limit 1)
                       where git_root='' and started_at_ms>=? and exists(
                         select 1 from usage_panes p where p.tmux_session=usage_interactions.tmux_session
                         and p.tmux_pane=usage_interactions.tmux_pane)",
        params![since_ms],
    )?;
    let orphans: Vec<[Sql; 5]> = {
        let mut stmt = tx.prepare(
            "select id,tmux_session,tmux_pane,started_at_ms,source from usage_interactions
                              where config_id='' and started_at_ms>=? order by started_at_ms",
        )?;
        stmt.query_map(params![since_ms], |r| {
            Ok([r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?])
        })?
        .collect::<rusqlite::Result<_>>()?
    };
    for [id, session, pane, started, source] in orphans {
        let at = sql_int(&started)?.div_euclid(1000);
        let mut via = "";
        let mut cfg = latest_config(&tx, &session, &pane, at)?;
        if cfg.is_some() {
            via = "race";
        }
        if cfg.is_none() {
            let turn: Option<[Sql; 8]> = tx
                .prepare(
                    "select provider,model,reasoning_effort,harness,motor,route_id,harness_account,motor_account
                                   from usage_turns where interaction_id=? and model<>'' order by turn_started_at limit 1",
                )?
                .query_row(params![id], |r| {
                    Ok([
                        r.get(0)?,
                        r.get(1)?,
                        r.get(2)?,
                        r.get(3)?,
                        r.get(4)?,
                        r.get(5)?,
                        r.get(6)?,
                        r.get(7)?,
                    ])
                })
                .optional()?;
            if let Some(
                [
                    provider,
                    model,
                    effort,
                    harness,
                    motor,
                    route_id,
                    h_account,
                    m_account,
                ],
            ) = turn
            {
                let motor = sql_text_or(&motor, &sql_str(&provider)?)?;
                let harness = sql_text_or(&harness, &motor)?;
                let route_id = sql_text_or(&route_id, &format!("{harness}:{motor}"))?;
                cfg = Some(upsert_config(
                    &tx,
                    &session,
                    &pane,
                    at,
                    (&harness, &motor, &route_id),
                    &sql_str(&model)?,
                    &sql_str(&effort)?,
                    (
                        &sql_text_or(&h_account, "unknown")?,
                        &sql_text_or(&m_account, "unknown")?,
                    ),
                    "backfill:turn",
                    "inferred",
                )?);
                via = "turn";
            }
        }
        if cfg.is_none() {
            let near = tx
                .prepare(
                    "select * from usage_session_configs where tmux_session=? and tmux_pane=?
                                      order by abs(effective_at-?) asc limit 1",
                )?
                .query_row(params![session, pane, at], config_row)
                .optional()?;
            if near.is_some() {
                cfg = near;
                via = "pane";
            }
        }
        if cfg.is_none() {
            let src = sql_str(&source)?;
            let provider = src.split_once(':').map_or("", |(_, p)| p).to_owned();
            if !provider.is_empty() && provider != "hook" {
                let route_id = format!("{provider}:{provider}");
                cfg = Some(upsert_config(
                    &tx,
                    &session,
                    &pane,
                    at,
                    (&provider, &provider, &route_id),
                    "",
                    "",
                    ("unknown", "unknown"),
                    "backfill:provider",
                    "provider_only",
                )?);
                via = "provider";
            }
        }
        if let Some(cfg) = cfg {
            // La vía se clasifica por el ORIGEN de la configuración.
            let src = if sql_truthy(&cfg.source) {
                sql_str(&cfg.source)?
            } else {
                String::new()
            };
            let via = match src.as_str() {
                "backfill:provider" => "provider",
                "backfill:turn" => "turn",
                _ if via == "pane" => "pane",
                _ => "race",
            };
            let confidence = if via == "race" {
                txt("exact")
            } else if sql_truthy(&cfg.confidence) {
                cfg.confidence
            } else {
                txt("inferred")
            };
            tx.execute(
                "update usage_interactions set config_id=?, confidence=? where id=?",
                params![cfg.id, confidence, id],
            )?;
        }
    }
    tx.commit()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read_lines(bytes: &[u8]) -> Vec<(usize, String)> {
        let dir = std::env::temp_dir().join(format!("cmd-lines-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(format!("l-{}.txt", bytes.len()));
        std::fs::write(&path, bytes).unwrap();
        let mut out = Vec::new();
        each_line(std::fs::File::open(&path).unwrap(), |no, text| {
            out.push((no, text.to_owned()));
            Ok(())
        })
        .unwrap();
        let _ = std::fs::remove_file(&path);
        out
    }

    #[test]
    fn a_batch_stays_under_glibc_mmap_threshold() {
        let batch = std::mem::size_of::<[Sql; 31]>() * BATCH;
        assert!(batch < 128 * 1024, "{batch}");
    }

    #[test]
    fn universal_newlines_like_python_text_files() {
        let got = read_lines(b"a\rb\r\nc\n\nd\xffe\r");
        let want: Vec<(usize, String)> = [(1, "a"), (2, "b"), (3, "c"), (4, ""), (5, "d\u{fffd}e")]
            .iter()
            .map(|(n, s)| (*n, (*s).to_owned()))
            .collect();
        assert_eq!(got, want);
        // `\r\n` partido entre dos lecturas del búfer de 64 KiB: un solo fin de línea.
        let mut big = vec![b'x'; 64 * 1024 - 1];
        big.extend(b"\r\nz");
        let got = read_lines(&big);
        assert_eq!(got.len(), 2);
        assert_eq!(got.get(1), Some(&(2, "z".to_owned())));
        // Un `\r` al final de un trozo seguido de otra línea.
        let mut big = vec![b'y'; 64 * 1024 - 1];
        big.extend(b"\rw\n");
        let got = read_lines(&big);
        assert_eq!(got.get(1), Some(&(2, "w".to_owned())));
        // Multibyte partido entre trozos: se decodifica entero.
        let mut big = vec![b'q'; 64 * 1024 - 1];
        big.extend("é\n".as_bytes());
        let got = read_lines(&big);
        assert!(got.first().is_some_and(|(_, l)| l.ends_with('é')));
    }

    #[test]
    fn changed_files_rebuild_seen_with_the_cut() {
        let mut seen = ImportSeen::default();
        let a = (1.5, PathBuf::from("/a"));
        let b = (2.5, PathBuf::from("/b"));
        assert_eq!(seen.changed("s", &[a.clone(), b.clone()]).len(), 2);
        assert!(seen.changed("s", &[a.clone(), b.clone()]).is_empty());
        // `/b` sale del corte y vuelve sin cambiar: se relee (el Python no).
        assert!(seen.changed("s", std::slice::from_ref(&a)).is_empty());
        assert_eq!(seen.len(), 1);
        assert_eq!(seen.changed("s", &[a, b.clone()]), vec![b]);
    }
}
