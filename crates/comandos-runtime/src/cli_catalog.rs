//! Catálogo de comandos de cada CLI: port de `lib/cli_catalog.py` (D8,
//! `3f0a01a`): validación de `config/cli-commands.json`, versión instalada,
//! binario nativo (con el marcador `COMANDOS_CODEX_ORIGINAL` del lanzador
//! YOLO), comandos presentes en el binario y la vista del catálogo; más
//! `latest_models` de `lib/model_watch.py` (chips de `/model`).
//!
//! Nada aquí lanza procesos: las versiones y las ayudas las pide quien llama
//! (el frente, con los plazos del Python) y se pasan ya leídas.
use crate::{Unsure, cli_help::regex_safe, hooks::py};
use comandos_core::{
    json::{PythonLoads, parse_value, python_loads},
    text::{is_space, strip},
};
use regex::{Regex, bytes::RegexSet};
use serde_json::{Map, Value, json};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    ffi::OsStr,
    fs,
    io::{self, Read},
    os::unix::ffi::OsStrExt,
    path::{Path, PathBuf},
    sync::LazyLock,
};

/// Lo que el Python haría: lanzar (500) o algo que no se reproduce (declinar).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewError {
    /// Excepción sin capturar del Python: 500.
    Raises,
    /// No se sabe con certeza: declinar.
    Unsure,
}

impl From<Unsure> for ViewError {
    fn from(_: Unsure) -> Self {
        ViewError::Unsure
    }
}

type Result<T, E = ViewError> = std::result::Result<T, E>;

fn obj(value: &Value) -> Result<&Map<String, Value>> {
    value.as_object().ok_or(ViewError::Unsure)
}

fn text(value: Option<&Value>) -> Result<&str> {
    value.and_then(Value::as_str).ok_or(ViewError::Unsure)
}

/// `str(x)` de un escalar de JSON; contenedores (su `repr`) o enteros que
/// `str()` no escribe (más de 4300 dígitos) → no se sabe.
pub fn py_str(value: &Value) -> Result<String, Unsure> {
    match value {
        Value::Array(_) | Value::Object(_) => Err(Unsure),
        Value::Number(n) if !py::is_float(n) && n.as_str().len() > 4300 => Err(Unsure),
        Value::Number(n) => Ok(py::number_str(n)),
        other => Ok(py::str_of(other)),
    }
}

/// `load_catalog` sobre el JSON ya leído: `Raises` es el `CatalogError` (o el
/// `TypeError`) que el Python lanza; lo raro que no se puede afirmar, `Unsure`.
pub fn validate_catalog(cat: &Value) -> Result<()> {
    let Value::Object(map) = cat else {
        return Err(ViewError::Raises);
    };
    let version_ok = map.get("version").is_some_and(|v| py::eq(v, &json!(1)));
    let Some(Value::Array(clis)) = map.get("clis").filter(|_| version_ok) else {
        return Err(ViewError::Raises);
    };
    let mut seen: Vec<&Value> = Vec::new();
    for cli in clis {
        let Value::Object(cli) = cli else {
            return Err(ViewError::Raises);
        };
        for key in ["id", "label", "binary", "pinnedVersion", "groups"] {
            if !cli.contains_key(key) {
                return Err(ViewError::Raises);
            }
        }
        let id = cli.get("id").ok_or(ViewError::Raises)?;
        if !matches!(id, Value::String(_)) {
            return Err(ViewError::Unsure);
        }
        if seen.iter().any(|s| py::eq(s, id)) {
            return Err(ViewError::Raises);
        }
        seen.push(id);
        let Some(Value::Array(groups)) = cli.get("groups") else {
            return Err(ViewError::Unsure);
        };
        for group in groups {
            let commands = match obj(group)?.get("commands") {
                None => continue,
                Some(Value::Array(commands)) => commands,
                Some(_) => return Err(ViewError::Unsure),
            };
            for cmd in commands {
                let cmd = obj(cmd)?;
                let cmd_text = cmd.get("text").filter(|v| py::truthy(v));
                let desc = cmd.get("description").filter(|v| py::truthy(v));
                let (Some(cmd_text), Some(_)) = (cmd_text, desc) else {
                    return Err(ViewError::Raises);
                };
                let Value::String(cmd_text) = cmd_text else {
                    return Err(ViewError::Unsure);
                };
                if cmd_text.contains('\n') {
                    return Err(ViewError::Raises);
                }
            }
        }
    }
    Ok(())
}

/// Las `clis` de un catálogo ya validado.
pub fn clis(catalog: &Value) -> Result<&Vec<Value>> {
    catalog
        .get("clis")
        .and_then(Value::as_array)
        .ok_or(ViewError::Unsure)
}

/// `cli["id"]`, `cli["binary"]` de un catálogo validado (textos).
pub fn cli_field<'a>(cli: &'a Value, key: &str) -> Result<&'a str> {
    text(cli.get(key))
}

/// `version_status(pinned, installed, verified)`.
pub fn version_status(pinned: &Value, installed: &Value, verified: bool) -> Result<&'static str> {
    if installed.is_null() {
        return Ok("missing");
    }
    if !verified {
        return Ok("unverified");
    }
    Ok(if py_str(installed)? == py_str(pinned)? {
        "ok"
    } else {
        "drift"
    })
}

