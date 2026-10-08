//! Lectores locales de los límites de proveedor: los rollouts de Codex, el log de
//! Grok, la cuota de agy, las cabeceras de Groq, las credenciales OAuth de Claude
//! y las cuotas declaradas (`bin/cc_usage.py:1040-1930` y `bin/cc-dash:492-1306`).
//!
//! Todo es síncrono y sin red: quien llama lo corre en `spawn_blocking`. Lo que
//! el Python lanza sin capturar en el hilo de refresco se devuelve como
//! `AbortRefresh` (el hilo moría y la caché no cambiaba); lo que lanza dentro de
//! un `try` del llamador se resuelve aquí con el mismo resultado que su `except`.
use crate::{Unsure, agent_procs};
use comandos_core::{
    json::{truthy, workspace_loads},
    text::{self, NumError},
    usage_state::{self, LocalZone, PyNum, UsageError, float_value, round_float},
};
use serde_json::{Map, Value};
use std::{
    fs,
    io::{self, Read, Seek, SeekFrom},
    os::unix::{
        ffi::{OsStrExt, OsStringExt},
        fs::MetadataExt,
    },
    path::{Path, PathBuf},
};

/// `CLAUDE_OAUTH_USAGE_URL` (cc_usage.py:1034).
pub const CLAUDE_OAUTH_USAGE_URL: &str = "https://api.anthropic.com/api/oauth/usage";

/// El Python lanzaría una excepción sin capturar dentro de `_refresh_provider_limits`:
/// el hilo de refresco termina y la caché de límites no cambia.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AbortRefresh;

/// Excepción del Python con su `str(e)` (el texto que acaba en `health.error`), o
/// un resultado que este port no reproduce con certeza.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Raised {
    Exception(String),
    Unsure,
}

impl From<Unsure> for AbortRefresh {
    fn from(_: Unsure) -> Self {
        AbortRefresh
    }
}

impl From<UsageError> for AbortRefresh {
    fn from(_: UsageError) -> Self {
        AbortRefresh
    }
}

type Row = Map<String, Value>;

const NULL: Value = Value::Null;

fn get<'a>(row: &'a Row, key: &str) -> &'a Value {
    row.get(key).unwrap_or(&NULL)
}

fn round1(x: f64) -> f64 {
    round_float(x, 1)
}

/// `max(a, b)` de Python entre `float`: devuelve `b` solo si `b > a`.
fn py_max(a: f64, b: f64) -> f64 {
    if b > a { b } else { a }
}

/// `min(a, b)` de Python entre `float`: devuelve `b` solo si `b < a`.
fn py_min(a: f64, b: f64) -> f64 {
    if b < a { b } else { a }
}

/// `str(value)` de Python (con `None` → `"None"`); el `repr` de contenedores no
/// se reproduce.
fn py_str(value: &Value) -> Result<String, Unsure> {
    match value {
        Value::Null => Ok("None".into()),
        other => usage_state::text(other).map_err(|_| Unsure),
    }
}

/// Nombre de tipo de Python para el texto de un `AttributeError`.
fn type_name(value: &Value) -> &'static str {
    match value {
        Value::Null => "NoneType",
        Value::Bool(_) => "bool",
        Value::String(_) => "str",
        Value::Array(_) => "list",
        Value::Object(_) => "dict",
        Value::Number(_) => match PyNum::of(value) {
            Ok(Some(PyNum::Float(_))) => "float",
            _ => "int",
        },
    }
}

/// `x.get(...)` sobre algo que no es `dict`.
fn no_get(value: &Value) -> Raised {
    Raised::Exception(format!(
        "'{}' object has no attribute 'get'",
        type_name(value)
    ))
}

/// `int(x)` de Python sin capturar nada (`int(data.get("at") or 0)`).
fn py_int(value: &Value) -> Result<i64, Raised> {
    let raised = |m: &str| Raised::Exception(m.to_owned());
    match value {
        Value::String(s) => match text::int(s) {
            Ok(n) => Ok(n),
            Err(NumError::Invalid) => Err(raised("invalid literal for int()")),
            Err(NumError::Exotic) => Err(Raised::Unsure),
        },
        Value::Bool(_) | Value::Number(_) => match PyNum::of(value) {
            Ok(Some(PyNum::Int(n))) => Ok(n),
            Ok(Some(PyNum::Float(x))) => trunc(x),
            _ => Err(Raised::Unsure),
        },
        _ => Err(raised("int() argument must be a string or a number")),
    }
}

/// `float(x)` de Python sin capturar nada.
fn py_float(value: &Value) -> Result<f64, Raised> {
    match value {
        Value::String(s) => match text::float(s) {
            Ok(x) => Ok(x),
            Err(NumError::Invalid) => Err(Raised::Exception(format!(
                "could not convert string to float: {s:?}"
            ))),
            Err(NumError::Exotic) => Err(Raised::Unsure),
        },
        Value::Bool(_) | Value::Number(_) => match PyNum::of(value) {
            Ok(Some(n)) => Ok(n.as_f64()),
            _ => Err(Raised::Unsure),
        },
        _ => Err(Raised::Exception(
            "float() argument must be a string or a number".into(),
        )),
    }
}

/// `int(x)` de un `float`: `inf` → `OverflowError`, `nan` → `ValueError`.
fn trunc(x: f64) -> Result<i64, Raised> {
    if x.is_nan() {
        return Err(Raised::Exception(
            "cannot convert float NaN to integer".into(),
        ));
    }
    if x.is_infinite() {
        return Err(Raised::Exception(
            "cannot convert float infinity to integer".into(),
        ));
    }
    let t = x.trunc();
    if (-(2f64.powi(63))..2f64.powi(63)).contains(&t) {
        Ok(t as i64)
    } else {
        Err(Raised::Unsure)
    }
}

/// Los errores de las conversiones de `usage_state` con el texto del Python:
/// `OverflowError` es siempre un `int(inf)`; `Raises`, un `int(nan)`.
fn usage_raised(e: UsageError) -> Raised {
    match e {
        UsageError::Overflow => {
            Raised::Exception("cannot convert float infinity to integer".into())
        }
        UsageError::Raises => Raised::Exception("cannot convert float NaN to integer".into()),
        UsageError::Unsure => Raised::Unsure,
    }
}

