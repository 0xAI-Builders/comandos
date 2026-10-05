//! `lib/tui_state.py`: configuración aplicada de una TUI (pantalla, transcript,
//! metadatos de Grok) y el rastreador «la evidencia que cambió gana».
//!
//! Las expresiones de `re` se portan a mano con las clases de CPython 3.10
//! (`\s` = `str.isspace`, que incluye U+001C–U+001F). Donde `regex` y `re`
//! pueden discrepar sobre un carácter concreto (`\b`/`\w` junto a marcas o
//! letras que `re` no conoce, `\d` de versiones de Unicode posteriores, letras
//! que `re.I` pliega a ASCII) la lectura devuelve `Unsure`; una pantalla normal
//! (`●`, `⎿`, `❯`, emojis) nunca lo provoca.
use crate::Unsure;
use crate::hooks::py::{float_value, int_text, is_float, repr};
use comandos_core::json::{python_eq, truthy, workspace_loads};
use comandos_core::text::splitlines;
use regex::Regex;
use serde_json::{Map, Value};
use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

pub type Obs = Map<String, Value>;

const EFFORT: &str = "(none|minimal|low|medium|high|xhigh|max|ultra|ultracode|auto)";
const EFFORTS: [&str; 10] = [
    "none",
    "minimal",
    "low",
    "medium",
    "high",
    "xhigh",
    "max",
    "ultra",
    "ultracode",
    "auto",
];
const MODEL: &str = r"(?:gpt-[a-z0-9.\-]+|claude-[a-z0-9.\-]+|grok-[a-z0-9.\-]+|(?:opus|sonnet|haiku|fable)(?:-[a-z0-9.]+)?)(?:\[1m\])?";

/// Profundidad de anidamiento a partir de la cual `json.loads` de CPython 3.10
/// puede acabar en `RecursionError`: el resultado del Python es desconocido.
/// El límite de recursión (1000) cuenta también los marcos que ya ocupa el hilo
/// de `cc-dash` al leer el transcript (arranque del hilo, `socketserver`,
/// manejador HTTP, `read_states`, `reconcile_card_config`, `read`): en
/// CPython 3.10.12 un `json.loads` en el nivel superior falla hacia los 997
/// niveles, así que 900 es seguro mientras esa pila no pase de ~95 marcos
/// (hoy son unos 20–30). Si el heredado ganara mucha profundidad de llamada,
/// este umbral tendría que bajar.
const PY_JSON_DEPTH: usize = 900;

/// Caracteres que bajo `re.I` casan con una letra ASCII en uno de los dos
/// motores y no en el otro (İ, ı, ſ, signo Kelvin).
const FOLDS: [(char, char); 4] = [
    ('\u{130}', 'i'),
    ('\u{131}', 'i'),
    ('\u{17f}', 's'),
    ('\u{212a}', 'k'),
];

type Pattern = LazyLock<Result<Regex, regex::Error>>;

/// `\s` de `re` es `str.isspace()`: el `\s` de `regex` más U+001C–U+001F.
fn compile(pattern: &str) -> Result<Regex, regex::Error> {
    Regex::new(&pattern.replace(r"\s", r"[\s\x1c-\x1f]"))
}

macro_rules! pattern {
    ($name:ident, $value:expr) => {
        static $name: Pattern = LazyLock::new(|| compile(&$value));
    };
}

pattern!(MODEL_FIND, format!("(?i){MODEL}"));
pattern!(
    DISPLAY,
    r"(?i)\b(?:Claude\s+)?(Opus|Sonnet|Haiku|Fable)\s+(\d+(?:[.\-]\d+)*)"
);
pattern!(ONE_M, r"(?i)1M context|\[1m\]");
pattern!(GROK, r"(?i)\bGrok\s+(\d+(?:\.\d+)*)");
pattern!(ANSI, r"\x1b\[[0-?]*[ -/]*[@-~]");
pattern!(
    CODEX_FOOTER,
    format!(r"(?i)^(?:\s*({MODEL})\s+{EFFORT}\s*[·•].*)$")
);
pattern!(OPENCODE, r"^\s*┃\s+[\w-]+\s+·\s+(.+)$");
pattern!(OPENCODE_EFFORT, format!(r"(?i)[·•]\s*{EFFORT}\s*$"));
pattern!(CLAUDE_RESPONSE, r"^\s*●\s");
pattern!(CLAUDE_PROMPT, r"^\s*❯\s*/(?:model|effort)\b");
pattern!(CLAUDE_PROMPT_PREFIX, r"^\s*❯\s*/(?:model|effort)");
pattern!(
    CLAUDE_CONFIRM,
    r"^\s*⎿\s+(?:Set model to|Kept model as|Current model:)\s+(.+)"
);
pattern!(
    CLAUDE_COMBINED,
    format!(r"(?i)\b(?:with|and)\s+{EFFORT}\s+effort\b")
);
pattern!(
    CLAUDE_EFFORT,
    format!(r"(?i)^\s*⎿\s+(?:Set effort level to|Effort(?: level)? set to)\s+{EFFORT}\b")
);
pattern!(
    CLAUDE_EFFORT_PREFIX,
    r"(?i)^\s*⎿\s+(?:Set effort level to|Effort(?: level)? set to)\s+"
);
pattern!(OPENCODE_PREFIX, r"^\s*┃\s+");
pattern!(OPENCODE_TAIL, r"^\s+·\s+.+$");
pattern!(WORD, r"\w");
pattern!(
    CLAUDE_STATUS,
    format!(r"(?i)[·•]\s*claude\s*[·•]\s*({MODEL})")
);
// `\w` de `re` es `str.isalnum() or '_'` (Unicode 13 en CPython 3.10): letras y
// números. Aquí, todo carácter cuya condición de palabra puede diferir.
pattern!(
    WORD_DIVERGENT,
    r"[[\w--[\p{L}\p{N}_]][[\p{L}\p{N}]--\w][[\w\p{L}\p{N}]&&\P{Age=13.0}]]"
);
// `\d` de `re` es `str.isdecimal()`: igual que `\p{Nd}` salvo versión de Unicode.
pattern!(DIGIT_DIVERGENT, r"[\d&&\P{Age=13.0}]");

