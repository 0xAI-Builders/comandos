//! Diálogos de arranque de un CLI (`bin/cc-dash`, `dialog_patterns`,
//! `screen_dialog`, `verify_attempts`): los patrones de
//! `config/detectors.json` (en/es) con los de `_DEFAULT_DIALOGS` como
//! respaldo, cacheados por `mtime` como `_DIALOG_CACHE`.
//!
//! Los patrones son `re` de Python: se traducen con `providers::py_regex_lines`
//! y solo se buscan sobre pantallas donde el resultado es el mismo con certeza
//! (`providers::screen_safe`). Un patrón que el port no traduce es
//! `BadPattern` (se ve antes del `claim` con `probe`); una pantalla dudosa,
//! `UnreadableScreen` (solo después de los efectos, donde se usa
//! `screen_dialog_fail_closed`). Si el Python ya habría devuelto antes, no
//! estorba.
use crate::{Unsure, claude_trust, providers};
use comandos_core::json::truthy;
use comandos_core::text::{int, strip};
use regex::Regex;
use serde_json::{Map, Value};
use std::{
    collections::HashMap,
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

/// `_LOGIN_LINE`.
const LOGIN_LINE: &str = r"(^|\n)[\s│┃>]*";

/// Por qué `screen_dialog` no decide con certeza.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DialogError {
    /// La configuración (o un patrón) no se reproduce: se detecta antes del
    /// `claim` con `DialogCache::probe`.
    BadPattern,
    /// La pantalla tiene caracteres que `re` y `regex` podrían leer distinto:
    /// solo existe después de teclear el comando.
    UnreadableScreen,
}

/// Orden en que `screen_dialog` prueba las categorías.
pub const KINDS: [&str; 5] = ["trust", "login", "onboarding", "effort", "error"];

/// `_DEFAULT_DIALOGS`.
fn default_dialogs() -> Vec<(&'static str, Vec<String>)> {
    let login = |rest: &str| format!("{LOGIN_LINE}{rest}");
    vec![
        (
            "trust",
            vec![
                r"trust this folder".into(),
                r"Do you trust".into(),
                r"Accessing workspace".into(),
            ],
        ),
        (
            "login",
            vec![
                login(r"(please\s+)?(sign in|log in)\s+to\s+(continue|use|proceed)"),
                login(r"(sign in|log in|login|authentication)\s+required"),
                login(r"(por favor,?\s+)?inicia(?:r)? sesi[oó]n para (continuar|usar)"),
                login(r"se requiere inicia(?:r)? sesi[oó]n"),
                r"(^|\n)\s*/login\b".into(),
            ],
        ),
        (
            "onboarding",
            vec![
                r"Choose the text style".into(),
                r"Elige el estilo de texto".into(),
                r"Choose your theme".into(),
                r"Elige tu tema".into(),
            ],
        ),
        (
            "effort",
            vec![
                r"(^|\n)\s*❯\s*(Keep|Mantener) (low|medium|high|xhigh|max)\s*$".into(),
                r"estimated cost of (low|medium|high|xhigh|max)\b".into(),
            ],
        ),
        (
            "error",
            vec![r"(^|\n)\s*error:".into(), r"(^|\n)\s*fatal:".into()],
        ),
    ]
}

/// Un patrón conservado: su texto y su forma compilada con `re.I | re.M`.
#[derive(Debug, Clone)]
struct Entry {
    pattern: String,
    regex: Result<Regex, Unsure>,
}

/// Los patrones de una categoría, o `Unsure` si no se sabe cuáles serían.
#[derive(Debug, Clone)]
enum Kind {
    Known(Vec<Entry>),
    Unsure,
}

type Patterns = Arc<HashMap<String, Kind>>;

/// `(st_mtime, patrones)`: `None` si `config/detectors.json` no existe.
type Cached = (Option<(i64, i64)>, Patterns);

