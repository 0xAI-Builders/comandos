//! Las dos piezas de bash que `cc-status.sh` aplica a datos ajenos: la aritmética
//! `$(( now - ${t:-0} ))` sobre el `ts` de cada estado y el `printf '%b'` final.
//!
//! La aritmética cubre lo que un `ts` puede traer en la práctica: un operando con
//! signos unarios (entero decimal, octal `0…`, hexadecimal `0x…` o `base#dígitos`,
//! con el desborde de `intmax_t`) o un identificador (las variables del script en
//! ese punto o del entorno; las inexistentes valen 0). Una expresión con operadores
//! se trata como error de bash (que aborta el bucle): desviación deliberada.

/// Variables del script visibles en la aritmética del bucle.
pub struct Vars<'a> {
    pub now: i64,
    /// `age` de la vuelta anterior (`None` = sin asignar todavía).
    pub age: Option<i64>,
    pub p: &'a [u8],
    pub s: &'a [u8],
    pub t: &'a [u8],
    pub rest: &'a [u8],
    pub line: &'a [u8],
    pub waiting: &'a [u8],
    pub done: &'a [u8],
}

/// `$(( now - ${t:-0} ))`; `None` = error aritmético de bash.
pub fn arith(vars: &Vars) -> Option<i64> {
    let t = if vars.t.is_empty() { b"0" } else { vars.t };
    Some(vars.now.wrapping_sub(operand(t, vars, 0)?))
}

fn is_space(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n')
}

/// Valor de un texto como expresión de un solo operando.
fn operand(text: &[u8], vars: &Vars, depth: usize) -> Option<i64> {
    if depth > 64 {
        return None; // "expression recursion level exceeded"
    }
    let mut i = 0;
    let skip = |i: &mut usize| {
        while *i < text.len() && is_space(text[*i]) {
            *i += 1;
        }
    };
    skip(&mut i);
    if i == text.len() {
        return Some(0);
    }
    let mut negative = false;
    while i < text.len() && matches!(text[i], b'+' | b'-') {
        // `--var`/`++var` serían pre-decremento/incremento: fuera del modelo.
        if text.get(i + 1) == Some(&text[i])
            && text
                .get(i + 2)
                .is_some_and(|b| b.is_ascii_alphabetic() || *b == b'_')
        {
            return None;
        }
        negative ^= text[i] == b'-';
        i += 1;
        skip(&mut i);
    }
    let start = i;
    while i < text.len()
        && (text[i].is_ascii_alphanumeric() || matches!(text[i], b'_' | b'#' | b'@'))
    {
        i += 1;
    }
    let token = &text[start..i];
    skip(&mut i);
    if token.is_empty() || i != text.len() {
        return None;
    }
    let value = if token[0].is_ascii_digit() {
        literal(token)?
    } else if token
        .iter()
        .all(|b| b.is_ascii_alphanumeric() || *b == b'_')
    {
        variable(token, vars, depth)?
    } else {
        return None;
    };
    Some(if negative {
        value.wrapping_neg()
    } else {
        value
    })
}

fn digits(text: &[u8], base: u32, case_folds: bool) -> Option<i64> {
    let mut value: i64 = 0;
    for &b in text {
        let d = match b {
            b'0'..=b'9' => u32::from(b - b'0'),
            b'a'..=b'z' => u32::from(b - b'a') + 10,
            b'A'..=b'Z' if case_folds => u32::from(b - b'A') + 10,
            b'A'..=b'Z' => u32::from(b - b'A') + 36,
            b'@' => 62,
            b'_' => 63,
            _ => return None,
        };
        if d >= base {
            return None; // "value too great for base"
        }
        value = value
            .wrapping_mul(i64::from(base))
            .wrapping_add(i64::from(d));
    }
    Some(value)
}

fn literal(token: &[u8]) -> Option<i64> {
    if let Some(hash) = token.iter().position(|&b| b == b'#') {
        let base: u32 = std::str::from_utf8(&token[..hash]).ok()?.parse().ok()?;
        if !(2..=64).contains(&base) {
            return None;
        }
        return digits(&token[hash + 1..], base, base <= 36);
    }
    if let Some(hex) = token
        .strip_prefix(b"0x")
        .or_else(|| token.strip_prefix(b"0X"))
    {
        return digits(hex, 16, true);
    }
    if token.len() > 1 && token[0] == b'0' {
        return digits(&token[1..], 8, true);
    }
    digits(token, 10, true)
}

fn variable(name: &[u8], vars: &Vars, depth: usize) -> Option<i64> {
    let text: &[u8] = match name {
        b"now" => return Some(vars.now),
        b"age" => return Some(vars.age.unwrap_or(0)),
        b"p" => vars.p,
        b"s" => vars.s,
        b"t" => vars.t,
        b"rest" => vars.rest,
        b"line" => vars.line,
        b"waiting" => vars.waiting,
        b"done_" => vars.done,
        // Rutas: nunca son una expresión válida.
        b"STATE" | b"CACHE" | b"files" => return None,
        other => {
            let name = std::str::from_utf8(other).ok()?;
            return match std::env::var_os(name) {
                Some(value) => operand(value.as_encoded_bytes(), vars, depth + 1),
                None => Some(0),
            };
        }
    };
    operand(text, vars, depth + 1)
}

