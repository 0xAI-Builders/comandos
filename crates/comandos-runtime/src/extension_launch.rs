//! `lib/extension_launch.py`, el subconjunto que usan los cambios de
//! configuración de un pane: envolver el comando con el lanzador privado de
//! extensiones (`wrap_command`, `wrap_environment`), capturar el entorno de
//! OpenCode, reconstruir y verificar un lanzamiento ya en marcha
//! (`launch_from_pid`, `verify_launch`, `configuration_status`), normalizar una
//! selección y leer la configuración TOML (`_read`, `_parse_toml`).
//!
//! El lanzador sigue siendo el Python (`bin/cc-extension-session`, O5): este
//! módulo construye la misma orden con la misma ruta, nunca lo ejecuta.
//!
//! Errores: `Value(texto)` es un `ValueError` del Python con su texto; `Other`
//! cualquier otra excepción (`OSError`, `KeyError`, `TypeError`,
//! `AttributeError`); `Unsure`, lo que el port no reproduce con certeza.
//!
//! El TOML se lee como lo hace el heredado en esta máquina: `python3` es 3.10
//! (sin `tomllib`), así que `_parse_toml` lo pasa por `python3.11 -I` y
//! `json.dumps`. Por eso un documento con fechas falla (no son JSON) y uno con
//! sintaxis de TOML 1.1 (que `toml_edit` acepta y `tomllib` no) también. La
//! herencia de confianza de Codex usa `parse_trust_toml`, que sí acepta
//! fechas (desviación deliberada: ver su comentario). Un número que
//! `toml_edit` no representa (`1e400`, `0x8000000000000000`) es `Unsure`.
use crate::Unsure;
use comandos_core::json::{PythonLoads, python_eq, python_loads, response_dumps, truthy};
use comandos_core::text::{shlex_quote, splitlines};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    ffi::OsStr,
    fs, io,
    io::Write,
    os::unix::{
        ffi::OsStrExt,
        fs::{DirBuilderExt, MetadataExt, OpenOptionsExt},
    },
    path::{Path, PathBuf},
};
use toml_edit::{DocumentMut, Item, Value as Tv};
use toml_parser::{Source, lexer::TokenKind};

pub const MARKER: &str = "COMANDOS_EXTENSION_OPERATION_ID";
pub const MANIFEST_ENV: &str = "COMANDOS_EXTENSION_MANIFEST";
pub const DIGEST_ENV: &str = "COMANDOS_EXTENSION_MANIFEST_SHA256";
pub const RECEIPT_ENV: &str = "COMANDOS_EXTENSION_RECEIPT";
pub const RECEIPT_HASH_ENV: &str = "COMANDOS_EXTENSION_RECEIPT_SHA256";
pub const OPENCODE_ENV_KEYS: [&str; 4] = [
    "OPENCODE_CONFIG_CONTENT",
    "OPENCODE_PERMISSION",
    "OPENCODE_CONFIG",
    "OPENCODE_CONFIG_DIR",
];

/// Lo que lanza una función del módulo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LaunchError {
    Value(String),
    Other,
    Unsure,
}

impl From<Unsure> for LaunchError {
    fn from(_: Unsure) -> Self {
        Self::Unsure
    }
}

/// Excepción interna: `Caught` es la que `verify_launch` y `launch_from_pid`
/// tragan (`OSError`, `KeyError`, `TypeError`); `Attr`, la que no
/// (`AttributeError`).
#[derive(Debug)]
enum Fail {
    Value(String),
    Caught,
    Attr,
    Unsure,
}

impl From<Unsure> for Fail {
    fn from(_: Unsure) -> Self {
        Self::Unsure
    }
}

impl From<io::Error> for Fail {
    fn from(_: io::Error) -> Self {
        Self::Caught
    }
}

impl From<Fail> for LaunchError {
    fn from(fail: Fail) -> Self {
        match fail {
            Fail::Value(text) => Self::Value(text),
            Fail::Caught | Fail::Attr => Self::Other,
            Fail::Unsure => Self::Unsure,
        }
    }
}

/// `HELPER`: `<repo>/bin/cc-extension-session` (con `repo_root` resuelto).
pub fn helper(repo_root: &Path) -> PathBuf {
    repo_root.join("bin/cc-extension-session")
}

// ------------------------------------------------------------------ texto

/// `bytes.decode()` estricto con los saltos universales de `read_text()`.
pub(crate) fn universal_text(bytes: Vec<u8>) -> Option<String> {
    let text = String::from_utf8(bytes).ok()?;
    Some(text.replace("\r\n", "\n").replace('\r', "\n"))
}

/// `shlex.split(s)` (POSIX, sin comentarios). `Err` = el `ValueError` con su
/// texto.
pub fn shlex_split(s: &str) -> Result<Vec<String>, String> {
    #[derive(Clone, Copy, PartialEq)]
    enum State {
        Space,
        Word,
        Single,
        Double,
    }
    let is_space = |c: char| matches!(c, ' ' | '\t' | '\r' | '\n');
    let mut out = Vec::new();
    let mut token = String::new();
    let mut quoted = false;
    let mut state = State::Space;
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        match state {
            State::Space | State::Word => {
                if is_space(c) {
                    if state == State::Word {
                        out.push(std::mem::take(&mut token));
                        quoted = false;
                        state = State::Space;
                    }
                } else if c == '\\' {
                    let next = chars.next().ok_or("No escaped character")?;
                    token.push(next);
                    state = State::Word;
                } else if c == '\'' {
                    quoted = true;
                    state = State::Single;
                } else if c == '"' {
                    quoted = true;
                    state = State::Double;
                } else {
                    token.push(c);
                    state = State::Word;
                }
            }
            State::Single => {
                if c == '\'' {
                    state = State::Word;
                } else {
                    token.push(c);
                }
            }
            State::Double => {
                if c == '"' {
                    state = State::Word;
                } else if c == '\\' {
                    let next = chars.next().ok_or("No escaped character")?;
                    if next != '"' && next != '\\' {
                        token.push('\\');
                    }
                    token.push(next);
                } else {
                    token.push(c);
                }
            }
        }
    }
    match state {
        State::Single | State::Double => Err("No closing quotation".into()),
        State::Word => {
            out.push(token);
            Ok(out)
        }
        State::Space => {
            if quoted {
                out.push(token);
            }
            Ok(out)
        }
    }
}

