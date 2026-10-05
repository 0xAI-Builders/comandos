//! Registro de proveedores (`lib/providers.py`) y lo que `bin/cc-dash` deriva
//! de él para `/state`: motores por modelo, tiers de costo, agentes y alias de
//! procesos, `which` y las rutas seleccionables de la matriz de capacidades.
//!
//! El `re` de Python se traduce a `regex` solo donde el resultado es el mismo
//! con certeza (patrones de usuario del registro y de los tiers); lo demás es
//! `Unsure` y quien lo recibe declina.
use crate::Unsure;
use crate::hooks::py::repr;
use crate::model_catalog::{Paths, catalog_signature, hydrate_registry};
use comandos_core::json::{python_eq, truthy, workspace_loads};
use regex::Regex;
use serde_json::{Map, Value, json};
use std::collections::{BTreeSet, HashMap};
use std::ffi::OsStr;
use std::fs;
use std::io;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};

// ---------------------------------------------------------------------------
// `re` de Python → `regex`
// ---------------------------------------------------------------------------

/// Un patrón de Python compilado. Las alternativas de nivel superior con un
/// lookahead final (`gpt-5\.4(?!-mini)`, de `config/model-tiers.json`) no
/// caben en `regex`: se evalúan aparte como «literal y luego (no) cuerpo».
#[derive(Clone, Debug)]
pub struct PyRegex {
    plain: Option<Regex>,
    looks: Vec<Look>,
    ignore_case: bool,
    /// Lleva `\B`: en CPython < 3.14 no casa con un sujeto vacío; en `regex` sí.
    not_boundary: bool,
}

#[derive(Clone, Debug)]
struct Look {
    literal: String,
    negative: bool,
    body: Regex,
}

/// `re.compile(pattern, re.I if ignore_case else 0)` traducido. `Unsure` si el
/// Python podría compilarlo de otro modo o no compilarlo: grupos `(?…)` que no
/// sean `(?:` (salvo un lookahead al final de una alternativa tras un literal),
/// escapes numéricos o con letra fuera de los de `re`, cuantificadores que el
/// Python rechaza y literales no ASCII bajo `re.I`.
pub fn py_regex(pattern: &str, ignore_case: bool) -> Result<PyRegex, Unsure> {
    let chars: Vec<char> = pattern.chars().collect();
    let mut plains = Vec::new();
    let mut looks = Vec::new();
    for alternative in split_alternatives(&chars)? {
        match trailing_lookahead(alternative)? {
            Some((prefix, negative, body)) => {
                let literal = literal_text(prefix, ignore_case)?;
                let body = compile(&translate(body, ignore_case)?, ignore_case)?;
                looks.push(Look {
                    literal,
                    negative,
                    body,
                });
            }
            None => plains.push(translate(alternative, ignore_case)?),
        }
    }
    let plain = if plains.is_empty() {
        None
    } else {
        let joined: Vec<String> = plains.iter().map(|p| format!("(?:{p})")).collect();
        Some(compile(&joined.join("|"), ignore_case)?)
    };
    Ok(PyRegex {
        plain,
        looks,
        ignore_case,
        not_boundary: has_not_boundary(&chars),
    })
}

/// ¿Aparece `\B` como escape (no `\\B`) en el patrón?
fn has_not_boundary(chars: &[char]) -> bool {
    let mut i = 0;
    while let Some(&c) = chars.get(i) {
        if c == '\\' {
            if chars.get(i + 1) == Some(&'B') {
                return true;
            }
            i += 2;
        } else {
            i += 1;
        }
    }
    false
}

