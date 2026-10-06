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
enum NumericHrefEntity {
    Literal,
    Scalar(u32),
}
fn numeric_href_entity(entity: &str) -> Option<NumericHrefEntity> {
    let digits = entity.strip_prefix("&#")?.strip_suffix(';')?;
    let (digits, radix) = digits
        .strip_prefix(['x', 'X'])
        .map_or((digits, 10), |hex| (hex, 16));
    if digits.is_empty()
        || !digits.bytes().all(|b| {
            if radix == 16 {
                b.is_ascii_hexdigit()
            } else {
                b.is_ascii_digit()
            }
        })
    {
        return None;
    }
    let value = u64::from_str_radix(digits, radix).ok();
    if digits.len() > 8 {
        // Markdown-it's bounded numeric grammar falls through to its HTML
        // entity decoder for longer digit strings. Overflow becomes U+FFFD.
        let value = value
            .filter(|n| *n != 0 && *n <= 0x10ffff && !(0xd800..=0xdfff).contains(n))
            .unwrap_or(0xfffd);
        return Some(NumericHrefEntity::Scalar(value as u32));
    }
    let n = value? as u32;
    let invalid = (0xd800..=0xdfff).contains(&n)
        || (0xfdd0..=0xfdef).contains(&n)
        || matches!(n & 0xffff, 0xffff | 0xfffe)
        || n <= 8
        || n == 11
        || (14..=31).contains(&n)
        || (127..=159).contains(&n)
        || n > 0x10ffff;
    Some(if invalid {
        NumericHrefEntity::Literal
    } else {
        NumericHrefEntity::Scalar(n)
    })
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
            if let Some(numeric) = numeric_href_entity(entity) {
                if let NumericHrefEntity::Scalar(n) = numeric {
                    let scalar = char::from_u32(n).unwrap_or(char::REPLACEMENT_CHARACTER);
                    let mut buffer = [0; 2];
                    units.extend_from_slice(scalar.encode_utf16(&mut buffer));
                    at += end + 1;
                    continue;
                }
            } else if entity[1..end]
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
    let mut value = String::new();
    // mdurl's protocol whitelist is case-sensitive. Punycode preserves basic
    // code points and permits lone surrogates, unlike WHATWG/UTS46 validation.
    let start = if units.starts_with(&[104, 116, 116, 112, 115, 58, 47, 47]) {
        Some(8)
    } else if units.starts_with(&[104, 116, 116, 112, 58, 47, 47]) {
        Some(7)
    } else {
        None
    };
    if let Some(start) = start {
        let end = units[start..]
            .iter()
            .position(|u| [47, 63, 35].contains(u))
            .map_or(units.len(), |n| start + n);
        let host_start = units[start..end]
            .iter()
            .rposition(|u| *u == 64)
            .map_or(start, |n| start + n + 1);
        // mdurl ends the hostname at these characters, then accepts only
        // labels whose ASCII spelling is basic hostname punctuation. An
        // invalid label moves its suffix back into the path before Punycode.
        let host_limit = units[host_start..end]
            .iter()
            .position(|u| u8::try_from(*u).is_ok_and(|b| b"%/?;#'{}|\\^`<>\" \r\n\t".contains(&b)))
            .map_or(end, |n| host_start + n);
        let mut host_end = host_limit;
        if let Some(colon) = units[host_start..host_limit].iter().rposition(|u| *u == 58)
            && units[host_start + colon + 1..host_limit]
                .iter()
                .all(|u| (48..=57).contains(u))
        {
            host_end = host_start + colon;
        }
        let mut label_start = host_start;
        for label in units[host_start..host_end].split(|u| *u == 46) {
            let basic = |u: &u16| {
                u8::try_from(*u).is_ok_and(|b| b.is_ascii_alphanumeric() || b"+_-".contains(&b))
            };
            if label.len() > 63 || label.iter().any(|u| *u < 128 && !basic(u)) {
                host_end = label_start + label.iter().take(63).take_while(|u| basic(u)).count();
                break;
            }
            label_start += label.len() + 1;
        }
        if units[host_start..host_end].iter().any(|u| *u > 127)
            && let Some(host) = ascii_host(units[host_start..host_end].iter().copied())
        {
            value.push_str(&String::from_utf16_lossy(&units[..host_start]));
            value.push_str(&host);
            value.push_str(&String::from_utf16_lossy(&units[host_end..]));
            return value;
        }
    }
    String::from_utf16_lossy(&units)
}
pub(super) fn unicode_destination(source: &str) -> Cow<'_, str> {
    let Some((scheme, rest)) = source.split_once("://") else {
        return Cow::Borrowed(source);
    };
    if !matches!(scheme, "http" | "https") {
        return Cow::Borrowed(source);
    }
    let authority = rest.split(['/', '?', '#']).next().unwrap_or(rest);
    let host = authority
        .rsplit('@')
        .next()
        .unwrap_or(authority)
        .split(':')
        .next()
        .unwrap_or(authority);
    if host.is_ascii() {
        return Cow::Borrowed(source);
    }
    let normalized = normalize_units(source.encode_utf16().collect());
    if normalized == source {
        Cow::Borrowed(source)
    } else {
        Cow::Owned(normalized)
    }
}
pub(super) fn transport_destination(source: &str) -> Cow<'_, str> {
    if !source.contains(MARK) {
        unicode_destination(source)
    } else {
        Cow::Owned(normalize_units(destination_units(source, false)))
    }
}
pub(super) fn parsed_destination(source: &str, kind: LinkType) -> Option<String> {
    if !source.contains(MARK) && !source.contains("&#") {
        return None;
    }
    Some(normalize_units(destination_units(
        source,
        !matches!(kind, LinkType::Autolink | LinkType::Email),
    )))
}
// RFC 3492 bootstring used by the original punycode.js. Checked arithmetic
// makes very large labels fail closed; no regex or global mutable cache.
pub(super) fn ascii_host(units: impl IntoIterator<Item = u16>) -> Option<String> {
    let points = char::decode_utf16(units)
        .map(|c| {
            c.map(u32::from)
                .unwrap_or_else(|e| u32::from(e.unpaired_surrogate()))
        })
        .collect::<Vec<_>>();
    let mut result = String::new();
    for (i, label) in points
        .split(|n| [46, 0x3002, 0xff0e, 0xff61].contains(n))
        .enumerate()
    {
        if i > 0 {
            result.push('.');
        }
        if label.iter().all(|n| *n < 128) {
            for &n in label {
                result.push(char::from_u32(n)?);
            }
        } else {
            result.push_str("xn--");
            result.push_str(&punycode(label)?);
        }
    }
    Some(result)
}
fn punycode(input: &[u32]) -> Option<String> {
    let mut out = String::new();
    for &cp in input.iter().filter(|&&cp| cp < 128) {
        out.push(char::from_u32(cp)?);
    }
    let basic = out.len() as u64;
    let mut handled = basic;
    if basic > 0 {
        out.push('-');
    }
    let mut n = 128u64;
    let mut delta = 0u64;
    let mut bias = 72u64;
    while handled < input.len() as u64 {
        let m = u64::from(*input.iter().filter(|&&cp| u64::from(cp) >= n).min()?);
        delta = delta.checked_add((m - n).checked_mul(handled + 1)?)?;
        n = m;
        for &cp in input {
            let cp = u64::from(cp);
            if cp < n {
                delta = delta.checked_add(1)?;
            }
            if cp == n {
                let mut q = delta;
                let mut k = 36;
                loop {
                    let threshold = if k <= bias { 1 } else { (k - bias).min(26) };
                    if q < threshold {
                        break;
                    }
                    out.push(digit(threshold + (q - threshold) % (36 - threshold))?);
                    q = (q - threshold) / (36 - threshold);
                    k += 36;
                }
                out.push(digit(q)?);
                let mut d = if handled == basic {
                    delta / 700
                } else {
                    delta / 2
                };
                d += d / (handled + 1);
                let mut k = 0;
                while d > 455 {
                    d /= 35;
                    k += 36;
                }
                bias = k + 36 * d / (d + 38);
                delta = 0;
                handled += 1;
            }
        }
        delta = delta.checked_add(1)?;
        n += 1;
    }
    Some(out)
}
fn digit(d: u64) -> Option<char> {
    let ascii = if d < 26 {
        b'a' + d as u8
    } else if d < 36 {
        b'0' + (d - 26) as u8
    } else {
        return None;
    };
    Some(char::from(ascii))
}

// Restore only real anchor attributes produced by this renderer. Literal raw
// HTML is escaped before sanitizing; quotes and angle brackets inside attribute
// values are escaped too. No replacement runs over text or decoded glyphs.
pub(super) fn restore_href_attributes(html: &str) -> Cow<'_, str> {
    if !html.contains("data-news-href=") {
        return Cow::Borrowed(html);
    }
    let mut result = String::with_capacity(html.len());
    let mut at = 0;
    while let Some(start) = html[at..].find("<a ").map(|n| at + n) {
        result.push_str(&html[at..start]);
        let mut end = start + 3;
        let mut quoted = false;
        while end < html.len() {
            match html.as_bytes()[end] {
                b'"' => quoted = !quoted,
                b'>' if !quoted => break,
                _ => {}
            }
            end += 1;
        }
        let tag = &html[start..end];
        result.push_str(&tag.replace(" data-news-href=\"", " href=\""));
        at = end;
    }
    result.push_str(&html[at..]);
    Cow::Owned(result)
}