fn shlex_join<S: AsRef<str>>(words: &[S]) -> String {
    words
        .iter()
        .map(|w| shlex_quote(w.as_ref()))
        .collect::<Vec<_>>()
        .join(" ")
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// `json.loads(text)`: `Ok(None)` = `JSONDecodeError` (con su texto en
/// `Err` de la segunda capa cuando hace falta).
fn py_json(text: &str) -> Result<Result<Value, String>, Unsure> {
    match python_loads(text) {
        PythonLoads::Ok => comandos_core::json::workspace_loads(text)
            .map(Ok)
            .map_err(|_| Unsure),
        PythonLoads::Error(message) => Ok(Err(message)),
        PythonLoads::Unsure => Err(Unsure),
    }
}

/// `json.loads(raw_bytes)`: solo UTF-8 sin BOM ni nulos al principio (la
/// detección de codificación del Python no se reproduce).
fn py_json_bytes(raw: &[u8]) -> Result<Result<Value, String>, Unsure> {
    if raw.starts_with(&[0xef, 0xbb, 0xbf]) || raw.iter().take(4).any(|b| *b == 0) {
        return Err(Unsure);
    }
    match std::str::from_utf8(raw) {
        Ok(text) => py_json(text),
        // `UnicodeDecodeError` (un `ValueError`).
        Err(_) => Ok(Err(String::new())),
    }
}

// ------------------------------------------------------------------ TOML

/// ¿Usa el texto sintaxis de TOML 1.1 (que `toml_edit` acepta y el `tomllib`
/// de 3.11 rechaza)? Saltos o comentarios dentro de una tabla en línea, coma
/// final en una tabla en línea y los escapes `\e`/`\x`.
fn toml_1_1(text: &str) -> bool {
    let source = Source::new(text);
    let tokens = source.lex().into_vec();
    let mut stack = Vec::new();
    let mut after_comma = false;
    for token in &tokens {
        let kind = token.kind();
        let raw = source.get(token).map(|r| r.as_str()).unwrap_or("");
        let in_curly = stack.last() == Some(&TokenKind::LeftCurlyBracket);
        match kind {
            TokenKind::LeftCurlyBracket | TokenKind::LeftSquareBracket => stack.push(kind),
            TokenKind::RightCurlyBracket => {
                if in_curly && after_comma {
                    return true;
                }
                stack.pop();
            }
            TokenKind::RightSquareBracket => {
                stack.pop();
            }
            TokenKind::Newline | TokenKind::Comment if in_curly => return true,
            TokenKind::BasicString | TokenKind::MlBasicString => {
                let mut chars = raw.chars();
                while let Some(c) = chars.next() {
                    if c == '\\' && matches!(chars.next(), Some('e' | 'x')) {
                        return true;
                    }
                }
            }
            _ => {}
        }
        match kind {
            TokenKind::Comma => after_comma = true,
            TokenKind::Whitespace | TokenKind::Newline | TokenKind::Comment => {}
            _ => after_comma = false,
        }
    }
    false
}

/// Un literal numérico que `toml_edit` no representa y `tomllib` sí: enteros
/// fuera de `i64` (decimales largos o `0x`/`0o`/`0b`) y flotantes que se van
/// al infinito (`1e400`). Si `toml_edit` falla con uno así, el veredicto del
/// Python es incierto.
fn risky_number(text: &str) -> bool {
    let source = Source::new(text);
    source.lex().into_vec().iter().any(|token| {
        if token.kind() != TokenKind::Atom {
            return false;
        }
        let Some(raw) = source.get(token) else {
            return false;
        };
        let raw = raw.as_str().trim_start_matches(['+', '-']);
        if !raw.starts_with(|c: char| c.is_ascii_digit()) {
            return false;
        }
        raw.starts_with("0x")
            || raw.starts_with("0o")
            || raw.starts_with("0b")
            || raw.contains(['e', 'E'])
            || raw.bytes().filter(u8::is_ascii_digit).count() >= 19
    })
}

/// Qué hacer con una fecha.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Dates {
    /// `json.dumps` del resultado lanza: el `_parse_toml` del Python 3.10.
    Fail,
    /// Un valor opaco verdadero (su texto): el lector de la confianza.
    Opaque,
}

/// Un valor de `toml_edit` como el JSON que devuelve `json.dumps(tomllib…)`;
/// `None` si el Python lanzaría.
fn toml_value(value: &Tv, dates: Dates) -> Option<Value> {
    Some(match value {
        Tv::String(s) => json!(s.value()),
        Tv::Integer(i) => json!(i.value()),
        Tv::Float(f) => {
            let n = *f.value();
            if n.is_finite() {
                json!(n)
            } else {
                // `json.dumps` escribe `NaN`/`Infinity`; `json.loads` los lee.
                let text = if n.is_nan() {
                    "NaN"
                } else if n.is_sign_positive() {
                    "Infinity"
                } else {
                    "-Infinity"
                };
                Value::Number(serde_json::Number::from_string_unchecked(text.into()))
            }
        }
        Tv::Boolean(b) => json!(b.value()),
        Tv::Datetime(dt) => {
            // Una hora sin segundos es TOML 1.1: `tomllib` 3.11 la rechaza.
            if dates == Dates::Fail || dt.value().time.is_some_and(|t| t.second.is_none()) {
                return None;
            }
            json!(dt.value().to_string())
        }
        Tv::Array(items) => Value::Array(
            items
                .iter()
                .map(|v| toml_value(v, dates))
                .collect::<Option<_>>()?,
        ),
        Tv::InlineTable(table) => {
            let mut map = Map::new();
            for (key, value) in table.iter() {
                map.insert(key.to_owned(), toml_value(value, dates)?);
            }
            Value::Object(map)
        }
    })
}

fn toml_item(item: &Item, dates: Dates) -> Option<Value> {
    match item {
        Item::None => None,
        Item::Value(value) => toml_value(value, dates),
        Item::Table(table) => {
            let mut map = Map::new();
            for (key, value) in table.iter() {
                map.insert(key.to_owned(), toml_item(value, dates)?);
            }
            Some(Value::Object(map))
        }
        Item::ArrayOfTables(tables) => Some(Value::Array(
            tables
                .iter()
                .map(|t| toml_item(&Item::Table(t.clone()), dates))
                .collect::<Option<_>>()?,
        )),
    }
}