/// `os.path.expanduser("~/<rest>")` con `HOME = home`.
fn expand(home: &Path, rest: &str) -> PathBuf {
    let raw = home.as_os_str().as_bytes();
    let end = raw.iter().rposition(|b| *b != b'/').map_or(0, |i| i + 1);
    let mut out = raw.get(..end).unwrap_or_default().to_vec();
    out.push(b'/');
    out.extend_from_slice(rest.as_bytes());
    PathBuf::from(std::ffi::OsString::from_vec(out))
}

/// `open(path).read()` en modo texto (UTF-8): `None` ante cualquier `OSError` o
/// `UnicodeDecodeError`.
fn read_text(path: &Path) -> Option<String> {
    String::from_utf8(fs::read(path).ok()?).ok()
}

/// `\uD800`–`\uDFFF`: el `json` del Python los admite sueltos; el parser portado no.
fn has_surrogate_escape(text: &str) -> bool {
    text.match_indices("\\u").any(|(i, _)| {
        let hex = text.as_bytes().get(i + 2..i + 4).unwrap_or_default();
        matches!(hex, [b'd' | b'D', b'8'..=b'9' | b'a'..=b'f' | b'A'..=b'F'])
    })
}

/// El `RecursionError` del `json` del Python (que no es `ValueError`).
fn deep(text: &str) -> bool {
    text.bytes().filter(|b| matches!(b, b'[' | b'{')).count() >= 1000
}

/// `json.loads(texto)`: `Ok(None)` si el Python lanza `ValueError` con certeza.
fn loads(text: &str) -> Result<Option<Value>, Unsure> {
    match workspace_loads(text) {
        Ok(value) => Ok(Some(value)),
        Err(_) if has_surrogate_escape(text) || deep(text) => Err(Unsure),
        Err(_) => Ok(None),
    }
}

// ---------------------------------------------------------------- Claude (OAuth)

/// `_claude_account_creds` (cc-dash:492): `main` y cada `~/.claude-accounts/<n>`
/// con `.credentials.json`, en el orden de `sorted(os.listdir)` y sin `*.lock`.
///
/// Desviación: como `claude_accounts()` (cc-dash:508), un nombre que empieza
/// por `-` o `.` no es una cuenta (existió un `--dangerously-skip-permissions`
/// creado por un argumento mal pasado); el Python de 492 sí lo leería.
pub fn claude_account_creds(home: &Path) -> Vec<(String, PathBuf)> {
    let mut out = vec![("main".to_owned(), expand(home, ".claude/.credentials.json"))];
    let base = expand(home, ".claude-accounts");
    let Ok(entries) = fs::read_dir(&base) else {
        return out;
    };
    let mut names: Vec<std::ffi::OsString> = entries.flatten().map(|e| e.file_name()).collect();
    names.sort_by(|a, b| a.as_bytes().cmp(b.as_bytes()));
    for name in names {
        let path = base.join(&name).join(".credentials.json");
        let bytes = name.as_bytes();
        if bytes.starts_with(b"-") || bytes.starts_with(b".") {
            continue;
        }
        if !bytes.ends_with(b".lock") && path.is_file() {
            out.push((name.to_string_lossy().into_owned(), path));
        }
    }
    out
}

/// El token de `fetch_claude_oauth_limits` (cc_usage.py:1103-1107). Ausente,
/// ilegible o JSON inválido → `""`; un JSON que no es objeto, o `claudeAiOauth`
/// verdadero que no es objeto, es el `AttributeError` que mataba el hilo.
pub fn oauth_token(path: &Path) -> Result<String, AbortRefresh> {
    let Some(text) = read_text(path) else {
        return Ok(String::new());
    };
    let Some(data) = loads(&text)? else {
        return Ok(String::new());
    };
    let obj = data.as_object().ok_or(AbortRefresh)?;
    let oauth = get(obj, "claudeAiOauth");
    if !truthy(oauth) {
        return Ok(String::new());
    }
    let oauth = oauth.as_object().ok_or(AbortRefresh)?;
    Ok(usage_state::text(get(oauth, "accessToken"))?)
}

/// `"".join(c for c in s.lower() if c.isalnum())`: exacto sobre ASCII; con otros
/// caracteres `lower()`/`isalnum()` de Unicode no se reproducen.
fn slug(s: &str) -> Result<String, Raised> {
    if !s.is_ascii() {
        return Err(Raised::Unsure);
    }
    Ok(s.chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|c| c.to_ascii_lowercase())
        .collect())
}

struct OauthRow<'a> {
    id: String,
    kind: &'a str,
    label: String,
    percent: &'a Value,
    resets_at: &'a Value,
    severity: &'a Value,
    scope: &'a str,
    is_active: bool,
    window: &'a str,
}

/// `add(...)` de `parse_claude_oauth_limits`.
fn oauth_row(r: OauthRow<'_>, now: i64, zone: &dyn LocalZone) -> Result<Value, Raised> {
    let mut row = Row::new();
    row.insert("id".into(), r.id.into());
    row.insert("provider".into(), "claude".into());
    row.insert("kind".into(), r.kind.into());
    row.insert("label".into(), r.label.into());
    row.insert("scope".into(), r.scope.into());
    let percent = usage_state::as_float(r.percent, 0.0).map_err(usage_raised)?;
    row.insert("percent".into(), float_value(round1(percent)));
    let resets = usage_state::as_epoch(r.resets_at, zone).map_err(usage_raised)?;
    row.insert("resets_at".into(), resets.into());
    let severity = if truthy(r.severity) {
        usage_state::text(r.severity).map_err(usage_raised)?
    } else {
        "normal".into()
    };
    row.insert("severity".into(), severity.into());
    row.insert("is_active".into(), r.is_active.into());
    row.insert("window".into(), r.window.into());
    row.insert("source".into(), "oauth".into());
    row.insert("confidence".into(), "exact".into());
    row.insert("captured_at".into(), now.into());
    Ok(Value::Object(row))
}

