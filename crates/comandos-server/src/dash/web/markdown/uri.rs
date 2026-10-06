//! URI normalization at the source boundary. Codec escapes are recognized only
//! in raw source; entity results are ordinary UTF-16 units, with no codec scan.
use pulldown_cmark::{Event, LinkType, Parser};
use std::{
    borrow::Cow,
    collections::{HashMap, VecDeque},
};

const MARK: char = '\u{e000}';

// The caller has already recognized this link with pulldown. Find its label's
// closing bracket, respecting escapes, nesting and code spans, then its raw URL.
fn after_label(source: &str, code_spans: bool) -> Option<&str> {
    let source = source.strip_prefix('!').unwrap_or(source);
    let bytes = source.as_bytes();
    if bytes.first() != Some(&b'[') {
        return None;
    }
    let mut delimiters: HashMap<usize, VecDeque<usize>> = HashMap::new();
    if code_spans {
        let mut at = 1;
        while at < bytes.len() {
            if bytes[at] == b'`' {
                let width = bytes[at..].iter().take_while(|&&b| b == b'`').count();
                delimiters.entry(width).or_default().push_back(at);
                at += width;
            } else {
                at += 1;
            }
        }
    }
    let mut at = 1;
    let mut depth = 1;
    while at < bytes.len() {
        match bytes[at] {
            b'\\' => at += 2,
            b'`' if code_spans => {
                let width = bytes[at..].iter().take_while(|&&b| b == b'`').count();
                let close = delimiters.get_mut(&width).and_then(|positions| {
                    while positions.front().is_some_and(|position| *position <= at) {
                        positions.pop_front();
                    }
                    positions.front().copied()
                });
                at = close.map_or(at + width, |close| close + width);
            }
            b'[' => {
                depth += 1;
                at += 1;
            }
            b']' => {
                depth -= 1;
                at += 1;
                if depth == 0 {
                    return source.get(at..);
                }
            }
            _ => at += 1,
        }
    }
    None
}
fn destination(source: &str) -> Option<&str> {
    let source = source.trim_start_matches(|c: char| c.is_ascii_whitespace());
    let bytes = source.as_bytes();
    let angle = bytes.first() == Some(&b'<');
    let start = usize::from(angle);
    let mut at = start;
    let mut depth = 0;
    while at < bytes.len() {
        match bytes[at] {
            b'\\' if bytes.get(at + 1).is_some_and(u8::is_ascii_punctuation) => at += 2,
            b'>' if angle => return source.get(start..at),
            b'(' if !angle => {
                depth += 1;
                at += 1;
            }
            b')' if !angle => {
                if depth == 0 {
                    break;
                }
                depth -= 1;
                at += 1;
            }
            0..=32 if !angle => break,
            _ => at += 1,
        }
    }
    if angle { None } else { source.get(start..at) }
}
pub(super) fn inline_destination(source: &str) -> Option<&str> {
    destination(after_label(source, true)?.strip_prefix('(')?)
}
pub(super) fn reference_destination(source: &str) -> Option<&str> {
    destination(after_label(source, false)?.strip_prefix(':')?)
}
fn invalid_numeric(entity: &str) -> bool {
    let Some(digits) = entity.strip_prefix("&#").and_then(|s| s.strip_suffix(';')) else {
        return false;
    };
    let value = if let Some(hex) = digits.strip_prefix(['x', 'X']) {
        u32::from_str_radix(hex, 16)
    } else {
        digits.parse()
    };
    value.is_ok_and(|n| (0xd800..=0xdfff).contains(&n) || n > 0x10ffff)
}
pub(super) fn entity_text(source: &str, decoded: &str) -> String {
    if invalid_numeric(source) {
        return source.into();
    }
    // Only a known entity source span receives this escape. Raw codec units
    // remain intact and are never scanned together with entity output.
    if decoded.contains(MARK) {
        comandos_web_view::utf16::encode(decoded.encode_utf16())
    } else {
        decoded.into()
    }
}
pub(super) fn source_title(source: &str, kind: LinkType) -> Option<String> {
    let raw = if kind == LinkType::Inline {
        inline_destination(source)?
    } else {
        reference_destination(source)?
    };
    let end = (raw.as_ptr() as usize).checked_sub(source.as_ptr() as usize)? + raw.len();
    let rest = source.get(end..)?;
    let rest = rest.strip_prefix('>').unwrap_or(rest).trim();
    let rest = if kind == LinkType::Inline {
        rest.strip_suffix(')')?.trim_end()
    } else {
        rest
    };
    let open = rest.chars().next()?;
    let close = match open {
        '\'' | '"' => open,
        '(' => ')',
        _ => return None,
    };
    let title = rest.strip_prefix(open)?.strip_suffix(close)?;
    Some(comandos_web_view::utf16::encode(destination_units(
        title, true,
    )))
}
// Decode escapes and entities independently, preserving their origin. In
// particular MARK emitted by an entity never meets the transport decoder.
fn destination_units(source: &str, entities: bool) -> Vec<u16> {
    let mut units = Vec::new();
    let mut at = 0;
    while at < source.len() {
        let tail = &source[at..];
        let ch = tail.chars().next().unwrap_or_default();
        if entities
            && ch == '\\'
            && let Some(next) = tail[ch.len_utf8()..]
                .chars()
                .next()
                .filter(char::is_ascii_punctuation)
        {
            units.push(next as u16);
            at += 2;
            continue;
        }
        if entities
            && ch == '&'
            && let Some(end) = tail
                .as_bytes()
                .iter()
                .take(33)
                .position(|&byte| byte == b';')
        {
            let entity = &tail[..end + 1];
            if !invalid_numeric(entity)
                && entity[1..end]
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'#')
            {
                let mut decoded = String::new();
                for event in Parser::new(entity) {
                    if let Event::Text(value) = event {
                        decoded.push_str(&value);
                    }
                }
                if decoded != entity {
                    units.extend(decoded.encode_utf16());
                    at += end + 1;
                    continue;
                }
            }
        }
        if ch == MARK {
            let next = tail[ch.len_utf8()..].chars().next();
            if next == Some(MARK) {
                units.push(MARK as u16);
                at += ch.len_utf8() * 2;
                continue;
            }
            if let Some(next) = next.filter(|c| (0xf0000..0xf0800).contains(&u32::from(*c))) {
                units.push((u32::from(next) - 0xf0000 + 0xd800) as u16);
                at += ch.len_utf8() + next.len_utf8();
                continue;
            }
        }
        let mut buffer = [0; 2];
        units.extend_from_slice(ch.encode_utf16(&mut buffer));
        at += ch.len_utf8();
    }
    units
}
fn normalize_units(units: Vec<u16>) -> String {
    String::from_utf16_lossy(&units)
}
pub(super) fn transport_destination(source: &str) -> Cow<'_, str> {
    if source.is_ascii() {
        Cow::Borrowed(source)
    } else {
        Cow::Owned(normalize_units(destination_units(source, false)))
    }
}
pub(super) fn parsed_destination(source: &str, kind: LinkType) -> Option<String> {
    if !source.contains(MARK)
        && !source.split_inclusive(';').any(|part| {
            part.rfind("&#")
                .is_some_and(|at| invalid_numeric(&part[at..]))
        })
    {
        return None;
    }
    Some(normalize_units(destination_units(
        source,
        !matches!(kind, LinkType::Autolink | LinkType::Email),
    )))
}