fn get(pattern: &'static Pattern) -> Result<&'static Regex, Unsure> {
    pattern.as_ref().map_err(|_| Unsure)
}

/// `str.isspace()` de CPython.
fn is_space(c: char) -> bool {
    c.is_whitespace() || ('\u{1c}'..='\u{1f}').contains(&c)
}

fn starts_after_space(line: &str, mark: char) -> bool {
    line.trim_start_matches(is_space).starts_with(mark)
}

fn is_in(pattern: &'static Pattern, c: char) -> Result<bool, Unsure> {
    let mut buf = [0u8; 4];
    Ok(get(pattern)?.is_match(c.encode_utf8(&mut buf)))
}

/// Texto con los `FOLDS` cambiados por su letra ASCII y los desplazamientos
/// (en el texto nuevo) de los cambiados. Bajo `re.I`, el Python trata cada uno
/// como su letra ASCII; `regex` solo pliega ſ y K. Buscar en el texto cambiado
/// reproduce al Python mientras la coincidencia no toque uno de ellos (si lo
/// toca, el `.lower()` del Python conservaría el carácter original).
fn unfold(s: &str) -> (String, Vec<usize>) {
    let mut out = String::with_capacity(s.len());
    let mut at = Vec::new();
    for c in s.chars() {
        let Some(&(_, ascii)) = FOLDS.iter().find(|(fold, _)| *fold == c) else {
            out.push(c);
            continue;
        };
        at.push(out.len());
        out.push(ascii);
    }
    (out, at)
}

fn overlaps(m: &regex::Match<'_>, at: &[usize]) -> bool {
    at.iter().any(|&i| m.start() <= i && i < m.end())
}

/// Búsqueda de una expresión con `re.I`: los grupos de la coincidencia, o
/// `Unsure` si toca un carácter plegable.
fn ci_captures(
    pattern: &'static Pattern,
    s: &str,
    groups: usize,
) -> Result<Option<Vec<String>>, Unsure> {
    let (mapped, folded) = unfold(s);
    let Some(caps) = get(pattern)?.captures(&mapped) else {
        return Ok(None);
    };
    if caps.get(0).is_some_and(|m| overlaps(&m, &folded)) {
        return Err(Unsure);
    }
    Ok(Some(
        (1..=groups).map(|g| group(&caps, g).to_owned()).collect(),
    ))
}

/// Primer grupo de `ci_captures`.
fn ci_first(pattern: &'static Pattern, s: &str) -> Result<Option<String>, Unsure> {
    Ok(ci_captures(pattern, s, 1)?.and_then(|g| g.into_iter().next()))
}

/// Un carácter cuya condición de palabra difiere entre `re` y `regex`.
fn divergent(c: Option<char>) -> Result<bool, Unsure> {
    match c {
        Some(c) => is_in(&WORD_DIVERGENT, c),
        None => Ok(false),
    }
}

/// Los `\b` de estas expresiones están antes de una palabra de `before` o
/// después de una de `after`: discrepan solo si ahí hay un carácter divergente.
fn edge_near(s: &str, before: &[&str], after: &[&str]) -> Result<bool, Unsure> {
    let at = |i: usize, w: &str| {
        s.get(i..i + w.len())
            .is_some_and(|t| t.eq_ignore_ascii_case(w))
    };
    for (i, _) in s.char_indices() {
        for w in before {
            if at(i, w) && divergent(s.get(..i).and_then(|h| h.chars().next_back()))? {
                return Ok(true);
            }
        }
        for w in after {
            if at(i, w) && divergent(s.get(i + w.len()..).and_then(|t| t.chars().next()))? {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

fn any_in(pattern: &'static Pattern, s: &str) -> Result<bool, Unsure> {
    for c in s.chars() {
        if is_in(pattern, c)? {
            return Ok(true);
        }
    }
    Ok(false)
}

fn group<'h>(caps: &regex::Captures<'h>, index: usize) -> &'h str {
    caps.get(index).map_or("", |m| m.as_str())
}

/// `model_id` (16): identificador de modelo de una etiqueta de pantalla.
pub fn model_id(label: &str) -> Result<String, Unsure> {
    const DISPLAY_WORDS: [&str; 6] = ["claude", "opus", "sonnet", "haiku", "fable", "grok"];
    if any_in(&DIGIT_DIVERGENT, label)? || edge_near(label, &DISPLAY_WORDS, &[])? {
        return Err(Unsure);
    }
    let (label, folded) = unfold(label);
    let found: Vec<regex::Match<'_>> = get(&MODEL_FIND)?.find_iter(&label).collect();
    if found.iter().any(|m| overlaps(m, &folded)) {
        return Err(Unsure);
    }
    let matches: Vec<&str> = found.iter().map(|m| m.as_str()).collect();
    // Un ID concreto entre paréntesis vale más que el alias que lo precede.
    if matches.iter().any(|m| m.contains('-'))
        && let Some(last) = matches.last()
    {
        return Ok(last.to_lowercase());
    }
    if let Some(caps) = get(&DISPLAY)?.captures(&label) {
        if caps.get(0).is_some_and(|m| overlaps(&m, &folded)) {
            return Err(Unsure);
        }
        // `ONE_M` solo decide si casa: el texto cambiado da la misma respuesta.
        let suffix = if get(&ONE_M)?.is_match(&label) {
            "[1m]"
        } else {
            ""
        };
        return Ok(format!(
            "claude-{}-{}{suffix}",
            group(&caps, 1).to_lowercase(),
            group(&caps, 2).replace('.', "-")
        ));
    }
    if let Some(caps) = get(&GROK)?.captures(&label) {
        if caps.get(0).is_some_and(|m| overlaps(&m, &folded)) {
            return Err(Unsure);
        }
        return Ok(format!("grok-{}", group(&caps, 1)));
    }
    Ok(matches.last().map(|m| m.to_lowercase()).unwrap_or_default())
}

fn text(value: &str) -> Value {
    Value::String(value.to_owned())
}

/// `[\w-]+` de la línea `┃` de opencode: el agente es la tira sin espacios que
/// sigue al borde. Incierto solo si contiene un carácter divergente, ninguno
/// que las dos `\w` rechacen y el resto de la línea casaría.
fn opencode_token_unsure(line: &str) -> Result<bool, Unsure> {
    let Some(prefix) = get(&OPENCODE_PREFIX)?.find(line) else {
        return Ok(false);
    };
    let after = line.get(prefix.end()..).unwrap_or("");
    let end = after.find(is_space).unwrap_or(after.len());
    let (token, tail) = after.split_at_checked(end).unwrap_or((after, ""));
    let (mut divergent_char, mut broken) = (false, token.is_empty());
    for c in token.chars().filter(|&c| c != '-') {
        if is_in(&WORD_DIVERGENT, c)? {
            divergent_char = true;
        } else if !is_in(&WORD, c)? {
            broken = true;
        }
    }
    Ok(divergent_char && !broken && get(&OPENCODE_TAIL)?.is_match(tail))
}

/// El `\b` tras la palabra de esfuerzo de una línea `⎿`: incierto si alguna
/// palabra de `EFFORT` que encaja ahí va seguida de un carácter divergente.
fn effort_edge_unsure(line: &str) -> Result<bool, Unsure> {
    let (mapped, _) = unfold(line);
    let Some(prefix) = get(&CLAUDE_EFFORT_PREFIX)?.find(&mapped) else {
        return Ok(false);
    };
    let rest = mapped.get(prefix.end()..).unwrap_or("");
    for word in EFFORTS {
        if rest
            .get(..word.len())
            .is_some_and(|t| t.eq_ignore_ascii_case(word))
            && divergent(rest.get(word.len()..).and_then(|t| t.chars().next()))?
        {
            return Ok(true);
        }
    }
    Ok(false)
}

/// `screen_state` (28): estado visible al pie de la pantalla de una TUI.
pub fn screen_state(harness: &str, screen: &str) -> Result<Obs, Unsure> {
    let clean = get(&ANSI)?.replace_all(screen, "");
    let lines = splitlines(&clean);
    let mut out = Obs::new();
    match harness {
        "codex" => {
            // Solo el pie dedicado; las opciones numeradas y la prosa no son estado.
            for line in lines.iter().skip(lines.len().saturating_sub(8)) {
                if !line.contains(['·', '•']) {
                    continue;
                }
                if let Some(hit) = ci_captures(&CODEX_FOOTER, line, 2)?
                    && let [model, effort] = hit.as_slice()
                {
                    out = Obs::new();
                    out.insert("model".into(), text(&model.to_lowercase()));
                    out.insert("effort".into(), text(&effort.to_lowercase()));
                    out.insert("kind".into(), text("status"));
                }
            }
        }
        "opencode" => {
            // Compositor de pantalla completa de 1.17.18: borde, agente, modelo y variante.
            for (line, next) in lines.iter().zip(lines.iter().skip(1)) {
                if !starts_after_space(line, '┃') {
                    continue;
                }
                let next_ok = next.trim_start_matches(is_space).starts_with("╹▀");
                if next_ok && opencode_token_unsure(line)? {
                    return Err(Unsure);
                }
                let Some(caps) = get(&OPENCODE)?.captures(line) else {
                    continue;
                };
                if !next_ok {
                    continue;
                }
                let rest = group(&caps, 1);
                let model = model_id(rest)?;
                if !model.is_empty() {
                    let effort = ci_first(&OPENCODE_EFFORT, rest)?
                        .map(|e| e.to_lowercase())
                        .unwrap_or_default();
                    out = Obs::new();
                    out.insert("model".into(), Value::String(model));
                    out.insert("effort".into(), Value::String(effort));
                    out.insert("kind".into(), text("status"));
                }
            }
        }
        "claude" => {
            let mut in_response = false;
            for line in &lines {
                if get(&CLAUDE_RESPONSE)?.is_match(line) {
                    // Una respuesta posterior deja viejas las salidas de órdenes anteriores.
                    out = Obs::new();
                    in_response = true;
                }
                if starts_after_space(line, '❯') {
                    // El `\b` solo mira el carácter tras `/model` o `/effort`.
                    if let Some(m) = get(&CLAUDE_PROMPT_PREFIX)?.find(line)
                        && divergent(line.get(m.end()..).and_then(|t| t.chars().next()))?
                    {
                        return Err(Unsure);
                    }
                    if get(&CLAUDE_PROMPT)?.is_match(line) {
                        in_response = false;
                    }
                }
                if in_response {
                    continue;
                }
                if let Some(hit) = get(&CLAUDE_CONFIRM)?.captures(line) {
                    let label = group(&hit, 1);
                    let model = model_id(label)?;
                    if !model.is_empty() {
                        out = Obs::new();
                        out.insert("model".into(), Value::String(model));
                        out.insert("kind".into(), text("confirmation"));
                        if edge_near(label, &["with", "and"], &["effort"])? {
                            return Err(Unsure);
                        }
                        if let Some(effort) = ci_first(&CLAUDE_COMBINED, label)? {
                            out.insert("effort".into(), text(&effort.to_lowercase()));
                        }
                    }
                }
                if starts_after_space(line, '⎿') {
                    if effort_edge_unsure(line)? {
                        return Err(Unsure);
                    }
                    if let Some(effort) = ci_first(&CLAUDE_EFFORT, line)? {
                        let lower = line.to_lowercase();
                        if !lower.contains("but") && !lower.contains("not applied") {
                            out.insert("effort".into(), text(&effort.to_lowercase()));
                            out.insert("kind".into(), text("confirmation"));
                        }
                    }
                }
            }
            for line in lines.iter().skip(lines.len().saturating_sub(5)) {
                // Las confirmaciones nativas pueden ser más nuevas que un script de
                // estado: con `out` ya lleno la búsqueda no cambia nada.
                if !out.is_empty() || !line.contains(['·', '•']) {
                    continue;
                }
                if let Some(model) = ci_first(&CLAUDE_STATUS, line)? {
                    out.insert("model".into(), text(&model.to_lowercase()));
                    out.insert("kind".into(), text("custom-status"));
                }
            }
        }
        _ => {}
    }
    Ok(out)
}

/// `OrderedDict` del Python: `get`, `move_to_end`, asignación y `popitem(last=False)`.
struct Lru<K, V> {
    max: usize,
    items: Vec<(K, V)>,
}

impl<K: PartialEq, V> Lru<K, V> {
    fn new(max: usize) -> Self {
        Self {
            max,
            items: Vec::new(),
        }
    }
    fn position(&self, key: &K) -> Option<usize> {
        self.items.iter().position(|(k, _)| k == key)
    }
    fn get(&self, key: &K) -> Option<&V> {
        self.items.iter().find(|(k, _)| k == key).map(|(_, v)| v)
    }
    fn take(&mut self, key: &K) -> Option<V> {
        let index = self.position(key)?;
        (index < self.items.len()).then(|| self.items.remove(index).1)
    }
    /// `move_to_end`.
    fn touch(&mut self, key: &K) {
        if let Some(index) = self.position(key)
            && index < self.items.len()
        {
            let item = self.items.remove(index);
            self.items.push(item);
        }
    }
    /// `d[key] = value`: una clave existente conserva su sitio.
    fn set(&mut self, key: K, value: V) {
        match self.position(&key) {
            Some(index) => {
                if let Some(slot) = self.items.get_mut(index) {
                    slot.1 = value;
                }
            }
            None => self.items.push((key, value)),
        }
    }
    /// Expulsa los más viejos por encima del máximo.
    fn trim(&mut self) {
        while self.items.len() > self.max && !self.items.is_empty() {
            self.items.remove(0);
        }
    }
    /// `d[key] = value; d.move_to_end(key); trim`.
    fn put(&mut self, key: K, value: V) {
        let _ = self.take(&key);
        self.items.push((key, value));
        self.trim();
    }
}

/// `(st_dev, st_ino, st_size, st_mtime_ns)` de `os.stat`.
type Signature = (u64, u64, u64, i128);

fn signature(meta: &fs::Metadata) -> Signature {
    let mtime_ns = i128::from(meta.mtime()) * 1_000_000_000 + i128::from(meta.mtime_nsec());
    (meta.dev(), meta.ino(), meta.size(), mtime_ns)
}

/// `repr` de la tupla de firma.
fn signature_repr(s: &Signature) -> String {
    format!("({}, {}, {}, {})", s.0, s.1, s.2, s.3)
}

/// `bytes.decode('utf-8', 'surrogatepass')`: los sustitutos codificados pasan
/// como U+FFFD y la línea queda «retocada»; lo demás inválido es `None`.
fn decode_utf8(mut rest: &[u8]) -> Option<(String, bool)> {
    let mut out = String::new();
    let mut touched = false;
    loop {
        match std::str::from_utf8(rest) {
            Ok(tail) => {
                out.push_str(tail);
                return Some((out, touched));
            }
            Err(error) => {
                let (good, bad) = rest.split_at_checked(error.valid_up_to())?;
                out.push_str(std::str::from_utf8(good).ok()?);
                match bad {
                    [0xED, b1, b2, ..]
                        if (0xA0..=0xBF).contains(b1) && (0x80..=0xBF).contains(b2) =>
                    {
                        out.push('\u{fffd}');
                        touched = true;
                        rest = bad.get(3..)?;
                    }
                    _ => return None,
                }
            }
        }
    }
}

fn decode_utf16(raw: &[u8], big: bool) -> Option<(String, bool)> {
    let units = raw.chunks_exact(2);
    if !units.remainder().is_empty() {
        return None;
    }
    let units = units.map(|u| match u {
        [a, b] if big => u16::from_be_bytes([*a, *b]),
        [a, b] => u16::from_le_bytes([*a, *b]),
        _ => 0,
    });
    let mut touched = false;
    let out = char::decode_utf16(units)
        .map(|c| {
            c.unwrap_or_else(|_| {
                touched = true;
                '\u{fffd}'
            })
        })
        .collect();
    Some((out, touched))
}

fn decode_utf32(raw: &[u8], big: bool) -> Option<(String, bool)> {
    let units = raw.chunks_exact(4);
    if !units.remainder().is_empty() {
        return None;
    }
    let mut out = String::new();
    let mut touched = false;
    for unit in units {
        let n = match unit {
            [a, b, c, d] if big => u32::from_be_bytes([*a, *b, *c, *d]),
            [a, b, c, d] => u32::from_le_bytes([*a, *b, *c, *d]),
            _ => return None,
        };
        if (0xD800..=0xDFFF).contains(&n) {
            touched = true;
            out.push('\u{fffd}');
        } else {
            out.push(char::from_u32(n)?);
        }
    }
    Some((out, touched))
}

/// `json.loads(bytes)` decodifica con `json.detect_encoding` y `surrogatepass`.
fn decode_json_bytes(b: &[u8]) -> Option<(String, bool)> {
    match b {
        [0x00, 0x00, 0xFE, 0xFF, rest @ ..] => decode_utf32(rest, true),
        [0xFF, 0xFE, 0x00, 0x00, rest @ ..] => decode_utf32(rest, false),
        [0xFE, 0xFF, rest @ ..] => decode_utf16(rest, true),
        [0xFF, 0xFE, rest @ ..] => decode_utf16(rest, false),
        [0xEF, 0xBB, 0xBF, rest @ ..] => decode_utf8(rest),
        [0, b1, _, _, ..] => {
            if *b1 != 0 {
                decode_utf16(b, true)
            } else {
                decode_utf32(b, true)
            }
        }
        [_, 0, b2, b3, ..] => {
            if *b2 != 0 || *b3 != 0 {
                decode_utf16(b, false)
            } else {
                decode_utf32(b, false)
            }
        }
        [0, _] => decode_utf16(b, true),
        [_, 0] => decode_utf16(b, false),
        _ => decode_utf8(b),
    }
}

fn hex4(s: &str) -> Option<u32> {
    let digits = s.get(..4)?;
    if digits.bytes().all(|b| b.is_ascii_hexdigit()) {
        u32::from_str_radix(digits, 16).ok()
    } else {
        None
    }
}

/// El `json` del Python acepta `\uD800`–`\uDFFF` sueltos; `serde_json` no. Se
/// cambian por `�` y se marca la línea como retocada.
fn replace_lone_surrogates(raw: &str) -> (String, bool) {
    let mut out = String::with_capacity(raw.len());
    let mut touched = false;
    let mut in_string = false;
    let mut rest = raw;
    while let Some(c) = rest.chars().next() {
        let width = c.len_utf8();
        if !in_string {
            in_string = c == '"';
            out.push(c);
            rest = rest.get(width..).unwrap_or("");
            continue;
        }
        match c {
            '"' => {
                in_string = false;
                out.push(c);
                rest = rest.get(1..).unwrap_or("");
            }
            '\\' => {
                let after = rest.get(1..).unwrap_or("");
                let escape = after.strip_prefix('u').and_then(hex4);
                match escape {
                    Some(high @ 0xD800..=0xDBFF) => {
                        let low = after
                            .get(5..)
                            .and_then(|s| s.strip_prefix("\\u"))
                            .and_then(hex4);
                        if low.is_some_and(|l| (0xDC00..=0xDFFF).contains(&l)) {
                            out.push_str(rest.get(..12).unwrap_or(""));
                            rest = rest.get(12..).unwrap_or("");
                        } else {
                            let _ = high;
                            out.push_str("\\ufffd");
                            touched = true;
                            rest = rest.get(6..).unwrap_or("");
                        }
                    }
                    Some(0xDC00..=0xDFFF) => {
                        out.push_str("\\ufffd");
                        touched = true;
                        rest = rest.get(6..).unwrap_or("");
                    }
                    _ => {
                        // Cualquier otro escape (o uno inválido) se copia tal cual.
                        let next = after.chars().next().map_or(0, char::len_utf8);
                        out.push_str(rest.get(..1 + next).unwrap_or(rest));
                        rest = rest.get(1 + next..).unwrap_or("");
                    }
                }
            }
            _ => {
                out.push(c);
                rest = rest.get(width..).unwrap_or("");
            }
        }
    }
    (out, touched)
}

/// `sys.get_int_max_str_digits()` por omisión de CPython 3.10.7+: `json.loads`
/// de un entero con más dígitos es `ValueError`.
const PY_INT_MAX_STR_DIGITS: usize = 4300;

/// Fuera de cadenas: profundidad máxima de `[`/`{` (vale para líneas cortadas) y
/// si algún entero supera el límite de dígitos de CPython.
fn scan_json(raw: &str) -> (usize, bool) {
    let (mut depth, mut max, mut in_string, mut escaped) = (0usize, 0usize, false, false);
    let mut huge_int = false;
    let mut token = String::new();
    let mut close = |token: &mut String| {
        let digits = token.strip_prefix('-').unwrap_or(token);
        if digits.len() > PY_INT_MAX_STR_DIGITS && digits.bytes().all(|b| b.is_ascii_digit()) {
            huge_int = true;
        }
        token.clear();
    };
    for b in raw.bytes() {
        if in_string {
            match b {
                _ if escaped => escaped = false,
                b'\\' => escaped = true,
                b'"' => in_string = false,
                _ => {}
            }
            continue;
        }
        match b {
            b'"' => {
                close(&mut token);
                in_string = true;
            }
            b'[' | b'{' => {
                close(&mut token);
                depth += 1;
                max = max.max(depth);
            }
            b']' | b'}' => {
                close(&mut token);
                depth = depth.saturating_sub(1);
            }
            b',' | b':' | b' ' | b'\t' | b'\r' | b'\n' => close(&mut token),
            other => token.push(char::from(other)),
        }
    }
    close(&mut token);
    (max, huge_int)
}

/// `json.loads(texto)`: `None` es el `ValueError` del Python; `Unsure` si el
/// anidamiento puede acabar en `RecursionError`.
pub(crate) fn loads_text(raw: &str) -> Result<Option<(Value, bool)>, Unsure> {
    let (fixed, touched) = replace_lone_surrogates(raw);
    let (depth, huge_int) = scan_json(&fixed);
    if depth >= PY_JSON_DEPTH {
        return Err(Unsure);
    }
    if huge_int {
        return Ok(None);
    }
    Ok(workspace_loads(&fixed).ok().map(|v| (v, touched)))
}

pub(crate) fn loads_bytes(raw: &[u8]) -> Result<Option<(Value, bool)>, Unsure> {
    let Some((text, decoded_touched)) = decode_json_bytes(raw) else {
        return Ok(None);
    };
    Ok(loads_text(&text)?.map(|(v, touched)| (v, touched || decoded_touched)))
}

/// `bytes.splitlines()`: `\n`, `\r` y `\r\n`, sin la línea vacía final.
fn split_byte_lines(raw: &[u8]) -> Vec<&[u8]> {
    let mut out = Vec::new();
    let mut rest = raw;
    while !rest.is_empty() {
        match rest.iter().position(|b| matches!(b, b'\n' | b'\r')) {
            Some(i) => {
                let (line, tail) = rest.split_at_checked(i).unwrap_or((rest, &[]));
                out.push(line);
                let skip = if tail.starts_with(b"\r\n") { 2 } else { 1 };
                rest = tail.get(skip..).unwrap_or(&[]);
            }
            None => {
                out.push(rest);
                rest = &[];
            }
        }
    }
    out
}

fn contains_replacement(value: &Value) -> bool {
    match value {
        Value::String(s) => s.contains('\u{fffd}'),
        Value::Array(items) => items.iter().any(contains_replacement),
        Value::Object(map) => map
            .iter()
            .any(|(k, v)| k.contains('\u{fffd}') || contains_replacement(v)),
        _ => false,
    }
}

/// `re.fullmatch(r'[A-Za-z0-9][A-Za-z0-9._:@/\-\[\]]{0,159}', model)`.
fn valid_model(value: &Value) -> bool {
    let Some(model) = value.as_str() else {
        return false;
    };
    let mut chars = model.chars();
    let first_ok = chars.next().is_some_and(|c| c.is_ascii_alphanumeric());
    first_ok
        && model.chars().count() <= 160
        && chars.all(|c| c.is_ascii_alphanumeric() || "._:@/-[]".contains(c))
}

/// `re.fullmatch(EFFORT, effort)` sin `re.I`.
fn valid_effort(value: &Value) -> bool {
    value.as_str().is_some_and(|e| EFFORTS.contains(&e))
}

fn truthy_get<'a>(map: &'a Obs, key: &str) -> Option<&'a Value> {
    map.get(key).filter(|v| truthy(v))
}

/// `TranscriptCache` (77): solo `stat` en reposo; tras un cambio lee como mucho
/// `max_bytes` del final.
/// `(harness, session_id, str(path))`.
type TranscriptKey = (String, String, PathBuf);

pub struct TranscriptCache {
    /// La incertidumbre también se guarda con su firma: un transcript incierto y
    /// sin cambios no se vuelve a leer en cada sondeo.
    entries: Lru<TranscriptKey, (Signature, Result<Obs, Unsure>)>,
    max_bytes: u64,
    /// Lecturas de cola hechas (las pruebas comprueban que el acierto no lee).
    reads: usize,
}

impl TranscriptCache {
    pub fn new(max_entries: usize, max_bytes: u64) -> Self {
        Self {
            entries: Lru::new(max_entries),
            max_bytes,
            reads: 0,
        }
    }

    pub fn read(&mut self, harness: &str, session_id: &str, path: &Path) -> Result<Obs, Unsure> {
        // `os.stat` sigue enlaces (como `/proc/<pid>/fd/<n>`); un `OSError` es `{}`.
        let Ok(meta) = fs::metadata(path) else {
            return Ok(Obs::new());
        };
        let signature = signature(&meta);
        let key = (
            harness.to_owned(),
            session_id.to_owned(),
            path.to_path_buf(),
        );
        let previous = self.entries.get(&key).cloned();
        if let Some((sig, value)) = &previous
            && *sig == signature
        {
            self.entries.touch(&key);
            return value.clone();
        }
        let start = meta.size().saturating_sub(self.max_bytes);
        let Ok(raw) = read_tail(path, start, self.max_bytes) else {
            return Ok(Obs::new());
        };
        self.reads += 1;
        let lines = split_byte_lines(&raw);
        let lines = lines.get(usize::from(start > 0)..).unwrap_or(&[]);
        let mut result = parse_tail(harness, session_id, lines, &signature);
        // Una salida enorme de herramienta puede dejar el último estado fuera de la
        // cola: se conserva el anterior, también si era incierto.
        if result.as_ref().is_ok_and(Obs::is_empty)
            && let Some((sig, value)) = &previous
            && sig.0 == signature.0
            && sig.1 == signature.1
            && signature.2 >= sig.2
        {
            result = value.clone();
        }
        self.entries.put(key, (signature, result.clone()));
        result
    }

    #[cfg(test)]
    fn reads(&self) -> usize {
        self.reads
    }
}

/// Las filas de la cola en orden (100-137).
fn parse_tail(
    harness: &str,
    session_id: &str,
    lines: &[&[u8]],
    signature: &Signature,
) -> Result<Obs, Unsure> {
    let mut result = Obs::new();
    for (index, line) in lines.iter().enumerate() {
        let Some((row, touched)) = loads_bytes(line)? else {
            continue;
        };
        let Value::Object(row) = row else {
            continue;
        };
        if truthy_get(&row, "isSidechain").is_some()
            || truthy_get(&row, "parent_session_id").is_some()
        {
            continue;
        }
        let sid = truthy_get(&row, "sessionId").or_else(|| truthy_get(&row, "session_id"));
        if let Some(sid) = sid {
            let equal = sid.as_str() == Some(session_id);
            // Con un sustituto suelto el Python ve otra cadena distinta.
            if equal && touched && session_id.contains('\u{fffd}') {
                return Err(Unsure);
            }
            if !equal {
                continue;
            }
        }
        let mut value = extract(harness, &row)?;
        if value.get("model").is_some_and(|m| !valid_model(m)) {
            value.shift_remove("model");
        }
        if value.get("effort").is_some_and(|e| !valid_effort(e)) {
            value.shift_remove("effort");
        }
        if value.is_empty() {
            continue;
        }
        if let Some(model) = truthy_get(&value, "model")
            && !python_eq(model, result.get("model").unwrap_or(&Value::Null))
        {
            result.shift_remove("effort");
        }
        let revision = truthy_get(&row, "uuid")
            .or_else(|| truthy_get(&row, "timestamp"))
            .cloned()
            .unwrap_or_else(|| Value::String(format!("{}:{index}", signature_repr(signature))));
        if touched && contains_replacement(&revision) {
            return Err(Unsure);
        }
        for (k, v) in value {
            result.insert(k, v);
        }
        result.insert("revision".into(), revision);
    }
    Ok(result)
}

fn read_tail(path: &Path, start: u64, max_bytes: u64) -> std::io::Result<Vec<u8>> {
    let mut handle = fs::File::open(path)?;
    handle.seek(SeekFrom::Start(start))?;
    let mut raw = Vec::new();
    handle.take(max_bytes).read_to_end(&mut raw)?;
    Ok(raw)
}

/// La configuración que aporta una fila según el arnés (99-129).
fn extract(harness: &str, row: &Obs) -> Result<Obs, Unsure> {
    let mut value = Obs::new();
    let kind = row.get("type").and_then(Value::as_str);
    if harness == "claude" && kind == Some("assistant") {
        if let Some(Value::Object(msg)) = truthy_get(row, "message")
            && let Some(model) = truthy_get(msg, "model")
            && model.as_str() != Some("<synthetic>")
        {
            value.insert("model".into(), model.clone());
            if let Some(effort) =
                truthy_get(row, "effort").or_else(|| truthy_get(row, "perTurnEffort"))
            {
                value.insert("effort".into(), effort.clone());
            }
        }
    } else if harness == "claude" && kind == Some("user") {
        let content = match row.get("message") {
            Some(Value::Object(msg)) => msg.get("content"),
            _ => None,
        };
        // Claude guarda la SALIDA de una orden local en una fila de usuario etiquetada.
        if let Some(Value::String(content)) = content
            && let Some(rest) = content.strip_prefix("<local-command-stdout>")
        {
            let output = rest.split("</local-command-stdout>").next().unwrap_or("");
            let screen: Vec<String> = splitlines(output)
                .into_iter()
                .map(|line| format!("⎿ {line}"))
                .collect();
            value = screen_state("claude", &screen.join("\n"))?;
        }
    } else if harness == "codex"
        && kind == Some("turn_context")
        && let Some(Value::Object(payload)) = truthy_get(row, "payload")
        && let Some(model) = truthy_get(payload, "model")
    {
        value.insert("model".into(), model.clone());
        if let Some(effort) =
            truthy_get(payload, "effort").or_else(|| truthy_get(payload, "reasoning_effort"))
        {
            value.insert("effort".into(), effort.clone());
        }
    }
    Ok(value)
}

type Fingerprint = [Option<Value>; 4];

fn fingerprint(evidence: &Obs) -> Fingerprint {
    ["model", "effort", "revision", "kind"].map(|k| evidence.get(k).cloned())
}

/// `==` de dos tuplas de huella: `get` da `None` también para una clave con null.
fn same(a: &Fingerprint, b: &Fingerprint) -> bool {
    a.iter().zip(b).all(|(x, y)| {
        let x = x.as_ref().unwrap_or(&Value::Null);
        let y = y.as_ref().unwrap_or(&Value::Null);
        python_eq(x, y)
    })
}

struct Entry {
    value: Obs,
    conversation: Option<Fingerprint>,
    pane: Option<Fingerprint>,
    pane_kind: Option<Value>,
}

/// `StateTracker` (151): la evidencia que cambió gana; un pie viejo sin cambios
/// no deshace un turno nuevo. Una entrada por identidad (proceso + conversación).
pub struct StateTracker {
    entries: Lru<String, Entry>,
}

impl StateTracker {
    pub fn new(max_entries: usize) -> Self {
        Self {
            entries: Lru::new(max_entries),
        }
    }

    pub fn observe(
        &mut self,
        identity: &str,
        launch: &Obs,
        conversation: &Obs,
        visible: &Obs,
        now: f64,
    ) -> Obs {
        let key = identity.to_owned();
        let mut entry = self.entries.take(&key).unwrap_or_else(|| {
            let model = truthy_get(launch, "model").cloned();
            let source = if model.is_some() {
                "process"
            } else {
                "unconfirmed"
            };
            let mut value = Obs::new();
            value.insert("model".into(), model.unwrap_or_else(|| text("")));
            value.insert(
                "effort".into(),
                truthy_get(launch, "effort")
                    .cloned()
                    .unwrap_or_else(|| text("")),
            );
            value.insert("source".into(), text(source));
            Entry {
                value,
                conversation: None,
                pane: None,
                pane_kind: None,
            }
        });
        let confirmation = text("confirmation");
        let custom_status = text("custom-status");
        for (is_pane, evidence) in [(false, conversation), (true, visible)] {
            let print = fingerprint(evidence);
            if evidence.is_empty() {
                if is_pane && entry.pane_kind.as_ref() == Some(&confirmation) {
                    // `entry.pop('pane')`: suelta la huella; `paneKind` se queda.
                    entry.pane = None;
                }
                continue;
            }
            let kind = evidence.get("kind");
            let custom = kind == Some(&custom_status);
            if is_pane && custom {
                let source = entry.value.get("source").and_then(Value::as_str);
                if !conversation.is_empty() || matches!(source, Some("conversation" | "pane")) {
                    continue;
                }
            }
            let slot = if is_pane {
                &mut entry.pane
            } else {
                &mut entry.conversation
            };
            if slot.as_ref().is_some_and(|old| same(old, &print)) {
                continue;
            }
            *slot = Some(print);
            if is_pane {
                entry.pane_kind = kind.cloned();
            }
            if let Some(model) = truthy_get(evidence, "model")
                && !python_eq(model, entry.value.get("model").unwrap_or(&Value::Null))
            {
                entry.value.insert("effort".into(), text(""));
            }
            for field in ["model", "effort"] {
                if let Some(v) = evidence.get(field) {
                    entry.value.insert(field.into(), v.clone());
                }
            }
            let source = match (custom, is_pane) {
                (true, _) => "status-script",
                (false, true) => "pane",
                (false, false) => "conversation",
            };
            entry.value.insert("source".into(), text(source));
            entry.value.insert("evidenceAt".into(), float_value(now));
        }
        let out = entry.value.clone();
        self.entries.put(key, entry);
        out
    }
}

/// `str()` de un valor de JSON para el resumen de Grok. Los contenedores se
/// formatean con `repr`, que aquí solo es exacto en ASCII; un número no finito
/// se escribe como `float` (`nan`, `inf`).
fn py_str(value: &Value) -> Result<String, Unsure> {
    fn non_finite(n: &serde_json::Number) -> Option<&'static str> {
        match n.as_str() {
            "NaN" => Some("nan"),
            "Infinity" => Some("inf"),
            "-Infinity" => Some("-inf"),
            _ => None,
        }
    }
    fn has_non_finite(value: &Value) -> bool {
        match value {
            Value::Number(n) => non_finite(n).is_some(),
            Value::Array(items) => items.iter().any(has_non_finite),
            Value::Object(map) => map.values().any(has_non_finite),
            _ => false,
        }
    }
    match value {
        Value::String(s) => Ok(s.clone()),
        Value::Number(n) => Ok(non_finite(n).map_or_else(
            || {
                if is_float(n) {
                    repr(value)
                } else {
                    int_text(n)
                }
            },
            str::to_owned,
        )),
        Value::Array(_) | Value::Object(_) if has_non_finite(value) => Err(Unsure),
        other => {
            let out = repr(other);
            if out.is_ascii() { Ok(out) } else { Err(Unsure) }
        }
    }
}