/// `re.search(...)` como booleano. Sujeto con `\n` (el `$` de Python también
/// casa antes del salto final), no ASCII (plegado de mayúsculas distinto) o con
/// U+001C–U+001F (el `\s` de Python los incluye) → `Unsure`.
pub fn py_search(re: &PyRegex, subject: &str) -> Result<bool, Unsure> {
    if !subject.is_ascii()
        || subject
            .bytes()
            .any(|b| b == b'\n' || (0x1c..=0x1f).contains(&b))
    {
        return Err(Unsure);
    }
    // `re.search(r"\B", "")` no casa en el Python del heredado; `regex` sí.
    if subject.is_empty() && re.not_boundary {
        return Err(Unsure);
    }
    if re.plain.as_ref().is_some_and(|p| p.is_match(subject)) {
        return Ok(true);
    }
    let bytes = subject.as_bytes();
    let literal = |look: &Look, start: usize| {
        bytes
            .get(start..start + look.literal.len())
            .is_some_and(|window| {
                if re.ignore_case {
                    window.eq_ignore_ascii_case(look.literal.as_bytes())
                } else {
                    window == look.literal.as_bytes()
                }
            })
    };
    for look in &re.looks {
        for start in 0..=bytes.len() {
            if !literal(look, start) {
                continue;
            }
            let end = start + look.literal.len();
            // La búsqueda más a la izquierda desde `end` empieza en `end` si
            // el cuerpo casa ahí; `find_at` respeta el contexto de `\b`.
            let here = look
                .body
                .find_at(subject, end)
                .is_some_and(|m| m.start() == end);
            if here != look.negative {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

fn compile(pattern: &str, ignore_case: bool) -> Result<Regex, Unsure> {
    let flagged = if ignore_case {
        format!("(?i){pattern}")
    } else {
        pattern.to_owned()
    };
    Regex::new(&flagged).map_err(|_| Unsure)
}

/// Salta una clase `[...]` con las reglas de `sre_parse`: tras `[` y `^`
/// opcional, el primer `]` es literal. Devuelve el índice tras el `]` final.
fn skip_class(p: &[char], open: usize) -> Result<usize, Unsure> {
    let mut i = open + 1;
    if p.get(i) == Some(&'^') {
        i += 1;
    }
    let mut first = true;
    loop {
        match p.get(i) {
            None => return Err(Unsure),
            Some(']') if !first => return Ok(i + 1),
            Some('\\') => i += 2,
            Some(_) => i += 1,
        }
        first = false;
    }
}

/// Alternativas de nivel superior (`|` fuera de grupos y clases).
fn split_alternatives(p: &[char]) -> Result<Vec<&[char]>, Unsure> {
    let mut out = Vec::new();
    let (mut depth, mut start, mut i) = (0usize, 0usize, 0usize);
    while let Some(&c) = p.get(i) {
        match c {
            '\\' => {
                if i + 1 >= p.len() {
                    return Err(Unsure);
                }
                i += 2;
                continue;
            }
            '[' => {
                i = skip_class(p, i)?;
                continue;
            }
            '(' => depth += 1,
            ')' => depth = depth.checked_sub(1).ok_or(Unsure)?,
            '|' if depth == 0 => {
                out.push(p.get(start..i).ok_or(Unsure)?);
                start = i + 1;
            }
            _ => {}
        }
        i += 1;
    }
    if depth != 0 {
        return Err(Unsure);
    }
    out.push(p.get(start..).ok_or(Unsure)?);
    Ok(out)
}

/// `(prefijo, negativo, cuerpo)` de un lookahead final.
type Lookahead<'a> = (&'a [char], bool, &'a [char]);

/// `prefijo(?!cuerpo)` o `prefijo(?=cuerpo)` con el grupo cerrando la
/// alternativa.
fn trailing_lookahead(p: &[char]) -> Result<Option<Lookahead<'_>>, Unsure> {
    if p.last() != Some(&')') {
        return Ok(None);
    }
    let mut stack = Vec::new();
    let mut i = 0;
    let mut last_open = None;
    while let Some(&c) = p.get(i) {
        match c {
            '\\' => {
                i += 2;
                continue;
            }
            '[' => {
                i = skip_class(p, i)?;
                continue;
            }
            '(' => stack.push(i),
            ')' => {
                let open = stack.pop().ok_or(Unsure)?;
                if i + 1 == p.len() && stack.is_empty() {
                    last_open = Some(open);
                }
            }
            _ => {}
        }
        i += 1;
    }
    let Some(open) = last_open else {
        return Ok(None);
    };
    let negative = match (p.get(open + 1), p.get(open + 2)) {
        (Some('?'), Some('!')) => true,
        (Some('?'), Some('=')) => false,
        _ => return Ok(None),
    };
    let prefix = p.get(..open).ok_or(Unsure)?;
    let body = p.get(open + 3..p.len() - 1).ok_or(Unsure)?;
    Ok(Some((prefix, negative, body)))
}

/// Carácter de `\a \f \n \r \t \v` (los `ESCAPES` de `sre_parse`, sin `\b`).
fn control_escape(c: char) -> Option<char> {
    Some(match c {
        'a' => '\u{7}',
        'f' => '\u{c}',
        'n' => '\n',
        'r' => '\r',
        't' => '\t',
        'v' => '\u{b}',
        _ => return None,
    })
}

/// Prefijo de un lookahead: solo literales.
fn literal_text(p: &[char], ignore_case: bool) -> Result<String, Unsure> {
    let mut out = String::new();
    let mut i = 0;
    while let Some(&c) = p.get(i) {
        let ch = match c {
            '\\' => {
                let d = *p.get(i + 1).ok_or(Unsure)?;
                i += 1;
                match control_escape(d) {
                    Some(ctrl) => ctrl,
                    None if d.is_ascii_alphanumeric() => return Err(Unsure),
                    None => d,
                }
            }
            '.' | '^' | '$' | '*' | '+' | '?' | '{' | '[' | '(' | ')' | '|' => {
                return Err(Unsure);
            }
            other => other,
        };
        if ignore_case && !ch.is_ascii() {
            return Err(Unsure);
        }
        out.push(ch);
        i += 1;
    }
    Ok(out)
}

/// Literal ya decidido: alfanumérico ASCII tal cual, el resto como `\x{…}`
/// (nunca metacarácter de `regex`, ni `\<`, ni operador de clases).
fn push_literal(out: &mut String, c: char, ignore_case: bool) -> Result<(), Unsure> {
    if c.is_ascii_alphanumeric() {
        out.push(c);
        return Ok(());
    }
    if ignore_case && !c.is_ascii() {
        return Err(Unsure);
    }
    out.push_str(&format!("\\x{{{:X}}}", u32::from(c)));
    Ok(())
}

/// `{m}`, `{m,}`, `{,n}`, `{m,n}` con dígitos ASCII (`sre_parse`); cualquier
/// otra llave es literal. `Ok(None)` = literal.
fn brace_quantifier(p: &[char], open: usize) -> Result<Option<(String, usize)>, Unsure> {
    const MAXREPEAT: u64 = 4_294_967_295;
    if p.get(open + 1) == Some(&'}') {
        return Ok(None);
    }
    let digits = |from: usize| {
        let mut end = from;
        while p.get(end).is_some_and(char::is_ascii_digit) {
            end += 1;
        }
        end
    };
    let lo_end = digits(open + 1);
    let lo: String = p.get(open + 1..lo_end).ok_or(Unsure)?.iter().collect();
    let (hi, close) = if p.get(lo_end) == Some(&',') {
        let hi_end = digits(lo_end + 1);
        let hi: String = p.get(lo_end + 1..hi_end).ok_or(Unsure)?.iter().collect();
        (Some(hi), hi_end)
    } else {
        (Some(lo.clone()), lo_end)
    };
    if p.get(close) != Some(&'}') {
        return Ok(None);
    }
    let number = |text: &str| -> Result<Option<u64>, Unsure> {
        if text.is_empty() {
            return Ok(None);
        }
        let n: u64 = text.parse().map_err(|_| Unsure)?;
        if n >= MAXREPEAT {
            return Err(Unsure);
        }
        Ok(Some(n))
    };
    let min = number(&lo)?.unwrap_or(0);
    let max = number(hi.as_deref().unwrap_or(""))?;
    if max.is_some_and(|m| m < min) {
        return Err(Unsure);
    }
    let text = match max {
        Some(max) => format!("{{{min},{max}}}"),
        None => format!("{{{min},}}"),
    };
    Ok(Some((text, close + 1 - open)))
}

enum ClassItem {
    Char(char),
    Category(char),
}

fn class_atom(p: &[char], i: &mut usize) -> Result<ClassItem, Unsure> {
    let c = *p.get(*i).ok_or(Unsure)?;
    if c != '\\' {
        *i += 1;
        return Ok(ClassItem::Char(c));
    }
    let d = *p.get(*i + 1).ok_or(Unsure)?;
    *i += 2;
    Ok(match d {
        'd' | 'D' | 'w' | 'W' | 's' | 'S' => ClassItem::Category(d),
        'b' => ClassItem::Char('\u{8}'),
        d => match control_escape(d) {
            Some(ctrl) => ClassItem::Char(ctrl),
            None if d.is_ascii_alphanumeric() => return Err(Unsure),
            None => ClassItem::Char(d),
        },
    })
}

/// Rango de clase sin divergencia de mayúsculas bajo `re.I`: sin letras, o
/// todo dentro de `a-z` o de `A-Z`.
fn range_safe(lo: char, hi: char, ignore_case: bool) -> bool {
    if !ignore_case {
        return true;
    }
    if !hi.is_ascii() {
        return false;
    }
    let letters = |a: char, b: char| lo <= b && a <= hi;
    let has_letter = letters('a', 'z') || letters('A', 'Z');
    !has_letter || lo.is_ascii_lowercase() && hi <= 'z' || lo.is_ascii_uppercase() && hi <= 'Z'
}

fn translate_class(p: &[char], i: &mut usize, ignore_case: bool) -> Result<String, Unsure> {
    let mut out = String::from("[");
    *i += 1;
    if p.get(*i) == Some(&'^') {
        out.push('^');
        *i += 1;
    }
    let mut first = true;
    loop {
        if p.get(*i) == Some(&']') && !first {
            *i += 1;
            break;
        }
        let lo = class_atom(p, i)?;
        first = false;
        if p.get(*i) == Some(&'-') {
            match p.get(*i + 1) {
                None => return Err(Unsure),
                Some(']') => {
                    push_item(&mut out, &lo, ignore_case)?;
                    out.push_str("\\x{2D}");
                    *i += 2;
                    break;
                }
                Some(_) => {
                    *i += 1;
                    let hi = class_atom(p, i)?;
                    let (ClassItem::Char(lo), ClassItem::Char(hi)) = (lo, hi) else {
                        return Err(Unsure);
                    };
                    if hi < lo || !range_safe(lo, hi, ignore_case) {
                        return Err(Unsure);
                    }
                    out.push_str(&format!(
                        "\\x{{{:X}}}-\\x{{{:X}}}",
                        u32::from(lo),
                        u32::from(hi)
                    ));
                    continue;
                }
            }
        }
        push_item(&mut out, &lo, ignore_case)?;
    }
    out.push(']');
    Ok(out)
}

fn push_item(out: &mut String, item: &ClassItem, ignore_case: bool) -> Result<(), Unsure> {
    match item {
        ClassItem::Category(c) => {
            out.push('\\');
            out.push(*c);
            Ok(())
        }
        ClassItem::Char(c) => push_literal(out, *c, ignore_case),
    }
}

/// Patrón sin lookarounds (los rechaza) con la sintaxis de `sre_parse`.
fn translate(p: &[char], ignore_case: bool) -> Result<String, Unsure> {
    let mut out = String::new();
    let mut i = 0;
    let mut depth = 0usize;
    // ¿Hay un elemento repetible justo antes? (`nothing to repeat`).
    let mut atom = false;
    while let Some(&c) = p.get(i) {
        match c {
            '\\' => {
                let d = *p.get(i + 1).ok_or(Unsure)?;
                i += 2;
                match d {
                    'd' | 'D' | 'w' | 'W' | 's' | 'S' => {
                        out.push('\\');
                        out.push(d);
                        atom = true;
                    }
                    'b' | 'B' | 'A' => {
                        out.push('\\');
                        out.push(d);
                        atom = false;
                    }
                    'Z' => {
                        out.push_str("\\z");
                        atom = false;
                    }
                    d => {
                        match control_escape(d) {
                            Some(ctrl) => push_literal(&mut out, ctrl, ignore_case)?,
                            None if d.is_ascii_alphanumeric() => return Err(Unsure),
                            None => push_literal(&mut out, d, ignore_case)?,
                        }
                        atom = true;
                    }
                }
                continue;
            }
            '[' => {
                out.push_str(&translate_class(p, &mut i, ignore_case)?);
                atom = true;
                continue;
            }
            '(' => {
                if p.get(i + 1) == Some(&'?') {
                    if p.get(i + 2) != Some(&':') {
                        return Err(Unsure);
                    }
                    out.push_str("(?:");
                    i += 3;
                } else {
                    out.push('(');
                    i += 1;
                }
                depth += 1;
                atom = false;
                continue;
            }
            ')' => {
                depth = depth.checked_sub(1).ok_or(Unsure)?;
                out.push(')');
                atom = true;
            }
            '|' | '^' | '$' => {
                out.push(c);
                atom = false;
            }
            '.' => {
                out.push('.');
                atom = true;
            }
            '*' | '+' | '?' => {
                if !atom {
                    return Err(Unsure);
                }
                out.push(c);
                i = after_quantifier(p, i + 1, &mut out)?;
                atom = false;
                continue;
            }
            '{' => match brace_quantifier(p, i)? {
                Some((text, len)) => {
                    if !atom {
                        return Err(Unsure);
                    }
                    out.push_str(&text);
                    i = after_quantifier(p, i + len, &mut out)?;
                    atom = false;
                    continue;
                }
                None => {
                    push_literal(&mut out, '{', ignore_case)?;
                    atom = true;
                }
            },
            other => {
                push_literal(&mut out, other, ignore_case)?;
                atom = true;
            }
        }
        i += 1;
    }
    if depth != 0 {
        return Err(Unsure);
    }
    Ok(out)
}

/// `?` perezoso opcional; otro cuantificador detrás es `multiple repeat`.
fn after_quantifier(p: &[char], mut i: usize, out: &mut String) -> Result<usize, Unsure> {
    if p.get(i) == Some(&'?') {
        out.push('?');
        i += 1;
    }
    match p.get(i) {
        Some('*' | '+' | '?') => Err(Unsure),
        Some('{') if brace_quantifier(p, i)?.is_some() => Err(Unsure),
        _ => Ok(i),
    }
}

/// Caché de patrones compilados (`re` tiene la suya): el registro y los tiers
/// se consultan por tarjeta en cada `/state`.
type Compiled = HashMap<(String, bool), Result<PyRegex, Unsure>>;
static COMPILED: LazyLock<Mutex<Compiled>> = LazyLock::new(|| Mutex::new(HashMap::new()));

fn cached_regex(pattern: &str, ignore_case: bool) -> Result<PyRegex, Unsure> {
    let key = (pattern.to_owned(), ignore_case);
    let mut cache = COMPILED.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(hit) = cache.get(&key) {
        return hit.clone();
    }
    let compiled = py_regex(pattern, ignore_case);
    if cache.len() >= 512 {
        cache.clear();
    }
    cache.insert(key, compiled.clone());
    compiled
}

/// `re.search(pattern, subject, re.I)` con el patrón como texto.
fn search_ci(pattern: &str, subject: &str) -> Result<bool, Unsure> {
    py_search(&cached_regex(pattern, true)?, subject)
}

// ---------------------------------------------------------------------------
// Valores de Python
// ---------------------------------------------------------------------------

/// `str.isspace()` de CPython.
fn is_space(c: char) -> bool {
    c.is_whitespace() || ('\u{1c}'..='\u{1f}').contains(&c)
}

/// `str(x)` de un escalar cuyo texto es seguro (cadena, `None`, booleano,
/// entero canónico); lo demás → `Unsure`.
fn scalar_str(value: &Value) -> Result<String, Unsure> {
    match value {
        Value::Null => Ok("None".into()),
        Value::Bool(b) => Ok(if *b { "True" } else { "False" }.into()),
        Value::String(s) => Ok(s.clone()),
        Value::Number(n) => {
            let text = n.as_str();
            let digits = text.strip_prefix('-').unwrap_or(text);
            let canonical = !digits.is_empty()
                && digits.bytes().all(|b| b.is_ascii_digit())
                && (digits == "0" || !digits.starts_with('0'))
                && text != "-0";
            if canonical {
                Ok(text.to_owned())
            } else {
                Err(Unsure)
            }
        }
        _ => Err(Unsure),
    }
}

/// `repr(x)` de un escalar: cadenas solo con ASCII imprimible (el `repr` de
/// Python escapa según `isprintable()`, que aquí no se reproduce).
fn scalar_repr(value: &Value) -> Result<String, Unsure> {
    match value {
        Value::String(s) if s.bytes().all(|b| (0x20..0x7f).contains(&b)) => Ok(repr(value)),
        Value::String(_) => Err(Unsure),
        other => scalar_str(other),
    }
}

/// Elementos de `for x in (value or [])`: cadena → caracteres, objeto →
/// claves; número o `true` → `TypeError` (`Unsure`).
fn py_iter(value: &Value) -> Result<Vec<Value>, Unsure> {
    if !truthy(value) {
        return Ok(Vec::new());
    }
    match value {
        Value::Array(items) => Ok(items.clone()),
        Value::String(s) => Ok(s.chars().map(|c| Value::String(c.into())).collect()),
        Value::Object(map) => Ok(map.keys().map(|k| Value::String(k.clone())).collect()),
        _ => Err(Unsure),
    }
}

/// `(value or {}).items()`: un valor verdadero que no es objeto no tiene `items`.
fn py_items(value: &Value) -> Result<Option<&Map<String, Value>>, Unsure> {
    if !truthy(value) {
        return Ok(None);
    }
    value.as_object().map(Some).ok_or(Unsure)
}

/// Un elemento de `set(...)`/clave de dict: contenedores → `TypeError`.
fn hashable(value: &Value) -> Result<(), Unsure> {
    match value {
        Value::Array(_) | Value::Object(_) => Err(Unsure),
        _ => Ok(()),
    }
}

/// `x in d` con `d` un dict de claves de texto.
fn in_keys(value: &Value, map: &Map<String, Value>) -> Result<bool, Unsure> {
    hashable(value)?;
    Ok(value.as_str().is_some_and(|k| map.contains_key(k)))
}

/// `set(items)` con la igualdad de Python (`1 == 1.0 == True`).
fn py_set(items: &[Value]) -> Result<Vec<Value>, Unsure> {
    let mut out: Vec<Value> = Vec::new();
    for item in items {
        hashable(item)?;
        if !out.iter().any(|seen| python_eq(seen, item)) {
            out.push(item.clone());
        }
    }
    Ok(out)
}

fn member(item: &Value, set: &[Value]) -> bool {
    set.iter().any(|seen| python_eq(seen, item))
}

/// `_ID.fullmatch(text)`: `^[a-z][a-z0-9._-]{0,39}$` sobre todo el texto.
fn valid_id(text: &str) -> bool {
    let b = text.as_bytes();
    (1..=40).contains(&b.len())
        && b.first().is_some_and(u8::is_ascii_lowercase)
        && b.iter()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || b"._-".contains(c))
}

/// `re.compile(str(modelMatch or ""), re.I)` como comprobación: el texto del
/// patrón, o `Unsure` si no es una cadena o no se puede juzgar.
fn model_match(item: &Value, default: &str) -> Result<String, Unsure> {
    let raw = item.get("modelMatch").unwrap_or(&Value::Null);
    let pattern = if truthy(raw) {
        raw.as_str().ok_or(Unsure)?.to_owned()
    } else {
        default.to_owned()
    };
    cached_regex(&pattern, true)?;
    Ok(pattern)
}

// ---------------------------------------------------------------------------
// validate_registry
// ---------------------------------------------------------------------------

type Verdict = Result<Result<(), String>, Unsure>;

macro_rules! invalid {
    ($($arg:tt)*) => {
        return Ok(Err(format!($($arg)*)))
    };
}

/// `_validate_models` (`lib/providers.py`).
fn validate_models(section: &str, ident: &str, item: &Value) -> Verdict {
    for model in py_iter(item.get("models").unwrap_or(&Value::Null))? {
        let id = model.get("id").unwrap_or(&Value::Null);
        if !model.is_object() || !truthy(id) {
            invalid!("bad model in {section}.{ident}");
        }
        let efforts = model.get("efforts").unwrap_or(&Value::Null);
        let listed = py_iter(efforts)?;
        if listed.len() != py_set(&listed)?.len() {
            invalid!("duplicate effort in {ident}/{}", scalar_str(id)?);
        }
        let default = model.get("defaultEffort").unwrap_or(&Value::Null);
        if truthy(default) {
            let allowed = match efforts {
                _ if !truthy(efforts) => false,
                Value::Array(items) => member(default, items),
                Value::String(text) => text.contains(default.as_str().ok_or(Unsure)?),
                Value::Object(map) => in_keys(default, map)?,
                _ => return Err(Unsure),
            };
            if !allowed {
                invalid!("default effort not allowed in {ident}/{}", scalar_str(id)?);
            }
        }
    }
    Ok(Ok(()))
}

/// `validate_registry` (`lib/providers.py`): mismas comprobaciones, mismo
/// orden y mismos mensajes de `ProviderRegistryError`. `Unsure` donde el
/// Python lanzaría otra excepción o su `re.compile` no se puede juzgar.
pub fn validate_registry(data: &Value) -> Verdict {
    let data = data.as_object().ok_or(Unsure)?;
    let get = |key: &str| data.get(key).unwrap_or(&Value::Null);
    let version = get("version");
    let v1 = python_eq(version, &json!(1));
    let v2 = python_eq(version, &json!(2));
    if !v1 && !v2 {
        invalid!("providers.json version must be 1 or 2");
    }
    for section in ["harnesses", "claudeEngines"] {
        let entries = match get(section) {
            Value::Object(map) if !map.is_empty() => map,
            _ => invalid!("{section} must be a non-empty object"),
        };
        for (ident, item) in entries {
            if !valid_id(ident) || !item.is_object() {
                invalid!(
                    "invalid {section} id: {}",
                    scalar_repr(&Value::String(ident.clone()))?
                );
            }
            let label = item.get("label").unwrap_or(&Value::Null);
            let empty_label = match label {
                Value::String(s) => s.trim_matches(is_space).is_empty(),
                other => !truthy(other),
            };
            if empty_label {
                invalid!("{section}.{ident} needs label");
            }
            if section == "claudeEngines" {
                model_match(item, "")?;
            }
            if let Err(message) = validate_models(section, ident, item)? {
                return Ok(Err(message));
            }
        }
    }
    if !v2 {
        return Ok(Ok(()));
    }
    let harnesses = get("harnesses").as_object().ok_or(Unsure)?;
    let motors = match get("motors") {
        Value::Object(map) if !map.is_empty() => map,
        _ => invalid!("motors must be a non-empty object"),
    };
    for (ident, item) in motors {
        if !harnesses.contains_key(ident) || !item.is_object() {
            invalid!("invalid motor: {ident}");
        }
        model_match(item, "")?;
        if let Err(message) = validate_models("motors", ident, item)? {
            return Ok(Err(message));
        }
    }
    let matrix = py_set(&py_iter(get("matrixHarnesses"))?)?;
    if ["claude", "codex", "grok"]
        .iter()
        .any(|h| !member(&json!(h), &matrix))
    {
        invalid!("matrixHarnesses must cover claude/codex/grok");
    }
    for h in &matrix {
        if !in_keys(h, harnesses)? {
            invalid!("matrixHarnesses references unknown harness");
        }
    }
    let acp = get("acpAgents");
    let acp_agents = py_items(acp)?;
    for (ident, spec) in acp_agents.into_iter().flatten() {
        let command = spec.get("command").unwrap_or(&Value::Null);
        if !motors.contains_key(ident) || !spec.is_object() || !truthy(command) {
            invalid!("bad acpAgent: {ident}");
        }
    }
    let mut cells: Vec<(Value, Value)> = Vec::new();
    let mut route_ids: Vec<String> = Vec::new();
    let same =
        |a: &(Value, Value), b: &(Value, Value)| python_eq(&a.0, &b.0) && python_eq(&a.1, &b.1);
    for route in py_iter(get("routes"))? {
        let route = route.as_object().ok_or(Unsure)?;
        let field = |key: &str| route.get(key).cloned().unwrap_or(Value::Null);
        let ident = field("id");
        let cell = (field("harness"), field("motor"));
        let text = if truthy(&ident) {
            scalar_str(&ident)?
        } else {
            String::new()
        };
        if !valid_id(&text) && !text.contains(':') {
            invalid!("bad route id: {}", scalar_str(&ident)?);
        }
        // El texto pasó: es una cadena (ningún otro escalar lo consigue).
        let ident = ident.as_str().ok_or(Unsure)?.to_owned();
        hashable(&cell.0)?;
        hashable(&cell.1)?;
        if route_ids.contains(&ident) || cells.iter().any(|c| same(c, &cell)) {
            invalid!("duplicate route/cell: {ident}");
        }
        if !in_keys(&cell.0, harnesses)? || !in_keys(&cell.1, motors)? {
            invalid!("bad route reference: {ident}");
        }
        let scopes = py_set(&py_iter(&field("actionScopes"))?)?;
        if scopes.iter().any(|s| {
            !matches!(
                s.as_str(),
                Some("new_session" | "session_motor" | "session_model")
            )
        }) {
            invalid!("bad route scope: {ident}");
        }
        if field("driver") == json!("acp")
            && !acp_agents.is_some_and(|agents| in_keys(&cell.1, agents).unwrap_or(false))
        {
            invalid!("acp route without acpAgent: {ident}");
        }
        route_ids.push(ident);
        cells.push(cell);
    }
    for exclusion in py_iter(get("exclusions"))? {
        let exclusion = exclusion.as_object().ok_or(Unsure)?;
        let field = |key: &str| exclusion.get(key).cloned().unwrap_or(Value::Null);
        let cell = (field("harness"), field("motor"));
        hashable(&cell.0)?;
        hashable(&cell.1)?;
        if cells.iter().any(|c| same(c, &cell)) {
            invalid!(
                "excluded cell is routed: ({}, {})",
                scalar_repr(&cell.0)?,
                scalar_repr(&cell.1)?
            );
        }
        cells.push(cell);
    }
    // Núcleo: cada celda (h, m) con h y m en `core` es ruta o exclusión.
    let core: Vec<Value> = if truthy(get("coreHarnesses")) {
        py_iter(get("coreHarnesses"))?
    } else {
        vec![json!("claude"), json!("codex"), json!("grok")]
    };
    for h in &core {
        hashable(h)?;
    }
    let in_core = |value: &Value| -> Result<bool, Unsure> {
        match get("coreHarnesses") {
            Value::String(text) if truthy(get("coreHarnesses")) => {
                Ok(text.contains(value.as_str().ok_or(Unsure)?))
            }
            Value::Object(map) if truthy(get("coreHarnesses")) => in_keys(value, map),
            _ => Ok(member(value, &core)),
        }
    };
    let mut expected: Vec<(Value, Value)> = Vec::new();
    for h in &core {
        for m in &core {
            let cell = (h.clone(), m.clone());
            if !expected.iter().any(|c| same(c, &cell)) {
                expected.push(cell);
            }
        }
    }
    let mut core_cells: Vec<(Value, Value)> = Vec::new();
    for cell in &cells {
        if in_core(&cell.0)? && in_core(&cell.1)? && !core_cells.iter().any(|c| same(c, cell)) {
            core_cells.push(cell.clone());
        }
    }
    let missing: Vec<&(Value, Value)> = expected
        .iter()
        .filter(|e| !core_cells.iter().any(|c| same(c, e)))
        .collect();
    let extra: Vec<&(Value, Value)> = core_cells
        .iter()
        .filter(|c| !expected.iter().any(|e| same(e, c)))
        .collect();
    if !missing.is_empty() || !extra.is_empty() {
        invalid!(
            "matrix coverage mismatch: missing={} extra={}",
            sorted_cells(&missing)?,
            sorted_cells(&extra)?
        );
    }
    Ok(Ok(()))
}

/// `sorted(conjunto_de_tuplas)` con su `repr`; solo pares de cadenas (otros
/// tipos pueden no compararse en Python).
fn sorted_cells(cells: &[&(Value, Value)]) -> Result<String, Unsure> {
    let mut pairs = Vec::new();
    for (h, m) in cells {
        pairs.push((h.as_str().ok_or(Unsure)?, m.as_str().ok_or(Unsure)?));
    }
    pairs.sort();
    let mut parts = Vec::new();
    for (h, m) in pairs {
        parts.push(format!(
            "({}, {})",
            scalar_repr(&json!(h))?,
            scalar_repr(&json!(m))?
        ));
    }
    Ok(format!("[{}]", parts.join(", ")))
}

// ---------------------------------------------------------------------------
// Carga con caché
// ---------------------------------------------------------------------------

/// `load_provider_registry` (`bin/cc-dash`): caché por `(mtime_ns de
/// providers.json, catalog_signature)`. Cualquier fallo (archivo ausente,
/// UTF-8 inválido, JSON roto, registro inválido) es una excepción del Python
/// que el frente no reproduce: `Unsure`, sin cachear.
#[derive(Default)]
pub struct RegistryCache {
    key: Option<(Option<i128>, Value)>,
    data: Option<Value>,
}

impl RegistryCache {
    pub fn load(&mut self, providers_json: &Path, catalog: &Paths) -> Result<Value, Unsure> {
        let mtime = fs::metadata(providers_json)
            .ok()
            .map(|m| i128::from(m.mtime()) * 1_000_000_000 + i128::from(m.mtime_nsec()));
        let key = (mtime, catalog_signature(catalog));
        if let (Some(cached), Some(data)) = (&self.key, &self.data)
            && *cached == key
        {
            return Ok(data.clone());
        }
        let bytes = fs::read(providers_json).map_err(|_| Unsure)?;
        let text = std::str::from_utf8(&bytes).map_err(|_| Unsure)?;
        let raw = workspace_loads(text).map_err(|_| Unsure)?;
        validate_registry(&raw)?.map_err(|_| Unsure)?;
        let hydrated = hydrate_registry(&raw, catalog).map_err(|_| Unsure)?;
        validate_registry(&hydrated)?.map_err(|_| Unsure)?;
        self.key = Some(key);
        self.data = Some(hydrated.clone());
        Ok(hydrated)
    }
}

// ---------------------------------------------------------------------------
// Consultas sobre el registro
// ---------------------------------------------------------------------------

/// `(registry.get(section) or {})` como objeto (vacío si es falso).
fn section<'a>(registry: &'a Value, key: &str) -> Result<Option<&'a Map<String, Value>>, Unsure> {
    py_items(registry.get(key).unwrap_or(&Value::Null))
}