static VERSION: LazyLock<Option<Regex>> = LazyLock::new(|| Regex::new(r"\d+\.\d+[\w.-]*").ok());

/// `_VERSION_RE.search(stdout + stderr)`: la versión o `"?"`.
pub fn version_of(output: &str) -> Result<String, Unsure> {
    if !regex_safe(output) {
        return Err(Unsure);
    }
    let rule = VERSION.as_ref().ok_or(Unsure)?;
    Ok(rule
        .find(output)
        .map_or_else(|| "?".to_owned(), |m| m.as_str().to_owned()))
}

// ---------------------------------------------------------------------------
// Binario nativo y comandos detectados
// ---------------------------------------------------------------------------

const ELF: &[u8; 4] = b"\x7fELF";
const MARKER: &[u8] = b"# COMANDOS_CODEX_ORIGINAL=";

/// Hasta `n` bytes desde el principio (`fh.read(n)`).
fn read_prefix(path: &Path, n: u64) -> io::Result<Vec<u8>> {
    let mut out = Vec::new();
    fs::File::open(path)?.take(n).read_to_end(&mut out)?;
    Ok(out)
}

/// `bytes.splitlines()`: `\n`, `\r` y `\r\n`.
fn bytes_lines(data: &[u8]) -> Vec<&[u8]> {
    let mut out = Vec::new();
    let mut start = 0;
    let mut i = 0;
    while let Some(&b) = data.get(i) {
        if b == b'\n' || b == b'\r' {
            out.push(data.get(start..i).unwrap_or_default());
            if b == b'\r' && data.get(i + 1) == Some(&b'\n') {
                i += 1;
            }
            start = i + 1;
        }
        i += 1;
    }
    if start < data.len() {
        out.push(data.get(start..).unwrap_or_default());
    }
    out
}

/// `json.loads(bytes)` del resto de la línea del marcador: `Ok(None)` es el
/// `ValueError`/`UnicodeDecodeError` que el Python captura.
fn marker_value(raw: &[u8]) -> Result<Option<Value>, Unsure> {
    // Con BOM o NUL, `json.detect_encoding` elige otra codificación.
    if raw.contains(&0) || raw.starts_with(b"\xef\xbb\xbf") {
        return Err(Unsure);
    }
    let Ok(text) = std::str::from_utf8(raw) else {
        return Ok(None);
    };
    match python_loads(text) {
        PythonLoads::Ok => parse_value(text).map(Some).map_err(|_| Unsure),
        PythonLoads::Error(_) => Ok(None),
        PythonLoads::Unsure => Err(Unsure),
    }
}

/// `native_binary(exe)`: la ruta del ejecutable real (enlaces resueltos, el
/// marcador `# COMANDOS_CODEX_ORIGINAL=<json>` de los lanzadores seguido sin
/// ciclos, y los envoltorios de node `../../<nombre>-*/vendor/*/bin/<nombre>`).
pub fn native_binary(exe: &Path) -> Result<Option<PathBuf>, Unsure> {
    native_binary_seen(exe, &mut HashSet::new())
}

fn native_binary_seen(exe: &Path, seen: &mut HashSet<PathBuf>) -> Result<Option<PathBuf>, Unsure> {
    if exe.as_os_str().is_empty() {
        return Ok(None);
    }
    // `os.path.realpath` no falla; si no resuelve, `open` sí (`OSError`).
    let Ok(real) = fs::canonicalize(exe) else {
        return Ok(None);
    };
    if !seen.insert(real.clone()) {
        return Ok(None);
    }
    let Ok(head) = read_prefix(&real, 4) else {
        return Ok(None);
    };
    if head.as_slice() == ELF.as_slice() {
        return Ok(Some(real));
    }
    let Ok(start) = read_prefix(&real, 8192) else {
        return Ok(None);
    };
    for line in bytes_lines(&start) {
        let Some(rest) = line.strip_prefix(MARKER) else {
            continue;
        };
        return match marker_value(rest)? {
            Some(Value::String(original)) => native_binary_seen(Path::new(&original), seen),
            _ => Ok(None),
        };
    }
    vendor_binary(&real)
}

fn has_glob_magic(name: &[u8]) -> bool {
    name.iter().any(|b| matches!(b, b'*' | b'?' | b'['))
}

/// Entradas no ocultas de `dir` (lo que `glob` ve con `*`); nombres que no son
/// UTF-8 (orden de `sorted` con sustitutos) → no se sabe.
fn visible_entries(dir: &Path) -> Result<Vec<String>, Unsure> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Ok(Vec::new());
    };
    let mut out = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name();
        if name.as_bytes().starts_with(b".") {
            continue;
        }
        out.push(name.into_string().map_err(|_| Unsure)?);
    }
    Ok(out)
}