/// `_DIALOG_CACHE`: `(mtime, patrones)` de `<repo>/config/detectors.json`.
#[derive(Debug)]
pub struct DialogCache {
    repo_root: PathBuf,
    state: Mutex<Option<Cached>>,
}

fn detectors_path(repo_root: &Path) -> PathBuf {
    repo_root.join("config").join("detectors.json")
}

/// `_detectors_cfg()`: `json.load(f) or {}`, `{}` ante cualquier excepción.
fn detectors_cfg(repo_root: &Path) -> Result<Value, Unsure> {
    Ok(match claude_trust::read_json(&detectors_path(repo_root))? {
        claude_trust::Read::Value(value) if truthy(&value) => value,
        _ => Value::Object(Map::new()),
    })
}

/// `cfg.get(key)`; un `cfg` que no es objeto lanzaría `AttributeError`.
fn cfg_get<'a>(cfg: &'a Value, key: &str) -> Result<Option<&'a Value>, Unsure> {
    match cfg {
        Value::Object(map) => Ok(map.get(key)),
        _ => Err(Unsure),
    }
}

/// `_clean_patterns(raw)`.
fn clean_patterns(raw: Option<&Value>) -> Result<HashMap<String, Kind>, Unsure> {
    let raw = match raw {
        Some(v) if truthy(v) => v.as_object().ok_or(Unsure)?,
        _ => return Ok(HashMap::new()),
    };
    let mut out = HashMap::new();
    for (kind, value) in raw {
        let list = match value {
            Value::String(s) => vec![Value::String(s.clone())],
            Value::Array(items) => items.clone(),
            _ => continue,
        };
        let mut entries = Vec::new();
        let (mut certain, mut doubtful) = (0, 0);
        for pattern in &list {
            let Some(pattern) = pattern.as_str().filter(|p| !strip(p).is_empty()) else {
                continue;
            };
            // `re.compile(pat)`: el traductor solo acepta lo que el Python
            // compila; lo que no traduce puede ser válido o no.
            if providers::py_regex_lines(pattern, false).is_ok() {
                certain += 1;
            } else if certainly_invalid(pattern) {
                // `re.error` seguro: el Python lo descarta.
                continue;
            } else {
                doubtful += 1;
            }
            entries.push(Entry {
                pattern: pattern.to_owned(),
                regex: providers::py_regex_lines(pattern, true),
            });
        }
        if certain == 0 && !list.is_empty() {
            if doubtful > 0 {
                // ¿Ningún patrón válido (categoría por omisión) o los dudosos?
                out.insert(kind.clone(), Kind::Unsure);
            }
            continue;
        }
        out.insert(kind.clone(), Kind::Known(entries));
    }
    Ok(out)
}

/// Errores de `sre_parse` que no dependen de nada más: paréntesis o clase sin
/// cerrar, `)` de sobra, `\\` final y un cuantificador al principio.
fn certainly_invalid(pattern: &str) -> bool {
    let chars: Vec<char> = pattern.chars().collect();
    if matches!(chars.first(), Some('*' | '+' | '?')) {
        return true;
    }
    let mut depth: i64 = 0;
    let mut i = 0;
    while let Some(&c) = chars.get(i) {
        match c {
            '\\' => {
                if i + 1 >= chars.len() {
                    return true;
                }
                i += 2;
                continue;
            }
            '[' => {
                // El primer `]` tras `[` o `[^` es literal.
                let mut j = i + 1;
                if chars.get(j) == Some(&'^') {
                    j += 1;
                }
                let mut first = true;
                loop {
                    match chars.get(j) {
                        None => return true,
                        Some(']') if !first => break,
                        Some('\\') => j += 2,
                        Some(_) => j += 1,
                    }
                    first = false;
                }
                i = j + 1;
                continue;
            }
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth < 0 {
                    return true;
                }
            }
            _ => {}
        }
        i += 1;
    }
    depth != 0
}

impl DialogCache {
    pub fn new(repo_root: &Path) -> Self {
        Self {
            repo_root: repo_root.to_path_buf(),
            state: Mutex::new(None),
        }
    }