/// `engine_for_model` (`lib/providers.py`): primer motor (o motor Claude en
/// v1) cuyo `modelMatch` (o `$^`) casa con el modelo bajo `re.I`.
pub fn engine_for_model(registry: &Value, model: &str) -> Result<String, Unsure> {
    let motors = registry.get("motors").unwrap_or(&Value::Null);
    let owners = if truthy(motors) {
        motors
    } else {
        registry.get("claudeEngines").unwrap_or(&Value::Null)
    };
    for (ident, item) in py_items(owners)?.into_iter().flatten() {
        let item = item.as_object().ok_or(Unsure)?;
        let raw = item.get("modelMatch").unwrap_or(&Value::Null);
        let pattern = if truthy(raw) {
            raw.as_str().ok_or(Unsure)?
        } else {
            "$^"
        };
        if search_ci(pattern, model)? {
            return Ok(ident.clone());
        }
    }
    Ok(String::new())
}

/// `bool(((harnesses or {}).get(agent) or {}).get("capabilities") or {}).get("accounts"))`
/// (`bin/cc-dash`, lanzamiento de sesiones). Un intermedio que no es objeto
/// cuenta como sin cuentas.
pub fn harness_has_accounts(registry: &Value, agent: &str) -> bool {
    registry
        .get("harnesses")
        .and_then(|h| h.get(agent))
        .and_then(|item| item.get("capabilities"))
        .and_then(|caps| caps.get("accounts"))
        .is_some_and(truthy)
}

