//! Lossless internal representation of JavaScript UTF-16 in Unicode JSON.
//! The marker is escaped too, so ordinary private-use text cannot collide.
const MARK: char = '\u{e000}';
const BASE: u32 = 0xf0000;

pub fn encode(units: impl IntoIterator<Item = u16>) -> String {
    let mut out = String::new();
    for unit in char::decode_utf16(units) {
        match unit {
            Ok(ch) => {
                out.push(ch);
                if ch == MARK {
                    out.push(MARK);
                }
            }
            Err(error) => {
                out.push(MARK);
                if let Some(ch) =
                    char::from_u32(BASE + u32::from(error.unpaired_surrogate() - 0xd800))
                {
                    out.push(ch);
                }
            }
        }
    }
    out
}

pub fn decode(text: &str) -> Vec<u16> {
    let mut units = Vec::new();
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == MARK {
            match chars.peek().copied() {
                Some(MARK) => {
                    chars.next();
                }
                Some(next) if (BASE..BASE + 0x800).contains(&u32::from(next)) => {
                    chars.next();
                    units.push((u32::from(next) - BASE + 0xd800) as u16);
                    continue;
                }
                _ => {}
            }
        }
        let mut buffer = [0u16; 2];
        units.extend_from_slice(ch.encode_utf16(&mut buffer));
    }
    units
}

/// JSON.stringify escapes lone surrogates; serde_json rejects those escapes.
/// Rewrite only string tokens, including object keys, retaining every value.
pub fn json_to_unicode(source: &str) -> String {
    let mut out = String::new();
    let mut chars = source.chars().peekable();
    let mut quoted = false;
    while let Some(ch) = chars.next() {
        if ch == '"' {
            quoted = !quoted;
            out.push(ch);
        } else if quoted && ch == '\\' {
            let escaped = chars.next().unwrap_or('\\');
            if escaped != 'u' {
                out.push('\\');
                out.push(escaped);
                continue;
            }
            let digits: String = chars.by_ref().take(4).collect();
            let code = u16::from_str_radix(&digits, 16).ok();
            if code.is_some_and(|n| (0xd800..0xdc00).contains(&n)) {
                let next: String = chars.clone().take(6).collect();
                let low = next
                    .strip_prefix("\\u")
                    .and_then(|s| u16::from_str_radix(s, 16).ok());
                if low.is_some_and(|n| (0xdc00..=0xdfff).contains(&n)) {
                    out.push_str("\\u");
                    out.push_str(&digits);
                    out.push_str(&next);
                    for _ in 0..6 {
                        chars.next();
                    }
                    continue;
                }
            }
            if let Some(code) = code.filter(|n| (0xd800..=0xdfff).contains(n)) {
                out.push_str(&encode([code]));
            } else if code == Some(MARK as u16) {
                out.push(MARK);
                out.push(MARK);
            } else {
                out.push_str("\\u");
                out.push_str(&digits);
            }
        } else {
            out.push(ch);
            if quoted && ch == MARK {
                out.push(MARK);
            }
        }
    }
    out
}

/// Serialize the internal representation back to valid JavaScript JSON.
pub fn json_to_javascript(source: &str) -> String {
    let mut out = String::new();
    let mut chars = source.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == MARK {
            match chars.peek().copied() {
                Some(MARK) => {
                    chars.next();
                }
                Some(next) if (BASE..BASE + 0x800).contains(&u32::from(next)) => {
                    chars.next();
                    out.push_str(&format!("\\u{:04x}", u32::from(next) - BASE + 0xd800));
                    continue;
                }
                _ => {}
            }
        }
        out.push(ch);
    }
    out
}