fn parse_with(text: &str, dates: Dates) -> Result<Option<Value>, Unsure> {
    if text.starts_with('\u{feff}') || toml_1_1(text) {
        return Ok(None);
    }
    match text.parse::<DocumentMut>() {
        Ok(doc) => Ok(toml_item(doc.as_item(), dates)),
        Err(_) if risky_number(text) => Err(Unsure),
        Err(_) => Ok(None),
    }
}

/// `_parse_toml(text)`: `Ok(None)` = el `ValueError` del Python.
pub fn parse_toml(text: &str) -> Result<Option<Value>, Unsure> {
    parse_with(text, Dates::Fail)
}

/// El TOML de la herencia de confianza de Codex. Como `parse_toml`, pero las
/// fechas son valores opacos verdaderos (su texto): desviación deliberada, el
/// Python solo falla con ellas por pasar el resultado por JSON, y entonces un
/// `config.toml` con una fecha nunca heredaba la confianza (el cambio de
/// cuenta se quedaba en el diálogo). Lo demás que `tomllib` 3.11 rechaza
/// (sintaxis 1.1, BOM, hora sin segundos) se sigue rechazando.
pub(crate) fn parse_trust_toml(text: &str) -> Result<Option<Value>, Unsure> {
    parse_with(text, Dates::Opaque)
}

/// `_read(path)` de un `.toml` con el lector dado: `Ok(None)` =
/// `ValueError('Configuración ilegible…')`.
fn read_toml_with(path: &Path, dates: Dates) -> Result<Option<Value>, Unsure> {
    let meta = match fs::metadata(path) {
        Ok(meta) => meta,
        // `path.exists()` es falso con estos errores.
        Err(e)
            if e.kind() == io::ErrorKind::NotFound
                || e.kind() == io::ErrorKind::NotADirectory
                || e.raw_os_error() == Some(40) =>
        {
            return Ok(Some(json!({})));
        }
        Err(_) => return Ok(None),
    };
    if meta.len() > 4_000_000 {
        return Ok(None);
    }
    let Ok(bytes) = fs::read(path) else {
        return Ok(None);
    };
    let Some(text) = universal_text(bytes) else {
        return Ok(None);
    };
    parse_with(&text, dates)
}

/// `_read(path)` para la confianza de Codex (`parse_trust_toml`).
pub(crate) fn read_trust_toml(path: &Path) -> Result<Option<Value>, Unsure> {
    read_toml_with(path, Dates::Opaque)
}

// ---------------------------------------------------------------- manifest

fn uid() -> u32 {
    nix::unistd::getuid().as_raw()
}

fn path_of(value: &Value) -> Result<PathBuf, Fail> {
    value.as_str().map(PathBuf::from).ok_or(Fail::Caught)
}

fn private_mode(meta: &fs::Metadata) -> bool {
    meta.uid() == uid() && meta.mode() & 0o077 == 0
}

fn is_link(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink())
}

/// `Path(a).parent == Path(b).parent`.
fn same_parent(a: &Path, b: &Path) -> bool {
    let parent = |p: &Path| {
        let parent = p.parent().map(Path::to_path_buf).unwrap_or_default();
        if parent.as_os_str().is_empty() && !p.is_absolute() {
            PathBuf::from(".")
        } else {
            parent
        }
    };
    parent(a).components().eq(parent(b).components())
}

/// `_load_manifest(path, bundle)`.
fn load_manifest(path: &Path, bundle: Option<&Value>) -> Result<Value, Fail> {
    let meta = fs::metadata(path)?;
    if is_link(path) || !private_mode(&meta) {
        return Err(Fail::Value("Manifest privado inválido.".into()));
    }
    let text = universal_text(fs::read(path)?).ok_or(Fail::Unsure)?;
    let data = py_json(&text)?.map_err(Fail::Value)?;
    let Value::Object(map) = &data else {
        return Err(Fail::Attr);
    };
    let version_ok = map.get("version").is_some_and(|v| python_eq(v, &json!(1)));
    let bundle_ok = bundle.is_none_or(|b| map.get("bundle").is_some_and(|v| python_eq(v, b)));
    if !version_ok || !bundle_ok {
        return Err(Fail::Value(
            "Manifest incompatible con el lanzamiento.".into(),
        ));
    }
    let parent = path.parent().ok_or(Fail::Unsure)?;
    if !private_mode(&fs::metadata(parent)?) {
        return Err(Fail::Value("Directorio del manifest no es privado.".into()));
    }
    let artifacts = match map.get("artifacts") {
        None => Map::new(),
        Some(Value::Object(artifacts)) => artifacts.clone(),
        Some(_) => return Err(Fail::Attr),
    };
    for (name, digest) in &artifacts {
        let artifact = Path::new(name);
        let bad = !same_parent(artifact, path)
            || is_link(artifact)
            || fs::metadata(artifact)?.mode() & 0o077 != 0;
        if bad {
            return Err(Fail::Value("Artefacto privado inválido.".into()));
        }
        if !python_eq(&json!(sha256_hex(&fs::read(artifact)?)), digest) {
            return Err(Fail::Value(
                "El artefacto de lanzamiento cambió después de prepararse.".into(),
            ));
        }
    }
    Ok(data)
}