/// El paso `glob` de `native_binary` para un archivo que no es ELF ni lleva
/// marcador: `sorted(glob(dirname(real)/../../<n>-*/vendor/*/bin/<n>))`, el
/// primero que empieza por `\x7fELF`.
fn vendor_binary(real: &Path) -> Result<Option<PathBuf>, Unsure> {
    let base_name = real.file_name().map(OsStr::as_bytes).unwrap_or_default();
    let name = base_name.split(|b| *b == b'.').next().unwrap_or_default();
    if has_glob_magic(name) {
        return Err(Unsure);
    }
    let name = std::str::from_utf8(name).map_err(|_| Unsure)?;
    let dir = real.parent().unwrap_or(real);
    // `dirname(real)/../..` con `real` ya canónico: dos padres físicos.
    let mut top = dir;
    for _ in 0..2 {
        top = top.parent().unwrap_or(top);
    }
    let prefix = format!("{name}-");
    let mut hits: Vec<(String, PathBuf)> = Vec::new();
    for entry in visible_entries(top)? {
        if !entry.starts_with(&prefix) {
            continue;
        }
        let vendor = top.join(&entry).join("vendor");
        if !vendor.is_dir() {
            continue;
        }
        for arch in visible_entries(&vendor)? {
            let candidate = vendor.join(&arch).join("bin").join(name);
            // `glob` pide `lexists` al último componente.
            if fs::symlink_metadata(&candidate).is_ok() {
                hits.push((format!("{entry}/vendor/{arch}/bin/{name}"), candidate));
            }
        }
    }
    hits.sort_by(|a, b| a.0.cmp(&b.0));
    for (_, candidate) in hits {
        if read_prefix(&candidate, 4).is_ok_and(|h| h.as_slice() == ELF.as_slice()) {
            return Ok(fs::canonicalize(&candidate).ok());
        }
    }
    Ok(None)
}

/// `_catalog_slash_names(cli)`: el primer token de cada comando que empieza
/// por `/`.
pub fn slash_names(cli: &Value) -> Result<HashSet<String>> {
    let mut out = HashSet::new();
    let Some(Value::Array(groups)) = cli.get("groups") else {
        return Err(ViewError::Unsure);
    };
    for group in groups {
        let Some(commands) = obj(group)?.get("commands") else {
            continue;
        };
        for cmd in commands.as_array().ok_or(ViewError::Unsure)? {
            let t = text(obj(cmd)?.get("text"))?;
            if t.starts_with('/') {
                let first = t.split(is_space).find(|w| !w.is_empty()).unwrap_or("");
                out.insert(first.to_owned());
            }
        }
    }
    Ok(out)
}

const CHUNK: usize = 1 << 20;

/// `{n for n in names if _name_in_binary(n, data)}` leyendo el archivo por
/// trozos (nunca entero en memoria): `/x` cuenta si aparece `/x` o `x`.
pub fn names_in_file(path: &Path, names: &HashSet<String>) -> io::Result<HashSet<String>> {
    let names: Vec<&String> = names.iter().collect();
    // Dos agujas por nombre: con barra y sin las barras iniciales.
    let needles: Vec<&str> = names
        .iter()
        .flat_map(|n| [n.as_str(), n.trim_start_matches('/')])
        .collect();
    let mut found: HashSet<String> = HashSet::new();
    let mut pending: Vec<usize> = Vec::new();
    for (i, needle) in needles.iter().enumerate() {
        if needle.is_empty() {
            // `b"" in data` siempre es verdad.
            if let Some(name) = names.get(i / 2) {
                found.insert((*name).clone());
            }
        } else {
            pending.push(i);
        }
    }
    if pending.is_empty() {
        return Ok(found);
    }
    let patterns: Vec<String> = pending
        .iter()
        .filter_map(|i| needles.get(*i))
        .map(|n| regex::escape(n))
        .collect();
    let set = RegexSet::new(&patterns).map_err(io::Error::other)?;
    let overlap = needles
        .iter()
        .map(|n| n.len())
        .max()
        .unwrap_or(1)
        .saturating_sub(1);
    let mut file = fs::File::open(path)?;
    let mut window: Vec<u8> = Vec::with_capacity(CHUNK + overlap);
    let mut buf = vec![0u8; CHUNK];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        window.extend_from_slice(buf.get(..n).unwrap_or_default());
        for hit in set.matches(&window).iter() {
            if let Some(name) = pending.get(hit).and_then(|i| names.get(i / 2)) {
                found.insert((*name).clone());
            }
        }
        let keep = window.len().saturating_sub(overlap);
        window.drain(..keep);
    }
    Ok(found)
}

// ---------------------------------------------------------------------------
// Modelos más nuevos (`model_watch.latest_models`)
// ---------------------------------------------------------------------------

const MODEL_CLIS: [&str; 3] = ["claude", "codex", "grok"];

static DATED_OR_BRACKET: LazyLock<Option<Regex>> = LazyLock::new(|| Regex::new(r"-\d{8}$|\[").ok());
static CLAUDE_FAMILY: LazyLock<Option<Regex>> =
    LazyLock::new(|| Regex::new(r"^claude-([a-z]+)-(\d+(?:-\d+)*)").ok());
