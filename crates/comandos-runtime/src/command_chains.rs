//! Cadenas de comandos: archivos Markdown que el usuario puede editar a mano.
//! Port de `lib/command_chains.py` (`default_dir`, `slugify`, `serialize`,
//! `parse`, `list_chains`, `save_chain`).
//!
//! `save_chain` valida todo lo que no depende del disco antes de crear nada:
//! lo que el port no reproduce con certeza (un `repr` o un `str()` de un valor
//! raro, `slugify` de un carácter fuera de los tramos de NFKD estable) es
//! `Unsure` sin efectos.
use crate::{Unsure, cli_catalog::py_str, cli_help::regex_safe};
use comandos_core::text::{is_space, splitlines, strip};
use icu_normalizer::DecomposingNormalizerBorrowed;
use regex::Regex;
use serde_json::{Map, Value, json};
use std::{
    ffi::OsStr,
    fs, io,
    path::{Path, PathBuf},
    sync::LazyLock,
};

const KINDS: [&str; 2] = ["shell", "pane"];

static STEP: LazyLock<Option<Regex>> =
    LazyLock::new(|| Regex::new(r"^\s*(?:\d+[.)]|[-*])\s*(\w+)\s*:\s*(.+?)\s*$").ok());

/// `ChainError` (400) o un `OSError`/`UnicodeError` (500) con el nombre de su
/// clase de Python.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SaveError {
    Chain(String),
    Os(&'static str),
}

/// `default_dir()`: `$XDG_CONFIG_HOME/comandos/cadenas` o
/// `~/.config/comandos/cadenas` (un `XDG_CONFIG_HOME` vacío no cuenta).
pub fn default_dir(xdg_config_home: Option<&OsStr>, home: &Path) -> PathBuf {
    let base = match xdg_config_home.filter(|x| !x.is_empty()) {
        Some(x) => PathBuf::from(x),
        None => home.join(".config"),
    };
    base.join("comandos").join("cadenas")
}

/// `_SLUG_RE.fullmatch(slug)`: `^[a-z0-9][a-z0-9-]{0,59}$`.
pub fn slug_valid(slug: &str) -> bool {
    let b = slug.as_bytes();
    (1..=60).contains(&b.len())
        && b.first()
            .is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        && b.iter()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'-')
}

/// Tramos donde la descomposición NFKD es la misma con la base de Unicode del
/// Python (`unicodedata` 13.0 de 3.10) y con la de ICU: latín, IPA,
/// modificadores, marcas combinantes y puntuación general, todos asignados
/// desde Unicode 6.3 (U+2065 no lo está en ninguna); la política de
/// estabilidad de la normalización garantiza que no cambian.
fn stable_nfkd(c: char) -> bool {
    matches!(u32::from(c), 0..=0x036F | 0x2000..=0x206F)
}

/// `slugify(name)`: NFKD, sin lo que no es ASCII, en minúsculas y con guiones.
/// Un carácter fuera de los tramos estables (emoji, CJK…) es `Unsure`.
pub fn slugify(name: &str) -> Result<String, Unsure> {
    if !name.chars().all(stable_nfkd) {
        return Err(Unsure);
    }
    let decomposed = DecomposingNormalizerBorrowed::new_nfkd().normalize(name);
    let name: String = decomposed.chars().filter(char::is_ascii).collect();
    let lower = name.to_ascii_lowercase();
    let mut out = String::new();
    let mut gap = false;
    for c in lower.chars() {
        if c.is_ascii_lowercase() || c.is_ascii_digit() {
            if gap {
                out.push('-');
                gap = false;
            }
            out.push(c);
        } else {
            gap = true;
        }
    }
    if gap {
        out.push('-');
    }
    let trimmed: String = out.trim_matches('-').chars().take(60).collect();
    let trimmed = trimmed.trim_matches('-');
    Ok(if trimmed.is_empty() {
        "cadena".to_owned()
    } else {
        trimmed.to_owned()
    })
}

/// `_has_control(text)`.
fn has_control(text: &str) -> bool {
    text.chars().any(|c| (c as u32) < 32 || c as u32 == 127)
}

/// `str(x or "")`.
fn str_or_empty(value: Option<&Value>) -> Result<String, Unsure> {
    match value {
        Some(v) if comandos_core::json::truthy(v) => py_str(v),
        _ => Ok(String::new()),
    }
}

