//! Texto con la semántica de CPython.

/// `str.splitlines()` de Python: `\n`, `\r`, `\r\n`, U+000B, U+000C,
/// U+001C–U+001E, U+0085, U+2028, U+2029; sin la línea vacía final.
pub fn splitlines(s: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0;
    let mut chars = s.char_indices().peekable();
    while let Some((i, c)) = chars.next() {
        let boundary = matches!(
            c,
            '\n' | '\r'
                | '\u{b}'
                | '\u{c}'
                | '\u{1c}'
                | '\u{1d}'
                | '\u{1e}'
                | '\u{85}'
                | '\u{2028}'
                | '\u{2029}'
        );
        if boundary {
            out.push(&s[start..i]);
            let mut next = i + c.len_utf8();
            if c == '\r' && chars.peek().is_some_and(|&(_, d)| d == '\n') {
                chars.next();
                next += 1;
            }
            start = next;
        }
    }
    if start < s.len() {
        out.push(&s[start..]);
    }
    out
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NumError {
    /// `ValueError` del Python.
    Invalid,
    /// Válido o inválido según reglas Unicode o de enteros grandes: se declina.
    Exotic,
}

/// Lo que `int()`/`float()` quitan a los lados de un texto ASCII: solo
/// `Py_ISSPACE` (` \t\n\v\f\r`), no U+001C–U+001F como `str.strip()`.
pub fn strip_numeric(s: &str) -> &str {
    s.trim_matches([' ', '\t', '\n', '\u{b}', '\u{c}', '\r'])
}

/// `str.isspace()` de CPython: `char::is_whitespace` más U+001C–U+001F.
pub fn is_space(c: char) -> bool {
    c.is_whitespace() || ('\u{1c}'..='\u{1f}').contains(&c)
}

/// `str.strip()` sin argumentos.
pub fn strip(s: &str) -> &str {
    s.trim_matches(is_space)
}

/// Dígitos ASCII con `_` solo entre dígitos (PEP 515).
fn digits_with_underscores(s: &str) -> Option<String> {
    let bytes = s.as_bytes();
    if bytes.is_empty() {
        return None;
    }
    let mut out = String::with_capacity(s.len());
    for (i, &b) in bytes.iter().enumerate() {
        if b == b'_' {
            let left = i
                .checked_sub(1)
                .and_then(|j| bytes.get(j))
                .is_some_and(u8::is_ascii_digit);
            let right = bytes.get(i + 1).is_some_and(u8::is_ascii_digit);
            if !(left && right) {
                return None;
            }
        } else if b.is_ascii_digit() {
            out.push(b as char);
        } else {
            return None;
        }
    }
    Some(out)
}

/// `int(text)` en base 10; un entero que no cabe en `i64` es `Exotic`.
pub fn int(text: &str) -> Result<i64, NumError> {
    if !text.is_ascii() {
        return Err(NumError::Exotic);
    }
    let t = strip_numeric(text);
    let (negative, body) = match t.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, t.strip_prefix('+').unwrap_or(t)),
    };
    let digits = digits_with_underscores(body).ok_or(NumError::Invalid)?;
    let magnitude: i128 = digits.parse().map_err(|_| NumError::Exotic)?;
    let value = if negative { -magnitude } else { magnitude };
    i64::try_from(value).map_err(|_| NumError::Exotic)
}

/// `float(text)`.
pub fn float(text: &str) -> Result<f64, NumError> {
    if !text.is_ascii() {
        return Err(NumError::Exotic);
    }
    let t = strip_numeric(text);
    let lower = t.to_ascii_lowercase();
    let unsigned = lower.trim_start_matches(['+', '-']);
    if lower.len() - unsigned.len() > 1 {
        return Err(NumError::Invalid);
    }
    if matches!(unsigned, "inf" | "infinity" | "nan") {
        return lower.parse::<f64>().map_err(|_| NumError::Invalid);
    }
    // Quita `_` solo si separa dos dígitos; cualquier otro `_` es inválido.
    let bytes = t.as_bytes();
    let mut clean = String::with_capacity(t.len());
    for (i, &b) in bytes.iter().enumerate() {
        if b == b'_' {
            let ok = i
                .checked_sub(1)
                .and_then(|j| bytes.get(j))
                .is_some_and(u8::is_ascii_digit)
                && bytes.get(i + 1).is_some_and(u8::is_ascii_digit);
            if !ok {
                return Err(NumError::Invalid);
            }
        } else if b.is_ascii_digit() || matches!(b, b'+' | b'-' | b'.' | b'e' | b'E') {
            clean.push(b as char);
        } else {
            return Err(NumError::Invalid);
        }
    }
    clean.parse::<f64>().map_err(|_| NumError::Invalid)
}
