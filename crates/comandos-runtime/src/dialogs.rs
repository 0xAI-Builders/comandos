//! Diálogos de arranque de un CLI (`bin/cc-dash`, `dialog_patterns`,
//! `screen_dialog`, `verify_attempts`): los patrones de
//! `config/detectors.json` (en/es) con los de `_DEFAULT_DIALOGS` como
//! respaldo, cacheados por `mtime` como `_DIALOG_CACHE`.
//!
//! Los patrones son `re` de Python: se traducen con `providers::py_regex_lines`
//! y solo se buscan sobre pantallas donde el resultado es el mismo con certeza
//! (`Screen::search`: los caracteres cuya clase de palabra difiere entre
//! `re` y `regex` se reclasifican antes de buscar). Un patrón que el port no traduce es
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
    sync::{Arc, LazyLock, Mutex},
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
    regex: Result<Regex, Unsure>,
    traits: Traits,
}

/// Lo que de un patrón importa para saber si una pantalla se lee igual en
/// `re` y en `regex`.
#[derive(Debug, Clone, Copy, Default)]
struct Traits {
    /// Usa `\b \B \w \W`: depende de qué es palabra.
    words: bool,
    /// Usa `\B`: `re.search(r"\B", "")` no casa en el Python; `regex` sí.
    not_boundary: bool,
    /// Usa `\s \S`: el `\s` de Python incluye U+001C–U+001F.
    spaces: bool,
    /// Tiene `i`/`I` o una clase: bajo `re.I`, Python iguala `İ`/`ı` a `i`.
    letter_i: bool,
    /// Su propio texto lleva un carácter de clase de palabra discrepante.
    odd_literal: bool,
}

impl Traits {
    fn of(pattern: &str) -> Self {
        let chars: Vec<char> = pattern.chars().collect();
        let mut traits = Self {
            letter_i: chars.iter().any(|c| matches!(c, 'i' | 'I' | '[')),
            odd_literal: chars.iter().any(|c| word_disagrees(*c)),
            ..Self::default()
        };
        let mut i = 0;
        while let Some(&c) = chars.get(i) {
            if c == '\\' {
                match chars.get(i + 1) {
                    Some('b' | 'w' | 'W') => traits.words = true,
                    Some('B') => {
                        traits.words = true;
                        traits.not_boundary = true;
                    }
                    Some('s' | 'S') => traits.spaces = true,
                    _ => {}
                }
                i += 2;
            } else {
                i += 1;
            }
        }
        traits
    }
}

fn entry(pattern: &str) -> Entry {
    Entry {
        regex: providers::py_regex_lines(pattern, true),
        traits: Traits::of(pattern),
    }
}

/// ¿Es `c` palabra en un motor y no en el otro? Python 3.10 (`re`, texto):
/// `c.isalnum() or c == '_'`, es decir letras (`\p{L}`), números (`\p{N}`,
/// también `²` o `①`) y `_`. `regex` (UTS #18): `Alphabetic`, marcas
/// (`\p{M}`), `\p{Nd}`, `\p{Pc}` y `Join_Control`. Discrepan las marcas
/// combinantes, la puntuación conectora distinta de `_`, ZWJ/ZWNJ, los números
/// que no son `Nd`/`Nl` y los símbolos alfabéticos (`Ⓐ`). Letras y dígitos de
/// cualquier escritura (griego, CJK, cirílico) coinciden.
fn word_disagrees(c: char) -> bool {
    static WORDS: LazyLock<Option<(Regex, Regex)>> = LazyLock::new(|| {
        Some((
            Regex::new(r"^\w$").ok()?,
            Regex::new(r"^[\p{L}\p{N}_]$").ok()?,
        ))
    });
    if c.is_ascii() {
        return false;
    }
    let mut buf = [0; 4];
    let text = c.encode_utf8(&mut buf);
    // Sin las tablas: mejor dudar de todo lo no ASCII.
    WORDS
        .as_ref()
        .is_none_or(|(rust, python)| rust.is_match(text) != python.is_match(text))
}

/// Una pantalla preparada una vez para todos los patrones.
struct Screen<'a> {
    text: &'a str,
    /// La pantalla con cada carácter discrepante cambiado por uno que
    /// `regex` clasifica como `re` clasifica el original: `ʬ` (U+02AC, letra
    /// sin mayúsculas) si es palabra en Python, U+E000 (uso privado) si no.
    /// Ninguno de los dos es espacio ni dígito en ningún motor, y `.` casa
    /// con ambos. `None` si no hay ninguno.
    reclassified: Option<String>,
    /// U+001C–U+001F.
    controls: bool,
    /// `İ` o `ı`.
    dotted: bool,
}