    /// `dialog_patterns()`.
    fn patterns(&self) -> Result<Patterns, Unsure> {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let mtime = std::fs::metadata(detectors_path(&self.repo_root))
            .ok()
            .map(|m| (m.mtime(), m.mtime_nsec()));
        if let Some((cached, value)) = state.as_ref()
            && *cached == mtime
        {
            return Ok(Arc::clone(value));
        }
        let cfg = detectors_cfg(&self.repo_root)?;
        let mut clean = clean_patterns(cfg_get(&cfg, "dialogPatterns")?)?;
        for (kind, fallback) in default_dialogs() {
            clean.entry(kind.into()).or_insert_with(|| {
                Kind::Known(
                    fallback
                        .into_iter()
                        .map(|pattern| Entry {
                            regex: providers::py_regex_lines(&pattern, true),
                            pattern,
                        })
                        .collect(),
                )
            });
        }
        let value = Arc::new(clean);
        *state = Some((mtime, Arc::clone(&value)));
        Ok(value)
    }

    /// Antes del `claim`: ¿son evaluables con certeza todos los patrones? Si
    /// no (configuración ilegible, categoría dudosa, patrón sin traducir),
    /// `BadPattern` y quien llama declina. Lo que dependa de la pantalla solo
    /// se sabe después de teclear el comando.
    pub fn probe(&self) -> Result<(), DialogError> {
        let patterns = self.patterns().map_err(|_| DialogError::BadPattern)?;
        for kind in KINDS {
            match patterns.get(kind) {
                None => {}
                Some(Kind::Unsure) => return Err(DialogError::BadPattern),
                Some(Kind::Known(entries)) if entries.iter().any(|e| e.regex.is_err()) => {
                    return Err(DialogError::BadPattern);
                }
                Some(Kind::Known(_)) => {}
            }
        }
        Ok(())
    }

    /// `screen_dialog(screen)`: `"trust"`, `"login"`, `"onboarding"`,
    /// `"effort"`, `"error"` o `""` según la pantalla.
    pub fn screen_dialog(&self, screen: &str) -> Result<&'static str, DialogError> {
        let patterns = self.patterns().map_err(|_| DialogError::BadPattern)?;
        for kind in KINDS {
            match patterns.get(kind) {
                None => {}
                Some(Kind::Unsure) => return Err(DialogError::BadPattern),
                Some(Kind::Known(entries)) => {
                    for entry in entries {
                        let regex = entry.regex.as_ref().map_err(|_| DialogError::BadPattern)?;
                        if !providers::screen_safe(&entry.pattern, true, screen) {
                            return Err(DialogError::UnreadableScreen);
                        }
                        if regex.is_match(screen) {
                            return Ok(kind);
                        }
                    }
                }
            }
        }
        Ok("")
    }

    /// `screen_dialog` después de los efectos, a prueba de fallos: cualquier
    /// duda es `"error"`. En `_verify` eso cuenta como diálogo presente (se
    /// sigue esperando; como mucho se agota el plazo) y en
    /// `pending_confirmation` como error (no se confirma). Nunca «seguir como
    /// si nada» ni `Decline` (regla 2: ya hubo efectos).
    pub fn screen_dialog_fail_closed(&self, screen: &str) -> &'static str {
        self.screen_dialog(screen).unwrap_or("error")
    }
}

/// `int(value or 180)`.
fn attempts_value(value: Option<&Value>) -> Result<i64, Unsure> {
    let Some(value) = value.filter(|v| truthy(v)) else {
        return Ok(180);
    };
    match value {
        Value::Bool(_) => Ok(1),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                return Ok(i);
            }
            match n.as_f64() {
                // `int(2.7)` trunca; un `float` enorme no cabe en `i64`.
                Some(f) if f.is_finite() && f.abs() < 9.0e18 => Ok(f.trunc() as i64),
                _ => Err(Unsure),
            }
        }
        Value::String(s) => int(s).map_err(|_| Unsure),
        _ => Err(Unsure),
    }
}