/// `parse_claude_oauth_limits` (cc_usage.py:1040). `Raised::Exception` lleva el
/// `str(e)` que `fetch_claude_oauth_limits` guarda en `health.error`.
pub fn parse_claude_oauth_limits(
    payload: &Value,
    now: i64,
    zone: &dyn LocalZone,
) -> Result<Vec<Value>, Raised> {
    let p = payload.as_object().ok_or_else(|| no_get(payload))?;
    let mut rows = Vec::new();
    for item in get(p, "limits").as_array().into_iter().flatten() {
        let Some(item) = item.as_object() else {
            continue;
        };
        if get(item, "percent").is_null() {
            continue;
        }
        let kind = usage_state::text(get(item, "kind")).map_err(usage_raised)?;
        let model = get(item, "scope")
            .as_object()
            .and_then(|s| s.get("model"))
            .and_then(Value::as_object);
        let scope_name = usage_state::text(model.map_or(&NULL, |m| get(m, "display_name")))
            .map_err(usage_raised)?;
        let (id, label, window) = match kind.as_str() {
            "session" => ("claude_session".to_owned(), "Sesion 5h".to_owned(), "5h"),
            "weekly_all" => ("claude_weekly".to_owned(), "Semana".to_owned(), "7d"),
            "weekly_scoped" => {
                let slug = slug(&scope_name)?;
                let slug = if slug.is_empty() {
                    "modelo".into()
                } else {
                    slug
                };
                let label = format!("Semana {scope_name}");
                (
                    format!("claude_weekly_{slug}"),
                    text::strip(&label).to_owned(),
                    "7d",
                )
            }
            other => {
                let slug = slug(other)?;
                let slug = if slug.is_empty() { "otro".into() } else { slug };
                let label = if other.is_empty() { "Limite" } else { other };
                (format!("claude_{slug}"), label.to_owned(), "")
            }
        };
        rows.push(oauth_row(
            OauthRow {
                id,
                kind: &kind,
                label,
                percent: get(item, "percent"),
                resets_at: get(item, "resets_at"),
                severity: get(item, "severity"),
                scope: &scope_name,
                is_active: item.get("is_active").is_none_or(truthy),
                window,
            },
            now,
            zone,
        )?);
    }
    if rows.is_empty() {
        let empty = Row::new();
        let five = get(p, "five_hour").as_object().unwrap_or(&empty);
        let seven = get(p, "seven_day").as_object().unwrap_or(&empty);
        for (block, id, kind, label, window) in [
            (five, "claude_session", "session", "Sesion 5h", "5h"),
            (seven, "claude_weekly", "weekly_all", "Semana", "7d"),
        ] {
            if get(block, "utilization").is_null() {
                continue;
            }
            rows.push(oauth_row(
                OauthRow {
                    id: id.into(),
                    kind,
                    label: label.into(),
                    percent: get(block, "utilization"),
                    resets_at: get(block, "resets_at"),
                    severity: &NULL,
                    scope: "",
                    is_active: true,
                    window,
                },
                now,
                zone,
            )?);
        }
    }
    Ok(rows)
}

// ---------------------------------------------------------------- Codex (rollouts)

/// `st_mtime` de `os.stat`: `sec + nsec * 1e-9` en `double`, como `fill_time`.
fn st_mtime(meta: &fs::Metadata) -> f64 {
    meta.mtime() as f64 + meta.mtime_nsec() as f64 * 1e-9
}

/// `os.walk(root)` sin seguir enlaces a directorios: `(st_mtime, ruta)` de cada
/// `*.jsonl`. Directorios ilegibles se saltan (el `onerror=None`).
fn walk_jsonl(dir: &Path, out: &mut Vec<(f64, PathBuf)>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let mut subdirs = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        // `entry.is_dir()` sigue enlaces; el descenso no (`followlinks=False`).
        if fs::metadata(&path).is_ok_and(|m| m.is_dir()) {
            if !entry.file_type().is_ok_and(|t| t.is_symlink()) {
                subdirs.push(path);
            }
            continue;
        }
        if !entry.file_name().as_bytes().ends_with(b".jsonl") {
            continue;
        }
        if let Ok(meta) = fs::metadata(&path) {
            out.push((st_mtime(&meta), path));
        }
    }
    for sub in subdirs {
        walk_jsonl(&sub, out);
    }
}

/// `_last_codex_rate_limit_snapshot` (cc_usage.py:1199): cola de 256 KiB,
/// `decode("utf-8", "replace")`, líneas al revés y la primera con `rate_limits`.
fn codex_snapshot(path: &Path, zone: &dyn LocalZone) -> Result<Option<(Row, i64)>, AbortRefresh> {
    let read = || -> io::Result<Vec<u8>> {
        let size = fs::metadata(path)?.len();
        let mut file = fs::File::open(path)?;
        file.seek(SeekFrom::Start(size.saturating_sub(262_144)))?;
        let mut chunk = Vec::new();
        file.read_to_end(&mut chunk)?;
        Ok(chunk)
    };
    let Ok(raw) = read() else {
        return Ok(None);
    };
    let chunk = String::from_utf8_lossy(&raw);
    for line in text::splitlines(&chunk).into_iter().rev() {
        if !line.contains("\"rate_limits\"") {
            continue;
        }
        let Some(data) = loads(line)? else {
            continue;
        };
        let Some(obj) = data.as_object() else {
            continue;
        };
        let limits = get(obj, "payload")
            .as_object()
            .map(|p| get(p, "rate_limits"))
            .and_then(Value::as_object);
        if let Some(limits) = limits.filter(|l| !l.is_empty()) {
            let captured = usage_state::as_epoch(get(obj, "timestamp"), zone)?;
            return Ok(Some((limits.clone(), captured)));
        }
    }
    Ok(None)
}