/// `str(x or "")`.
fn str_or_empty(value: Option<&Value>) -> Result<String, Unsure> {
    match value.filter(|v| truthy(v)) {
        Some(v) => py_str(v),
        None => Ok(String::new()),
    }
}

/// `grok_state._summary_public` (`lib/grok_state.py:100`): `None` es el
/// `AttributeError` (`data` o `info` sin `.get`).
pub fn grok_summary_public(data: &Value) -> Result<Option<Obs>, Unsure> {
    let Value::Object(data) = data else {
        return Ok(None);
    };
    let empty = Obs::new();
    let info = match truthy_get(data, "info") {
        None => &empty,
        Some(Value::Object(info)) => info,
        Some(_) => return Ok(None),
    };
    let title = truthy_get(data, "generated_title").or_else(|| data.get("session_summary"));
    let mut out = Obs::new();
    out.insert("cwd".into(), Value::String(str_or_empty(info.get("cwd"))?));
    out.insert(
        "model".into(),
        Value::String(str_or_empty(data.get("current_model_id"))?),
    );
    out.insert(
        "effort".into(),
        Value::String(str_or_empty(data.get("reasoning_effort"))?),
    );
    out.insert("title".into(), Value::String(str_or_empty(title)?));
    out.insert("contextWindow".into(), Value::from(0));
    out.insert(
        "lastActiveAt".into(),
        Value::String(str_or_empty(data.get("last_active_at"))?),
    );
    Ok(Some(out))
}