impl<'a> Screen<'a> {
    fn new(text: &'a str) -> Self {
        static PYTHON_WORD: LazyLock<Option<Regex>> =
            LazyLock::new(|| Regex::new(r"^[\p{L}\p{N}_]$").ok());
        let mut reclassified = None::<String>;
        for (at, c) in text.char_indices() {
            if word_disagrees(c) {
                let out =
                    reclassified.get_or_insert_with(|| text.get(..at).unwrap_or("").to_owned());
                let mut buf = [0; 4];
                let python_word = PYTHON_WORD
                    .as_ref()
                    .is_some_and(|re| re.is_match(c.encode_utf8(&mut buf)));
                out.push(if python_word { '\u{2AC}' } else { '\u{E000}' });
            } else if let Some(out) = reclassified.as_mut() {
                out.push(c);
            }
        }
        Self {
            text,
            reclassified,
            controls: text.chars().any(|c| ('\u{1c}'..='\u{1f}').contains(&c)),
            dotted: text.chars().any(|c| matches!(c, '\u{130}' | '\u{131}')),
        }
    }

    /// `re.search(pattern, screen, re.I | re.M)` con certeza, o `Unsure`.
    fn search(&self, regex: &Regex, traits: Traits) -> Result<bool, Unsure> {
        let doubtful = (self.text.is_empty() && traits.not_boundary)
            || (self.controls && traits.spaces)
            || (self.dotted && traits.letter_i);
        if doubtful {
            return Err(Unsure);
        }
        match &self.reclassified {
            Some(fixed) if traits.words => {
                if traits.odd_literal {
                    return Err(Unsure);
                }
                Ok(regex.is_match(fixed))
            }
            _ => Ok(regex.is_match(self.text)),
        }
    }
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
            entries.push(entry(pattern));
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
                        .map(|pattern| entry(&pattern))
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
        let text = Screen::new(screen);
        for kind in KINDS {
            match patterns.get(kind) {
                None => {}
                Some(Kind::Unsure) => return Err(DialogError::BadPattern),
                Some(Kind::Known(entries)) => {
                    for entry in entries {
                        let regex = entry.regex.as_ref().map_err(|_| DialogError::BadPattern)?;
                        let found = text
                            .search(regex, entry.traits)
                            .map_err(|_| DialogError::UnreadableScreen)?;
                        if found {
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

    /// Vivacidad: texto griego, CJK o cirílico en pantalla se lee igual que en
    /// Python (letras y números son palabra en ambos motores); solo los
    /// caracteres cuya clase de palabra difiere cuentan, y se reclasifican.
    #[test]
    fn non_latin_screens_are_read() {
        let dir = std::env::temp_dir().join(crate::fresh_id("dialogs-scripts").unwrap());
        let cache = DialogCache::new(&dir);
        assert_eq!(
            cache.screen_dialog("Παρακαλώ συνδεθείτε\n /login για συνέχεια"),
            Ok("login")
        );
        assert_eq!(
            cache.screen_dialog("请先登录\n/login 继续使用"),
            Ok("login")
        );
        assert_eq!(
            cache.screen_dialog("Войдите\n  /login чтобы продолжить"),
            Ok("login")
        );
        assert_eq!(
            cache.screen_dialog("Стоимость: estimated cost of max — дорого"),
            Ok("effort")
        );
        // Una letra pegada al límite: palabra en ambos motores, no hay `\b`.
        assert_eq!(cache.screen_dialog("/loginλ ok"), Ok(""));
        assert_eq!(cache.screen_dialog("/login中"), Ok(""));
        // Una marca combinante pegada al límite: `regex` la toma por palabra y
        // `re` no; se reclasifica y el resultado es el del Python.
        assert_eq!(cache.screen_dialog("/login\u{301} x"), Ok("login"));
        assert_eq!(cache.screen_dialog("/login\u{2082}"), Ok(""));
        assert_eq!(cache.screen_dialog("/login\u{200d}"), Ok("login"));
    }

    /// Lo que queda incierto: un patrón cuyo propio texto contiene uno de
    /// esos caracteres (no se puede reclasificar sin cambiar el literal), `İ`
    /// con un patrón que tiene `i`, U+001C–U+001F con `\s`.
    #[test]
    fn remaining_doubts_are_narrow() {
        let dir = std::env::temp_dir().join(crate::fresh_id("dialogs-narrow").unwrap());
        std::fs::create_dir_all(dir.join("config")).unwrap();
        std::fs::write(
            dir.join("config/detectors.json"),
            "{\"dialogPatterns\": {\"trust\": [\"x\\u0301\\\\b\"], \"login\": [\"\\\\sZQ\"], \"onboarding\": [\"tema\"], \"effort\": [], \"error\": []}}",
        )
        .unwrap();
        let cache = DialogCache::new(&dir);
        assert_eq!(cache.probe(), Ok(()));
        assert_eq!(
            cache.screen_dialog("a\u{301}b"),
            Err(DialogError::UnreadableScreen)
        );
        assert_eq!(
            cache.screen_dialog("x\u{301} tema"),
            Err(DialogError::UnreadableScreen)
        );
        assert_eq!(
            cache.screen_dialog("\u{1c}ZQ"),
            Err(DialogError::UnreadableScreen)
        );
        assert_eq!(
            cache.screen_dialog("x tema \u{1c}"),
            Err(DialogError::UnreadableScreen)
        );
        assert_eq!(cache.screen_dialog("ıyi tema"), Ok("onboarding"));
        assert_eq!(cache.screen_dialog("xz"), Ok(""));
        let _ = std::fs::remove_dir_all(&dir);
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