/// `read_codex_rate_limits` (cc_usage.py:1221): la ventana más fresca de cada
/// `window_minutes` (300 y 10080) entre los `max_files` rollouts más recientes.
/// Sin `try` en el llamador: una excepción es `AbortRefresh`.
pub fn read_codex_rate_limits(
    sessions_root: &Path,
    now: i64,
    max_files: usize,
    zone: &dyn LocalZone,
) -> Result<Vec<Value>, AbortRefresh> {
    if !sessions_root.is_dir() {
        return Ok(Vec::new());
    }
    let mut files = Vec::new();
    walk_jsonl(sessions_root, &mut files);
    files.sort_by(|a, b| {
        b.0.total_cmp(&a.0)
            .then_with(|| b.1.as_os_str().as_bytes().cmp(a.1.as_os_str().as_bytes()))
    });
    // window_minutes → (captured_at, item, plan_type), en orden de primera inserción.
    let mut freshest: Vec<(i64, (i64, Row, String))> = Vec::new();
    for (_, path) in files.iter().take(max_files) {
        let Some((limits, captured)) = codex_snapshot(path, zone)? else {
            continue;
        };
        for key in ["primary", "secondary"] {
            let Some(item) = get(&limits, key).as_object() else {
                continue;
            };
            if get(item, "used_percent").is_null() {
                continue;
            }
            let wm = usage_state::as_int(get(item, "window_minutes"), 0)?;
            if wm != 300 && wm != 10_080 {
                continue;
            }
            match freshest.iter_mut().find(|(w, _)| *w == wm) {
                Some((_, slot)) => {
                    if captured > slot.0 {
                        let plan = usage_state::text(get(&limits, "plan_type"))?;
                        *slot = (captured, item.clone(), plan);
                    }
                }
                None => {
                    let plan = usage_state::text(get(&limits, "plan_type"))?;
                    freshest.push((wm, (captured, item.clone(), plan)));
                }
            }
        }
    }
    let mut rows = Vec::new();
    for (wm, (captured, item, plan)) in freshest {
        let (id, kind, window, label) = if wm == 300 {
            ("codex_session", "session", "5h", "Sesion 5h")
        } else {
            ("codex_weekly", "weekly_all", "7d", "Semana")
        };
        // Más viejo que 1.5 veces la propia ventana: ya reseteó desde entonces.
        let age = i128::from(now) - i128::from(captured);
        if captured != 0 && age * 10 > i128::from(wm) * 60 * 15 {
            continue;
        }
        let mut row = Row::new();
        row.insert("id".into(), id.into());
        row.insert("provider".into(), "codex".into());
        row.insert("account".into(), "main".into());
        row.insert("kind".into(), kind.into());
        row.insert("label".into(), label.into());
        row.insert("scope".into(), "".into());
        let percent = usage_state::as_float(get(&item, "used_percent"), 0.0)?;
        row.insert("percent".into(), float_value(round1(percent)));
        let resets = usage_state::as_epoch(get(&item, "resets_at"), zone)?;
        row.insert("resets_at".into(), resets.into());
        row.insert("severity".into(), "normal".into());
        row.insert("is_active".into(), true.into());
        row.insert("window".into(), window.into());
        row.insert("plan_type".into(), plan.into());
        row.insert("source".into(), "rollout".into());
        row.insert("confidence".into(), "exact".into());
        row.insert("captured_at".into(), captured.into());
        rows.push(Value::Object(row));
    }
    // `rows.sort(key=lambda r: r["window"])`: estable, 5h antes que 7d.
    rows.sort_by(|a, b| {
        let w = |v: &Value| {
            v.get("window")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_owned()
        };
        w(a).cmp(&w(b))
    });
    Ok(rows)
}

// ---------------------------------------------------------------- Grok (log local)

/// Marca de las líneas de cobro en el log de Grok.
const GROK_BILLING_MARK: &str = "fetched credits config";

/// Bloque de lectura de la cola del log de Grok (64 KiB). Un búfer de 1 MiB
/// lo servía glibc con `mmap` y, al soltarlo en cada refresco de límites,
/// subía su umbral de `mmap` a 1 MiB y el de recorte a 2 MiB: desde entonces
/// las arenas de los hilos no se recortaban (visto con `gdb` en el sondeo).
const GROK_BLOCK: usize = 64 << 10;

/// Tope de la línea en curso (entre dos `\n`) mientras se lee por bloques:
/// 4 MiB. Una línea más larga se descarta entera; la de cobro mide < 1 KiB
/// (diferencia documentada con el Python, que la leería entera).
const GROK_LINE_CAP: usize = 4 << 20;

/// `_grok_billing_lines` (cc_usage.py:1825): cola de `tail` bytes que crece ×8
/// mientras no haya líneas de cobro, hasta cubrir el archivo o pasar `max`.
/// La ventana se recorre hacia delante en bloques de 64 KiB (memoria acotada
/// aunque la ventana llegue a 256 MiB) con el mismo resultado que decodificar
/// la ventana entera y partirla con `splitlines`.
fn grok_billing_lines(path: &Path, tail: u64, max: u64) -> io::Result<Vec<String>> {
    grok_billing_lines_with(path, tail, max, GROK_BLOCK, GROK_LINE_CAP)
}

fn grok_billing_lines_with(
    path: &Path,
    tail: u64,
    max: u64,
    block: usize,
    cap: usize,
) -> io::Result<Vec<String>> {
    let mut file = fs::File::open(path)?;
    let size = file.seek(SeekFrom::End(0))?;
    let mut span = tail;
    loop {
        file.seek(SeekFrom::Start(size.saturating_sub(span)))?;
        let lines = scan_billing(&mut (&mut file).take(span), block, cap)?;
        if !lines.is_empty() || span >= size || span >= max {
            return Ok(lines);
        }
        span = span.saturating_mul(8);
    }
}

/// `chunk.decode(errors="replace").splitlines()` filtrado por la marca, por
/// bloques. Se parte en los bytes `\n`: nunca están dentro de un carácter
/// UTF-8 válido ni los consume una secuencia inválida (son ASCII), así que
/// decodificar cada tramo por separado da el mismo texto; y `\n` es siempre
/// un límite de `splitlines`, así que dentro de cada tramo se aplica
/// `splitlines` (`\r`, `\x0b`, `\x0c`, `\x1c`-`\x1e`, U+0085, U+2028/9) y
/// las líneas no vacías coinciden. Las vacías no llevan la marca.
fn scan_billing(reader: &mut impl Read, block: usize, cap: usize) -> io::Result<Vec<String>> {
    let mut lines = Vec::new();
    let mut carry: Vec<u8> = Vec::new();
    // La línea en curso pasó del tope: se descarta hasta el próximo `\n`.
    let mut skipping = false;
    let mut buf = vec![0u8; block.max(1)];
    loop {
        let n = match reader.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        };
        let mut chunk = buf.get(..n).unwrap_or_default();
        while let Some(pos) = chunk.iter().position(|&b| b == b'\n') {
            let (head, rest) = chunk.split_at(pos);
            if !skipping {
                carry.extend_from_slice(head);
                billing_segment(&carry, &mut lines);
            }
            carry.clear();
            skipping = false;
            chunk = rest.get(1..).unwrap_or_default();
        }
        if !skipping {
            carry.extend_from_slice(chunk);
            if carry.len() > cap {
                carry = Vec::new();
                skipping = true;
            }
        }
    }
    if !skipping {
        billing_segment(&carry, &mut lines);
    }
    Ok(lines)
}

/// Un tramo sin `\n`: decodificado con reemplazo y partido con `splitlines`.
fn billing_segment(segment: &[u8], lines: &mut Vec<String>) {
    if segment.is_empty() {
        return;
    }
    let text = String::from_utf8_lossy(segment);
    lines.extend(
        text::splitlines(&text)
            .into_iter()
            .filter(|l| l.contains(GROK_BILLING_MARK))
            .map(str::to_owned),
    );
}