/// Resultado de `int(x)` de Python para un `pid` de `active_sessions.json`.
enum PyInt {
    /// Entero que cabe en `i64`.
    Small(i64),
    /// Entero válido que nunca puede ser igual a un `pid` de `i64`.
    Huge,
    /// `ValueError`/`TypeError`: `_json` lo captura y devuelve `{}`.
    Invalid,
}

/// `int(r.get('pid') or 0)`. `OverflowError` (infinito) y los dígitos no ASCII
/// son `Unsure`.
fn py_int(value: Option<&Value>) -> Result<PyInt, Unsure> {
    let small = |n: i128| i64::try_from(n).map_or(PyInt::Huge, PyInt::Small);
    let Some(value) = value.filter(|v| truthy(v)) else {
        return Ok(PyInt::Small(0));
    };
    match value {
        Value::Bool(true) => Ok(PyInt::Small(1)),
        Value::Number(n)
            if !is_float(n) && !matches!(n.as_str(), "NaN" | "Infinity" | "-Infinity") =>
        {
            Ok(n.as_str().parse::<i128>().map_or(PyInt::Huge, small))
        }
        Value::Number(n) => {
            let f = n.as_f64().ok_or(Unsure)?;
            if f.is_nan() {
                Ok(PyInt::Invalid)
            } else if f.is_infinite() {
                Err(Unsure)
            } else if f.trunc().abs() < 1e30 {
                // Truncamiento hacia cero, como `int(float)`.
                Ok(small(f.trunc() as i128))
            } else {
                Ok(PyInt::Huge)
            }
        }
        Value::String(s) => {
            let body = s.trim_matches(is_space);
            if !body.is_ascii() {
                return Err(Unsure);
            }
            let (negative, digits) = match body.as_bytes().first() {
                Some(b'-') => (true, body.get(1..).unwrap_or("")),
                Some(b'+') => (false, body.get(1..).unwrap_or("")),
                _ => (false, body),
            };
            // Dígitos con `_` solo entre dígitos (PEP 515).
            let ok = !digits.is_empty()
                && !digits.starts_with('_')
                && !digits.ends_with('_')
                && !digits.contains("__")
                && digits.bytes().all(|b| b.is_ascii_digit() || b == b'_');
            if !ok {
                return Ok(PyInt::Invalid);
            }
            let clean: String = digits.chars().filter(char::is_ascii_digit).collect();
            Ok(match clean.parse::<i128>() {
                Ok(n) => small(if negative { -n } else { n }),
                Err(_) => PyInt::Huge,
            })
        }
        _ => Ok(PyInt::Invalid),
    }
}