/// `read_conf` (`bin/cc-dash`) sobre `cc-notify.conf`: pares en el orden del
/// dict del Python (la primera aparición fija la posición, la última el
/// valor). Ausente → vacío; otro error de E/S o UTF-8 inválido → `Unsure`.
pub fn read_conf(path: &Path) -> Result<Vec<(String, String)>, Unsure> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(_) => return Err(Unsure),
    };
    let text = std::str::from_utf8(&bytes).map_err(|_| Unsure)?;
    let mut out: Vec<(String, String)> = Vec::new();
    // Modo texto con saltos universales: `\r\n`, `\r` y `\n` acaban línea.
    for raw in text.split('\n').flat_map(|l| l.split('\r')) {
        let line = raw.trim_matches(is_space);
        if line.is_empty() || line.starts_with('#') || !line.contains('=') {
            continue;
        }
        let (key, value) = line.split_once('=').ok_or(Unsure)?;
        let mut value = value.trim_matches(is_space);
        let mut chars = value.chars();
        if let (Some(first), Some(last)) = (chars.next(), chars.next_back())
            && first == last
            && (first == '"' || first == '\'')
        {
            value = chars.as_str();
        }
        let key = key.trim_matches(is_space).to_owned();
        match out.iter_mut().find(|(k, _)| *k == key) {
            Some(slot) => slot.1 = value.to_owned(),
            None => out.push((key, value.to_owned())),
        }
    }
    Ok(out)
}