/// `x.get(k) or {}` cuando `x` es un `dict`: el valor verdadero debe ser `dict`.
fn obj_or_empty(value: &Value) -> Result<Option<&Row>, Raised> {
    if !truthy(value) {
        return Ok(None);
    }
    value.as_object().map(Some).ok_or_else(|| no_get(value))
}

/// `_iso_epoch(value)` (cc_usage.py:1928) sobre lo que `str()` daría: solo un
/// texto puede ser una fecha ISO.
fn iso_epoch_value(value: &Value, zone: &dyn LocalZone) -> i64 {
    value
        .as_str()
        .map_or(0, |s| usage_state::iso_epoch(s, zone))
}

/// Una línea de cobro de Grok como `entry` de `read_grok_credit_limits`.
fn grok_entry(row: &Value, zone: &dyn LocalZone) -> Result<Row, Raised> {
    let row = row.as_object().ok_or_else(|| no_get(row))?;
    let empty = Row::new();
    let ctx = obj_or_empty(get(row, "ctx"))?.unwrap_or(&empty);
    let cfg = obj_or_empty(get(ctx, "config"))?.unwrap_or(&empty);
    let period = obj_or_empty(get(cfg, "currentPeriod"))?.unwrap_or(&empty);
    let ts = get(row, "ts");
    let captured = if truthy(ts) {
        let s = ts.as_str().ok_or_else(|| {
            Raised::Exception(format!(
                "'{}' object has no attribute 'replace'",
                type_name(ts)
            ))
        })?;
        usage_state::fromisoformat_epoch(&s.replace('Z', "+00:00"), zone).unwrap_or(0)
    } else {
        // `fromisoformat("")`: `ValueError` → 0.
        0
    };
    let credit = get(cfg, "creditUsagePercent");
    let percent = if truthy(credit) {
        py_float(credit)?
    } else {
        0.0
    };
    let tier = get(ctx, "subscriptionTier");
    let tier = if truthy(tier) {
        py_str(tier).map_err(|_| Raised::Unsure)?
    } else {
        String::new()
    };
    let mut entry = Row::new();
    entry.insert("percent".into(), float_value(percent));
    entry.insert(
        "period_start".into(),
        iso_epoch_value(get(period, "start"), zone).into(),
    );
    entry.insert(
        "resets_at".into(),
        iso_epoch_value(get(period, "end"), zone).into(),
    );
    entry.insert(
        "tier".into(),
        tier.chars().take(40).collect::<String>().into(),
    );
    entry.insert("captured_at".into(), captured.into());
    Ok(entry)
}

fn int_of(row: &Row, key: &str) -> i64 {
    get(row, key).as_i64().unwrap_or(0)
}

/// `read_grok_credit_limits` (cc_usage.py:1842). Cualquier excepción la atrapa el
/// `except Exception: official = None` de `_refresh_provider_limits`: `None`.
pub fn read_grok_credit_limits(homes: &[PathBuf], now: i64, zone: &dyn LocalZone) -> Option<Row> {
    let mut best: Option<Row> = None;
    for home in homes {
        let path = home.join("logs").join("unified.jsonl");
        let Ok(lines) = grok_billing_lines(&path, 524_288, 64 << 20) else {
            continue;
        };
        for line in lines {
            let Some(row) = loads(&line).ok()? else {
                continue;
            };
            let entry = grok_entry(&row, zone).ok()?;
            let captured = int_of(&entry, "captured_at");
            if int_of(&entry, "resets_at") != 0
                && best
                    .as_ref()
                    .is_none_or(|b| captured > int_of(b, "captured_at"))
            {
                best = Some(entry);
            }
        }
    }
    let mut best = best?;
    // Ventana ya vencida: el % es de un periodo viejo.
    if int_of(&best, "resets_at") <= now {
        best.insert("stale_period".into(), true.into());
    }
    Some(best)
}

/// `realpath` sobre una ruta del sistema.
fn resolve(path: &Path) -> PathBuf {
    PathBuf::from(std::ffi::OsString::from_vec(agent_procs::realpath(
        path.as_os_str().as_bytes(),
    )))
}

/// `grok_state.account_homes` (lib/grok_state.py:16): `main` = `~/.grok` y cada
/// subcarpeta de `~/.grok-accounts` (sin `.`/`-` al principio), todas resueltas.
pub fn grok_account_homes(home: &Path) -> Vec<(String, PathBuf)> {
    let mut out = vec![("main".to_owned(), resolve(&expand(home, ".grok")))];
    let root = resolve(&expand(home, ".grok-accounts"));
    if !root.is_dir() {
        return out;
    }
    let Ok(entries) = fs::read_dir(&root) else {
        return out;
    };
    let mut names: Vec<std::ffi::OsString> = entries.flatten().map(|e| e.file_name()).collect();
    names.sort_by(|a, b| a.as_bytes().cmp(b.as_bytes()));
    for name in names {
        let path = root.join(&name);
        if path.is_dir() && !name.as_bytes().starts_with(b".") && !name.as_bytes().starts_with(b"-")
        {
            out.push((name.to_string_lossy().into_owned(), resolve(&path)));
        }
    }
    out
}