/// Proyecciones que `GrokMetadataCache._json` guarda por ruta.
#[derive(Clone)]
enum Projected {
    /// `{int(pid): session_id}`; las claves fuera de `i64` no se guardan.
    Active(Vec<(i64, Value)>),
    Summary(Obs),
}

fn project_active(rows: &Value) -> Result<Option<Projected>, Unsure> {
    let items: &[Value] = match rows {
        Value::Array(items) => items,
        // Iterar un dict o una cadena da claves o caracteres: ninguno es un dict.
        Value::Object(_) | Value::String(_) => &[],
        _ => return Ok(None),
    };
    let mut out: Vec<(i64, Value)> = Vec::new();
    for row in items {
        let Value::Object(row) = row else {
            continue;
        };
        let sid = row.get("session_id").cloned().unwrap_or(Value::Null);
        match py_int(row.get("pid"))? {
            PyInt::Small(pid) => {
                // `dict` por comprensión: la última fila gana.
                out.retain(|(p, _)| *p != pid);
                out.push((pid, sid));
            }
            PyInt::Huge => {}
            PyInt::Invalid => return Ok(None),
        }
    }
    Ok(Some(Projected::Active(out)))
}

fn project_summary(data: &Value, touched: bool) -> Result<Option<Projected>, Unsure> {
    let Some(summary) = grok_summary_public(data)? else {
        return Ok(None);
    };
    // Un sustituto suelto que llega a la salida: el Python lo volcaría escapado.
    if touched && summary.values().any(contains_replacement) {
        return Err(Unsure);
    }
    Ok(Some(Projected::Summary(summary)))
}

