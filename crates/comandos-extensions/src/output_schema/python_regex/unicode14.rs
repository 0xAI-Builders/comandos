//! Private, unwired CPython 3.11.15 Unicode14 translation.
#![allow(dead_code)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct CodePoint(u32);
impl CodePoint {
    pub(crate) fn new(cp: u32) -> Option<Self> {
        (cp <= 0x10ffff).then_some(Self(cp))
    }
    pub(crate) fn value(self) -> u32 {
        self.0
    }
}
#[path = "unicode14_tables.rs"]
mod tables;
fn record(cp: CodePoint) -> &'static tables::TypeRecord {
    let n = cp.0 as usize;
    &tables::TYPE_RECORDS
        [tables::TYPE_INDEX2[((tables::TYPE_INDEX1[n >> 7] as usize) << 7) + (n & 127)] as usize]
}
fn mapped(cp: CodePoint, field: i32) -> CodePoint {
    let value = if record(cp).5 & 0x4000 != 0 {
        tables::EXTENDED[(field as usize) & 65535]
    } else {
        (cp.0 as i64 + field as i64) as u32
    };
    // Generator verifies every mapped result over the complete domain.
    CodePoint::new(value).expect("validated immutable Unicode14 mapping")
}
pub(crate) fn lower(cp: CodePoint) -> CodePoint {
    mapped(cp, record(cp).1)
}
pub(crate) fn upper(cp: CodePoint) -> CodePoint {
    mapped(cp, record(cp).0)
}
pub(crate) fn decimal(cp: CodePoint) -> Option<u8> {
    (record(cp).5 & 2 != 0).then(|| record(cp).3)
}
pub(crate) fn is_alnum(cp: CodePoint) -> bool {
    record(cp).5 & (1 | 2 | 4 | 0x800) != 0
}
pub(crate) fn is_word(cp: CodePoint) -> bool {
    cp.0 == 95 || is_alnum(cp)
}
pub(crate) fn is_space(cp: CodePoint) -> bool {
    matches!(cp.0,9..=13|28..=32|0x85|0xa0|0x1680|0x2000..=0x200a|0x2028|0x2029|0x202f|0x205f|0x3000)
}
pub(crate) fn is_linebreak(cp: CodePoint) -> bool {
    matches!(cp.0,10..=13|28..=30|0x85|0x2028|0x2029)
}
pub(crate) fn regex_iscased(cp: CodePoint) -> bool {
    cp != lower(cp) || cp != upper(cp)
}
pub(crate) fn xid_start(cp: CodePoint) -> bool {
    record(cp).5 & 0x100 != 0
}
pub(crate) fn xid_continue(cp: CodePoint) -> bool {
    record(cp).5 & 0x200 != 0
}
pub(crate) fn identifier_start(cp: CodePoint) -> bool {
    cp.0 == 95 || xid_start(cp)
}
pub(crate) fn identifier_continue(cp: CodePoint) -> bool {
    xid_continue(cp)
}
pub(crate) fn ascii_decimal(cp: CodePoint) -> Option<u8> {
    (48..=57).contains(&cp.0).then(|| (cp.0 - 48) as u8)
}
pub(crate) fn ascii_word(cp: CodePoint) -> bool {
    matches!(cp.0,48..=57|65..=90|97..=122|95)
}
pub(crate) fn ascii_space(cp: CodePoint) -> bool {
    matches!(cp.0, 9..=13 | 32)
}
pub(crate) fn extra_cases(cp: CodePoint) -> &'static [u32] {
    match tables::EXTRA_KEYS.binary_search(&cp.0) {
        Ok(i) => {
            &tables::EXTRA_VALUES
                [tables::EXTRA_OFFSETS[i] as usize..tables::EXTRA_OFFSETS[i + 1] as usize]
        }
        Err(_) => &[],
    }
}
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) enum NameFailure {
    Missing,
    UnicodeEncodeError,
    BudgetBoundary,
    InternalTable,
}
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) enum NameValue {
    Character(CodePoint),
    Sequence { index: u16, len: u8 },
}
const CJK: &[(u32, u32)] = &[
    (0x3400, 0x4dbf),
    (0x4e00, 0x9fff),
    (0x20000, 0x2a6df),
    (0x2a700, 0x2b738),
    (0x2b740, 0x2b81d),
    (0x2b820, 0x2cea1),
    (0x2ceb0, 0x2ebe0),
    (0x30000, 0x3134a),
];
const L: &[&[u8]] = &[
    b"G", b"GG", b"N", b"D", b"DD", b"R", b"M", b"B", b"BB", b"S", b"SS", b"", b"J", b"JJ", b"C",
    b"K", b"T", b"P", b"H",
];
const V: &[&[u8]] = &[
    b"A", b"AE", b"YA", b"YAE", b"EO", b"E", b"YEO", b"YE", b"O", b"WA", b"WAE", b"OE", b"YO",
    b"U", b"WEO", b"WE", b"WI", b"YU", b"EU", b"YI", b"I",
];
const T: &[&[u8]] = &[
    b"", b"G", b"GG", b"GS", b"N", b"NJ", b"NH", b"D", b"L", b"LG", b"LM", b"LB", b"LS", b"LT",
    b"LP", b"LH", b"M", b"B", b"BS", b"S", b"SS", b"NG", b"J", b"C", b"K", b"T", b"P", b"H",
];
fn cjk(cp: u32) -> bool {
    CJK.iter().any(|&(a, b)| (a..=b).contains(&cp))
}
fn synthetic(cp: u32) -> bool {
    (0xf0000..0xf01d6).contains(&cp) || (0xf0200..0xf03cd).contains(&cp)
}
fn put(
    buf: &mut [u8; 257],
    n: &mut usize,
    b: u8,
    charge: &mut impl FnMut(usize) -> Result<(), NameFailure>,
) -> Result<(), NameFailure> {
    charge(1)?;
    if *n >= 256 {
        return Err(NameFailure::InternalTable);
    }
    buf[*n] = b;
    *n += 1;
    Ok(())
}
fn decode(
    cp: CodePoint,
    with_synthetic: bool,
    buf: &mut [u8; 257],
    charge: &mut impl FnMut(usize) -> Result<(), NameFailure>,
) -> Result<Option<usize>, NameFailure> {
    if !with_synthetic && synthetic(cp.0) {
        return Ok(None);
    }
    let mut n = 0;
    if (0xac00..=0xd7a3).contains(&cp.0) {
        let index = (cp.0 - 0xac00) as usize;
        for part in [
            b"HANGUL SYLLABLE ".as_slice(),
            L[index / 588],
            V[(index % 588) / 28],
            T[index % 28],
        ] {
            for &b in part {
                put(buf, &mut n, b, charge)?;
            }
        }
    } else if cjk(cp.0) {
        for &b in b"CJK UNIFIED IDEOGRAPH-" {
            put(buf, &mut n, b, charge)?;
        }
        let digits = if cp.0 >= 0x10000 { 5 } else { 4 };
        for pos in (0..digits).rev() {
            let d = ((cp.0 >> (pos * 4)) & 15) as u8;
            put(
                buf,
                &mut n,
                if d < 10 { b'0' + d } else { b'A' + d - 10 },
                charge,
            )?;
        }
    } else {
        let cp = cp.0 as usize;
        let block = *tables::NAME_INDEX1
            .get(cp >> 7)
            .ok_or(NameFailure::InternalTable)? as usize;
        let mut off = *tables::NAME_INDEX2
            .get((block << 7) + (cp & 127))
            .ok_or(NameFailure::InternalTable)? as usize;
        if off == 0 {
            return Ok(None);
        }
        loop {
            charge(1)?;
            let first = *tables::PHRASEBOOK
                .get(off)
                .ok_or(NameFailure::InternalTable)? as usize;
            off += 1;
            let word = if first >= 190 {
                charge(1)?;
                let next = *tables::PHRASEBOOK
                    .get(off)
                    .ok_or(NameFailure::InternalTable)? as usize;
                off += 1;
                ((first - 190) << 8) + next
            } else {
                first
            };
            let mut w = *tables::LEXICON_OFFSET
                .get(word)
                .ok_or(NameFailure::InternalTable)? as usize;
            if n > 0 {
                put(buf, &mut n, b' ', charge)?;
            }
            loop {
                let b = *tables::LEXICON.get(w).ok_or(NameFailure::InternalTable)?;
                w += 1;
                if b == 128 {
                    charge(1)?;
                    buf[n] = 0;
                    return Ok(Some(n));
                }
                put(buf, &mut n, b & 127, charge)?;
                if b >= 128 {
                    break;
                }
            }
        }
    }
    buf[n] = 0;
    Ok(Some(n))
}
pub(crate) fn canonical_name(
    cp: CodePoint,
    buf: &mut [u8; 257],
) -> Result<Option<usize>, NameFailure> {
    decode(cp, false, buf, &mut |_| Ok(()))
}
pub(crate) fn synthetic_name(
    cp: CodePoint,
    buf: &mut [u8; 257],
) -> Result<Option<usize>, NameFailure> {
    if !synthetic(cp.0) {
        return Ok(None);
    }
    decode(cp, true, buf, &mut |_| Ok(()))
}
pub(crate) fn sequence(index: u16) -> Result<&'static [u16], NameFailure> {
    let len = *tables::SEQUENCE_LENGTHS
        .get(index as usize)
        .ok_or(NameFailure::InternalTable)? as usize;
    tables::SEQUENCE_DATA
        .get(index as usize)
        .and_then(|r| r.get(..len))
        .ok_or(NameFailure::InternalTable)
}
fn starts(
    name: &[CodePoint],
    prefix: &[u8],
    charge: &mut impl FnMut(usize) -> Result<(), NameFailure>,
) -> Result<bool, NameFailure> {
    for (i, b) in prefix.iter().enumerate() {
        charge(1)?;
        if name.get(i).map(|cp| cp.0) != Some(*b as u32) {
            return Ok(false);
        }
    }
    Ok(true)
}
fn syllable(
    name: &[CodePoint],
    parts: &[&[u8]],
    charge: &mut impl FnMut(usize) -> Result<(), NameFailure>,
) -> Result<Option<(usize, usize)>, NameFailure> {
    let mut best = None;
    for (index, part) in parts.iter().enumerate() {
        charge(part.len() + 1)?;
        if best.is_some_and(|(_, n)| n >= part.len()) {
            continue;
        }
        if starts(name, part, charge)? {
            best = Some((index, part.len()));
        }
    }
    Ok(best)
}
pub(crate) fn lookup_name(
    name: &[CodePoint],
    charge: &mut impl FnMut(usize) -> Result<(), NameFailure>,
) -> Result<NameValue, NameFailure> {
    let mut bytes = 0usize;
    let mut ascii = true;
    // Scan before semantic length rejection to preserve UTF-8 conversion errors.
    for cp in name {
        charge(1)?;
        if (0xd800..=0xdfff).contains(&cp.0) {
            return Err(NameFailure::UnicodeEncodeError);
        }
        let width = match cp.0 {
            0..=0x7f => 1,
            0x80..=0x7ff => 2,
            0x800..=0xffff => 3,
            _ => 4,
        };
        charge(width)?;
        if cp.0 > 127 {
            ascii = false;
        }
        bytes = bytes
            .checked_add(width)
            .ok_or(NameFailure::BudgetBoundary)?;
    }
    if bytes > 256 || !ascii {
        return Err(NameFailure::Missing);
    }
    if starts(name, b"HANGUL SYLLABLE ", charge)? {
        let mut rest = &name[16..];
        let mut values = [0; 3];
        for (i, parts) in [L, V, T].iter().enumerate() {
            let Some((index, len)) = syllable(rest, parts, charge)? else {
                return Err(NameFailure::Missing);
            };
            values[i] = index;
            rest = &rest[len..];
        }
        if !rest.is_empty() {
            return Err(NameFailure::Missing);
        }
        return Ok(NameValue::Character(
            CodePoint::new(0xac00 + ((values[0] * 21 + values[1]) * 28 + values[2]) as u32)
                .ok_or(NameFailure::InternalTable)?,
        ));
    }
    if starts(name, b"CJK UNIFIED IDEOGRAPH-", charge)? {
        let digits = &name[22..];
        if digits.len() != 4 && digits.len() != 5 {
            return Err(NameFailure::Missing);
        }
        let mut cp = 0;
        for &d in digits {
            charge(1)?;
            cp = cp * 16
                + match d.0 as u8 {
                    b'0'..=b'9' => d.0 - b'0' as u32,
                    b'A'..=b'F' => d.0 - b'A' as u32 + 10,
                    _ => return Err(NameFailure::Missing),
                };
        }
        if !cjk(cp) {
            return Err(NameFailure::Missing);
        }
        return Ok(NameValue::Character(
            CodePoint::new(cp).ok_or(NameFailure::InternalTable)?,
        ));
    }
    let mut h = 0u32;
    for b in name {
        charge(1)?;
        h = h * 47 + (b.0 as u8).to_ascii_uppercase() as u32;
        let ix = h & 0xff000000;
        if ix != 0 {
            h = (h ^ ((ix >> 24) & 255)) & 0xffffff;
        }
    }
    let mut slot = (!h) & 65535;
    let mut inc = (h ^ (h >> 3)) & 65535;
    if inc == 0 {
        inc = 65535;
    }
    let mut buf = [0u8; 257];
    for _ in 0..65536 {
        charge(1)?;
        let candidate = *tables::CODE_HASH
            .get(slot as usize)
            .ok_or(NameFailure::InternalTable)?;
        if candidate == 0 {
            return Err(NameFailure::Missing);
        }
        let point = CodePoint::new(candidate).ok_or(NameFailure::InternalTable)?;
        let n = decode(point, true, &mut buf, charge)?.ok_or(NameFailure::InternalTable)?;
        let mut equal = n == name.len();
        if equal {
            for (a, b) in name.iter().zip(&buf[..n]) {
                charge(1)?;
                if (a.0 as u8).to_ascii_uppercase() != *b {
                    equal = false;
                    break;
                }
            }
        }
        if equal {
            if (0xf0000..0xf01d6).contains(&candidate) {
                let target = *tables::ALIASES
                    .get((candidate - 0xf0000) as usize)
                    .ok_or(NameFailure::InternalTable)?;
                return Ok(NameValue::Character(
                    CodePoint::new(target).ok_or(NameFailure::InternalTable)?,
                ));
            }
            if (0xf0200..0xf03cd).contains(&candidate) {
                let index = (candidate - 0xf0200) as u16;
                let len = *tables::SEQUENCE_LENGTHS
                    .get(index as usize)
                    .ok_or(NameFailure::InternalTable)?;
                return Ok(NameValue::Sequence { index, len });
            }
            return Ok(NameValue::Character(point));
        }
        slot = (slot + inc) & 65535;
        inc <<= 1;
        if inc > 65535 {
            inc ^= 65581;
        }
    }
    Err(NameFailure::InternalTable)
}
pub(crate) fn payload_bytes() -> usize {
    use std::mem::size_of_val;
    size_of_val(&tables::TYPE_RECORDS)
        + size_of_val(&tables::EXTENDED)
        + size_of_val(&tables::TYPE_INDEX1)
        + size_of_val(&tables::TYPE_INDEX2)
        + size_of_val(&tables::LEXICON)
        + size_of_val(&tables::LEXICON_OFFSET)
        + size_of_val(&tables::PHRASEBOOK)
        + size_of_val(&tables::NAME_INDEX1)
        + size_of_val(&tables::NAME_INDEX2)
        + size_of_val(&tables::CODE_HASH)
        + size_of_val(&tables::ALIASES)
        + size_of_val(&tables::SEQUENCE_LENGTHS)
        + size_of_val(&tables::SEQUENCE_DATA)
        + size_of_val(&tables::EXTRA_KEYS)
        + size_of_val(&tables::EXTRA_OFFSETS)
        + size_of_val(&tables::EXTRA_VALUES)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn installed_lower_records() {
        let data = std::fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/unicode14/primitive_records.bin"
        ))
        .unwrap();
        for cp in [65u32, 0x130, 0x212a, 0x3a3, 0xd800] {
            let r = &data[cp as usize * 16..][..16];
            assert_eq!(
                lower(CodePoint::new(cp).unwrap()).value(),
                u32::from_le_bytes(r[4..8].try_into().unwrap()),
                "U+{cp:04X}"
            );
        }
    }
    fn name(s: &str) -> Vec<CodePoint> {
        s.chars()
            .map(|c| CodePoint::new(c as u32).unwrap())
            .collect()
    }
    #[test]
    fn names_and_encoding_precedence() {
        let mut charge = |_: usize| Ok(());
        assert_eq!(
            lookup_name(&name("LATIN CAPITAL LETTER A"), &mut charge),
            Ok(NameValue::Character(CodePoint::new(65).unwrap()))
        );
        assert_eq!(
            lookup_name(&[CodePoint::new(0xd800).unwrap()], &mut charge),
            Err(NameFailure::UnicodeEncodeError)
        );
    }
    #[test]
    fn caller_budget_is_propagated() {
        let mut charge = |_: usize| Err(NameFailure::BudgetBoundary);
        assert_eq!(
            lookup_name(&name("LATIN CAPITAL LETTER A"), &mut charge),
            Err(NameFailure::BudgetBoundary)
        );
    }
    #[test]
    fn algorithmic_case_and_names() {
        let mut charge = |_: usize| Ok(());
        for (s, cp) in [
            ("HANGUL SYLLABLE GA", 0xac00),
            ("CJK UNIFIED IDEOGRAPH-04E00", 0x4e00),
            ("latin capital letter a", 65),
            ("LF", 10),
        ] {
            assert_eq!(
                lookup_name(&name(s), &mut charge),
                Ok(NameValue::Character(CodePoint::new(cp).unwrap())),
                "{s}"
            );
        }
        for s in [
            "hangul syllable ga",
            "CJK UNIFIED IDEOGRAPH-4e00",
            "cjk unified ideograph-4E00",
            "TANGUT IDEOGRAPH-17000",
            " HANGUL SYLLABLE GA",
            "HANGUL SYLLABLE GA ",
        ] {
            assert_eq!(
                lookup_name(&name(s), &mut charge),
                Err(NameFailure::Missing),
                "{s}"
            );
        }
    }
    #[test]
    fn long_input_surrogate_and_aggregate_budget() {
        let mut v = vec![CodePoint::new(65).unwrap(); 257];
        v.push(CodePoint::new(0xd800).unwrap());
        assert_eq!(
            lookup_name(&v, &mut |_| Ok(())),
            Err(NameFailure::UnicodeEncodeError)
        );
        let mut work = 0;
        let mut charge = |n| {
            work += n;
            if work > 10 {
                Err(NameFailure::BudgetBoundary)
            } else {
                Ok(())
            }
        };
        assert_eq!(
            lookup_name(&v, &mut charge),
            Err(NameFailure::BudgetBoundary)
        );
        assert_eq!(
            lookup_name(&name("LF"), &mut charge),
            Err(NameFailure::BudgetBoundary)
        );
    }
    #[test]
    fn codepoint_and_ascii_contract() {
        assert!(CodePoint::new(0xd800).is_some());
        assert!(CodePoint::new(0x110000).is_none());
        let c = CodePoint::new(0x1c).unwrap();
        assert!(is_space(c));
        assert!(!ascii_space(c));
        let c = CodePoint::new(0x0661).unwrap();
        assert_eq!(decimal(c), Some(1));
        assert_eq!(ascii_decimal(c), None);
        assert!(is_word(c));
        assert!(!ascii_word(c));
        assert_eq!(payload_bytes(), 944473);
        assert_eq!(std::mem::size_of::<tables::TypeRecord>(), 16);
    }
}