/// `re.fullmatch(r'[A-Za-z_][A-Za-z0-9_]*=.*', word, re.S)`.
fn assignment(word: &str) -> Option<(&str, &str)> {
    let (key, value) = word.split_once('=')?;
    let mut chars = key.chars();
    let first = chars.next()?;
    let ok = (first.is_ascii_alphabetic() || first == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_');
    ok.then_some((key, value))
}

/// `_resolve_command(data, command)` solo por sus excepciones: `wrap_command`
/// descarta lo que devuelve.
fn resolve_command(
    data: &Map<String, Value>,
    words: &[String],
    environ: &HashMap<String, String>,
) -> Result<(), Fail> {
    let mut command: Vec<String> = words.to_vec();
    let mut env = environ.clone();
    if command.first().is_some_and(|w| w == "env") {
        command.remove(0);
        while command.first().is_some_and(|w| w.starts_with('-')) {
            let flag = command.remove(0);
            if matches!(flag.as_str(), "-u" | "--unset") && !command.is_empty() {
                env.remove(&command.remove(0));
            } else if let Some(name) = flag.strip_prefix("--unset=") {
                env.remove(name);
            } else if flag == "--" {
                break;
            } else {
                return Err(Fail::Value(
                    "Opción env no soportada por el lanzamiento aislado.".into(),
                ));
            }
        }
    }
    while let Some((key, value)) = command.first().and_then(|w| assignment(w)) {
        env.insert(key.to_owned(), value.to_owned());
        command.remove(0);
    }
    if command.is_empty() {
        return Err(Fail::Value(
            "El comando intenta cambiar HOME o está vacío.".into(),
        ));
    }
    let home = data.get("home").ok_or(Fail::Caught)?;
    let ours = env.get("HOME").map_or(Value::Null, |h| json!(h));
    if !python_eq(&ours, home) {
        return Err(Fail::Value(
            "El comando intenta cambiar HOME o está vacío.".into(),
        ));
    }
    let extra: Vec<Value> = match data.get("args").ok_or(Fail::Caught)? {
        Value::Array(items) => items.clone(),
        _ => return Err(Fail::Unsure),
    };
    let harness = match data.get("bundle").ok_or(Fail::Caught)? {
        Value::Object(bundle) => bundle.get("harness").ok_or(Fail::Caught)?.clone(),
        _ => return Err(Fail::Unsure),
    };
    let env_spec = data.get("env").ok_or(Fail::Caught)?;
    if harness == json!("opencode") {
        resolve_opencode(&env, env_spec)?;
    } else if !env_spec.is_object() {
        return Err(Fail::Unsure);
    }
    if harness == json!("codex") {
        resolve_codex(data, &command, &extra)?;
    }
    if harness == json!("claude") {
        resolve_claude(&command, &extra)?;
    }
    Ok(())
}

fn resolve_opencode(env: &HashMap<String, String>, env_spec: &Value) -> Result<(), Fail> {
    let invalid = || Fail::Value("Overlay OpenCode inválido.".into());
    let content = env
        .get("OPENCODE_CONFIG_CONTENT")
        .map_or("{}", String::as_str);
    let prior = py_json(content)?.map_err(|_| invalid())?;
    if !prior.is_object() {
        return Err(Fail::Attr);
    }
    let selected = match env_spec {
        Value::Object(map) => match map.get("OPENCODE_CONFIG_CONTENT") {
            Some(Value::String(text)) => py_json(text)?.map_err(|_| invalid())?,
            Some(_) => return Err(invalid()),
            None => return Err(Fail::Caught),
        },
        // `data['env'][…]` sobre una lista o una cadena: `TypeError`.
        Value::Array(_) | Value::String(_) | Value::Null | Value::Bool(_) | Value::Number(_) => {
            return Err(invalid());
        }
    };
    if !selected.is_object() {
        return Err(Fail::Attr);
    }
    Ok(())
}

fn resolve_codex(
    data: &Map<String, Value>,
    command: &[String],
    extra: &[Value],
) -> Result<(), Fail> {
    let mut old = Value::Array(Vec::new());
    for (index, word) in command.iter().enumerate() {
        let value = if matches!(word.as_str(), "-c" | "--config") && index + 1 < command.len() {
            command.get(index + 1).map_or("", String::as_str)
        } else {
            word.strip_prefix("--config=").unwrap_or("")
        };
        if value.starts_with("skills.config=") {
            let invalid = || Fail::Value("Override previo de skills inválido.".into());
            let parsed = parse_toml(value)?.ok_or_else(invalid)?;
            old = parsed
                .get("skills")
                .and_then(|s| s.get("config"))
                .cloned()
                .ok_or_else(invalid)?;
        }
    }
    if !truthy(&old) {
        return Ok(());
    }
    for word in extra {
        let Value::String(word) = word else {
            return Err(Fail::Attr);
        };
        if !word.starts_with("skills.config=") {
            continue;
        }
        let parsed = parse_toml(word)?
            .ok_or_else(|| Fail::Value("Configuración TOML no soportada.".into()))?;
        let rules = parsed
            .get("skills")
            .and_then(|s| s.get("config"))
            .ok_or(Fail::Caught)?;
        let count = data.get("codexSelectedSkillCount").ok_or(Fail::Caught)?;
        if count.as_u64().is_none() {
            return Err(Fail::Unsure);
        }
        // `before + old + selected`: listas, o `TypeError`.
        if !rules.is_array() || !old.is_array() {
            return Err(Fail::Caught);
        }
    }
    Ok(())
}

fn resolve_claude(command: &[String], extra: &[Value]) -> Result<(), Fail> {
    let unreadable = || Fail::Value("Settings de Claude ilegibles.".into());
    let mut index = 0;
    while let Some(word) = command.get(index) {
        index += 1;
        if word == "--settings" || word.starts_with("--settings=") {
            let value = if word == "--settings" {
                let value = command
                    .get(index)
                    .ok_or_else(|| Fail::Value("Settings de Claude incompletos.".into()))?;
                index += 1;
                value.clone()
            } else {
                word.split_once('=').map_or("", |(_, v)| v).to_owned()
            };
            let text = if strip_start(&value).starts_with('{') {
                value
            } else {
                let bytes = fs::read(Path::new(if value.is_empty() { "." } else { &value }))
                    .map_err(|_| unreadable())?;
                universal_text(bytes).ok_or_else(unreadable)?
            };
            let old = py_json(&text)?.map_err(|_| unreadable())?;
            if !old.is_object() {
                // `_deep_merge(…, old)`: `old.items()` fuera del `except`.
                return Err(Fail::Attr);
            }
        } else if word == "--mcp-config" {
            while command.get(index).is_some_and(|w| !w.starts_with('-')) {
                index += 1;
            }
        }
    }
    let Some(at) = extra
        .iter()
        .position(|w| python_eq(w, &json!("--settings")))
    else {
        return Err(Fail::Value("'--settings' is not in list".into()));
    };
    let path = path_of(extra.get(at + 1).ok_or(Fail::Caught)?)?;
    let text = universal_text(fs::read(&path)?).ok_or(Fail::Unsure)?;
    let selected = py_json(&text)?.map_err(Fail::Value)?;
    if !selected.is_object() {
        return Err(Fail::Attr);
    }
    Ok(())
}

/// `s.lstrip()` de Python.
fn strip_start(s: &str) -> &str {
    s.trim_start_matches(comandos_core::text::is_space)
}

/// `wrap_command(command, launch)`: envuelve un comando simple, conservando
/// su argv y su prefijo de entorno. `environ` es `os.environ` del proceso.
pub fn wrap_command(
    command: &str,
    launch: &Value,
    helper: &Path,
    environ: &HashMap<String, String>,
) -> Result<String, LaunchError> {
    let words = shlex_split(command).map_err(LaunchError::Value)?;
    let compound = words
        .iter()
        .any(|w| matches!(w.as_str(), ";" | "&&" | "||" | "|" | ">" | "<" | "&"));
    if words.is_empty() || compound {
        return Err(LaunchError::Value(
            "Se requiere un comando simple para aplicar extensiones.".into(),
        ));
    }
    let Value::Object(bundle) = launch else {
        return Err(LaunchError::Other);
    };
    let manifest = bundle.get("manifest").ok_or(LaunchError::Other)?;
    let manifest = manifest.as_str().ok_or(LaunchError::Other)?;
    let data = load_manifest(Path::new(manifest), Some(launch))?;
    let Value::Object(data) = data else {
        return Err(LaunchError::Other);
    };
    resolve_command(&data, &words, environ)?;
    let helper = helper.to_str().ok_or(LaunchError::Unsure)?;
    let mut argv = vec![
        helper.to_owned(),
        "--manifest".into(),
        manifest.to_owned(),
        "--".into(),
    ];
    argv.extend(words);
    Ok(shlex_join(&argv))
}

/// `wrap_environment(command, reference)`.
pub fn wrap_environment(
    command: &str,
    reference: &Value,
    helper: &Path,
) -> Result<String, LaunchError> {
    let field = |key: &str| {
        reference
            .get(key)
            .and_then(Value::as_str)
            .map(str::to_owned)
            .ok_or(LaunchError::Other)
    };
    let helper = helper.to_str().ok_or(LaunchError::Unsure)?;
    let mut argv = vec![
        helper.to_owned(),
        "--environment-file".into(),
        field("path")?,
        "--environment-sha256".into(),
        field("sha256")?,
        "--".into(),
    ];
    argv.extend(shlex_split(command).map_err(LaunchError::Value)?);
    Ok(shlex_join(&argv))
}

/// `uuid.uuid4().hex`.
fn uuid4_hex() -> Result<String, Unsure> {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).map_err(|_| Unsure)?;
    if let Some(b) = bytes.get_mut(6) {
        *b = (*b & 0x0f) | 0x40;
    }
    if let Some(b) = bytes.get_mut(8) {
        *b = (*b & 0x3f) | 0x80;
    }
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

/// `_private(path, data)`: creación exclusiva 0600, escritura y `fsync`.
fn write_private(path: &Path, raw: &[u8]) -> io::Result<()> {
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)?;
    file.write_all(raw)?;
    file.flush()?;
    file.sync_all()
}