pub const DEFAULT_AGENTS: &str = "claude codex grok acp opencode gemini agy aider";

/// `agent_set` (`bin/cc-dash`): `AGENTS` (o el valor por defecto) partido por
/// blancos, más los harnesses del registro salvo `shell`.
pub fn agent_set(conf_agents: Option<&str>, registry: &Value) -> BTreeSet<String> {
    let agents = conf_agents
        .filter(|a| !a.is_empty())
        .unwrap_or(DEFAULT_AGENTS);
    let mut out: BTreeSet<String> = agents
        .split(is_space)
        .filter(|a| !a.is_empty())
        .map(str::to_owned)
        .collect();
    if let Some(Value::Object(harnesses)) = registry.get("harnesses") {
        out.extend(harnesses.keys().filter(|h| *h != "shell").cloned());
    }
    out
}

/// `agent_process_aliases` (`bin/cc-dash`): `{agente: agente}` más los
/// `processNames` no genéricos por basename, en el orden del registro (el
/// último gana). Un error a mitad (el `except` del Python) conserva lo hecho.
pub fn process_aliases(agents: &BTreeSet<String>, registry: &Value) -> HashMap<String, String> {
    const GENERIC: [&str; 7] = ["node", "python", "python3", "bash", "zsh", "fish", "sh"];
    let mut out: HashMap<String, String> = agents.iter().map(|a| (a.clone(), a.clone())).collect();
    let Some(Value::Object(harnesses)) = registry.get("harnesses") else {
        return out;
    };
    for (ident, item) in harnesses {
        if !agents.contains(ident) {
            continue;
        }
        let Ok(names) = py_iter(item.get("processNames").unwrap_or(&Value::Null)) else {
            return out;
        };
        for name in names {
            if hashable(&name).is_err() {
                return out;
            }
            if name.as_str().is_some_and(|n| GENERIC.contains(&n)) {
                continue;
            }
            let Ok(text) = scalar_str(&name) else {
                return out;
            };
            let base = text.rsplit('/').next().unwrap_or(&text).to_owned();
            out.insert(base, ident.clone());
        }
    }
    out
}

