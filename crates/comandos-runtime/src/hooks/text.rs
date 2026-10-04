//! Semántica de bytes de las herramientas que usa `hooks/cc-notify.sh`: `head -c`,
//! sustitución de comandos `$(...)`, `tr`, `sed` de `mdclean`, y la forma exacta en
//! que jq 1.6 repara UTF-8 inválido y serializa JSON. Las variables de bash son
//! bytes; aquí también, y solo se convierten a texto donde jq las recibe.

/// Bytes de un `--arg` de jq 1.6: cada secuencia UTF-8 inválida se cambia por
/// U+FFFD con la misma partición que `jvp_utf8_next` (no la de `from_utf8_lossy`:
/// jq consume la secuencia entera que anuncia el primer byte).
pub fn jq_lossy(bytes: &[u8]) -> String {
    if let Ok(text) = std::str::from_utf8(bytes) {
        return text.to_owned();
    }
    let mut out = String::with_capacity(bytes.len() + 8);
    let mut i = 0;
    while i < bytes.len() {
        let first = bytes[i];
        let length = coding_length(first);
        let (codepoint, used) = if first < 0x80 {
            (Some(u32::from(first)), 1)
        } else if length == 0 || length == CONT {
            (None, 1)
        } else if i + length > bytes.len() {
            (None, bytes.len() - i)
        } else {
            let mut cp = u32::from(first) & [0, 0x7f, 0x1f, 0x0f, 0x07][length];
            let mut used = length;
            let mut valid = true;
            for (k, &ch) in bytes[i + 1..i + length].iter().enumerate() {
                if coding_length(ch) != CONT {
                    valid = false;
                    used = k + 1;
                    break;
                }
                cp = (cp << 6) | u32::from(ch & 0x3f);
            }
            let overlong = valid && cp < [0, 0, 0x80, 0x800, 0x10000][used];
            let bad = !valid || overlong || (0xd800..=0xdfff).contains(&cp) || cp > 0x10ffff;
            (if bad { None } else { Some(cp) }, used)
        };
        out.push(codepoint.and_then(char::from_u32).unwrap_or('\u{fffd}'));
        i += used;
    }
    out
}

const CONT: usize = 255;
fn coding_length(byte: u8) -> usize {
    match byte {
        0x00..=0x7f => 1,
        0x80..=0xbf => CONT,
        0xc0..=0xc1 | 0xf5..=0xff => 0,
        0xc2..=0xdf => 2,
        0xe0..=0xef => 3,
        0xf0..=0xf4 => 4,
    }
}

/// `head -c n`.
pub fn head_c(bytes: &[u8], n: usize) -> &[u8] {
    &bytes[..bytes.len().min(n)]
}

/// Lo que deja `$(...)`: sin los saltos de línea finales.
pub fn strip_nl(bytes: &[u8]) -> &[u8] {
    let end = bytes.iter().rposition(|&b| b != b'\n').map_or(0, |i| i + 1);
    &bytes[..end]
}

/// `$(printf '%s' X | head -c n)`.
pub fn cut(bytes: &[u8], n: usize) -> Vec<u8> {
    strip_nl(head_c(bytes, n)).to_vec()
}

/// `tr 'a' 'b'` de un solo byte.
pub fn tr(bytes: &[u8], from: u8, to: u8) -> Vec<u8> {
    bytes
        .iter()
        .map(|&b| if b == from { to } else { b })
        .collect()
}

/// `tail -n n`: una última línea sin `\n` también cuenta.
pub fn tail_lines(bytes: &[u8], n: usize) -> &[u8] {
    if n == 0 || bytes.is_empty() {
        return &bytes[..0];
    }
    let body = bytes.strip_suffix(b"\n").unwrap_or(bytes);
    let mut seen = 0;
    for (i, &b) in body.iter().enumerate().rev() {
        if b == b'\n' {
            seen += 1;
            if seen == n {
                return &bytes[i + 1..];
            }
        }
    }
    bytes
}

/// `basename` de una ruta no vacía.
pub fn basename(path: &[u8]) -> Vec<u8> {
    let trimmed = &path[..path.iter().rposition(|&b| b != b'/').map_or(0, |i| i + 1)];
    if trimmed.is_empty() {
        return if path.is_empty() {
            Vec::new()
        } else {
            b"/".to_vec()
        };
    }
    let start = trimmed
        .iter()
        .rposition(|&b| b == b'/')
        .map_or(0, |i| i + 1);
    trimmed[start..].to_vec()
}

/// `printf '%s' X | grep -Eq PATRÓN` con un patrón anclado `^...$`: basta una línea.
pub fn any_line(bytes: &[u8], matches: impl Fn(&[u8]) -> bool) -> bool {
    !bytes.is_empty() && strip_one_nl(bytes).split(|&b| b == b'\n').any(matches)
}