/// `capture_opencode_environment(source, runtime_dir, inventory, managed)`:
/// guarda solo los overrides admitidos de OpenCode en
/// `<runtime_dir>/environment-<uuid>.json` (0600) y devuelve `{path, sha256}`.
/// `source` es el entorno del proceso (bytes, como `/proc/<pid>/environ`).
pub fn capture_opencode_environment(
    source: &HashMap<Vec<u8>, Vec<u8>>,
    runtime_dir: &Path,
    inventory: &Value,
    managed: bool,
) -> Result<Value, LaunchError> {
    let mut values = Map::new();
    for key in OPENCODE_ENV_KEYS {
        if let Some(raw) = source.get(key.as_bytes()) {
            // `UnicodeDecodeError` sale con su propio texto.
            let text = String::from_utf8(raw.clone()).map_err(|_| LaunchError::Unsure)?;
            values.insert(key.into(), json!(text));
        }
    }
    let set = |key: &str| values.get(key).is_some_and(truthy);
    if set("OPENCODE_CONFIG") || set("OPENCODE_CONFIG_DIR") {
        return Err(LaunchError::Value(
            "OpenCode usa un catálogo alternativo no compatible; el agente sigue abierto".into(),
        ));
    }
    opencode_overrides(&values, inventory, managed)?;
    let directory = runtime_dir;
    if !directory.is_dir() {
        if let Some(parent) = directory.parent() {
            fs::create_dir_all(parent).map_err(|_| LaunchError::Other)?;
        }
        match fs::DirBuilder::new().mode(0o700).create(directory) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists && directory.is_dir() => {}
            Err(_) => return Err(LaunchError::Other),
        }
    }
    let meta = fs::metadata(directory).map_err(|_| LaunchError::Other)?;
    if !private_mode(&meta) {
        return Err(LaunchError::Value(
            "Directorio de entorno no privado".into(),
        ));
    }
    let path = directory.join(format!("environment-{}.json", uuid4_hex()?));
    let body = response_dumps(&json!({"version": 1, "values": values})).map_err(|_| Unsure)?;
    write_private(&path, body.as_bytes()).map_err(|_| LaunchError::Other)?;
    let digest = sha256_hex(&fs::read(&path).map_err(|_| LaunchError::Other)?);
    let path = path.to_str().ok_or(LaunchError::Unsure)?;
    Ok(json!({"path": path, "sha256": digest}))
}

/// El bloque `try` de `capture_opencode_environment`.
fn opencode_overrides(
    values: &Map<String, Value>,
    inventory: &Value,
    managed: bool,
) -> Result<(), LaunchError> {
    let refused = || {
        LaunchError::Value(
            "OpenCode contiene overrides no compatibles; el agente sigue abierto".into(),
        )
    };
    let text = |key: &str| values.get(key).and_then(Value::as_str).unwrap_or("{}");
    let content = py_json(text("OPENCODE_CONFIG_CONTENT"))?.map_err(|_| refused())?;
    let Value::Object(content) = content else {
        return Err(refused());
    };
    // `{row['id'] for row in inventory['mcps']}`.
    let rows = match inventory.get("mcps") {
        Some(Value::Array(rows)) => rows,
        Some(_) => return Err(LaunchError::Unsure),
        None => return Err(LaunchError::Other),
    };
    let mut known = Vec::new();
    for row in rows {
        match row {
            Value::Object(row) => known.push(row.get("id").ok_or(LaunchError::Other)?.clone()),
            _ => return Err(LaunchError::Unsure),
        }
    }
    // `set(content.get('mcp', {})) - known`: cualquier fallo es el mismo texto.
    let members: Vec<Value> = match content.get("mcp") {
        None => Vec::new(),
        Some(Value::Object(map)) => map.keys().map(|k| json!(k)).collect(),
        Some(Value::Array(items)) => items.clone(),
        Some(Value::String(s)) => s.chars().map(|c| json!(c.to_string())).collect(),
        Some(_) => return Err(refused()),
    };
    if members
        .iter()
        .any(|m| !m.is_string() || !known.iter().any(|k| python_eq(k, m)))
    {
        return Err(refused());
    }
    let get = |key: &str| content.get(key).is_some_and(truthy);
    if get("plugin") || ((get("skills") || get("mcp")) && !managed) {
        return Err(refused());
    }
    let permission = py_json(text("OPENCODE_PERMISSION"))?.map_err(|_| refused())?;
    if !(permission.is_object() || permission.is_string()) {
        return Err(refused());
    }
    Ok(())
}

