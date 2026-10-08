//! `str(json.JSONDecodeError)` de `json.loads(text)` en CPython 3.10 (el
//! escáner en C de `_json.c`, `strict=True`): el mensaje y su posición
//! (`<msg>: line L column C (char P)`, en puntos de código). Sirve para las
//! rutas que devuelven `{"error": str(exc)}` al capturar un `ValueError` sobre
//! texto guardado.

/// Qué haría `json.loads(text)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PythonLoads {
    /// Lo decodifica (sin error).
    Ok,
    /// Lanza `JSONDecodeError` con este `str(exc)`.
    Error(String),
    /// Otra excepción o un caso que no se reproduce con certeza
    /// (`RecursionError`, enteros de más de 4300 dígitos).
    Unsure,
}

/// Anidamiento a partir del que CPython puede agotar la recursión (con la
/// pila del servidor por debajo): no se afirma nada.
const MAX_NESTING: usize = 800;
/// `sys.get_int_max_str_digits()` por omisión (3.10.7+).
const MAX_INT_DIGITS: usize = 4300;

enum Fail {
    /// `StopIteration(idx)`: `Expecting value` en `idx`.
    Stop(usize),
    Message(&'static str, usize),
    Unsure,
}

struct Scanner {
    s: Vec<char>,
}

fn ws(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\r')
}

fn digit(c: Option<char>) -> bool {
    c.is_some_and(|c| c.is_ascii_digit())
}

impl Scanner {
    fn at(&self, idx: usize) -> Option<char> {
        self.s.get(idx).copied()
    }

    fn skip_ws(&self, mut idx: usize) -> usize {
        while self.at(idx).is_some_and(ws) {
            idx += 1;
        }
        idx
    }

    fn literal(&self, idx: usize, word: &str) -> bool {
        word.chars()
            .enumerate()
            .all(|(i, c)| self.at(idx + i) == Some(c))
    }

    /// `scan_once_unicode`: fin del valor que empieza en `idx`.
    fn scan(&self, idx: usize, depth: usize) -> Result<usize, Fail> {
        let Some(c) = self.at(idx) else {
            return Err(Fail::Stop(idx));
        };
        match c {
            '"' => return self.string(idx + 1),
            '{' | '[' if depth >= MAX_NESTING => return Err(Fail::Unsure),
            '{' => return self.object(idx + 1, depth + 1),
            '[' => return self.array(idx + 1, depth + 1),
            'n' if self.literal(idx, "null") => return Ok(idx + 4),
            't' if self.literal(idx, "true") => return Ok(idx + 4),
            'f' if self.literal(idx, "false") => return Ok(idx + 5),
            'N' if self.literal(idx, "NaN") => return Ok(idx + 3),
            'I' if self.literal(idx, "Infinity") => return Ok(idx + 8),
            '-' if self.literal(idx, "-Infinity") => return Ok(idx + 9),
            _ => {}
        }
        self.number(idx)
    }

    /// `_match_number_unicode`.
    fn number(&self, start: usize) -> Result<usize, Fail> {
        let len = self.s.len();
        let mut idx = start;
        if self.at(idx) == Some('-') {
            idx += 1;
            if idx >= len {
                return Err(Fail::Stop(start));
            }
        }
        let integer = idx;
        match self.at(idx) {
            Some('1'..='9') => {
                idx += 1;
                while digit(self.at(idx)) {
                    idx += 1;
                }
            }
            Some('0') => idx += 1,
            _ => return Err(Fail::Stop(start)),
        }
        let mut float = false;
        if idx + 1 < len && self.at(idx) == Some('.') && digit(self.at(idx + 1)) {
            float = true;
            idx += 2;
            while digit(self.at(idx)) {
                idx += 1;
            }
        }
        if idx + 1 < len && matches!(self.at(idx), Some('e' | 'E')) {
            let e_start = idx;
            idx += 1;
            if idx + 1 < len && matches!(self.at(idx), Some('-' | '+')) {
                idx += 1;
            }
            while digit(self.at(idx)) {
                idx += 1;
            }
            if digit(self.at(idx - 1)) {
                float = true;
            } else {
                idx = e_start;
            }
        }
        // `int(text)` con el límite de dígitos: `ValueError`, no `JSONDecodeError`.
        if !float && idx - integer > MAX_INT_DIGITS {
            return Err(Fail::Unsure);
        }
        Ok(idx)
    }