static GPT_FAMILY: LazyLock<Option<Regex>> =
    LazyLock::new(|| Regex::new(r"^(gpt)-(\d+(?:\.\d+)?)").ok());
static GROK_FAMILY: LazyLock<Option<Regex>> =
    LazyLock::new(|| Regex::new(r"^(grok)-(\d+(?:\.\d+)?)").ok());

fn rule(cell: &'static LazyLock<Option<Regex>>) -> Result<&'static Regex, Unsure> {
    cell.as_ref().ok_or(Unsure)
}

/// `_family_ver(mid)` para un id ASCII que ya pasó el filtro de
/// `_DATED_OR_BRACKET`: sin fecha final ni `[`, `_norm` es solo `lower()`.
fn family_ver(mid: &str) -> Result<(String, Vec<u64>), Unsure> {
    let norm = mid.to_ascii_lowercase();
    let parse = |digits: &str, sep: char| -> Result<Vec<u64>, Unsure> {
        digits
            .split(sep)
            .map(|d| d.parse::<u64>().map_err(|_| Unsure))
            .collect()
    };
    if let Some(c) = rule(&CLAUDE_FAMILY)?.captures(&norm) {
        let fam = c.get(1).map_or("", |m| m.as_str()).to_owned();
        return Ok((fam, parse(c.get(2).map_or("", |m| m.as_str()), '-')?));
    }
    for family in [&GPT_FAMILY, &GROK_FAMILY] {
        if let Some(c) = rule(family)?.captures(&norm) {
            let fam = c.get(1).map_or("", |m| m.as_str()).to_owned();
            return Ok((fam, parse(c.get(2).map_or("", |m| m.as_str()), '.')?));
        }
    }
    Ok((norm, Vec::new()))
}

/// `set(x or [])` de un valor de JSON: los elementos (lista), los caracteres
/// (texto) o las claves (objeto); números y `true` no son iterables (500);
/// listas u objetos dentro no se pueden meter en un `set` (500).
fn py_set_items(value: Option<&Value>) -> Result<Vec<Value>> {
    let items = match value {
        None => Vec::new(),
        Some(v) if !py::truthy(v) => Vec::new(),
        Some(Value::Array(items)) => items.clone(),
        Some(Value::String(s)) => s.chars().map(|c| Value::from(c.to_string())).collect(),
        Some(Value::Object(map)) => map.keys().map(|k| Value::from(k.clone())).collect(),
        Some(_) => return Err(ViewError::Raises),
    };
    if items
        .iter()
        .any(|v| matches!(v, Value::Array(_) | Value::Object(_)))
    {
        return Err(ViewError::Raises);
    }
    Ok(items)
}

/// `latest_models(discovered, registry_ids)`: por CLI, el id más nuevo de
/// cada familia (versión descendente; empate, familia alfabética).
pub fn latest_models(
    discovered: Option<&Value>,
    registry_ids: &BTreeMap<String, Vec<String>>,
) -> Result<Map<String, Value>> {
    let discovered = match discovered {
        Some(v) if py::truthy(v) => Some(v.as_object().ok_or(ViewError::Raises)?),
        _ => None,
    };
    let mut out = Map::new();
    for cli in MODEL_CLIS {
        let mut ids: Vec<String> = Vec::new();
        let found = py_set_items(discovered.and_then(|d| d.get(cli)))?;
        let registered = registry_ids.get(cli).cloned().unwrap_or_default();
        for value in found.iter().chain(
            registered
                .iter()
                .map(|s| Value::from(s.clone()))
                .collect::<Vec<_>>()
                .iter(),
        ) {
            if let Value::String(s) = value
                && !ids.contains(s)
            {
                ids.push(s.clone());
            }
        }
        // (versión, -largo) y el id; gana el mayor, y en empate el menor id.
        let mut best: BTreeMap<String, ((Vec<u64>, i64), String)> = BTreeMap::new();
        for mid in ids {
            if !mid.is_ascii() {
                return Err(ViewError::Unsure);
            }
            if rule(&DATED_OR_BRACKET)?.is_match(&mid) {
                continue;
            }
            let (fam, ver) = family_ver(&mid)?;
            if ver.is_empty() {
                continue;
            }
            let len = i64::try_from(mid.len()).map_err(|_| ViewError::Unsure)?;
            let rank = (ver, -len);
            let better = match best.get(&fam) {
                None => true,
                Some((cur_rank, cur_mid)) => {
                    rank > *cur_rank || (rank == *cur_rank && mid < *cur_mid)
                }
            };
            if better {
                best.insert(fam, (rank, mid));
            }
        }
        // `sorted` por familia (ya en el `BTreeMap`), luego versión descendente estable.
        let mut ordered: Vec<(Vec<u64>, String)> = best
            .into_values()
            .map(|((ver, _), mid)| (ver, mid))
            .collect();
        ordered.sort_by(|a, b| b.0.cmp(&a.0));
        out.insert(
            cli.into(),
            Value::Array(ordered.into_iter().map(|(_, m)| Value::from(m)).collect()),
        );
    }
    Ok(out)
}

/// `list(x or [])` de Python sobre un valor de JSON (`newSince.<cli>.models`).
pub fn py_list(value: Option<&Value>) -> Result<Vec<Value>> {
    match value {
        None => Ok(Vec::new()),
        Some(v) if !py::truthy(v) => Ok(Vec::new()),
        Some(Value::Array(items)) => Ok(items.clone()),
        Some(Value::String(s)) => Ok(s.chars().map(|c| Value::from(c.to_string())).collect()),
        Some(Value::Object(map)) => Ok(map.keys().map(|k| Value::from(k.clone())).collect()),
        Some(_) => Err(ViewError::Raises),
    }
}

// ---------------------------------------------------------------------------
// Vista
// ---------------------------------------------------------------------------

/// Lo que la vista necesita además del catálogo (`catalog_view` del Python).
pub struct ViewInputs<'a> {
    /// `{cli: versión instalada}` (snapshot o sondeo).
    pub versions: &'a Map<String, Value>,
    /// `{cli: [entorno de cada cuenta que no es main]}`.
    pub accounts: &'a HashMap<String, Vec<Value>>,
    pub models: &'a Map<String, Value>,
    pub new_models: &'a Map<String, Value>,
    /// `None`: `detected_commands` lanzó (el catálogo va entero).
    pub detected: Option<&'a HashMap<String, Option<HashSet<String>>>>,
    pub helps: &'a HashMap<String, Map<String, Value>>,
}