/// `GrokMetadataCache` (191): resuelve una vez la sesión exacta de un PID y
/// después solo hace `stat` de su `summary.json`.
pub struct GrokMetadataCache {
    entries: Lru<PathBuf, (Signature, Projected)>,
    paths: Lru<(PathBuf, String), PathBuf>,
}

impl GrokMetadataCache {
    pub fn new(max_entries: usize) -> Self {
        Self {
            entries: Lru::new(max_entries),
            paths: Lru::new(max_entries),
        }
    }

    /// `_json` (198): `None` es el `{}` de una excepción capturada (no se guarda).
    fn json(
        &mut self,
        path: &Path,
        project: fn(&Value, bool) -> Result<Option<Projected>, Unsure>,
    ) -> Result<Option<Projected>, Unsure> {
        let Ok(meta) = fs::metadata(path) else {
            return Ok(None);
        };
        let signature = signature(&meta);
        let key = path.to_path_buf();
        if let Some((sig, value)) = self.entries.get(&key)
            && *sig == signature
        {
            let value = value.clone();
            self.entries.touch(&key);
            return Ok(Some(value));
        }
        // `open(path)` en modo texto UTF-8: lo que no decodifica es `ValueError`.
        let Ok(raw) = fs::read(path) else {
            return Ok(None);
        };
        let Ok(text) = String::from_utf8(raw) else {
            return Ok(None);
        };
        let Some((data, touched)) = loads_text(&text)? else {
            return Ok(None);
        };
        let Some(value) = project(&data, touched)? else {
            return Ok(None);
        };
        self.entries.set(key, (signature, value.clone()));
        self.entries.trim();
        Ok(Some(value))
    }