/// `_check_steps(steps)`: los pasos limpios o el `ChainError`.
pub fn check_steps(steps: Option<&Value>) -> Result<Result<Vec<Value>, String>, Unsure> {
    let Some(Value::Array(steps)) = steps.filter(|s| comandos_core::json::truthy(s)) else {
        return Ok(Err("La cadena necesita al menos un paso".into()));
    };
    let mut out = Vec::new();
    for (i, step) in steps.iter().enumerate() {
        let i = i + 1;
        let empty = Map::new();
        let step = step.as_object().unwrap_or(&empty);
        let kind = step.get("kind");
        let raw_text = step.get("text");
        // `str(st.get("text") or "")` va antes del tipo: un entero que `str()`
        // no escribe ya lanzaría.
        if let Some(Value::Number(n)) = raw_text
            && n.as_str().len() > 4300
        {
            return Err(Unsure);
        }
        let kind_text = match kind {
            Some(Value::String(k)) if KINDS.contains(&k.as_str()) => k.clone(),
            other => {
                let shown = match other {
                    None => "None".to_owned(),
                    Some(v) => py_str(v)?,
                };
                return Ok(Err(format!(
                    "Paso inválido {i}: tipo '{shown}' (usa shell o pane)"
                )));
            }
        };
        let text = str_or_empty(raw_text)?;
        if strip(&text).is_empty() || has_control(&text) {
            return Ok(Err(format!(
                "Paso inválido {i}: texto vacío o con saltos de línea o caracteres de control"
            )));
        }
        out.push(json!({"kind": kind_text, "text": strip(&text)}));
    }
    Ok(Ok(out))
}

/// `_check_name(name)`.
pub fn check_name(name: Option<&Value>) -> Result<Result<String, String>, Unsure> {
    let name = str_or_empty(name)?;
    let name = strip(&name);
    if name.is_empty() || has_control(name) {
        return Ok(Err("Nombre inválido: vacío o con saltos de línea".into()));
    }
    Ok(Ok(name.to_owned()))
}

/// `serialize(name, steps)`.
pub fn serialize(
    name: Option<&Value>,
    steps: Option<&Value>,
) -> Result<Result<String, String>, Unsure> {
    let name = match check_name(name)? {
        Ok(name) => name,
        Err(e) => return Ok(Err(e)),
    };
    let steps = match check_steps(steps)? {
        Ok(steps) => steps,
        Err(e) => return Ok(Err(e)),
    };
    let mut lines = vec![format!("# {name}"), String::new()];
    for (i, step) in steps.iter().enumerate() {
        let kind = step.get("kind").and_then(Value::as_str).unwrap_or("");
        let text = step.get("text").and_then(Value::as_str).unwrap_or("");
        lines.push(format!("{}. {kind}: {text}", i + 1));
    }
    Ok(Ok(lines.join("\n") + "\n"))
}

/// `repr(str)` para texto ASCII; `None` si hay no ASCII.
fn repr_ascii(s: &str) -> Option<String> {
    if !s.is_ascii() {
        return None;
    }
    let quote = if s.contains('\'') && !s.contains('"') {
        '"'
    } else {
        '\''
    };
    let mut out = String::with_capacity(s.len() + 2);
    out.push(quote);
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            c if c == quote => {
                out.push('\\');
                out.push(c);
            }
            c if (c as u32) < 0x20 || c == '\u{7f}' => {
                out.push_str(&format!("\\x{:02x}", c as u32))
            }
            c => out.push(c),
        }
    }
    out.push(quote);
    Some(out)
}

/// `parse(text)`: `{"name", "steps"}` o el `ChainError`.
pub fn parse(text: &str) -> Result<Result<(String, Vec<Value>), String>, Unsure> {
    let rule = STEP.as_ref().ok_or(Unsure)?;
    let mut name = String::new();
    let mut steps = Vec::new();
    for raw in splitlines(text) {
        let line = raw.trim_end_matches(is_space);
        if strip(line).is_empty() {
            continue;
        }
        if name.is_empty() && line.starts_with("# ") {
            name = strip(line.get(2..).unwrap_or("")).to_owned();
            continue;
        }
        if !regex_safe(line) {
            return Err(Unsure);
        }
        let Some(caps) = rule.captures(line) else {
            let shown = repr_ascii(strip(line)).ok_or(Unsure)?;
            return Ok(Err(format!("Paso inválido: {shown}")));
        };
        let kind = caps.get(1).map_or("", |m| m.as_str());
        let body = caps.get(2).map_or("", |m| m.as_str());
        steps.push(json!({"kind": kind, "text": body}));
    }
    match check_steps(Some(&Value::Array(steps)))? {
        Ok(steps) => Ok(Ok((name, steps))),
        Err(e) => Ok(Err(e)),
    }
}