fn pick(item: &Value) -> Value {
    json!({
        "text": item.get("text").cloned().unwrap_or(Value::Null),
        "description": item.get("description").cloned().unwrap_or(Value::Null),
        "args": item.get("args").cloned().unwrap_or_else(|| json!([])),
    })
}

fn item_text(item: &Value) -> &str {
    strip(item.get("text").and_then(Value::as_str).unwrap_or(""))
}

fn help_items(help: &Map<String, Value>) -> Vec<(&Value, &Value)> {
    let mut out = Vec::new();
    for sec in help
        .get("sections")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        for item in sec
            .get("items")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            out.push((sec, item));
        }
    }
    out
}

/// `_start_view(cli, help_, accounts)`.
fn start_view(
    cli_id: &str,
    binary: &str,
    help: Option<&Map<String, Value>>,
    accounts: &HashMap<String, Vec<Value>>,
) -> Result<Value> {
    let mut acc = Vec::new();
    for env in accounts.get(cli_id).into_iter().flatten() {
        let mut pairs: Vec<(&String, &Value)> = obj(env)?.iter().collect();
        pairs.sort_by(|a, b| a.0.cmp(b.0));
        let joined = pairs
            .iter()
            .map(|(k, v)| Ok(format!("{k}={}", py_str(v)?)))
            .collect::<Result<Vec<_>, Unsure>>()?
            .join(" ");
        if !joined.is_empty() {
            acc.push(json!({"text": format!("{joined} {binary}"), "description": "", "args": []}));
        }
    }
    let Some(help) = help else {
        let mut rows = vec![json!({"text": binary, "description": "", "args": []})];
        rows.extend(acc);
        return Ok(
            json!({"command": format!("{binary} --help"), "rows": rows, "yolo": [], "sections": []}),
        );
    };
    let items = help_items(help);
    let bare = items.iter().find(|(_, it)| item_text(it) == binary);
    let summary = help.get("summary").filter(|s| py::truthy(s));
    let description = match (summary, bare) {
        (Some(s), _) => s.clone(),
        (None, Some((_, it))) => it.get("description").cloned().unwrap_or_else(|| json!("")),
        (None, None) => json!(""),
    };
    let mut rows = vec![json!({"text": binary, "description": description, "args": []})];
    rows.extend(acc);
    let yolo: Vec<Value> = items
        .iter()
        .filter(|(_, it)| it.get("yolo").is_some_and(py::truthy))
        .map(|(_, it)| pick(it))
        .collect();
    let mut sections = Vec::new();
    for sec in help
        .get("sections")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let picked: Vec<Value> = sec
            .get("items")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter(|it| !it.get("yolo").is_some_and(py::truthy) && item_text(it) != binary)
            .map(pick)
            .collect();
        if !picked.is_empty() {
            sections.push(
                json!({"title": sec.get("title").cloned().unwrap_or(Value::Null), "items": picked}),
            );
        }
    }
    let mut shortcuts = Vec::new();
    if cli_id == "codex" {
        // `{text.strip(): item}`: el último gana.
        let mut by_text: HashMap<&str, &Value> = HashMap::new();
        for (_, it) in &items {
            by_text.insert(item_text(it), it);
        }
        let args_of = |key: &str| -> Vec<Value> {
            by_text
                .get(key)
                .and_then(|it| it.get("args"))
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default()
        };
        let has = |args: &[Value], v: &str| args.iter().any(|a| a.as_str() == Some(v));
        let sandbox = args_of(&format!("{binary} --sandbox"));
        let approvals = args_of(&format!("{binary} --ask-for-approval"));
        let resume = by_text.contains_key(format!("{binary} resume").as_str());
        if has(&sandbox, "danger-full-access") && has(&approvals, "on-request") {
            let flags = "--sandbox danger-full-access --ask-for-approval on-request";
            shortcuts.push(json!({"text": format!("{binary} {flags}"), "description": "Acceso completo: escritura y red. Permite aprobar solicitudes de herramientas.", "args": []}));
            if resume {
                shortcuts.push(json!({"text": format!("{binary} resume {flags}"), "description": "Retomar una sesión con escritura y red completas. Abre el selector de sesiones.", "args": []}));
            }
        }
        if resume
            && by_text.contains_key(
                format!("{binary} --dangerously-bypass-approvals-and-sandbox").as_str(),
            )
        {
            shortcuts.push(json!({"text": format!("{binary} resume --dangerously-bypass-approvals-and-sandbox"), "description": "Retomar con todos los permisos: escritura, red y ejecución sin sandbox ni aprobaciones.", "args": []}));
        }
    }
    let command = match help.get("command").filter(|c| py::truthy(c)) {
        Some(c) => c.clone(),
        None => json!(format!("{binary} --help")),
    };
    Ok(
        json!({"command": command, "rows": rows, "yolo": yolo, "sections": sections, "shortcuts": shortcuts}),
    )
}

