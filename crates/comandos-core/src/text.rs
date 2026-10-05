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