// ---------------------------------------------------------------------------
// which
// ---------------------------------------------------------------------------

/// `_USER_BIN_DIRS` de `lib/providers.py`.
const USER_BIN_DIRS: [&str; 7] = [
    "~/.local/bin",
    "~/.bun/bin",
    "~/.cargo/bin",
    "~/.npm-global/bin",
    "~/.opencode/bin",
    "~/bin",
    "/usr/local/bin",
];

fn executable(path: &Path) -> bool {
    nix::unistd::access(path, nix::unistd::AccessFlags::X_OK).is_ok()
}

/// `os.path.expanduser` sin base de usuarios: `~` y `~/…` → `home`.
fn expanduser(raw: &str, home: &Path) -> PathBuf {
    match raw.strip_prefix('~') {
        Some("") => home.into(),
        Some(rest) if rest.starts_with('/') => home.join(rest.trim_start_matches('/')),
        _ => raw.into(),
    }
}

/// `shutil.which(name)` (CPython 3.10) con `PATH` dado (`None` = variable
/// ausente: `os.confstr("CS_PATH")` de glibc). Entradas vacías = directorio
/// actual; el resultado conserva la forma relativa como el Python.
pub fn which_path(name: &str, path: Option<&OsStr>) -> Option<PathBuf> {
    let check = |candidate: &Path| {
        fs::metadata(candidate).is_ok_and(|m| !m.is_dir()) && executable(candidate)
    };
    let cmd = Path::new(name);
    if name.contains('/') {
        return check(cmd).then(|| cmd.to_path_buf());
    }
    let path = path.unwrap_or(OsStr::new("/bin:/usr/bin"));
    if path.is_empty() {
        return None;
    }
    let mut seen = Vec::new();
    for dir in path.as_bytes().split(|b| *b == b':') {
        if seen.contains(&dir) {
            continue;
        }
        seen.push(dir);
        let candidate = Path::new(OsStr::from_bytes(dir)).join(name);
        let candidate = if dir.is_empty() {
            PathBuf::from(name)
        } else {
            candidate
        };
        if check(&candidate) {
            return Some(candidate);
        }
    }
    None
}