/// `grok_row` de `_refresh_provider_limits` (cc-dash:1199-1238): la fila de Grok
/// con el límite oficial del log (ajustado a la semana vigente si venció) o el
/// consumo medido local. `None` si no hay ninguno de los dos.
pub fn grok_row(
    measured: Option<&Value>,
    official: Option<&Row>,
    quota: i64,
    now: i64,
) -> Option<Value> {
    let official = official.map(|o| {
        let mut o = o.clone();
        if truthy(get(&o, "stale_period")) {
            // Sus periodos son semanas seguidas: el reset es el fin viejo + N semanas.
            let resets = int_of(&o, "resets_at");
            let elapsed = now.saturating_sub(resets);
            let weeks =
                match elapsed.div_euclid(604_800) + i64::from(elapsed.rem_euclid(604_800) != 0) {
                    0 => 1,
                    w => w,
                };
            o.insert("percent".into(), float_value(0.0));
            o.insert(
                "resets_at".into(),
                resets.saturating_add(weeks.saturating_mul(604_800)).into(),
            );
        }
        o
    });
    if measured.is_none() && official.is_none() {
        return None;
    }
    let gm = measured.cloned().unwrap_or_else(empty_measured);
    let mut row = Row::new();
    let (pct, resets, conf, label, plan, captured, note) = match &official {
        Some(o) => {
            let pct = PyNum::of(get(o, "percent"))
                .ok()
                .flatten()
                .map_or(0.0, PyNum::as_f64);
            let tier = get(o, "tier");
            let plan = if truthy(tier) {
                tier.clone()
            } else {
                Value::Null
            };
            (
                float_value(round1(pct)),
                get(o, "resets_at").clone(),
                "exact",
                "Semana",
                plan,
                get(o, "captured_at").clone(),
                "creditUsagePercent oficial del CLI de grok (log local)",
            )
        }
        None => (
            measured_percent(&gm, quota),
            Value::from(0),
            "measured",
            "Consumo medido",
            Value::Null,
            Value::from(now),
            "sin dato oficial reciente — abre grok para refrescarlo",
        ),
    };
    let window = official.is_some();
    row.insert("id".into(), "grok_measured".into());
    row.insert("provider".into(), "grok".into());
    row.insert(
        "kind".into(),
        if window { "window" } else { "measured" }.into(),
    );
    row.insert("label".into(), label.into());
    row.insert("scope".into(), "".into());
    row.insert("percent".into(), pct);
    row.insert("resets_at".into(), resets);
    row.insert("severity".into(), "info".into());
    row.insert("is_active".into(), true.into());
    row.insert("window".into(), "7d".into());
    row.insert(
        "source".into(),
        if window { "grok_cli_log" } else { "local" }.into(),
    );
    row.insert("confidence".into(), conf.into());
    row.insert("captured_at".into(), captured);
    row.insert("account".into(), "main".into());
    row.insert("plan_type".into(), plan);
    measured_tail(&mut row, &gm, quota, note);
    Some(Value::Object(row))
}

/// `gm or {...}`: el consumo vacío.
fn empty_measured() -> Value {
    serde_json::json!({"tokens_today": 0, "turns_today": 0, "tokens_7d": 0,
                       "turns_7d": 0, "daily": []})
}

/// `round(gm["tokens_7d"] / quota * 100, 1) if quota > 0 else None`.
fn measured_percent(gm: &Value, quota: i64) -> Value {
    if quota <= 0 {
        return Value::Null;
    }
    let tokens = gm.get("tokens_7d").and_then(Value::as_i64).unwrap_or(0);
    float_value(round1(tokens as f64 / quota as f64 * 100.0))
}

/// Las claves finales comunes de las filas medidas de Grok y Groq.
fn measured_tail(row: &mut Row, gm: &Value, quota: i64, note: &str) {
    for key in ["tokens_today", "turns_today", "tokens_7d", "turns_7d"] {
        row.insert(key.into(), gm.get(key).cloned().unwrap_or(Value::Null));
    }
    let daily = gm.get("daily").filter(|d| truthy(d)).cloned();
    row.insert(
        "daily".into(),
        daily.unwrap_or_else(|| Value::Array(Vec::new())),
    );
    row.insert(
        "quota_7d".into(),
        if quota != 0 {
            quota.into()
        } else {
            Value::Null
        },
    );
    row.insert("note".into(), note.into());
}

// ---------------------------------------------------------------- agy

/// `AGY_BUCKETS` (cc_usage.py:1890): cubeta → (ventana, scope).
const AGY_BUCKETS: [(&str, &str, &str); 3] = [
    ("gemini-weekly", "7d", ""),
    ("gemini-5h", "5h", ""),
    ("3p-weekly", "7d", "Claude·GPT"),
];

/// `read_agy_quota` (cc_usage.py:1893). Cualquier excepción la atrapa el
/// llamador (`except Exception: agy_rows = []`): lista vacía.
pub fn read_agy_quota(path: &Path, now: f64, zone: &dyn LocalZone) -> Vec<Value> {
    read_text(path).map_or_else(Vec::new, |text| {
        agy_rows_text(&text, now, zone).unwrap_or_default()
    })
}

pub fn read_agy_quota_bytes(bytes: &[u8], now: f64, zone: &dyn LocalZone) -> Vec<Value> {
    std::str::from_utf8(bytes)
        .ok()
        .and_then(|text| agy_rows_text(text, now, zone))
        .unwrap_or_default()
}
fn agy_rows_text(text: &str, now: f64, zone: &dyn LocalZone) -> Option<Vec<Value>> {
    let ts = trunc(now).ok()?;
    let data = loads(text).ok()??;
    let data = data.as_object()?;
    let quota = data.get("quota")?;
    let at = get(data, "captured_at");
    let captured = if truthy(at) { py_int(at).ok()? } else { 0 };
    let plan = get(data, "plan_tier");
    let plan = if truthy(plan) {
        Value::from(py_str(plan).ok()?)
    } else {
        Value::Null
    };
    let mut rows = Vec::new();
    for (name, window, scope) in AGY_BUCKETS {
        let Some(b) = quota
            .as_object()
            .and_then(|q| q.get(name))
            .and_then(Value::as_object)
        else {
            continue;
        };
        // `isinstance(x, (int, float))`: `bool` cuenta como `int`.
        let fraction = get(b, "remaining_fraction");
        if !matches!(fraction, Value::Number(_) | Value::Bool(_)) {
            continue;
        }
        let reset = get(b, "reset_time");
        let mut resets = match reset.as_str() {
            Some(s) => usage_state::iso_epoch(&s.replace('Z', "+00:00"), zone),
            None => 0,
        };
        let fraction = PyNum::of(fraction).ok()??.as_f64();
        let mut used = round1((1.0 - fraction) * 100.0);
        if resets <= ts {
            used = 0.0;
            resets = 0;
        }
        let mut row = Row::new();
        row.insert("id".into(), format!("agy_{name}").into());
        row.insert("provider".into(), "agy".into());
        row.insert("account".into(), "main".into());
        row.insert("kind".into(), "window".into());
        row.insert(
            "label".into(),
            if window == "5h" {
                "Sesión 5 h"
            } else {
                "Semana"
            }
            .into(),
        );
        row.insert("scope".into(), scope.into());
        row.insert(
            "percent".into(),
            float_value(py_max(0.0, py_min(100.0, used))),
        );
        row.insert("resets_at".into(), resets.into());
        row.insert("severity".into(), "info".into());
        row.insert("is_active".into(), true.into());
        row.insert("window".into(), window.into());
        row.insert("source".into(), "agy_statusline".into());
        row.insert("confidence".into(), "exact".into());
        row.insert("captured_at".into(), captured.into());
        row.insert("plan_type".into(), plan.clone());
        rows.push(Value::Object(row));
    }
    Some(rows)
}