// ---------------------------------------------------------------- procesos

/// `_process_env(pid)`: el último valor de cada clave, en bytes.
fn process_env(pid: u32) -> io::Result<HashMap<Vec<u8>, Vec<u8>>> {
    let raw = fs::read(format!("/proc/{pid}/environ"))?;
    let mut env = HashMap::new();
    for entry in raw.split(|b| *b == 0) {
        if let Some(at) = entry.iter().position(|b| *b == b'=') {
            env.insert(
                entry.get(..at).unwrap_or_default().to_vec(),
                entry.get(at + 1..).unwrap_or_default().to_vec(),
            );
        }
    }
    Ok(env)
}

/// `env.get(key) != value` con `value` de JSON (`None` = clave ausente).
fn env_differs(env: &HashMap<Vec<u8>, Vec<u8>>, key: &str, value: &Value) -> bool {
    match (env.get(key.as_bytes()), value) {
        (None, Value::Null) => false,
        (Some(ours), Value::String(theirs)) => ours.as_slice() != theirs.as_bytes(),
        _ => true,
    }
}

/// `verify_launch(pid, launch)`: evidencia de configuración del proceso (no
/// conexiones MCP). `Err(Unsure)` = un `AttributeError` que el Python no traga.
pub fn verify_launch(pid: u32, launch: &Value) -> Result<bool, Unsure> {
    match verify(pid, launch) {
        Ok(ok) => Ok(ok),
        Err(Fail::Value(_) | Fail::Caught) => Ok(false),
        Err(Fail::Attr | Fail::Unsure) => Err(Unsure),
    }
}

fn verify(pid: u32, launch: &Value) -> Result<bool, Fail> {
    let Value::Object(bundle) = launch else {
        return Err(Fail::Caught);
    };
    let manifest_value = bundle.get("manifest").ok_or(Fail::Caught)?;
    let manifest = path_of(manifest_value)?;
    let Value::Object(data) = load_manifest(&manifest, Some(launch))? else {
        return Err(Fail::Attr);
    };
    let env = process_env(pid)?;
    let operation = bundle.get("operationId").ok_or(Fail::Caught)?;
    if env_differs(&env, MARKER, operation) || env_differs(&env, MANIFEST_ENV, manifest_value) {
        return Ok(false);
    }
    let digest = sha256_hex(&fs::read(&manifest)?);
    if let Some(artifacts) = data.get("artifacts") {
        let Value::Object(artifacts) = artifacts else {
            return Err(Fail::Attr);
        };
        for (path, hash) in artifacts {
            if !python_eq(&json!(sha256_hex(&fs::read(path)?)), hash) {
                return Ok(false);
            }
        }
    }
    let home = data.get("home").ok_or(Fail::Caught)?;
    if env_differs(&env, DIGEST_ENV, &json!(digest)) || env_differs(&env, "HOME", home) {
        return Ok(false);
    }
    let receipt_path = PathBuf::from(OsStr::from_bytes(
        env.get(RECEIPT_ENV.as_bytes())
            .map_or(&[][..], Vec::as_slice),
    ));
    let receipt_path = if receipt_path.as_os_str().is_empty() {
        PathBuf::from(".")
    } else {
        receipt_path
    };
    if !same_parent(&receipt_path, &manifest) {
        return Ok(false);
    }
    let meta = fs::metadata(&receipt_path)?;
    if is_link(&receipt_path) || !private_mode(&meta) {
        return Ok(false);
    }
    let receipt_raw = fs::read(&receipt_path)?;
    let receipt_hash = env.get(RECEIPT_HASH_ENV.as_bytes());
    if receipt_hash.map(Vec::as_slice) != Some(sha256_hex(&receipt_raw).as_bytes()) {
        return Ok(false);
    }
    let receipt = py_json_bytes(&receipt_raw)?.map_err(|_| Fail::Caught)?;
    let Value::Object(receipt) = receipt else {
        return Err(Fail::Attr);
    };
    if !receipt
        .get("manifestSha256")
        .is_some_and(|v| python_eq(v, &json!(digest)))
    {
        return Ok(false);
    }
    match receipt.get("env").ok_or(Fail::Caught)? {
        Value::Object(expected) => {
            if expected.iter().any(|(k, v)| env_differs(&env, k, v)) {
                return Ok(false);
            }
        }
        _ => return Err(Fail::Attr),
    }
    match receipt.get("artifacts").ok_or(Fail::Caught)? {
        Value::Object(artifacts) => {
            for (path, hash) in artifacts {
                if !python_eq(&json!(sha256_hex(&fs::read(path)?)), hash) {
                    return Ok(false);
                }
            }
        }
        _ => return Err(Fail::Attr),
    }
    let cmdline =
        String::from_utf8(fs::read(format!("/proc/{pid}/cmdline"))?).map_err(|_| Fail::Caught)?;
    let argv: Vec<&str> = cmdline.trim_end_matches('\0').split('\0').collect();
    match receipt.get("argv").ok_or(Fail::Caught)? {
        Value::Array(items) => {
            let expected = items.get(1..).unwrap_or_default();
            if !expected.is_empty() {
                let tail = argv
                    .get(argv.len().saturating_sub(expected.len())..)
                    .unwrap_or(&argv);
                let same = tail.len() == expected.len()
                    && tail
                        .iter()
                        .zip(expected)
                        .all(|(a, e)| python_eq(&json!(a), e));
                if !same {
                    return Ok(false);
                }
            }
        }
        // `receipt['argv'][1:]` de una cadena: otra cadena, nunca igual a la lista.
        Value::String(s) => {
            if s.chars().count() > 1 {
                return Ok(false);
            }
        }
        _ => return Err(Fail::Caught),
    }
    match data.get("mounts").ok_or(Fail::Caught)? {
        Value::Array(mounts) if !mounts.is_empty() => verify_mounts(pid, mounts),
        value if !truthy(value) => Ok(true),
        // Iterar un objeto, una cadena o un escalar acaba en `TypeError`.
        _ => Ok(false),
    }
}