fn push_char(code: u32, out: &mut Vec<u8>) {
    // UTF-8 por bits, como bash (también sustitutos y más allá de U+10FFFF).
    let n = code;
    if n < 0x80 {
        out.push(n as u8);
    } else if n < 0x800 {
        out.extend([0xc0 | (n >> 6) as u8, 0x80 | (n & 0x3f) as u8]);
    } else if n < 0x10000 {
        out.extend([
            0xe0 | (n >> 12) as u8,
            0x80 | ((n >> 6) & 0x3f) as u8,
            0x80 | (n & 0x3f) as u8,
        ]);
    } else if n < 0x20_0000 {
        out.extend([
            0xf0 | (n >> 18) as u8,
            0x80 | ((n >> 12) & 0x3f) as u8,
            0x80 | ((n >> 6) & 0x3f) as u8,
            0x80 | (n & 0x3f) as u8,
        ]);
    }
}

/// `printf '%b' TEXTO` de bash 5.
pub fn printf_b(text: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(text.len());
    let mut i = 0;
    let take = |i: &mut usize, max: usize, radix: u32| -> Option<u32> {
        let start = *i;
        while *i < text.len() && *i - start < max && char::from(text[*i]).is_digit(radix) {
            *i += 1;
        }
        (*i > start).then(|| {
            text[start..*i].iter().fold(0u32, |v, &b| {
                v * radix + char::from(b).to_digit(radix).unwrap_or(0)
            })
        })
    };
    while i < text.len() {
        if text[i] != b'\\' || i + 1 >= text.len() {
            out.push(text[i]);
            i += 1;
            continue;
        }
        let escape = text[i + 1];
        i += 2;
        match escape {
            b'a' => out.push(7),
            b'b' => out.push(8),
            b'e' | b'E' => out.push(0x1b),
            b'f' => out.push(0xc),
            b'n' => out.push(b'\n'),
            b'r' => out.push(b'\r'),
            b't' => out.push(b'\t'),
            b'v' => out.push(0xb),
            b'\\' => out.push(b'\\'),
            b'c' => return out,
            b'0' => out.push(take(&mut i, 3, 8).unwrap_or(0) as u8),
            b'1'..=b'7' => {
                i -= 1;
                out.push(take(&mut i, 3, 8).unwrap_or(0) as u8);
            }
            b'x' => match take(&mut i, 2, 16) {
                Some(v) => out.push(v as u8),
                None => out.extend_from_slice(b"\\x"),
            },
            b'u' | b'U' => match take(&mut i, if escape == b'u' { 4 } else { 8 }, 16) {
                Some(v) => push_char(v, &mut out),
                None => out.extend([b'\\', escape]),
            },
            other => out.extend([b'\\', other]),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vars(t: &[u8]) -> Vars<'_> {
        Vars {
            now: 1000,
            age: None,
            p: b"proj",
            s: b"done",
            t,
            rest: b"",
            line: b"",
            waiting: b"",
            done: b"",
        }
    }

    #[test]
    fn arithmetic_like_bash() {
        // Medido con `bash -c 'now=1000; t=...; echo $(( now - ${t:-0} ))'`.
        for (t, want) in [
            (" 12 ", Some(988)),
            ("012", Some(990)),
            ("089", None),
            ("12.5", None),
            ("null", Some(1000)),
            ("now", Some(0)),
            ("-5", Some(1005)),
            ("+3", Some(997)),
            ("0x1A", Some(974)),
            ("2#101", Some(995)),
            ("1e5", None),
            ("a b", None),
            ("", Some(1000)),
            ("9999999999999999999999", Some(-1864712049423023127)),
            ("--5", Some(995)),
            ("- 5", Some(1005)),
            ("0x", Some(1000)),
            ("64#_@", Some(-3094)),
            ("36#Zz", Some(-295)),
            ("1#1", None),
        ] {
            assert_eq!(arith(&vars(t.as_bytes())), want, "{t:?}");
        }
    }

    #[test]
    fn printf_b_like_bash() {
        for (input, want) in [
            (&br"a\nb"[..], &b"a\nb"[..]),
            (br"\101", b"A"),
            (br"\0101", b"A"),
            (br"\x41\x4", b"A\x04"),
            (&[b'\\', b'u', b'0', b'0', b'e', b'9'][..], "é".as_bytes()),
            (br"x\cy", b"x"),
            (br"\q", br"\q"),
            (br"a\", br"a\"),
            (br"\08", b"\x008"),
            (br"\777", b"\xff"),
            (br"\xZ", br"\xZ"),
            (br"\uD800", b"\xed\xa0\x80"),
            (br"\00001", b"\x001"),
        ] {
            assert_eq!(
                printf_b(input),
                want,
                "{:?}",
                String::from_utf8_lossy(input)
            );
        }
    }
}
