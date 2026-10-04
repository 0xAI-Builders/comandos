//! Las dos sustituciones de `grok-hooks.py` que tachan credenciales dentro del
//! detalle, con la semántica de `re` de Python (`str`, `IGNORECASE`, `\b` y `\s`
//! Unicode, alternativas probadas en orden, búsqueda de izquierda a derecha):
//!
//! - `(?i)\bbearer\s+[A-Za-z0-9._~+/=-]+` → `Bearer [REDACTED]`
//! - `(?i)\b(authorization|auth|access[_-]?token|refresh[_-]?token|token|`
//!   `api[_-]?key|password|passwd|client[_-]?secret)\b(\s*[:=]\s*)([^\s,;]+)`
//!   → `\1\2[REDACTED]`

/// `\w` de `re` con `str`: alfanumérico Unicode o `_`.
fn word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// `\s` de `re` con `str` (`str.isspace`).
fn space(c: char) -> bool {
    c.is_whitespace() || ('\u{1c}'..='\u{1f}').contains(&c)
}

/// Letra ASCII del patrón bajo `IGNORECASE`: `re` también iguala `ı`/`İ` con `i`,
/// `ſ` con `s` y el signo Kelvin con `k`.
fn ci(c: char, letter: char) -> bool {
    c.eq_ignore_ascii_case(&letter)
        || match letter {
            'i' => matches!(c, '\u{131}' | '\u{130}'),
            's' => c == '\u{17f}',
            'k' => c == '\u{212a}',
            _ => false,
        }
}

/// `[A-Za-z]` bajo `IGNORECASE`.
fn ci_letter(c: char) -> bool {
    c.is_ascii_alphabetic() || matches!(c, '\u{131}' | '\u{130}' | '\u{17f}' | '\u{212a}')
}

fn boundary(chars: &[char], at: usize) -> bool {
    let before = at > 0 && word(chars[at - 1]);
    let after = at < chars.len() && word(chars[at]);
    before != after
}

/// Literal del patrón (minúsculas ASCII; `?` tras `_-` = separador opcional).
fn literal(chars: &[char], mut at: usize, pattern: &str) -> Option<usize> {
    let pat: Vec<char> = pattern.chars().collect();
    let mut i = 0;
    while i < pat.len() {
        if pat[i] == '~' {
            // `[_-]?`: codicioso; si consume, el resto del literal no empieza por `_`/`-`.
            if at < chars.len() && matches!(chars[at], '_' | '-') {
                at += 1;
            }
            i += 1;
            continue;
        }
        if at < chars.len() && ci(chars[at], pat[i]) {
            at += 1;
            i += 1;
        } else {
            return None;
        }
    }
    Some(at)
}

fn bearer_at(chars: &[char], at: usize) -> Option<usize> {
    if !boundary(chars, at) {
        return None;
    }
    let mut i = literal(chars, at, "bearer")?;
    let spaces = chars[i..].iter().take_while(|&&c| space(c)).count();
    if spaces == 0 {
        return None;
    }
    i += spaces;
    let token = chars[i..]
        .iter()
        .take_while(|&&c| ci_letter(c) || c.is_ascii_digit() || "._~+/=-".contains(c))
        .count();
    (token > 0).then_some(i + token)
}

const KEYS: [&str; 9] = [
    "authorization",
    "auth",
    "access~token",
    "refresh~token",
    "token",
    "api~key",
    "password",
    "passwd",
    "client~secret",
];

/// (fin de `\1\2`, fin del valor).
fn assignment_at(chars: &[char], at: usize) -> Option<(usize, usize)> {
    if !boundary(chars, at) {
        return None;
    }
    for key in KEYS {
        let Some(end) = literal(chars, at, key) else {
            continue;
        };
        if !boundary(chars, end) {
            continue;
        }
        let mut i = end + chars[end..].iter().take_while(|&&c| space(c)).count();
        if i >= chars.len() || !matches!(chars[i], ':' | '=') {
            continue;
        }
        i += 1;
        i += chars[i..].iter().take_while(|&&c| space(c)).count();
        let value = chars[i..]
            .iter()
            .take_while(|&&c| !space(c) && c != ',' && c != ';')
            .count();
        if value == 0 {
            continue;
        }
        return Some((i, i + value));
    }
    None
}

pub fn redact(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut first = String::with_capacity(text.len());
    let mut i = 0;
    while i < chars.len() {
        if let Some(end) = bearer_at(&chars, i) {
            first.push_str("Bearer [REDACTED]");
            i = end;
        } else {
            first.push(chars[i]);
            i += 1;
        }
    }
    let chars: Vec<char> = first.chars().collect();
    let mut out = String::with_capacity(first.len());
    let mut i = 0;
    while i < chars.len() {
        if let Some((keep, end)) = assignment_at(&chars, i) {
            out.extend(&chars[i..keep]);
            out.push_str("[REDACTED]");
            i = end;
        } else {
            out.push(chars[i]);
            i += 1;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::redact;

    #[test]
    fn redacts_like_python_re() {
        for (input, want) in [
            // Medido con `_text()` de `adapters/grok-hooks.py`.
            (
                "authorization: Bearer abc.def",
                "authorization: [REDACTED] [REDACTED]",
            ),
            ("token=a token=b", "token=[REDACTED] token=[REDACTED]"),
            ("Bearer x Bearer y", "Bearer [REDACTED] Bearer [REDACTED]"),
            ("auth : x", "auth : [REDACTED]"),
            ("TOKEN = s3cr3t, next", "TOKEN = [REDACTED], next"),
            ("auth_token=x", "auth_token=x"),
            ("access-token:y;z", "access-token:[REDACTED];z"),
            ("my api_key=1", "my api_key=[REDACTED]"),
            ("bearer\t\nX", "Bearer [REDACTED]"),
            ("xbearer y", "xbearer y"),
            ("password=", "password="),
            ("Passwd:  é", "Passwd:  [REDACTED]"),
        ] {
            assert_eq!(redact(input), want, "{input:?}");
        }
    }
}