/// La parte de montajes de `verify_launch`.
fn verify_mounts(pid: u32, mounts: &[Value]) -> Result<bool, Fail> {
    if fs::read_link(format!("/proc/{pid}/ns/mnt"))? == fs::read_link("/proc/self/ns/mnt")? {
        return Ok(false);
    }
    let info =
        String::from_utf8(fs::read(format!("/proc/{pid}/mountinfo"))?).map_err(|_| Fail::Caught)?;
    for mount in mounts {
        let Value::Object(mount) = mount else {
            return Err(Fail::Caught);
        };
        let target = mount
            .get("target")
            .ok_or(Fail::Caught)?
            .as_str()
            .ok_or(Fail::Attr)?;
        let escaped = target
            .replace('\\', "\\134")
            .replace(' ', "\\040")
            .replace('\t', "\\011")
            .replace('\n', "\\012");
        let mounted = splitlines(&info)
            .iter()
            .any(|line| line.split_whitespace().nth(4) == Some(escaped.as_str()));
        if !mounted {
            return Ok(false);
        }
        let child = PathBuf::from(format!("/proc/{pid}/root")).join(target.trim_start_matches('/'));
        if mount.get("kind") == Some(&json!("directory")) {
            let source = path_of(mount.get("source").ok_or(Fail::Caught)?)?;
            let (original, inside) = (fs::metadata(source)?, fs::metadata(&child)?);
            if (original.dev(), original.ino()) != (inside.dev(), inside.ino()) {
                return Ok(false);
            }
        } else {
            let expected = mount.get("sha256").ok_or(Fail::Caught)?;
            if !python_eq(&json!(sha256_hex(&fs::read(&child)?)), expected) {
                return Ok(false);
            }
        }
    }
    Ok(true)
}

/// `configuration_status(pid, verified)`: un proceso externo corriente no es
/// un lanzamiento gestionado fallido.
pub fn configuration_status(pid: Option<u32>, verified: bool) -> &'static str {
    if verified {
        return "verified";
    }
    let Some(pid) = pid.filter(|p| *p != 0) else {
        return "not_started";
    };
    match process_env(pid) {
        Ok(env) => {
            let set = |key: &str| env.get(key.as_bytes()).is_some_and(|v| !v.is_empty());
            if set(MANIFEST_ENV) || set(MARKER) {
                "unverified"
            } else {
                "external"
            }
        }
        Err(_) => "unknown",
    }
}

/// `launch_from_pid(pid)`: el paquete saneado, solo si la evidencia viva lo
/// verifica.
pub fn launch_from_pid(pid: u32) -> Result<Option<Value>, Unsure> {
    let Ok(env) = process_env(pid) else {
        return Ok(None);
    };
    let Some(path) = env.get(MANIFEST_ENV.as_bytes()).filter(|p| !p.is_empty()) else {
        return Ok(None);
    };
    let Ok(path) = std::str::from_utf8(path) else {
        return Err(Unsure);
    };
    let bundle = match load_manifest(Path::new(path), None) {
        Ok(Value::Object(data)) => match data.get("bundle") {
            Some(bundle) => bundle.clone(),
            None => return Ok(None),
        },
        Ok(_) | Err(Fail::Attr | Fail::Unsure) => return Err(Unsure),
        Err(Fail::Value(_) | Fail::Caught) => return Ok(None),
    };
    Ok(verify_launch(pid, &bundle)?.then_some(bundle))
}

// ------------------------------------------------- caminos aún sin portar

/// Lo que decide, antes del `claim`, si una operación llegaría a
/// `inventory`/`_internal_inventory`/`prepare_launch` (aún sin portar).
#[derive(Debug, Clone, Copy)]
pub struct InventoryNeed<'a> {
    /// `request.extensionsOnly`: `PaneExtensionConfiguration.prepare`
    /// (`bin/cc-dash:3332`) llama a `prepare_launch` siempre.
    pub extensions_only: bool,
    /// El harness del agente original (`frm`): con `opencode`, `snapshot`
    /// (`:3075`) pide `inventory` para `capture_opencode_environment`.
    pub from: &'a str,
    /// `plan['to']`.
    pub to: &'a str,
    /// `plan['sameConversation']`.
    pub same_conversation: bool,
    /// `plan['unchanged']`: con él, `_preserve_extension_plan` (`:2712`)
    /// reutiliza el lanzamiento verificado sin inventario.
    pub unchanged: bool,
    /// `adapter.original['pid']`.
    pub original_pid: Option<u32>,
    /// `plan['returnOrigin']['extensionLaunch']`.
    pub return_origin_launch: Option<&'a Value>,
}

/// `true` si la operación pasaría por un camino que necesita
/// `inventory`/`prepare_launch`: quien llama declina ANTES del `claim`.
///
/// Reproduce la decisión de `_preserve_extension_plan`: `prior` es el
/// lanzamiento de `returnOrigin` o, con la misma conversación y un pid, el
/// que `launch_from_pid` verifica. Si el proceso tiene
/// `COMANDOS_EXTENSION_MANIFEST` pero no verifica, el Python lanza
/// `ValueError('no se pudo verificar el lanzamiento original; el agente sigue
/// abierto')` sin inventario: eso lo porta el adaptador, aquí es `false`.
/// `Unsure` = `launch_from_pid` dudoso o un `prior` sin `harness`.
pub fn needs_inventory(need: &InventoryNeed<'_>) -> Result<bool, Unsure> {
    if need.extensions_only || need.from == "opencode" {
        return Ok(true);
    }
    let mut prior = need.return_origin_launch.filter(|v| truthy(v)).cloned();
    if need.same_conversation
        && let Some(pid) = need.original_pid.filter(|p| *p != 0)
    {
        prior = launch_from_pid(pid)?;
    }
    let Some(prior) = prior.filter(truthy) else {
        return Ok(false);
    };
    let harness = prior.get("harness").ok_or(Unsure)?;
    if !python_eq(harness, &json!(need.to)) {
        return Ok(false);
    }
    Ok(!need.unchanged)
}