    pub fn read(&mut self, pid: i64, home: &Path) -> Result<Obs, Unsure> {
        let Some(home_text) = home.to_str() else {
            return Err(Unsure);
        };
        let active = match self.json(&home.join("active_sessions.json"), |rows, _| {
            project_active(rows)
        })? {
            Some(Projected::Active(rows)) => rows,
            _ => Vec::new(),
        };
        let Some(sid) = active.into_iter().find(|(p, _)| *p == pid).map(|(_, s)| s) else {
            return Ok(Obs::new());
        };
        if !truthy(&sid) {
            return Ok(Obs::new());
        }
        let valid = |s: &str| {
            !s.is_empty()
                && s.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
        };
        let sid = match sid {
            Value::String(s) if valid(&s) => s,
            // El `repr` de un contenedor empieza por `[` o `{`: nunca es válido.
            Value::String(_) | Value::Array(_) | Value::Object(_) => return Ok(Obs::new()),
            other => {
                // `str(sid)` válido pero no cadena: `os.path.join` daría `TypeError`.
                return if valid(&py_str(&other)?) {
                    Err(Unsure)
                } else {
                    Ok(Obs::new())
                };
            }
        };
        let key = (home.to_path_buf(), sid.clone());
        let cached = self.paths.get(&key).cloned();
        let path = match cached.filter(|p| fs::metadata(p).is_ok_and(|m| m.is_file())) {
            Some(path) => path,
            None => {
                // `glob` interpretaría metacaracteres de `home`.
                if home_text.contains(['*', '?', '[']) {
                    return Err(Unsure);
                }
                let hits = find_summaries(&home.join("sessions"), &sid)?;
                let [path] = hits.as_slice() else {
                    return Ok(Obs::new());
                };
                let path = path.clone();
                self.paths.set(key.clone(), path.clone());
                self.paths.trim();
                path
            }
        };
        self.paths.touch(&key);
        let mut out = match self.json(&path, project_summary)? {
            Some(Projected::Summary(summary)) => summary,
            _ => Obs::new(),
        };
        out.insert("sessionId".into(), Value::String(sid));
        out.insert("home".into(), text(home_text));
        Ok(out)
    }
}