    /// `scanstring_unicode(s, end, strict=True)`: `end` es el índice tras la comilla.
    fn string(&self, mut end: usize) -> Result<usize, Fail> {
        let len = self.s.len();
        let begin = end - 1;
        loop {
            let mut next = end;
            let mut c = '\0';
            while let Some(d) = self.at(next) {
                c = d;
                if c == '"' || c == '\\' {
                    break;
                }
                if (c as u32) <= 0x1f {
                    return Err(Fail::Message("Invalid control character at", next));
                }
                next += 1;
            }
            if next >= len || !(c == '"' || c == '\\') {
                return Err(Fail::Message("Unterminated string starting at", begin));
            }
            next += 1;
            if c == '"' {
                return Ok(next);
            }
            if next == len {
                return Err(Fail::Message("Unterminated string starting at", begin));
            }
            let escaped = self.at(next).unwrap_or('\0');
            if escaped != 'u' {
                end = next + 1;
                if !matches!(escaped, '"' | '\\' | '/' | 'b' | 'f' | 'n' | 'r' | 't') {
                    return Err(Fail::Message("Invalid \\escape", end - 2));
                }
                continue;
            }
            next += 1;
            end = next + 4;
            if end >= len {
                return Err(Fail::Message("Invalid \\uXXXX escape", next - 1));
            }
            let Some(unit) = self.hex4(next) else {
                return Err(Fail::Message("Invalid \\uXXXX escape", end - 5));
            };
            next = end;
            // Par sustituto: solo con `\u` justo detrás y sitio de sobra.
            if (0xd800..=0xdbff).contains(&unit)
                && end + 6 < len
                && self.at(next) == Some('\\')
                && self.at(next + 1) == Some('u')
            {
                end += 6;
                let Some(low) = self.hex4(next + 2) else {
                    return Err(Fail::Message("Invalid \\uXXXX escape", end - 5));
                };
                if !(0xdc00..=0xdfff).contains(&low) {
                    end -= 6;
                }
            }
        }
    }

    fn hex4(&self, at: usize) -> Option<u32> {
        let mut out = 0;
        for i in 0..4 {
            out = out * 16 + self.at(at + i)?.to_digit(16)?;
        }
        Some(out)
    }

    /// `_parse_object_unicode`: `idx` es el índice tras `{`.
    fn object(&self, idx: usize, depth: usize) -> Result<usize, Fail> {
        let mut idx = self.skip_ws(idx);
        if self.at(idx) != Some('}') {
            loop {
                if self.at(idx) != Some('"') {
                    return Err(Fail::Message(
                        "Expecting property name enclosed in double quotes",
                        idx,
                    ));
                }
                idx = self.skip_ws(self.string(idx + 1)?);
                if self.at(idx) != Some(':') {
                    return Err(Fail::Message("Expecting ':' delimiter", idx));
                }
                idx = self.skip_ws(idx + 1);
                idx = self.skip_ws(self.scan(idx, depth)?);
                if self.at(idx) == Some('}') {
                    break;
                }
                if self.at(idx) != Some(',') {
                    return Err(Fail::Message("Expecting ',' delimiter", idx));
                }
                idx = self.skip_ws(idx + 1);
            }
        }
        Ok(idx + 1)
    }

    /// `_parse_array_unicode`: `idx` es el índice tras `[`.
    fn array(&self, idx: usize, depth: usize) -> Result<usize, Fail> {
        let mut idx = self.skip_ws(idx);
        if self.at(idx) != Some(']') {
            loop {
                idx = self.skip_ws(self.scan(idx, depth)?);
                if self.at(idx) == Some(']') {
                    break;
                }
                if self.at(idx) != Some(',') {
                    return Err(Fail::Message("Expecting ',' delimiter", idx));
                }
                idx = self.skip_ws(idx + 1);
            }
        }
        Ok(idx + 1)
    }
}

/// `JSONDecodeError(msg, doc, pos)`: `lineno` y `colno` como el Python.
fn message(s: &[char], msg: &str, pos: usize) -> String {
    let before = s.get(..pos).unwrap_or(s);
    let line = before.iter().filter(|c| **c == '\n').count() + 1;
    let column = match before.iter().rposition(|c| *c == '\n') {
        Some(newline) => pos - newline,
        None => pos + 1,
    };
    format!("{msg}: line {line} column {column} (char {pos})")
}

/// `json.loads(text)` de CPython 3.10: si decodifica, el `str()` de su
/// `JSONDecodeError`, o si no se sabe con certeza.
pub fn python_loads(text: &str) -> PythonLoads {
    let scanner = Scanner {
        s: text.chars().collect(),
    };
    let s = &scanner.s;
    if s.first() == Some(&'\u{feff}') {
        return PythonLoads::Error(message(
            s,
            "Unexpected UTF-8 BOM (decode using utf-8-sig)",
            0,
        ));
    }
    let start = scanner.skip_ws(0);
    match scanner.scan(start, 0) {
        Ok(end) => {
            let end = scanner.skip_ws(end);
            if end == s.len() {
                PythonLoads::Ok
            } else {
                PythonLoads::Error(message(s, "Extra data", end))
            }
        }
        Err(Fail::Stop(idx)) => PythonLoads::Error(message(s, "Expecting value", idx)),
        Err(Fail::Message(msg, idx)) => PythonLoads::Error(message(s, msg, idx)),
        Err(Fail::Unsure) => PythonLoads::Unsure,
    }
}