/// `str(OSError)` de Python: `[Errno N] <strerror>: '<ruta>'`; `None` si no
/// se escribe con certeza (sin errno o ruta no ASCII).
fn os_error_text(error: &io::Error, path: &Path) -> Option<String> {
    let errno = error.raw_os_error()?;
    let shown = io::Error::from_raw_os_error(errno).to_string();
    let strerror = shown
        .strip_suffix(&format!(" (os error {errno})"))
        .unwrap_or(&shown)
        .to_owned();
    let path = repr_ascii(path.to_str()?)?;
    Some(format!("[Errno {errno}] {strerror}: {path}"))
}

/// `list_chains(directory)`.
pub fn list_chains(directory: &Path) -> Result<Vec<Value>, Unsure> {
    if !directory.is_dir() {
        return Ok(Vec::new());
    }
    let entries = fs::read_dir(directory).map_err(|_| Unsure)?;
    let mut found: Vec<(String, String, PathBuf)> = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|_| Unsure)?;
        let name = entry.file_name().into_string().map_err(|_| Unsure)?;
        // `Path.glob("*.md")` (3.10): también ocultos, directorios y enlaces rotos.
        let Some(stem) = name.strip_suffix(".md") else {
            continue;
        };
        // `PurePath.stem`: «.md» no tiene sufijo (el punto inicial no cuenta).
        let stem = if stem.is_empty() { ".md" } else { stem };
        found.push((stem.to_owned(), name.clone(), directory.join(&name)));
    }
    found.sort_by(|a, b| a.0.cmp(&b.0));
    let mut out = Vec::new();
    for (slug, name, path) in found {
        if !slug_valid(&slug) {
            out.push(json!({"slug": slug, "name": name,
                "error": "Nombre de archivo no válido: usa minúsculas, números y guiones (máx. 60)"}));
            continue;
        }
        let bytes = match fs::read(&path) {
            Ok(bytes) => bytes,
            Err(e) => {
                let message = os_error_text(&e, &path).ok_or(Unsure)?;
                out.push(json!({"slug": slug, "name": name, "error": message}));
                continue;
            }
        };
        // `utf-8-sig`: sin el BOM inicial; lo que no decodifica no se reproduce.
        let text = std::str::from_utf8(&bytes).map_err(|_| Unsure)?;
        let text = text.strip_prefix('\u{feff}').unwrap_or(text);
        match parse(text)? {
            Ok((chain_name, steps)) => {
                let shown = if chain_name.is_empty() {
                    slug.clone()
                } else {
                    chain_name
                };
                out.push(json!({"slug": slug, "name": shown, "steps": steps}));
            }
            Err(message) => out.push(json!({"slug": slug, "name": name, "error": message})),
        }
    }
    Ok(out)
}

/// Clase de Python de un `OSError` por su `errno` (`PEP 3151`).
pub fn os_error_class(error: &io::Error) -> &'static str {
    match error.raw_os_error() {
        Some(2) => "FileNotFoundError",
        Some(17) => "FileExistsError",
        Some(1 | 13) => "PermissionError",
        Some(21) => "IsADirectoryError",
        Some(20) => "NotADirectoryError",
        Some(4) => "InterruptedError",
        Some(11 | 114 | 115) => "BlockingIOError",
        Some(10) => "ChildProcessError",
        Some(32 | 108) => "BrokenPipeError",
        Some(103) => "ConnectionAbortedError",
        Some(111) => "ConnectionRefusedError",
        Some(104) => "ConnectionResetError",
        Some(3) => "ProcessLookupError",
        Some(110) => "TimeoutError",
        _ => "OSError",
    }
}

/// `Path.exists()` de 3.10: falso con `ENOENT`, `ENOTDIR`, `EBADF` o `ELOOP`;
/// otro error se lanza.
fn exists(path: &Path) -> io::Result<bool> {
    match fs::metadata(path) {
        Ok(_) => Ok(true),
        Err(e) if matches!(e.raw_os_error(), Some(2 | 20 | 9 | 40)) => Ok(false),
        Err(e) => Err(e),
    }
}