/// `which` (`lib/providers.py`): `shutil.which` y luego los directorios de
/// binarios de usuario (archivo regular ejecutable).
pub fn which(name: &str, path: Option<&OsStr>, home: &Path) -> Option<PathBuf> {
    if name.is_empty() {
        return None;
    }
    if let Some(hit) = which_path(name, path) {
        return Some(hit);
    }
    USER_BIN_DIRS.iter().find_map(|dir| {
        let candidate = expanduser(dir, home).join(name);
        (fs::metadata(&candidate).is_ok_and(|m| m.is_file()) && executable(&candidate))
            .then_some(candidate)
    })
}

// ---------------------------------------------------------------------------
// Hechos de ejecución y rutas seleccionables
// ---------------------------------------------------------------------------

fn get<'a>(map: &'a Map<String, Value>, key: &str) -> &'a Value {
    map.get(key).unwrap_or(&Value::Null)
}

/// `any(a.get("selectable") for a in accounts)` con cortocircuito.
fn any_selectable(accounts: &Value) -> Result<bool, Unsure> {
    for account in py_iter(accounts)? {
        if truthy(
            account
                .as_object()
                .ok_or(Unsure)?
                .get("selectable")
                .unwrap_or(&Value::Null),
        ) {
            return Ok(true);
        }
    }
    Ok(false)
}

/// `public_state` (`lib/providers.py`), solo `available` y `authenticated`
/// (`None` sin `authFile`).
fn public_harness(
    item: &Map<String, Value>,
    available: &dyn Fn(&str) -> bool,
    home: &Path,
) -> Result<(bool, Option<bool>), Unsure> {
    let binary = get(item, "binary");
    let is_available = if binary.is_null() {
        true
    } else {
        let name = if truthy(binary) {
            scalar_str(binary)?
        } else {
            String::new()
        };
        !name.is_empty() && available(&name)
    };
    let default_home = get(item, "defaultHome");
    let auth_home = if truthy(default_home) {
        expanduser(default_home.as_str().ok_or(Unsure)?, home)
    } else {
        PathBuf::new()
    };
    let auth = get(item, "authFile");
    let authenticated = if truthy(auth) {
        Some(
            !auth_home.as_os_str().is_empty()
                && fs::metadata(auth_home.join(auth.as_str().ok_or(Unsure)?))
                    .is_ok_and(|m| m.is_file()),
        )
    } else {
        None
    };
    Ok((is_available, authenticated))
}

/// `provider_runtime_facts` (`bin/cc-dash`). `discovered` es
/// `accounts::public_accounts(registry, …)`; `available` decide si existe el
/// binario de un harness (`provider_registry.which`); `home` es el de
/// `defaultHome`. `Unsure` donde el Python lanzaría (tipos imposibles en un
/// registro válido).
pub fn runtime_facts(
    registry: &Value,
    discovered: &Value,
    available: &dyn Fn(&str) -> bool,
    home: &Path,
    gateway_installed: bool,
    gateway_alive: bool,
) -> Result<Value, Unsure> {
    let discovered = discovered.as_object().ok_or(Unsure)?;
    let mut harnesses = Map::new();
    let mut publics = Vec::new();
    for (key, item) in section(registry, "harnesses")?.into_iter().flatten() {
        let item = item.as_object().ok_or(Unsure)?;
        publics.push((key, item, public_harness(item, available, home)?));
    }
    for (key, item, (is_available, public_auth)) in publics {
        let caps = get(item, "capabilities");
        let accounts = if truthy(caps) {
            caps.as_object()
                .ok_or(Unsure)?
                .get("accounts")
                .is_some_and(truthy)
        } else {
            false
        };
        let authenticated = if accounts {
            any_selectable(discovered.get(key).unwrap_or(&Value::Null))?
        } else {
            public_auth != Some(false)
        };
        // Sin `authFile` (`None`): basta la disponibilidad.
        let authenticated = if public_auth.is_none() {
            is_available
        } else {
            authenticated
        };
        harnesses.insert(
            key.clone(),
            json!({"available": is_available, "authenticated": authenticated}),
        );
    }
    let mut motors = Map::new();
    for provider in section(registry, "motors")?
        .into_iter()
        .flatten()
        .map(|(k, _)| k)
    {
        let authenticated = match discovered.get(provider) {
            Some(accounts) => any_selectable(accounts)?,
            None => harnesses
                .get(provider)
                .and_then(|h| h.get("available"))
                .is_some_and(truthy),
        };
        motors.insert(provider.clone(), json!({"authenticated": authenticated}));
    }
    Ok(json!({
        "harnesses": harnesses,
        "motors": motors,
        "gateway": {"installed": gateway_installed, "alive": gateway_alive},
    }))
}