// ---------------------------------------------------------------- Groq

/// `_reset_at` de `parse_groq_ratelimit_headers`: epoch o duración (`7s`, `1m30s`).
fn groq_reset_at(value: Option<&String>, ts: i64) -> Result<i64, Raised> {
    let raw = value.map_or("", String::as_str);
    let text = text::strip(raw);
    if text.is_empty() {
        return Ok(0);
    }
    match text::float(text) {
        Ok(number) => {
            if number >= (ts - 86_400) as f64 {
                return trunc(number);
            }
            if 0.0 < number && number < 86_400.0 {
                return ts.checked_add(trunc(number)?).ok_or(Raised::Unsure);
            }
        }
        Err(NumError::Invalid) => {}
        Err(NumError::Exotic) => return Err(Raised::Unsure),
    }
    // `ch.isdigit()` y `lower()` de Unicode: solo ASCII es exacto.
    if !text.is_ascii() {
        return Err(Raised::Unsure);
    }
    let mut total = 0.0f64;
    let mut num = String::new();
    for ch in text.chars().map(|c| c.to_ascii_lowercase()) {
        if ch.is_ascii_digit() || ch == '.' {
            num.push(ch);
            continue;
        }
        if num.is_empty() {
            continue;
        }
        let amount = match text::float(&num) {
            Ok(x) => x,
            Err(_) => {
                return Err(Raised::Exception(format!(
                    "could not convert string to float: {num:?}"
                )));
            }
        };
        num.clear();
        match ch {
            'h' => total += amount * 3600.0,
            'm' => total += amount * 60.0,
            's' => total += amount,
            _ => {}
        }
    }
    if total == 0.0 {
        return Ok(0);
    }
    ts.checked_add(trunc(total)?).ok_or(Raised::Unsure)
}

/// `parse_groq_ratelimit_headers` (cc_usage.py:1126): `%` de las cabeceras
/// `x-ratelimit-*` de la última respuesta.
pub fn parse_groq_ratelimit_headers(headers: &Row, now: i64) -> Result<Vec<Value>, Raised> {
    // `{str(k).lower(): str(v)}`: el `str(v)` solo se calcula para las seis claves
    // que se leen (el `repr` de un contenedor en otra clave no importa).
    let mut raw: Vec<(String, &Value)> = Vec::new();
    for (k, v) in headers {
        let key = k.to_lowercase();
        match raw.iter_mut().find(|(rk, _)| *rk == key) {
            Some(slot) => slot.1 = v,
            None => raw.push((key, v)),
        }
    }
    let lookup = |key: &str| -> Result<Option<String>, Raised> {
        raw.iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| py_str(v).map_err(|_| Raised::Unsure))
            .transpose()
    };
    let as_float = |s: &Option<String>| -> Result<f64, Raised> {
        match s {
            None => Ok(0.0),
            Some(s) => usage_state::as_float(&Value::from(s.as_str()), 0.0).map_err(usage_raised),
        }
    };
    let mut rows = Vec::new();
    for (kind, id, label, window) in [
        ("requests", "groq_requests", "Requests", "rpm"),
        ("tokens", "groq_tokens", "Tokens", "tpm"),
    ] {
        let limit = as_float(&lookup(&format!("x-ratelimit-limit-{kind}"))?)?;
        let remaining = lookup(&format!("x-ratelimit-remaining-{kind}"))?;
        if limit <= 0.0 || remaining.as_deref().is_none_or(str::is_empty) {
            continue;
        }
        let left = as_float(&remaining)?;
        let used = py_max(0.0, limit - left);
        let reset = groq_reset_at(lookup(&format!("x-ratelimit-reset-{kind}"))?.as_ref(), now)?;
        let mut row = Row::new();
        row.insert("id".into(), id.into());
        row.insert("provider".into(), "groq".into());
        row.insert("kind".into(), "window".into());
        row.insert("label".into(), label.into());
        row.insert("scope".into(), "".into());
        row.insert("percent".into(), float_value(round1(used / limit * 100.0)));
        row.insert("resets_at".into(), reset.into());
        row.insert("severity".into(), "normal".into());
        row.insert("is_active".into(), true.into());
        row.insert("window".into(), window.into());
        row.insert("source".into(), "headers".into());
        row.insert("confidence".into(), "exact".into());
        row.insert("captured_at".into(), now.into());
        rows.push(Value::Object(row));
    }
    Ok(rows)
}

/// `read_groq_rate_limits` (cc-dash:1261): `H/groq-ratelimit.json`; `now` es el
/// `time.time()` que se usa si el archivo no trae `at`. Todo error → `[]`.
pub fn read_groq_headers(path: &Path, now: i64) -> Vec<Value> {
    read_text(path).map_or_else(Vec::new, |text| {
        read_groq_headers_bytes(text.as_bytes(), now)
    })
}
pub fn read_groq_headers_bytes(bytes: &[u8], now: i64) -> Vec<Value> {
    let rows = || -> Option<Vec<Value>> {
        let data = loads(std::str::from_utf8(bytes).ok()?).ok()??;
        let (headers, captured) = match data.as_object() {
            Some(d) => {
                let at = get(d, "at");
                let captured = if truthy(at) { py_int(at).ok()? } else { 0 };
                (get(d, "headers").as_object(), captured)
            }
            None => (None, 0),
        };
        let headers = headers?;
        let now = if captured != 0 { captured } else { now };
        let mut rows = parse_groq_ratelimit_headers(headers, now).ok()?;
        for row in &mut rows {
            if let Some(row) = row.as_object_mut() {
                row.insert("account".into(), "main".into());
            }
        }
        Some(rows)
    };
    rows().unwrap_or_default()
}