fn strip_one_nl(bytes: &[u8]) -> &[u8] {
    bytes.strip_suffix(b"\n").unwrap_or(bytes)
}

/// `grep -qi aguja` con aguja ASCII sin saltos de línea.
pub fn contains_ci(hay: &[u8], needle: &[u8]) -> bool {
    hay.windows(needle.len())
        .any(|w| w.eq_ignore_ascii_case(needle))
}

/// `mdclean` del bash más el `$(... <<<"$body")` que lo envuelve.
pub fn mdclean(body: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(body.len());
    for (n, line) in body.split(|&b| b == b'\n').enumerate() {
        if n > 0 {
            out.push(b'\n');
        }
        out.extend(clean_line(line));
    }
    strip_nl(&out).to_vec()
}

fn clean_line(line: &[u8]) -> Vec<u8> {
    // s/\*\*//g y s/`//g
    let mut s = Vec::with_capacity(line.len());
    let mut i = 0;
    while i < line.len() {
        if line[i..].starts_with(b"**") {
            i += 2;
        } else {
            s.push(line[i]);
            i += 1;
        }
    }
    s.retain(|&b| b != b'`');
    // s/^#\{1,6\} //
    let hashes = s.iter().take_while(|&&b| b == b'#').count();
    if (1..=6).contains(&hashes) && s.get(hashes) == Some(&b' ') {
        s.drain(..=hashes);
    }
    // s/\[\([^]]*\)\]([^)]*)/\1/g
    let mut linked = Vec::with_capacity(s.len());
    let mut i = 0;
    while i < s.len() {
        if s[i] == b'['
            && let Some(close) = s[i + 1..]
                .iter()
                .position(|&b| b == b']')
                .map(|p| i + 1 + p)
            && s.get(close + 1) == Some(&b'(')
            && let Some(end) = s[close + 2..].iter().position(|&b| b == b')')
        {
            linked.extend_from_slice(&s[i + 1..close]);
            i = close + 2 + end + 1;
            continue;
        }
        linked.push(s[i]);
        i += 1;
    }
    // s/[&<>]/ /g
    for b in &mut linked {
        if matches!(*b, b'&' | b'<' | b'>') {
            *b = b' ';
        }
    }
    linked
}

#[cfg(test)]
mod tests {
    use super::*;

    fn codepoints(bytes: &[u8]) -> Vec<u32> {
        jq_lossy(bytes).chars().map(|c| c as u32).collect()
    }

    #[test]
    fn utf8_repair_follows_jq_not_rust() {
        // Casos medidos con `jq -cn --arg x ... '$x|explode'` (jq 1.6).
        assert_eq!(codepoints(b"a\xc3"), [97, 65533]);
        assert_eq!(codepoints(b"a\xe2\x82x"), [97, 65533, 120]);
        assert_eq!(codepoints(b"a\xc0\xafb"), [97, 65533, 65533, 98]);
        assert_eq!(codepoints(b"a\xed\xa0\x80b"), [97, 65533, 98]);
        assert_eq!(codepoints(b"a\xf4\x90\x80\x80b"), [97, 65533, 98]);
        assert_eq!(codepoints(b"x\xe2a"), [120, 65533]);
    }

    #[test]
    fn mdclean_matches_sed_examples() {
        let cases: [(&str, &str); 6] = [
            (
                "**Hecho:** `cargo` <ok> & [docs](https://x.y/z)",
                "Hecho: cargo  ok    docs",
            ),
            (
                "## Título\n####### no\n#sin espacio",
                "Título\n####### no\n#sin espacio",
            ),
            ("[a [b](c) y [rota](sin cierre", "a [b y [rota](sin cierre"),
            ("***", "*"),
            ("línea\n\n\n", "línea"),
            ("", ""),
        ];
        for (input, expected) in cases {
            assert_eq!(
                String::from_utf8(mdclean(input.as_bytes())).unwrap(),
                expected,
                "{input:?}"
            );
        }
    }

    #[test]
    fn shell_helpers() {
        assert_eq!(basename(b"/a/b/"), b"b");
        assert_eq!(basename(b"///"), b"/");
        assert_eq!(tail_lines(b"1\n2\n3\n", 2), b"2\n3\n");
        assert_eq!(tail_lines(b"1\n2\n3", 2), b"2\n3");
        assert_eq!(strip_nl(b"x\n\n"), b"x");
        assert!(any_line(b"foo\n%3", |l| l == b"%3"));
        assert!(contains_ci(b"Needs PERMISSION", b"permi"));
    }
}