/// `_command_view(cli_id, cmd, models, new_models)`.
fn command_view(cli_id: &str, cmd: &Map<String, Value>, inputs: &ViewInputs) -> Result<Value> {
    let args = match cmd.get("args") {
        None => Vec::new(),
        Some(Value::Array(a)) => a.clone(),
        Some(_) => return Err(ViewError::Unsure),
    };
    let mut out = Map::new();
    out.insert(
        "text".into(),
        cmd.get("text").cloned().unwrap_or(Value::Null),
    );
    out.insert(
        "description".into(),
        cmd.get("description").cloned().unwrap_or(Value::Null),
    );
    out.insert("args".into(), Value::Array(args));
    if cmd
        .get("argsFrom")
        .is_some_and(|v| v.as_str() == Some("models"))
    {
        let live = py_list(inputs.models.get(cli_id))?;
        if !live.is_empty() {
            out.insert("args".into(), Value::Array(live.clone()));
            let fresh = py_set_items(inputs.new_models.get(cli_id))?;
            let new: Vec<Value> = live
                .into_iter()
                .filter(|a| fresh.iter().any(|f| py::eq(a, f)))
                .collect();
            if !new.is_empty() {
                out.insert("newArgs".into(), Value::Array(new));
            }
        }
    }
    Ok(Value::Object(out))
}

/// `catalog_view(catalog, versions=…, accounts=…, models=…, new_models=…,
/// detected=…, helps=…)`.
pub fn catalog_view(catalog: &Value, inputs: &ViewInputs) -> Result<Value> {
    let mut out = Vec::new();
    for cli in clis(catalog)? {
        let id = cli_field(cli, "id")?;
        let binary = cli_field(cli, "binary")?;
        let cli_map = obj(cli)?;
        let installed = inputs.versions.get(id).cloned().unwrap_or(Value::Null);
        let found: Option<&HashSet<String>> = inputs
            .detected
            .and_then(|d| d.get(id))
            .and_then(Option::as_ref);
        let pinned = cli_map.get("pinnedVersion").cloned().unwrap_or(Value::Null);
        let verified = cli_map.get("verified").is_none_or(py::truthy);
        let mut groups = Vec::new();
        let Some(Value::Array(cli_groups)) = cli_map.get("groups") else {
            return Err(ViewError::Unsure);
        };
        for group in cli_groups {
            let group = obj(group)?;
            let title = group.get("title").cloned().ok_or(ViewError::Raises)?;
            let icon = group.get("icon").cloned().unwrap_or_else(|| json!(""));
            let mut commands = Vec::new();
            for cmd in group
                .get("commands")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                let cmd = obj(cmd)?;
                let t = text(cmd.get("text"))?;
                let keep = match found {
                    None => true,
                    Some(set) => {
                        !t.starts_with('/')
                            || set.contains(t.split(is_space).find(|w| !w.is_empty()).unwrap_or(""))
                    }
                };
                if keep {
                    commands.push(command_view(id, cmd, inputs)?);
                }
            }
            groups.push(json!({"title": title, "icon": icon, "commands": commands}));
        }
        let status = version_status(&pinned, &installed, verified)?;
        let mut view = Map::new();
        view.insert(
            "id".into(),
            cli_map.get("id").cloned().unwrap_or(Value::Null),
        );
        view.insert(
            "label".into(),
            cli_map.get("label").cloned().unwrap_or(Value::Null),
        );
        view.insert(
            "binary".into(),
            cli_map.get("binary").cloned().unwrap_or(Value::Null),
        );
        view.insert(
            "version".into(),
            json!({"pinned": pinned, "installed": installed, "status": status}),
        );
        view.insert(
            "start".into(),
            start_view(id, binary, inputs.helps.get(id), inputs.accounts)?,
        );
        view.insert("groups".into(), Value::Array(groups.clone()));
        if inputs.detected.is_some() {
            let total = i64::try_from(slash_names(cli)?.len()).map_err(|_| ViewError::Unsure)?;
            match found {
                Some(_) => {
                    let n: usize = groups
                        .iter()
                        .map(|g| {
                            g.get("commands")
                                .and_then(Value::as_array)
                                .map_or(0, Vec::len)
                        })
                        .sum();
                    let n = i64::try_from(n).map_err(|_| ViewError::Unsure)?;
                    view.insert("detected".into(), json!({"found": n, "total": total}));
                    view.insert("detectedMissing".into(), json!(total - n));
                }
                None => {
                    view.insert("detected".into(), Value::Null);
                    view.insert("detectedMissing".into(), json!(0));
                }
            }
        }
        out.push(Value::Object(view));
    }
    Ok(json!({"version": 1, "clis": out}))
}