/// Ids de las celdas `selectable` de `evaluate_capability_matrix`
/// (`lib/providers.py`): solo rutas del registro sin ninguna de las cinco
/// razones (binario, login del harness, pasarela, login del motor, puente
/// experimental sin verificar). Exclusiones y celdas sin ruta nunca lo son.
pub fn selectable_routes(registry: &Value, facts: &Value) -> Result<BTreeSet<String>, Unsure> {
    let flag = |map: &Value, key: &str| map.get(key).is_some_and(truthy);
    let mut out = BTreeSet::new();
    for route in py_iter(registry.get("routes").unwrap_or(&Value::Null))? {
        let route = route.as_object().ok_or(Unsure)?;
        let harness = get(route, "harness").as_str().ok_or(Unsure)?;
        let motor = get(route, "motor").as_str().ok_or(Unsure)?;
        let hf = facts
            .get("harnesses")
            .and_then(|h| h.get(harness))
            .unwrap_or(&Value::Null);
        let mf = facts
            .get("motors")
            .and_then(|m| m.get(motor))
            .unwrap_or(&Value::Null);
        let gateway = facts.get("gateway").unwrap_or(&Value::Null);
        let requirements = py_iter(get(route, "authRequirements"))?;
        let requires = |want: &str| requirements.iter().any(|r| r.as_str() == Some(want));
        let blocked = !flag(hf, "available")
            || !flag(hf, "authenticated")
            || (requires("gateway") && !flag(gateway, "alive"))
            || (requires(&format!("motor:{motor}")) && !flag(mf, "authenticated"))
            || (truthy(get(route, "experimental")) && !truthy(get(route, "liveVerified")));
        if !blocked {
            out.insert(get(route, "id").as_str().ok_or(Unsure)?.to_owned());
        }
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// Tiers y proxy
// ---------------------------------------------------------------------------

/// Texto o `""` para `x or ""` cuando el resultado llega como texto.
fn text_or_empty(value: &Value) -> Result<String, Unsure> {
    if truthy(value) {
        value.as_str().map(str::to_owned).ok_or(Unsure)
    } else {
        Ok(String::new())
    }
}

/// `model_tier` (`bin/cc-dash`) sobre el `model-tiers.json` ya leído (un no
/// objeto es el `{}` de `load_model_tiers`): primer patrón que casa bajo
/// `re.I`, si no `defaultTier`. Patrón incierto, elemento que no es objeto o
/// `match` que no es texto → `Unsure`.
pub fn model_tier(tiers: &Value, model: &str) -> Result<String, Unsure> {
    if model.is_empty() {
        return Ok(String::new());
    }
    let empty = Map::new();
    let cfg = tiers.as_object().unwrap_or(&empty);
    for pattern in py_iter(get(cfg, "patterns"))? {
        let pattern = pattern.as_object().ok_or(Unsure)?;
        let matcher = match pattern.get("match") {
            None => "",
            Some(Value::String(s)) => s.as_str(),
            Some(_) => return Err(Unsure),
        };
        if search_ci(matcher, model)? {
            return text_or_empty(get(pattern, "tier"));
        }
    }
    text_or_empty(get(cfg, "defaultTier"))
}

/// `(tier_style(tier).get("symbol") or "") if tier else ""` de
/// `write_app_tab_models` (`bin/cc-dash`): el símbolo crudo si es verdadero.
pub fn tier_symbol(tiers: &Value, tier: &str) -> Result<Value, Unsure> {
    if tier.is_empty() {
        return Ok(json!(""));
    }
    let empty = Map::new();
    let cfg = tiers.as_object().unwrap_or(&empty);
    let style = py_items(get(cfg, "tiers"))?
        .and_then(|t| t.get(tier))
        .and_then(Value::as_object);
    Ok(style
        .and_then(|s| s.get("symbol"))
        .filter(|s| truthy(s))
        .cloned()
        .unwrap_or_else(|| json!("")))
}

/// `int(x)` de Python sobre un texto ASCII simple (signo, dígitos con `_`
/// sueltos, blancos de `int`); lo demás → `Unsure`.
fn py_int_text(text: &str) -> Result<i128, Unsure> {
    if !text.is_ascii() || text.bytes().any(|b| (0x1c..=0x1f).contains(&b)) {
        return Err(Unsure);
    }
    let t = comandos_core::text::strip_numeric(text);
    let (negative, digits) = match t.as_bytes().first() {
        Some(b'-') => (true, t.get(1..).unwrap_or("")),
        Some(b'+') => (false, t.get(1..).unwrap_or("")),
        _ => (false, t),
    };
    let well_formed = !digits.is_empty()
        && !digits.starts_with('_')
        && !digits.ends_with('_')
        && !digits.contains("__")
        && digits.bytes().all(|b| b.is_ascii_digit() || b == b'_');
    if !well_formed {
        return Err(Unsure);
    }
    let n: i128 = digits.replace('_', "").parse().map_err(|_| Unsure)?;
    Ok(if negative { -n } else { n })
}

/// `\uD800`–`\uDFFF`: el `json` del Python los admite sueltos; el Rust no.
fn surrogate_escape(text: &str) -> bool {
    text.match_indices("\\u").any(|(i, _)| {
        let hex = text.as_bytes().get(i + 2..i + 4).unwrap_or_default();
        matches!(hex, [b'd' | b'D', b'8'..=b'9' | b'a'..=b'f' | b'A'..=b'F'])
    })
}

/// El límite de anidamiento del parser portado no es el de CPython.
fn deep(text: &str) -> bool {
    text.bytes().filter(|b| matches!(b, b'[' | b'{')).count()
        >= comandos_core::json::MAX_WORKSPACE_JSON_DEPTH
}

/// `int(load_proxy_cfg().get("port") or 18765)` (`bin/cc-dash`) con
/// `repo_root/config/proxy.json`. Archivo ilegible o JSON roto → 18765 (el
/// `except Exception` de `load_proxy_cfg`); UTF-8 inválido, no objeto, `int()`
/// imposible o puerto fuera de 0–65535 (el `OverflowError` de `socket`) →
/// `Unsure`.
pub fn proxy_port(repo_root: &Path) -> Result<u16, Unsure> {
    const DEFAULT: u16 = 18765;
    let Ok(bytes) = fs::read(repo_root.join("config/proxy.json")) else {
        return Ok(DEFAULT);
    };
    let text = std::str::from_utf8(&bytes).map_err(|_| Unsure)?;
    // `json.loads` de un `str` con BOM falla: el `except` devuelve el defecto.
    if text.starts_with('\u{feff}') {
        return Ok(DEFAULT);
    }
    let cfg = match workspace_loads(text) {
        Ok(value) => value,
        Err(_) if surrogate_escape(text) || deep(text) => return Err(Unsure),
        Err(_) => return Ok(DEFAULT),
    };
    let cfg = cfg.as_object().ok_or(Unsure)?;
    let port = get(cfg, "port");
    let n: i128 = if !truthy(port) {
        i128::from(DEFAULT)
    } else {
        match port {
            Value::Bool(true) => 1,
            Value::String(s) => py_int_text(s)?,
            Value::Number(n) => {
                let text = n.as_str();
                if text.contains(['.', 'e', 'E']) {
                    let f = n.as_f64().ok_or(Unsure)?;
                    if !f.is_finite() {
                        return Err(Unsure);
                    }
                    f.trunc() as i128
                } else {
                    text.parse().map_err(|_| Unsure)?
                }
            }
            _ => return Err(Unsure),
        }
    };
    u16::try_from(n).map_err(|_| Unsure)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn translation_emits_safe_syntax() {
        let t = |p: &str, ic: bool| -> Option<String> {
            translate(&p.chars().collect::<Vec<_>>(), ic).ok()
        };
        assert_eq!(t(r"a\<b", false).as_deref(), Some(r"a\x{3C}b"));
        assert_eq!(
            t(r"[[:a:]]", false).as_deref(),
            Some(r"[\x{5B}\x{3A}a\x{3A}]\x{5D}")
        );
        assert_eq!(t(r"a{,2}", false).as_deref(), Some("a{0,2}"));
        assert_eq!(t(r"a{2,1}", false), None);
        assert_eq!(t(r"a**", false), None);
        assert_eq!(t(r"^*", false), None);
        assert_eq!(t(r"\p{L}", false), None);
        assert_eq!(t(r"é", true), None);
        assert_eq!(t(r"é", false).as_deref(), Some(r"\x{E9}"));
        assert_eq!(t(r"[Z-a]", true), None);
        assert!(t(r"[a-z]", true).is_some());
    }

    #[test]
    fn lookahead_alternatives() {
        let re = py_regex(r"sonnet|gpt-5\.4(?!-mini)", true).unwrap();
        assert_eq!(py_search(&re, "GPT-5.4"), Ok(true));
        assert_eq!(py_search(&re, "gpt-5.4-mini"), Ok(false));
        assert_eq!(py_search(&re, "gpt-5.4-mini gpt-5.4"), Ok(true));
        assert_eq!(py_search(&re, "x-sonnet"), Ok(true));
        assert!(py_regex(r"(?:a)(?!b)", false).is_err());
        assert!(py_regex(r"a(?!b(?=c))", false).is_err());
    }

    #[test]
    fn not_boundary_on_empty_subject_is_unsure() {
        // `re.search(r"\B", "")` es None en CPython 3.10; `regex` casaría.
        let re = py_regex(r"x|\B", true).unwrap();
        assert_eq!(py_search(&re, ""), Err(Unsure));
        assert_eq!(py_search(&re, "ab"), Ok(true));
        let escaped = py_regex(r"\\B", false).unwrap();
        assert_eq!(py_search(&escaped, ""), Ok(false));
    }
}