/// `verify_attempts(cwd, config_dir)`: 60 intentos (~30 s) normalmente; más
/// (`verifyAttemptsWithMcp`, 180 por omisión) si el destino carga MCPs.
/// `home` es `os.path.expanduser("~")`.
pub fn verify_attempts(
    repo_root: &Path,
    cwd: &str,
    config_dir: Option<&str>,
    home: &str,
) -> Result<i64, Unsure> {
    let cfg = detectors_cfg(repo_root)?;
    let with_mcp = attempts_value(cfg_get(&cfg, "verifyAttemptsWithMcp")?)?;
    if !cwd.is_empty() && Path::new(cwd).join(".mcp.json").exists() {
        return Ok(with_mcp);
    }
    let dir = config_dir.filter(|d| !d.is_empty()).unwrap_or(home);
    if let claude_trust::Read::Value(Value::Object(map)) =
        claude_trust::read_json(&Path::new(dir).join(".claude.json"))?
        && map.get("mcpServers").is_some_and(truthy)
    {
        return Ok(with_mcp);
    }
    Ok(60)
}

#[cfg(test)]
mod tests {
    use super::{DialogCache, DialogError, attempts_value};
    use serde_json::json;

    #[test]
    fn defaults_cover_every_kind() {
        let dir = std::env::temp_dir().join(crate::fresh_id("dialogs").unwrap());
        let cache = DialogCache::new(&dir);
        assert_eq!(cache.screen_dialog("Do you trust the files?"), Ok("trust"));
        assert_eq!(cache.screen_dialog("│ > Sign in required"), Ok("login"));
        assert_eq!(cache.screen_dialog("note: login required for x"), Ok(""));
        assert_eq!(cache.screen_dialog("  ❯ Keep xhigh\n"), Ok("effort"));
        assert_eq!(
            cache.screen_dialog("se requiere iniciar sesión"),
            Ok("login")
        );
        assert_eq!(cache.screen_dialog("x\n fatal: bad"), Ok("error"));
        assert_eq!(
            cache.screen_dialog("ılogin required"),
            Err(DialogError::UnreadableScreen)
        );
        assert_eq!(cache.screen_dialog_fail_closed("ılogin required"), "error");
        assert_eq!(cache.probe(), Ok(()));
    }

    /// Un patrón de configuración que el port no traduce se ve antes del
    /// `claim` (`probe`); los inválidos seguros se descartan como en el Python.
    #[test]
    fn probe_flags_untranslatable_config() {
        let dir = std::env::temp_dir().join(crate::fresh_id("dialogs-probe").unwrap());
        std::fs::create_dir_all(dir.join("config")).unwrap();
        let write = |text: &str| std::fs::write(dir.join("config/detectors.json"), text).unwrap();
        write(r#"{"dialogPatterns": {"login": ["(?P<x>a)", "sign in"]}}"#);
        let cache = DialogCache::new(&dir);
        assert_eq!(cache.probe(), Err(DialogError::BadPattern));
        assert_eq!(cache.screen_dialog("x"), Err(DialogError::BadPattern));
        assert_eq!(cache.screen_dialog_fail_closed("x"), "error");
        let cache = DialogCache::new(&dir);
        write(r#"{"dialogPatterns": {"login": ["(", "sign in"]}}"#);
        assert_eq!(cache.probe(), Ok(()));
        assert_eq!(cache.screen_dialog("please sign in"), Ok("login"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn attempts_follow_python_int() {
        assert_eq!(attempts_value(None), Ok(180));
        assert_eq!(attempts_value(Some(&json!(0))), Ok(180));
        assert_eq!(attempts_value(Some(&json!(7.9))), Ok(7));
        assert_eq!(attempts_value(Some(&json!(" 12 "))), Ok(12));
        assert_eq!(attempts_value(Some(&json!(true))), Ok(1));
        assert!(attempts_value(Some(&json!([1]))).is_err());
    }
}