// ---------------------------------------------------------------------------
// Modelos de OpenCode (`cc_usage.parse_opencode_models`)
// ---------------------------------------------------------------------------

/// Lo que `JSONDecoder.raw_decode(text[start:])` haría con el trozo.
enum RawDecode {
    /// Un valor y los bytes que ocupa.
    Value(Value, usize),
    /// `ValueError`: el Python avanza un carácter.
    Fail,
}

/// Un número entero con más dígitos de los que `int()` acepta (4300).
fn has_huge_int(value: &Value) -> bool {
    match value {
        Value::Number(n) => !py::is_float(n) && n.as_str().trim_start_matches('-').len() > 4300,
        Value::Array(items) => items.iter().any(has_huge_int),
        Value::Object(map) => map.values().any(has_huge_int),
        _ => false,
    }
}

/// `raw_decode` sobre un trozo que empieza por `{`.
fn raw_decode(slice: &str) -> Result<RawDecode, Unsure> {
    let mut stream = serde_json::Deserializer::from_str(slice).into_iter::<Value>();
    match stream.next() {
        Some(Ok(value)) => {
            if has_huge_int(&value) {
                return Err(Unsure);
            }
            Ok(RawDecode::Value(value, stream.byte_offset()))
        }
        Some(Err(error)) => {
            // Lo que `json` de Python acepta y `serde_json` no: `NaN`,
            // `Infinity`, sustitutos sueltos y anidamiento profundo.
            let lower = slice.to_ascii_lowercase();
            let surrogate = lower.match_indices("\\ud").any(|(i, _)| {
                lower
                    .as_bytes()
                    .get(i + 3)
                    .is_some_and(|b| (b'8'..=b'f').contains(b))
            });
            if surrogate
                || slice.contains("NaN")
                || slice.contains("Infinity")
                || error.to_string().contains("recursion limit")
            {
                return Err(Unsure);
            }
            Ok(RawDecode::Fail)
        }
        None => Ok(RawDecode::Fail),
    }
}

/// Una excepción del Python dentro de `_opencode_models_fetch`: la lista
/// entera queda vacía (`except Exception: return []`).
struct Raised;

fn get_or_empty(value: Option<&Value>) -> Result<Option<&Map<String, Value>>, Raised> {
    match value {
        Some(v) if py::truthy(v) => v.as_object().map(Some).ok_or(Raised),
        _ => Ok(None),
    }
}

/// `x.get("text", True)` de un diccionario opcional (vacío = `{}`).
fn text_flag(map: Option<&Map<String, Value>>) -> bool {
    map.and_then(|m| m.get("text")).is_none_or(py::truthy)
}

/// `_usable_for_coding(obj)`.
fn usable_for_coding(obj: &Map<String, Value>) -> Result<bool, Raised> {
    let Some(caps) = get_or_empty(obj.get("capabilities"))? else {
        return Ok(true); // opencode viejo sin capabilities
    };
    if !caps.get("toolcall").is_some_and(py::truthy) {
        return Ok(false);
    }
    if !text_flag(get_or_empty(caps.get("input"))?) {
        return Ok(false);
    }
    Ok(text_flag(get_or_empty(caps.get("output"))?))
}

/// `_as_int(value)`: `None`/`""` → 0; `ValueError`/`TypeError` → 0.
fn as_int(value: &Value) -> Result<Result<i64, Raised>, Unsure> {
    match value {
        Value::Null => Ok(Ok(0)),
        Value::String(s) if s.is_empty() => Ok(Ok(0)),
        Value::Array(_) | Value::Object(_) => Ok(Ok(0)),
        Value::Bool(b) => Ok(Ok(i64::from(*b))),
        Value::Number(n) => {
            let raw = n.as_str();
            match raw {
                "NaN" => Ok(Ok(0)),
                "Infinity" | "-Infinity" => Ok(Err(Raised)),
                _ if !py::is_float(n) => raw.parse::<i64>().map(Ok).map_err(|_| Unsure),
                _ => {
                    let x: f64 = raw.parse().map_err(|_| Unsure)?;
                    let t = x.trunc();
                    if (-(2f64.powi(63))..2f64.powi(63)).contains(&t) {
                        Ok(Ok(t as i64))
                    } else {
                        Err(Unsure)
                    }
                }
            }
        }
        Value::String(s) => match comandos_core::text::int(s) {
            Ok(v) => Ok(Ok(v)),
            Err(comandos_core::text::NumError::Invalid) => Ok(Ok(0)),
            Err(comandos_core::text::NumError::Exotic) => Err(Unsure),
        },
    }
}