/// La versión prudente de `needs_inventory` para cuando el plan aún no se
/// conoce: declina con `extensionsOnly`, origen `opencode`, un
/// `returnOrigin.extensionLaunch` o un proceso original con huella de
/// lanzamiento gestionado (`COMANDOS_EXTENSION_MANIFEST` u
/// `COMANDOS_EXTENSION_OPERATION_ID`), verificado o no.
pub fn may_need_inventory(
    extensions_only: bool,
    from: &str,
    original_pid: Option<u32>,
    return_origin_launch: Option<&Value>,
) -> bool {
    extensions_only
        || from == "opencode"
        || return_origin_launch.is_some_and(truthy)
        || original_pid
            .filter(|p| *p != 0)
            .is_some_and(|pid| configuration_status(Some(pid), false) != "external")
}

// ---------------------------------------------------------------- selección

/// `_normalize(inv, selection)`: la selección completa (solo filas con estado
/// booleano), validada contra el inventario.
pub fn normalize(inventory: &Value, selection: &Value) -> Result<Value, LaunchError> {
    let Value::Object(selection) = selection else {
        return Err(LaunchError::Value(
            "Selección de extensiones inválida.".into(),
        ));
    };
    if selection.keys().any(|k| k != "mcps" && k != "skills") {
        return Err(LaunchError::Value(
            "Selección de extensiones inválida.".into(),
        ));
    }
    let unknown =
        || LaunchError::Value("Selección desconocida o estado de extensión inválido.".into());
    let mut result = Map::new();
    for kind in ["mcps", "skills"] {
        let empty = Value::Object(Map::new());
        let supplied = selection.get(kind).unwrap_or(&empty);
        let Value::Array(list) = inventory.get(kind).ok_or(LaunchError::Other)? else {
            return Err(LaunchError::Unsure);
        };
        let mut rows: Map<String, Value> = Map::new();
        for row in list {
            let id = row.get("id").ok_or(LaunchError::Other)?;
            rows.insert(
                id.as_str().ok_or(LaunchError::Unsure)?.to_owned(),
                row.clone(),
            );
        }
        let Value::Object(supplied) = supplied else {
            return Err(unknown());
        };
        if supplied.keys().any(|k| !rows.contains_key(k))
            || supplied.values().any(|v| !v.is_boolean())
        {
            return Err(unknown());
        }
        for (ident, value) in supplied {
            let row = rows.get(ident).ok_or(LaunchError::Other)?;
            let enabled = row.get("enabled").ok_or(LaunchError::Other)?;
            let toggleable = row.get("toggleable").ok_or(LaunchError::Other)?;
            if !truthy(toggleable) && !python_eq(value, enabled) {
                let name = row.get("name").ok_or(LaunchError::Other)?;
                let name = name.as_str().ok_or(LaunchError::Other)?;
                return Err(LaunchError::Value(format!(
                    "Esta extensión no admite cambios aislados: {name}"
                )));
            }
        }
        let mut chosen = Map::new();
        for (ident, row) in &rows {
            let enabled = row.get("enabled").ok_or(LaunchError::Other)?;
            if enabled.is_boolean() {
                chosen.insert(
                    ident.clone(),
                    supplied.get(ident).unwrap_or(enabled).clone(),
                );
            }
        }
        result.insert(kind.into(), Value::Object(chosen));
    }
    Ok(Value::Object(result))
}

#[cfg(test)]
mod tests {
    use super::{parse_toml, parse_trust_toml, shlex_split};
    use serde_json::json;

    #[test]
    fn shlex_split_matches_posix_shlex() {
        assert_eq!(
            shlex_split(r#"env 'A=b c' "x\"y\$" z\ w ''"#).unwrap(),
            vec!["env", "A=b c", "x\"y\\$", "z w", ""]
        );
        assert_eq!(shlex_split("  ").unwrap(), Vec::<String>::new());
        assert_eq!(shlex_split("a 'b").unwrap_err(), "No closing quotation");
        assert_eq!(shlex_split("a \\").unwrap_err(), "No escaped character");
    }

    #[test]
    fn toml_rejects_what_tomllib_3_11_rejects() {
        assert_eq!(parse_toml("a = 1").unwrap(), Some(json!({"a": 1})));
        for text in [
            "a = {b=1,\n c=2}",
            "a = {b=1,}",
            "a = \"\\e\"",
            "a = \"\\x41\"",
            "a = 1979-05-27",
            "\u{feff}a = 1",
            "a = 1\na = 2",
        ] {
            assert_eq!(parse_toml(text).unwrap(), None, "{text:?}");
        }
        assert_eq!(
            parse_toml("a = [\n1,\n]\nb = {c = [1,\n2]}").unwrap(),
            Some(json!({"a": [1], "b": {"c": [1, 2]}}))
        );
        assert!(parse_toml("a = 99999999999999999999").is_err());
    }

    /// Lo que `toml_edit` no representa y `tomllib` sí: `Unsure`, nunca un
    /// rechazo.
    #[test]
    fn overflowing_numbers_are_unsure() {
        for text in [
            "a = 1e400",
            "a = -1e400",
            "a = 1.8e308",
            "a = 0x8000000000000000",
            "a = 0o1777777777777777777777",
            "a = 0b1000000000000000000000000000000000000000000000000000000000000000",
        ] {
            assert!(parse_toml(text).is_err(), "{text}");
            assert!(parse_trust_toml(text).is_err(), "{text}");
        }
    }

    /// Lector de la confianza: las fechas son valores opacos (desviación: el
    /// Python 3.10 falla al pasarlas por JSON); lo que `tomllib` 3.11 rechaza
    /// se sigue rechazando.
    #[test]
    fn trust_reader_accepts_dates() {
        assert_eq!(
            parse_trust_toml("a = 1979-05-27\nb = 1979-05-27T07:32:00Z\nc = 07:32:00").unwrap(),
            Some(json!({"a": "1979-05-27", "b": "1979-05-27T07:32:00Z", "c": "07:32:00"}))
        );
        assert_eq!(parse_toml("a = 1979-05-27").unwrap(), None);
        for text in [
            "a = 07:32",
            "a = 1979-05-27T07:32",
            "a = {b=1,}",
            "a = \"\\e\"",
            "\u{feff}a=1",
        ] {
            assert_eq!(parse_trust_toml(text).unwrap(), None, "{text:?}");
        }
    }
}