/// `_groq_limit_rows` (cc-dash:1279) con las cabeceras ya leídas: las filas de
/// cabeceras y la fila de consumo medido. Vacío si no hay ni consumo ni cabeceras.
pub fn groq_rows(
    measured: Option<&Value>,
    headers: Vec<Value>,
    quota: i64,
    now: i64,
) -> Vec<Value> {
    if measured.is_none() && headers.is_empty() {
        return Vec::new();
    }
    let gm = measured.cloned().unwrap_or_else(empty_measured);
    let note = if headers.is_empty() {
        "sin API de cuota — declara un límite o pega headers x-ratelimit-*"
    } else {
        " Groq x-ratelimit de la última respuesta"
    };
    let mut rows = headers;
    let mut row = Row::new();
    row.insert("id".into(), "groq_measured".into());
    row.insert("provider".into(), "groq".into());
    row.insert("kind".into(), "measured".into());
    row.insert("label".into(), "Consumo medido".into());
    row.insert("scope".into(), "".into());
    row.insert("percent".into(), measured_percent(&gm, quota));
    row.insert("resets_at".into(), 0.into());
    row.insert("severity".into(), "info".into());
    row.insert("is_active".into(), true.into());
    row.insert("window".into(), "7d".into());
    row.insert("source".into(), "local".into());
    row.insert("confidence".into(), "measured".into());
    row.insert("captured_at".into(), now.into());
    row.insert("account".into(), "main".into());
    measured_tail(&mut row, &gm, quota, note);
    rows.push(Value::Object(row));
    rows
}

// ---------------------------------------------------------------- cuotas y correos

/// `user_quotas` (cc-dash:540): `H/provider-quotas.json` si es un objeto; si no, `{}`.
pub fn user_quotas(path: &Path) -> Row {
    read_text(path).map_or_else(Row::new, |text| user_quotas_bytes(text.as_bytes()))
}
pub fn user_quotas_bytes(bytes: &[u8]) -> Row {
    std::str::from_utf8(bytes)
        .ok()
        .and_then(|t| loads(t).ok().flatten())
        .and_then(|v| match v {
            Value::Object(m) => Some(m),
            _ => None,
        })
        .unwrap_or_default()
}

/// `int((user_quotas().get(provider) or {}).get("tokens_7d") or 0)`, sin `try`:
/// un error mata el hilo de refresco.
pub fn quota_tokens_7d(quotas: &Row, provider: &str) -> Result<i64, AbortRefresh> {
    let entry = get(quotas, provider);
    if !truthy(entry) {
        return Ok(0);
    }
    let entry = entry.as_object().ok_or(AbortRefresh)?;
    let tokens = get(entry, "tokens_7d");
    if !truthy(tokens) {
        return Ok(0);
    }
    py_int(tokens).map_err(|_| AbortRefresh)
}

/// Caché de `account_email_for_dir` (la de `agent_procs`: firma del archivo de
/// identidad, máx. 4096 entradas; aquí solo entran las carpetas de cuentas).
pub type EmailCache = agent_procs::AccountCache;

/// `account_email_for_dir(dir, agent)` (cc-dash:3761). `Unsure` si el correo
/// lleva un sustituto suelto que el Python emitiría.
pub fn email_for_dir(cache: &mut EmailCache, dir: &Path, agent: &str) -> Result<Value, Unsure> {
    cache.email_for_dir(dir.as_os_str().as_bytes(), agent)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// La lectura anterior en memoria: la ventana entera decodificada y partida.
    fn whole(bytes: &[u8]) -> Vec<String> {
        let text = String::from_utf8_lossy(bytes);
        text::splitlines(&text)
            .into_iter()
            .filter(|l| l.contains(GROK_BILLING_MARK))
            .map(str::to_owned)
            .collect()
    }

    fn fixture() -> Vec<u8> {
        let mut b = Vec::new();
        b.extend_from_slice(b"{\"msg\":\"otra\"}\n");
        b.extend_from_slice(
            "{\"msg\":\"billing: fetched credits config\",\"ñ\":\"€\"}\r\n".as_bytes(),
        );
        b.extend_from_slice(b"basura\xff\xfe fetched credits config \xe2\x82\n");
        b.extend_from_slice(b"a fetched credits config\rb fetched credits config\x0bc\n");
        b.extend_from_slice(
            "x fetched credits config\u{2028}y fetched credits config\u{85}z\n".as_bytes(),
        );
        b.extend_from_slice(b"\x1cfetched credits config\x1d\x1e\x0c\r\r\n\n\n");
        b.extend_from_slice(b"\xf0\x9f\x98\nfetched credits config\xf0\x9f\x98\x80\xc3");
        b.extend_from_slice(b"\nfetched credits config sin salto final\r");
        b
    }

    #[test]
    fn block_scan_matches_whole_window() {
        let bytes = fixture();
        let expected = whole(&bytes);
        assert!(expected.len() >= 8, "{expected:?}");
        for block in 1..=40 {
            let got = scan_billing(&mut bytes.as_slice(), block, usize::MAX).unwrap();
            assert_eq!(got, expected, "bloque de {block} bytes");
        }
        // También con la ventana empezando a media línea y a medio carácter.
        for cut in 0..bytes.len() {
            let tail = bytes.get(cut..).unwrap();
            for block in [1, 3, 7, 64] {
                let got = scan_billing(&mut &tail[..], block, usize::MAX).unwrap();
                assert_eq!(got, whole(tail), "corte {cut}, bloque {block}");
            }
        }
    }

    #[test]
    fn overlong_line_is_dropped_and_the_rest_kept() {
        let mut bytes = b"fetched credits config ok 1\n".to_vec();
        bytes.extend(std::iter::repeat_n(b'x', 100));
        bytes.extend_from_slice(b" fetched credits config larga\nfetched credits config ok 2");
        let got = scan_billing(&mut bytes.as_slice(), 8, 64).unwrap();
        assert_eq!(
            got,
            ["fetched credits config ok 1", "fetched credits config ok 2"]
        );
    }

    #[test]
    fn window_grows_like_python() {
        let dir = std::env::temp_dir().join(format!("grok-tail-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("unified.jsonl");
        let mut bytes = b"{\"msg\":\"billing: fetched credits config\"}\n".to_vec();
        bytes.extend(std::iter::repeat_n(b'.', 5000));
        bytes.push(b'\n');
        fs::write(&path, &bytes).unwrap();
        // Cola de 100: no hay cobro; ×8 = 800 tampoco; 6400 cubre el archivo.
        let got = grok_billing_lines_with(&path, 100, 1 << 20, 16, 1 << 20).unwrap();
        assert_eq!(got, whole(&bytes));
        assert_eq!(got.len(), 1);
        // Con `max` por debajo del archivo no se llega a la línea.
        let short = grok_billing_lines_with(&path, 100, 800, 16, 1 << 20).unwrap();
        assert!(short.is_empty());
        let _ = fs::remove_dir_all(&dir);
    }
}