/// Un modelo del bloque JSON, o `None` si el filtro lo deja fuera.
fn opencode_model(obj: &Map<String, Value>) -> Result<Result<Option<Value>, Raised>, Unsure> {
    let (Some(id), Some(provider)) = (
        obj.get("id").filter(|v| py::truthy(v)),
        obj.get("providerID").filter(|v| py::truthy(v)),
    ) else {
        return Ok(Ok(None));
    };
    match obj.get("status") {
        None | Some(Value::Null) => {}
        Some(Value::String(s)) if s == "active" => {}
        Some(_) => return Ok(Ok(None)),
    }
    match usable_for_coding(obj) {
        Ok(true) => {}
        Ok(false) => return Ok(Ok(None)),
        Err(raised) => return Ok(Err(raised)),
    }
    let mid = py_str(id)?;
    if mid.starts_with('~') {
        return Ok(Ok(None));
    }
    let limit = match get_or_empty(obj.get("limit")) {
        Ok(limit) => limit,
        Err(raised) => return Ok(Err(raised)),
    };
    let ctx = limit
        .and_then(|l| l.get("context"))
        .filter(|v| py::truthy(v))
        .cloned()
        .unwrap_or_else(|| json!(0));
    let context = match as_int(&ctx)? {
        Ok(n) => n,
        Err(raised) => return Ok(Err(raised)),
    };
    let provider = py_str(provider)?;
    let name_text = match obj.get("name") {
        Some(v) if !v.is_null() => py_str(v)?,
        _ => String::new(),
    };
    let name = match obj.get("name").filter(|v| py::truthy(v)) {
        Some(v) => py_str(v)?,
        None => mid.clone(),
    };
    let free = mid.to_lowercase().contains("free") || name_text.to_lowercase().contains("free");
    Ok(Ok(Some(json!({
        "provider": provider,
        "id": format!("{provider}/{mid}"),
        "name": name,
        "context": context,
        "free": free,
    }))))
}

/// `parse_opencode_models(verbose_output)` (40 por proveedor como mucho, si
/// no solo los gratis), ya con el `except Exception: return []` de quien lo
/// llama: una excepción del Python da la lista vacía.
pub fn parse_opencode_models(output: &str) -> Result<Vec<Value>, Unsure> {
    const MAX_PER_PROVIDER: usize = 40;
    let mut models = Vec::new();
    let mut i = 0;
    while let Some(rel) = output.get(i..).and_then(|rest| rest.find('{')) {
        let start = i + rel;
        let slice = output.get(start..).unwrap_or("");
        match raw_decode(slice)? {
            RawDecode::Fail => {
                i = start + 1;
                continue;
            }
            RawDecode::Value(value, used) => {
                i = start + used;
                let Value::Object(obj) = value else {
                    continue;
                };
                match opencode_model(&obj)? {
                    Ok(Some(model)) => models.push(model),
                    Ok(None) => {}
                    Err(Raised) => return Ok(Vec::new()),
                }
            }
        }
    }
    let mut by_provider: Vec<(String, Vec<Value>)> = Vec::new();
    for model in models {
        let provider = model
            .get("provider")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned();
        match by_provider.iter_mut().find(|(p, _)| *p == provider) {
            Some((_, items)) => items.push(model),
            None => by_provider.push((provider, vec![model])),
        }
    }
    let mut result = Vec::new();
    for (provider, mut items) in by_provider {
        if items.len() > MAX_PER_PROVIDER {
            items.retain(|m| m.get("free").and_then(Value::as_bool) == Some(true));
        }
        if !items.is_empty() {
            result.push((provider, items));
        }
    }
    result.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(result
        .into_iter()
        .map(|(provider, items)| json!({"provider": provider, "models": items}))
        .collect())
}

#[cfg(test)]
mod opencode_tests {
    use super::*;

    #[test]
    fn raw_decode_stops_after_the_object() {
        let Ok(RawDecode::Value(v, used)) = raw_decode("{\"a\": 1}MODELOS {") else {
            panic!("no decodificó");
        };
        assert_eq!(v, json!({"a": 1}));
        assert_eq!(used, 8);
        assert!(matches!(
            raw_decode("{\"a\": \n{\"b\": 2}"),
            Ok(RawDecode::Fail)
        ));
        assert!(raw_decode("{\"a\": NaN}").is_err());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bytes_lines_like_python() {
        assert_eq!(
            bytes_lines(b"a\r\nb\rc\n\nd"),
            vec![&b"a"[..], b"b", b"c", b"", b"d"]
        );
        assert!(bytes_lines(b"").is_empty());
    }

    #[test]
    fn version_regex() {
        assert_eq!(version_of("codex-cli 0.159.2\n").unwrap(), "0.159.2");
        assert_eq!(version_of("nada").unwrap(), "?");
        assert_eq!(version_of("v2.1.286 (Claude Code)").unwrap(), "2.1.286");
        assert!(version_of("x\u{1d}1.2").is_err());
    }
}