/// `save_chain(directory, name, steps, slug)` con `slug = data.get("slug") or
/// None`. `Ok(Ok(cadena))`, `Ok(Err(SaveError))` o `Err(Unsure)` antes de
/// tocar el disco.
pub fn save_chain(
    directory: &Path,
    name: Option<&Value>,
    steps: Option<&Value>,
    slug: Option<&Value>,
) -> Result<Result<Value, SaveError>, Unsure> {
    let body = match serialize(name, steps)? {
        Ok(body) => body,
        Err(message) => return Ok(Err(SaveError::Chain(message))),
    };
    // Todo lo que decide el `slug` y la respuesta, antes del `mkdir`.
    let given = slug.filter(|s| comandos_core::json::truthy(s));
    let given_text = match given {
        Some(value) => {
            let text = py_str(value)?;
            if !slug_valid(&text) {
                let shown = match value {
                    Value::String(s) => repr_ascii(s).ok_or(Unsure)?,
                    Value::Number(_) | Value::Bool(_) => py_str(value)?,
                    _ => return Err(Unsure),
                };
                // `_path` lanza tras el `mkdir`: se crea igual el directorio.
                return Ok(match fs::create_dir_all(directory) {
                    Ok(()) => Err(SaveError::Chain(format!("Slug inválido: {shown}"))),
                    Err(e) => Err(SaveError::Os(os_error_class(&e))),
                });
            }
            Some(text)
        }
        None => None,
    };
    let base = match &given_text {
        Some(_) => None,
        None => {
            // `slugify(name)` sobre el valor crudo (`str(name or "")`).
            let raw = str_or_empty(name)?;
            Some(slugify(&raw)?)
        }
    };
    let shown_name = match name {
        Some(v) => strip(&py_str(v)?).to_owned(),
        None => "None".to_owned(),
    };
    let steps = match check_steps(steps)? {
        Ok(steps) => steps,
        Err(message) => return Ok(Err(SaveError::Chain(message))),
    };
    // Efectos.
    if let Err(e) = fs::create_dir_all(directory) {
        return Ok(Err(SaveError::Os(os_error_class(&e))));
    }
    let (slug_value, slug_text) = match (given, given_text, base) {
        (Some(value), Some(text), _) => (value.clone(), text),
        (_, _, Some(base)) => {
            let mut candidate = base.clone();
            let mut n = 2u64;
            loop {
                if !slug_valid(&candidate) {
                    return Ok(Err(SaveError::Chain(format!(
                        "Slug inválido: {}",
                        repr_ascii(&candidate).ok_or(Unsure)?
                    ))));
                }
                match exists(&directory.join(format!("{candidate}.md"))) {
                    Ok(false) => break,
                    Ok(true) => {
                        candidate = format!("{base}-{n}");
                        n += 1;
                    }
                    Err(e) => return Ok(Err(SaveError::Os(os_error_class(&e)))),
                }
            }
            (Value::from(candidate.clone()), candidate)
        }
        _ => return Err(Unsure),
    };
    let path = directory.join(format!("{slug_text}.md"));
    let tmp = directory.join(format!("{slug_text}.md.tmp"));
    let written = fs::write(&tmp, body.as_bytes()).and_then(|()| fs::rename(&tmp, &path));
    if let Err(e) = written {
        let _ = fs::remove_file(&tmp);
        return Ok(Err(SaveError::Os(os_error_class(&e))));
    }
    Ok(Ok(
        json!({"slug": slug_value, "name": shown_name, "steps": steps}),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugify_like_python() {
        assert_eq!(slugify("  Mi Cadena!! ").unwrap(), "mi-cadena");
        assert_eq!(slugify("***").unwrap(), "cadena");
        assert_eq!(slugify("").unwrap(), "cadena");
        assert_eq!(slugify(&"a".repeat(70)).unwrap(), "a".repeat(60));
        assert_eq!(slugify("Revisión diaria").unwrap(), "revision-diaria");
        assert!(slugify("café ☕").is_err());
    }

    #[test]
    fn slug_rule() {
        assert!(slug_valid("a-b-1"));
        assert!(!slug_valid("-a"));
        assert!(!slug_valid("A"));
        assert!(!slug_valid(&"a".repeat(61)));
        assert!(!slug_valid(""));
    }

    #[test]
    fn os_error_texts() {
        let e = io::Error::from_raw_os_error(21);
        assert_eq!(
            os_error_text(&e, Path::new("/x/y.md")).unwrap(),
            "[Errno 21] Is a directory: '/x/y.md'"
        );
        assert_eq!(os_error_class(&e), "IsADirectoryError");
    }
}