/// `glob(home/sessions/**/<sid>/summary.json, recursive=True)` de CPython 3.10:
/// `**` es el propio directorio y todos sus subdirectorios no ocultos, siguiendo
/// enlaces a directorios (`DirEntry.is_dir()`); el resto se comprueba con `lstat`.
/// Un ciclo de enlaces hace que el Python recorra hasta `ELOOP`: `Unsure`.
fn find_summaries(root: &Path, sid: &str) -> Result<Vec<PathBuf>, Unsure> {
    fn lexists(path: &Path) -> bool {
        fs::symlink_metadata(path).is_ok()
    }
    fn walk(
        dir: &Path,
        sid: &str,
        hits: &mut Vec<PathBuf>,
        ancestors: &mut Vec<(u64, u64)>,
    ) -> Result<(), Unsure> {
        let Ok(entries) = fs::read_dir(dir) else {
            return Ok(());
        };
        let mut names = Vec::new();
        for entry in entries {
            // Un error a mitad del listado lo corta, como `_iterdir`.
            let Ok(entry) = entry else {
                break;
            };
            let Ok(meta) = fs::metadata(entry.path()) else {
                continue;
            };
            if meta.is_dir() {
                names.push((entry.file_name(), (meta.dev(), meta.ino())));
            }
        }
        for (name, identity) in names {
            if name.as_encoded_bytes().first() == Some(&b'.') {
                continue;
            }
            // Ciclo de enlaces, o tan hondo que el Python daría `RecursionError`.
            if ancestors.contains(&identity) || ancestors.len() > 200 {
                return Err(Unsure);
            }
            let path = dir.join(&name);
            let candidate = path.join(sid).join("summary.json");
            if lexists(&candidate) {
                hits.push(candidate);
            }
            ancestors.push(identity);
            walk(&path, sid, hits, ancestors)?;
            ancestors.pop();
        }
        Ok(())
    }
    let mut hits = Vec::new();
    let candidate = root.join(sid).join("summary.json");
    if lexists(&candidate) {
        hits.push(candidate);
    }
    let mut ancestors = Vec::new();
    if let Ok(meta) = fs::metadata(root) {
        ancestors.push((meta.dev(), meta.ino()));
    }
    walk(root, sid, &mut hits, &mut ancestors)?;
    Ok(hits)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn patterns_compile() {
        for pattern in [
            &MODEL_FIND,
            &DISPLAY,
            &ONE_M,
            &GROK,
            &ANSI,
            &CODEX_FOOTER,
            &OPENCODE,
            &OPENCODE_EFFORT,
            &CLAUDE_RESPONSE,
            &CLAUDE_PROMPT,
            &CLAUDE_CONFIRM,
            &CLAUDE_COMBINED,
            &CLAUDE_EFFORT,
            &CLAUDE_STATUS,
            &CLAUDE_PROMPT_PREFIX,
            &CLAUDE_EFFORT_PREFIX,
            &OPENCODE_PREFIX,
            &OPENCODE_TAIL,
            &WORD,
            &WORD_DIVERGENT,
            &DIGIT_DIVERGENT,
        ] {
            assert!(get(pattern).is_ok());
        }
    }

    #[test]
    fn lone_surrogate_escapes_are_replaced_and_pairs_kept() {
        // Los pares se escriben partidos para que ninguna herramienta los decodifique.
        let pair = concat!(r"\ud83d", r"\ude00");
        let raw = format!(r#"{{"a":"{pair}","b":"\ud800x\udc00","c":"\\u0041"}}"#);
        let (out, touched) = replace_lone_surrogates(&raw);
        assert!(touched);
        assert_eq!(
            out,
            format!(r#"{{"a":"{pair}","b":"\ufffdx\ufffd","c":"\\u0041"}}"#)
        );
        let low = concat!(r"\ud800", r"\udc00");
        let raw = format!(r#"{{"a":"\ud800{low}"}}"#);
        assert_eq!(
            replace_lone_surrogates(&raw).0,
            format!(r#"{{"a":"\ufffd{low}"}}"#)
        );
        assert!(!replace_lone_surrogates(r#"["\u00e9"]"#).1);
    }

    #[test]
    fn depth_ignores_strings_and_flags_python_recursion() {
        assert_eq!(scan_json(r#"{"a":"[[[[","b":[{}]}"#), (3, false));
        let huge = format!("[1, -{}]", "7".repeat(4301));
        assert_eq!(scan_json(&huge), (1, true));
        assert!(matches!(loads_text(&huge), Ok(None)));
        assert_eq!(scan_json(&format!("[{}.0]", "7".repeat(5000))), (1, false));
        let deep = format!("{}{}", "[".repeat(950), "]".repeat(950));
        assert!(loads_text(&deep).is_err());
        let truncated = format!("{{\"a\":{}", "[".repeat(200));
        assert!(matches!(loads_text(&truncated), Ok(None)));
    }

    #[test]
    fn utf8_surrogatepass_and_invalid() {
        assert_eq!(
            decode_utf8(b"a\xed\xa0\x80b"),
            Some(("a\u{fffd}b".into(), true))
        );
        assert_eq!(decode_utf8(b"a\xffb"), None);
    }

    #[test]
    fn signature_repr_is_python_tuple() {
        assert_eq!(signature_repr(&(1, 2, 3, 4)), "(1, 2, 3, 4)");
    }

    #[test]
    fn folding_letters_are_unsure() {
        assert!(model_id("claude-opus-\u{212a}").is_err());
        assert!(screen_state("codex", "gpt-5 h\u{131}gh · x").is_err());
        assert!(model_id("²Sonnet 5").is_err());
        assert_eq!(model_id("⚠\u{fe0f} Opus 5").unwrap(), "claude-opus-5");
        // Divergentes donde el Python decide con certeza: no declinan.
        assert_eq!(model_id("Opus\u{301} 5").unwrap(), "opus");
        assert_eq!(
            model_id("Opus 4.6 O(n²) \u{131}lk").unwrap(),
            "claude-opus-4-6"
        );
    }

    #[test]
    fn word_edges_are_unsure_only_where_the_pattern_looks() {
        assert!(screen_state("claude", "❯ /model²\n").is_err());
        assert!(screen_state("claude", "  ⎿  Set effort level to high²").is_err());
        assert!(screen_state("opencode", "┃  km² · gpt-5\n╹▀\n").is_err());
        assert!(screen_state("claude", "── · claude · claude-\u{131}pus-5\n").is_err());
        let ok = |h: &str, t: &str| screen_state(h, t).is_ok();
        assert!(ok("claude", "❯ /models² x\n  ⎿  Done O(n²) km² e\u{301}\n"));
        assert!(ok("opencode", "┃  km² · gpt-5\n  sin borde\n"));
        assert!(ok("opencode", "┃  km²! · gpt-5\n╹▀\n"));
    }

    #[test]
    fn unsure_transcript_is_cached_by_signature() {
        let dir = std::env::temp_dir().join(format!("cmd-tui-cache-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("t.jsonl");
        let unsure = concat!(
            r#"{"type":"assistant","uuid":"u"#,
            r"\ud800",
            r#"","message":{"model":"claude-opus-5"}}"#,
            "\n"
        );
        fs::write(&path, unsure).unwrap();
        let mut cache = TranscriptCache::new(128, 4096);
        assert!(cache.read("claude", "s", &path).is_err());
        assert!(cache.read("claude", "s", &path).is_err());
        assert_eq!(cache.reads(), 1);
        // Crece sin estado nuevo y la fila incierta sale de la cola: el
        // «resultado anterior» que conserva el Python es desconocido.
        let mut grown = String::from(unsure);
        grown.push_str(&"{\"type\":\"event\"}\n".repeat(400));
        fs::write(&path, grown).unwrap();
        assert!(cache.read("claude", "s", &path).is_err());
        assert_eq!(cache.reads(), 2);
        assert!(cache.read("claude", "s", &path).is_err());
        assert_eq!(cache.reads(), 2);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn py_int_matches_python() {
        let v = |s: &str| serde_json::from_str::<Value>(s).unwrap();
        assert!(matches!(
            py_int(Some(&v("\" 7_7 \""))),
            Ok(PyInt::Small(77))
        ));
        assert!(matches!(py_int(Some(&v("\"7__7\""))), Ok(PyInt::Invalid)));
        assert!(matches!(py_int(Some(&v("3.9"))), Ok(PyInt::Small(3))));
        assert!(matches!(py_int(Some(&v("[]"))), Ok(PyInt::Small(0))));
        assert!(matches!(py_int(Some(&v("[1]"))), Ok(PyInt::Invalid)));
        assert!(matches!(py_int(Some(&v("\"٧\""))), Err(Unsure)));
    }
}
