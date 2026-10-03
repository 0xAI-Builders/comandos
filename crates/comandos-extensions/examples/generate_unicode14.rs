//! Offline deterministic development converter. Pinned Python files are inert.
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Write as _,
    fs,
    path::{Path, PathBuf},
};
const SOURCES: &[(&str, &str, &str)] = &[
    (
        "cpython/Objects/unicodectype.c",
        "https://raw.githubusercontent.com/python/cpython/2340a037f7450e70fccfe411e6531afb4d57a312/Objects/unicodectype.c",
        "4b2f396d99072fc3d19fc2e812067b188c377433138416d5a74cd1a14a413353",
    ),
    (
        "cpython/Objects/unicodetype_db.h",
        "https://raw.githubusercontent.com/python/cpython/2340a037f7450e70fccfe411e6531afb4d57a312/Objects/unicodetype_db.h",
        "a908fb7364d321984cd1cbdd8737f115b61e34499899f7597df3ee606109de1e",
    ),
    (
        "cpython/Include/cpython/unicodeobject.h",
        "https://raw.githubusercontent.com/python/cpython/2340a037f7450e70fccfe411e6531afb4d57a312/Include/cpython/unicodeobject.h",
        "9183d40b4aec778c4f59d2091b4374722f635116a33ae6fc66c16e0b5fa700b5",
    ),
    (
        "cpython/Objects/unicodeobject.c",
        "https://raw.githubusercontent.com/python/cpython/2340a037f7450e70fccfe411e6531afb4d57a312/Objects/unicodeobject.c",
        "5dde8eb1b99878b4ea113cd6b5f88bca3667b0e23dc8c5262a07f5ea14d0d82e",
    ),
    (
        "cpython/Modules/_sre/sre.c",
        "https://raw.githubusercontent.com/python/cpython/2340a037f7450e70fccfe411e6531afb4d57a312/Modules/_sre/sre.c",
        "db0b655aad48299782cfa460d6bac5d0093015b675ab193c0526fbc8ef5d2b78",
    ),
    (
        "cpython/Lib/re/_casefix.py",
        "https://raw.githubusercontent.com/python/cpython/2340a037f7450e70fccfe411e6531afb4d57a312/Lib/re/_casefix.py",
        "41572ac50cf96b04496e676d8a6708898bb8e752e06dad34ed4c50c5d8f1fe40",
    ),
    (
        "cpython/Lib/re/_compiler.py",
        "https://raw.githubusercontent.com/python/cpython/2340a037f7450e70fccfe411e6531afb4d57a312/Lib/re/_compiler.py",
        "c05067f8bfa4c13cbbf1eedc4d5cafc9b621bcb6ebc5771ba0518a18095af15a",
    ),
    (
        "cpython/Modules/unicodedata.c",
        "https://raw.githubusercontent.com/python/cpython/2340a037f7450e70fccfe411e6531afb4d57a312/Modules/unicodedata.c",
        "45cdebdddfd074ea4a3ec1f99d59012953a35adb3b1235e9cfd38e346d511837",
    ),
    (
        "cpython/Modules/unicodename_db.h",
        "https://raw.githubusercontent.com/python/cpython/2340a037f7450e70fccfe411e6531afb4d57a312/Modules/unicodename_db.h",
        "fc3a439c81f2aa3eed4fbfc1cdb0f7f821bf4aed3fea1120460d9e05bed6a771",
    ),
    (
        "cpython/Modules/clinic/unicodedata.c.h",
        "https://raw.githubusercontent.com/python/cpython/2340a037f7450e70fccfe411e6531afb4d57a312/Modules/clinic/unicodedata.c.h",
        "9de06dcfdf2128586bf94a549f94d3495a42c7047f56cb03241d6db985abf0ae",
    ),
    (
        "cpython/Python/getargs.c",
        "https://raw.githubusercontent.com/python/cpython/2340a037f7450e70fccfe411e6531afb4d57a312/Python/getargs.c",
        "9bf9d7ef93e2f2aea0b5b863503b06ca11cbd8d647c1052cf77f8114f30f5956",
    ),
    (
        "cpython/Lib/re/_parser.py",
        "https://raw.githubusercontent.com/python/cpython/2340a037f7450e70fccfe411e6531afb4d57a312/Lib/re/_parser.py",
        "4748e39c77d6dc14f81af80e68a62ad99031a8182d5e0b219a6666d0cfb1626f",
    ),
    (
        "cpython/Modules/unicodedata_db.h",
        "https://raw.githubusercontent.com/python/cpython/2340a037f7450e70fccfe411e6531afb4d57a312/Modules/unicodedata_db.h",
        "93e7515143be306af19275a087a68da230561e0d61472fe64dbf363ca9939066",
    ),
    (
        "cpython/Tools/unicode/makeunicodedata.py",
        "https://raw.githubusercontent.com/python/cpython/2340a037f7450e70fccfe411e6531afb4d57a312/Tools/unicode/makeunicodedata.py",
        "4167934e4a2fd3d8208005bd590a679bd7309f6915df77ef0c9631d3c1b9187d",
    ),
    (
        "cpython/LICENSE",
        "https://raw.githubusercontent.com/python/cpython/2340a037f7450e70fccfe411e6531afb4d57a312/LICENSE",
        "3b2f81fe21d181c499c59a256c8e1968455d6689d269aa85373bfb6af41da3bf",
    ),
    (
        "unicode/ReadMe.txt",
        "https://www.unicode.org/Public/14.0.0/ucd/ReadMe.txt",
        "7316825946e47cdfb77f6c9037c611d74d58b2e55761ea0d6dfa162fa07a926a",
    ),
    (
        "unicode/license.txt",
        "https://www.unicode.org/license.txt",
        "e7a93b009565cfce55919a381437ac4db883e9da2126fa28b91d12732bc53d96",
    ),
    (
        "cpython/Tools/scripts/generate_re_casefix.py",
        "https://raw.githubusercontent.com/python/cpython/2340a037f7450e70fccfe411e6531afb4d57a312/Tools/scripts/generate_re_casefix.py",
        "21f3f9365223887913c2d924f74bce7a2c2a3e08ae4a49efde994532e7c78a9f",
    ),
];
const MAX_INPUT: usize = 4 * 1024 * 1024;
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
fn digest(data: &[u8]) -> String {
    format!("{:x}", Sha256::digest(data))
}
fn verify(data: &[u8], hash: &str, max: usize) -> Result<(), String> {
    if data.len() > max || digest(data) != hash {
        return Err("source size/hash mismatch".into());
    }
    Ok(())
}
// A lexer strips C comments without touching quoted text or evaluating source.
fn uncomment(s: &str, python: bool) -> Result<String, String> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    let mut quote = None;
    while i < b.len() {
        if let Some(q) = quote {
            out.push(b[i]);
            if b[i] == b'\\' {
                i += 1;
                if i == b.len() {
                    return Err("truncated escape".into());
                }
                out.push(b[i]);
            } else if b[i] == q {
                quote = None;
            }
            i += 1;
            continue;
        }
        if b[i] == b'"' || b[i] == b'\'' {
            quote = Some(b[i]);
            out.push(b[i]);
            i += 1;
            continue;
        }
        if python && b[i] == b'#' || !python && b.get(i..i + 2) == Some(b"//") {
            while i < b.len() && b[i] != b'\n' {
                i += 1;
            }
            out.push(b' ');
            continue;
        }
        if !python && b.get(i..i + 2) == Some(b"/*") {
            i += 2;
            loop {
                if i + 1 >= b.len() {
                    return Err("unterminated comment".into());
                }
                if b.get(i..i + 2) == Some(b"*/") {
                    i += 2;
                    break;
                }
                if b[i] == b'\n' {
                    out.push(b'\n');
                }
                i += 1;
            }
            out.push(b' ');
            continue;
        }
        out.push(b[i]);
        i += 1;
    }
    if quote.is_some() {
        return Err("unterminated quote".into());
    }
    String::from_utf8(out).map_err(|e| e.to_string())
}
#[derive(Debug, Clone, PartialEq, Eq)]
enum Node {
    N(i64),
    L(Vec<Node>),
}
struct Parser<'a> {
    s: &'a [u8],
    p: usize,
}
impl<'a> Parser<'a> {
    fn new(s: &'a str) -> Self {
        Self {
            s: s.as_bytes(),
            p: 0,
        }
    }
    fn ws(&mut self) {
        while self.s.get(self.p).is_some_and(u8::is_ascii_whitespace) {
            self.p += 1;
        }
    }
    fn eat(&mut self, c: u8) -> Result<(), String> {
        self.ws();
        if self.s.get(self.p) != Some(&c) {
            return Err(format!("expected {} at {}", c as char, self.p));
        }
        self.p += 1;
        Ok(())
    }
    fn number(&mut self) -> Result<i64, String> {
        self.ws();
        let a = self.p;
        if self.s.get(self.p) == Some(&b'-') {
            self.p += 1;
        }
        while self
            .s
            .get(self.p)
            .is_some_and(|c| c.is_ascii_alphanumeric())
        {
            self.p += 1;
        }
        let text = std::str::from_utf8(&self.s[a..self.p]).map_err(|e| e.to_string())?;
        let (sign, text) = text.strip_prefix('-').map_or((1, text), |t| (-1, t));
        let n = if let Some(t) = text.strip_prefix("0x") {
            i64::from_str_radix(t, 16)
        } else {
            text.parse::<i64>()
        }
        .map_err(|_| "malformed/overflow integer".to_string())?;
        n.checked_mul(sign).ok_or("signed overflow".into())
    }
    fn list(&mut self, open: u8, close: u8) -> Result<Vec<Node>, String> {
        self.eat(open)?;
        let mut v = Vec::new();
        loop {
            self.ws();
            if self.s.get(self.p) == Some(&close) {
                self.p += 1;
                return Ok(v);
            }
            if self.s.get(self.p) == Some(&open) {
                v.push(Node::L(self.list(open, close)?));
            } else {
                v.push(Node::N(self.number()?));
            }
            self.ws();
            match self.s.get(self.p) {
                Some(b',') => self.p += 1,
                Some(c) if *c == close => {}
                _ => return Err("missing comma/terminator".into()),
            }
        }
    }
    fn end(&mut self) -> Result<(), String> {
        self.ws();
        if self.p == self.s.len() {
            Ok(())
        } else {
            Err("trailing tokens".into())
        }
    }
}
fn symbol<'a>(s: &'a str, name: &str) -> Result<&'a str, String> {
    let positions: Vec<_> = s
        .match_indices(name)
        .filter(|(p, _)| {
            let before = s.as_bytes().get(p.wrapping_sub(1));
            let after = s.as_bytes().get(p + name.len());
            !before.is_some_and(|c| c.is_ascii_alphanumeric() || *c == b'_')
                && !after.is_some_and(|c| c.is_ascii_alphanumeric() || *c == b'_')
        })
        .collect();
    if positions.len() != 1 {
        return Err(format!("missing/duplicate symbol {name}"));
    }
    Ok(&s[positions[0].0 + name.len()..])
}
fn array(s: &str, name: &str) -> Result<Vec<Node>, String> {
    let mut p = Parser::new(symbol(s, name)?);
    p.eat(b'[')?;
    p.eat(b']')?;
    p.eat(b'=')?;
    let v = p.list(b'{', b'}')?;
    p.eat(b';')?;
    Ok(v)
}
fn scalar(s: &str, name: &str) -> Result<i64, String> {
    let mut p = Parser::new(symbol(s, name)?);
    p.eat(b'=')?;
    let v = p.number()?;
    p.eat(b';')?;
    Ok(v)
}
fn define(s: &str, name: &str) -> Result<i64, String> {
    let lines: Vec<_> = s
        .lines()
        .filter_map(|l| l.trim().strip_prefix("#define "))
        .filter(|l| l.split_whitespace().next() == Some(name))
        .collect();
    if lines.len() != 1 {
        return Err(format!("missing/duplicate define {name}"));
    }
    let mut p = Parser::new(lines[0].strip_prefix(name).unwrap());
    let v = p.number()?;
    p.end()?;
    Ok(v)
}
fn flat(v: Vec<Node>) -> Result<Vec<i64>, String> {
    v.into_iter()
        .map(|n| match n {
            Node::N(x) => Ok(x),
            _ => Err("unexpected nested list".into()),
        })
        .collect()
}
fn rows(v: Vec<Node>) -> Result<Vec<Vec<i64>>, String> {
    v.into_iter()
        .map(|n| match n {
            Node::L(x) => flat(x),
            _ => Err("expected record".into()),
        })
        .collect()
}
fn cases(s: &str) -> Result<BTreeMap<i64, Vec<i64>>, String> {
    let s = uncomment(s, true)?;
    let prefix = "_EXTRA_CASES";
    if !s.trim_start().starts_with(prefix) {
        return Err("extra dictionary declaration".into());
    }
    let mut p = Parser::new(symbol(&s, prefix)?);
    p.eat(b'=')?;
    p.eat(b'{')?;
    let mut m = BTreeMap::new();
    loop {
        p.ws();
        if p.s.get(p.p) == Some(&b'}') {
            p.p += 1;
            break;
        }
        let key = p.number()?;
        p.eat(b':')?;
        let value = flat(p.list(b'(', b')')?)?;
        if value.is_empty() || m.insert(key, value).is_some() {
            return Err("empty/duplicate extra cases".into());
        }
        p.eat(b',')?;
    }
    p.end()?;
    Ok(m)
}
#[derive(Clone)]
struct Tables {
    arrays: BTreeMap<String, Vec<i64>>,
    records: Vec<Vec<i64>>,
    sequences: Vec<Vec<i64>>,
    extra: BTreeMap<i64, Vec<i64>>,
}
impl Tables {
    fn parse(types: &str, names: &str, extra: &str) -> Result<Self, String> {
        let t = uncomment(types, false)?;
        let n = uncomment(names, false)?;
        for (s, name, want) in [
            (&t, "SHIFT", 7),
            (&n, "NAME_MAXLEN", 256),
            (&n, "phrasebook_shift", 7),
            (&n, "phrasebook_short", 190),
            (&n, "code_magic", 47),
            (&n, "code_size", 65536),
            (&n, "code_poly", 65581),
        ] {
            if define(s, name)? != want {
                return Err(format!("wrong {name}"));
            }
        }
        for (name, want) in [
            ("aliases_start", 0xf0000),
            ("aliases_end", 0xf01d6),
            ("named_sequences_start", 0xf0200),
            ("named_sequences_end", 0xf03cd),
        ] {
            if scalar(&n, name)? != want {
                return Err(format!("wrong {name}"));
            }
        }
        let records = rows(array(&t, "_PyUnicode_TypeRecords")?)?;
        let mut arrays = BTreeMap::new();
        for (s, original, generated) in [
            (&t, "_PyUnicode_ExtendedCase", "EXTENDED"),
            (&t, "index1", "TYPE_INDEX1"),
            (&t, "index2", "TYPE_INDEX2"),
            (&n, "lexicon", "LEXICON"),
            (&n, "lexicon_offset", "LEXICON_OFFSET"),
            (&n, "phrasebook", "PHRASEBOOK"),
            (&n, "phrasebook_offset1", "NAME_INDEX1"),
            (&n, "phrasebook_offset2", "NAME_INDEX2"),
            (&n, "code_hash", "CODE_HASH"),
            (&n, "name_aliases", "ALIASES"),
        ] {
            arrays.insert(generated.into(), flat(array(s, original)?)?);
        }
        let sequences = array(&n, "named_sequences")?
            .into_iter()
            .map(|r| {
                let Node::L(row) = r else {
                    return Err("sequence record".into());
                };
                if row.len() != 2 {
                    return Err("sequence fields".into());
                }
                let Node::N(len) = row[0] else {
                    return Err("sequence length".into());
                };
                let Node::L(data) = &row[1] else {
                    return Err("sequence values".into());
                };
                let data = flat(data.clone())?;
                if !(2..=4).contains(&len) || data.len() != len as usize {
                    return Err("sequence width".into());
                }
                let mut v = vec![len];
                v.extend(data);
                while v.len() < 5 {
                    v.push(0);
                }
                Ok(v)
            })
            .collect::<Result<Vec<_>, String>>()?;
        let tables = Self {
            arrays,
            records,
            sequences,
            extra: cases(extra)?,
        };
        tables.validate()?;
        Ok(tables)
    }
    fn a(&self, n: &str) -> &[i64] {
        &self.arrays[n]
    }
    fn decode(&self, cp: usize) -> Result<Option<Vec<u8>>, String> {
        let off1 = *self.a("NAME_INDEX1").get(cp >> 7).ok_or("name index1")? as usize;
        let mut off = *self
            .a("NAME_INDEX2")
            .get((off1 << 7) + (cp & 127))
            .ok_or("name index2")? as usize;
        if off == 0 {
            return Ok(None);
        }
        let mut out = Vec::new();
        loop {
            let b = *self.a("PHRASEBOOK").get(off).ok_or("phrase offset")? as usize;
            off += 1;
            let word = if b >= 190 {
                let b2 = *self.a("PHRASEBOOK").get(off).ok_or("phrase second byte")? as usize;
                off += 1;
                ((b - 190) << 8) + b2
            } else {
                b
            };
            let mut w = *self.a("LEXICON_OFFSET").get(word).ok_or("lexicon word")? as usize;
            if !out.is_empty() {
                out.push(b' ');
            }
            loop {
                let b = *self
                    .a("LEXICON")
                    .get(w)
                    .ok_or("unterminated lexicon phrase")? as u8;
                w += 1;
                if out.len() >= 256 {
                    return Err("overlong decoded name".into());
                }
                out.push(b & 127);
                if b == 128 {
                    out.pop();
                    return Ok(Some(out));
                }
                if b >= 128 {
                    break;
                }
            }
        }
    }
    fn validate(&self) -> Result<(), String> {
        for (name, len, max) in [
            ("EXTENDED", 1236, 0x10ffff),
            ("TYPE_INDEX1", 8704, 65535),
            ("TYPE_INDEX2", 35840, 65535),
            ("LEXICON", 124555, 255),
            ("LEXICON_OFFSET", 16803, 0xffffffff),
            ("PHRASEBOOK", 194503, 255),
            ("NAME_INDEX1", 8704, 65535),
            ("NAME_INDEX2", 42496, 0xffffffff),
            ("CODE_HASH", 65536, 0x10ffff),
            ("ALIASES", 470, 0x10ffff),
        ] {
            let a = self.a(name);
            if a.len() != len || a.iter().any(|n| *n < 0 || *n > max) {
                return Err(format!("array length/range {name}"));
            }
        }
        if self.records.len() != 505
            || self.sequences.len() != 461
            || self.extra.len() != 50
            || self.extra.values().map(Vec::len).sum::<usize>() != 56
        {
            return Err("record/inventory counts".into());
        }
        for r in &self.records {
            if r.len() != 6
                || r[..3].iter().any(|n| i32::try_from(*n).is_err())
                || r[3..5].iter().any(|n| !(0..=9).contains(n))
                || !(0..=0x7fff).contains(&r[5])
            {
                return Err("type record width/values".into());
            }
            if r[5] & 0x4000 != 0 {
                for &field in &r[..3] {
                    if field < 0
                        || (field & 65535) + (field >> 24) + ((field >> 20) & 15)
                            > self.a("EXTENDED").len() as i64
                    {
                        return Err("extended bounds".into());
                    }
                }
            }
        }
        for cp in 0..0x110000usize {
            let block = self.a("TYPE_INDEX1")[cp >> 7] as usize;
            let rec = *self
                .a("TYPE_INDEX2")
                .get((block << 7) + (cp & 127))
                .ok_or("type index2 bounds")? as usize;
            let r = self.records.get(rec).ok_or("type record bounds")?;
            for &f in &r[..3] {
                let mapped = if r[5] & 0x4000 != 0 {
                    self.a("EXTENDED")[(f & 65535) as usize]
                } else {
                    (cp as i64).checked_add(f).ok_or("case arithmetic")?
                };
                if !(0..0x110000).contains(&mapped) {
                    return Err("case result bounds".into());
                }
            }
            self.decode(cp)?;
        }
        for &off in self.a("LEXICON_OFFSET") {
            let tail = self
                .a("LEXICON")
                .get(off as usize..)
                .ok_or("lexicon offset")?;
            if !tail.iter().take(257).any(|b| *b >= 128) {
                return Err("unterminated lexicon word".into());
            }
        }
        for row in &self.sequences {
            if row.len() != 5 || !(2..=4).contains(&row[0]) {
                return Err("sequence length/width".into());
            }
            let len = row[0] as usize;
            if row[1..=len].iter().any(|n| !(0..=65535).contains(n))
                || row[len + 1..].iter().any(|n| *n != 0)
            {
                return Err("sequence values/padding".into());
            }
        }
        for (&key, values) in &self.extra {
            if !(0..0x110000).contains(&key) || values.iter().any(|n| !(0..0x110000).contains(n)) {
                return Err("extra bounds".into());
            }
        }
        let mut cps = BTreeSet::new();
        let mut names = BTreeSet::new();
        for &cp in self.a("CODE_HASH").iter().filter(|n| **n != 0) {
            if !cps.insert(cp) {
                return Err("duplicate hash target".into());
            }
            let name = self
                .decode(cp as usize)?
                .ok_or("missing hash target name")?;
            if name.is_empty()
                || !name.is_ascii()
                || !names.insert(name.iter().map(u8::to_ascii_uppercase).collect::<Vec<_>>())
            {
                return Err("duplicate/nonASCII name".into());
            }
            // Independently prove every target is reachable by its source hash.
            let h = name_hash(&name);
            let mut slot = (!h) & 65535;
            let mut inc = (h ^ (h >> 3)) & 65535;
            if inc == 0 {
                inc = 65535;
            }
            let mut found = false;
            for _ in 0..65536 {
                let v = self.a("CODE_HASH")[slot as usize];
                if v == cp {
                    found = true;
                    break;
                }
                if v == 0 {
                    break;
                }
                slot = (slot + inc) & 65535;
                inc <<= 1;
                if inc > 65535 {
                    inc ^= 65581;
                }
            }
            if !found {
                return Err("unreachable hash target".into());
            }
        }
        if cps.len() != 35458 {
            return Err("hash inventory".into());
        }
        for cp in 0xf0000..0xf01d6 {
            if !cps.contains(&cp) {
                return Err("missing alias".into());
            }
        }
        for cp in 0xf0200..0xf03cd {
            if !cps.contains(&cp) {
                return Err("missing sequence".into());
            }
        }
        // Multiplication by x under the source polynomial visits every nonzero increment.
        let mut inc = 1u32;
        let mut cycle = BTreeSet::new();
        while cycle.insert(inc) {
            inc <<= 1;
            if inc > 65535 {
                inc ^= 65581;
            }
        }
        if inc != 1 || cycle.len() != 65535 {
            return Err("probe polynomial cycle".into());
        }
        Ok(())
    }
}
fn name_hash(name: &[u8]) -> u32 {
    let mut h = 0u32;
    for b in name {
        h = h * 47 + b.to_ascii_uppercase() as u32;
        let ix = h & 0xff000000;
        if ix != 0 {
            h = (h ^ ((ix >> 24) & 255)) & 0xffffff;
        }
    }
    h
}
fn emit_array(out: &mut String, name: &str, ty: &str, v: &[i64]) {
    writeln!(
        out,
        "#[rustfmt::skip]\npub(super) static {name}: [{ty}; {}] = [",
        v.len()
    )
    .unwrap();
    for chunk in v.chunks(12) {
        out.push_str("    ");
        for n in chunk {
            write!(out, "{n}, ").unwrap();
        }
        out.push('\n');
    }
    out.push_str("];\n");
}
fn emitted(t: &Tables) -> String {
    let mut out = String::from(
        "// Generated offline from pinned CPython 3.11.15, Unicode 14.0.0.\n// See UNICODE14-PROVENANCE.json and UNICODE14-NOTICES.\n#![allow(clippy::large_const_arrays)]\n#[repr(C)]\n#[derive(Clone, Copy)]\npub(super) struct TypeRecord(pub i32, pub i32, pub i32, pub u8, pub u8, pub u16);\n#[rustfmt::skip]\npub(super) static TYPE_RECORDS: [TypeRecord; 505] = [\n",
    );
    for r in &t.records {
        writeln!(
            out,
            "    TypeRecord({}, {}, {}, {}, {}, {}),",
            r[0], r[1], r[2], r[3], r[4], r[5]
        )
        .unwrap();
    }
    out.push_str("];\n");
    for name in [
        "EXTENDED",
        "TYPE_INDEX1",
        "TYPE_INDEX2",
        "LEXICON",
        "LEXICON_OFFSET",
        "PHRASEBOOK",
        "NAME_INDEX1",
        "NAME_INDEX2",
        "CODE_HASH",
        "ALIASES",
    ] {
        let ty = match name {
            "LEXICON" | "PHRASEBOOK" => "u8",
            "TYPE_INDEX1" | "TYPE_INDEX2" | "NAME_INDEX1" => "u16",
            _ => "u32",
        };
        emit_array(&mut out, name, ty, t.a(name));
    }
    emit_array(
        &mut out,
        "SEQUENCE_LENGTHS",
        "u8",
        &t.sequences.iter().map(|r| r[0]).collect::<Vec<_>>(),
    );
    out.push_str("#[rustfmt::skip]\npub(super) static SEQUENCE_DATA: [[u16; 4]; 461] = [\n");
    for r in &t.sequences {
        writeln!(out, "    [{}, {}, {}, {}],", r[1], r[2], r[3], r[4]).unwrap();
    }
    out.push_str("];\n");
    emit_array(
        &mut out,
        "EXTRA_KEYS",
        "u32",
        &t.extra.keys().copied().collect::<Vec<_>>(),
    );
    let mut offsets = vec![0];
    let mut values = Vec::new();
    for v in t.extra.values() {
        values.extend(v);
        offsets.push(values.len() as i64);
    }
    emit_array(&mut out, "EXTRA_OFFSETS", "u16", &offsets);
    emit_array(&mut out, "EXTRA_VALUES", "u32", &values);
    out
}
// Parse emitted Rust numeric initializers back through a separate linear token path.
// Check every source value, both index levels and all sequence padding.
fn roundtrip(t: &Tables, text: &str) -> Result<(), String> {
    let mut expected = t.arrays.clone();
    expected.insert(
        "TYPE_RECORDS".into(),
        t.records.iter().flatten().copied().collect(),
    );
    expected.insert(
        "SEQUENCE_LENGTHS".into(),
        t.sequences.iter().map(|r| r[0]).collect(),
    );
    expected.insert(
        "SEQUENCE_DATA".into(),
        t.sequences
            .iter()
            .flat_map(|r| r[1..].iter().copied())
            .collect(),
    );
    expected.insert("EXTRA_KEYS".into(), t.extra.keys().copied().collect());
    let mut off = vec![0];
    let mut vals = Vec::new();
    for v in t.extra.values() {
        vals.extend(v);
        off.push(vals.len() as i64);
    }
    expected.insert("EXTRA_OFFSETS".into(), off);
    expected.insert("EXTRA_VALUES".into(), vals);
    for (name, want) in expected {
        let declaration = text
            .split(&format!("static {name}: "))
            .nth(1)
            .ok_or("emitted declaration")?;
        let init = declaration
            .split_once("= [\n")
            .ok_or("emitted initializer")?
            .1
            .split_once("];\n")
            .ok_or("emitted terminator")?
            .0;
        let mut got = Vec::new();
        for token in init
            .split(|c: char| c.is_whitespace() || ",[]()".contains(c))
            .filter(|s| !s.is_empty())
        {
            if token == "TypeRecord" {
                continue;
            }
            got.push(token.parse::<i64>().map_err(|_| "emitted numeric token")?);
        }
        if got != want {
            return Err(format!("emitted value mismatch {name}"));
        }
    }
    Ok(())
}
fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}
fn load() -> Result<(Tables, serde_json::Value, Vec<u8>), String> {
    let mut inputs = BTreeMap::new();
    let mut sources = Vec::new();
    for &(file, url, hash) in SOURCES {
        let path = root().join("tests/fixtures/unicode14/upstream").join(file);
        if fs::metadata(&path).map_err(|e| e.to_string())?.len() > MAX_INPUT as u64 {
            return Err("source over manifest bound".into());
        }
        let bytes = fs::read(path).map_err(|e| e.to_string())?;
        verify(&bytes, hash, MAX_INPUT)?;
        sources.push(
            json!({"file":file,"url":url,"sha256":hash,"bytes":bytes.len(),"max_bytes":MAX_INPUT}),
        );
        inputs.insert(file, String::from_utf8(bytes).map_err(|e| e.to_string())?);
    }
    let generation = &inputs["cpython/Tools/unicode/makeunicodedata.py"];
    let ranges = generation
        .split_once("cjk_ranges = [")
        .ok_or("CJK generation reference")?
        .1
        .split_once(']')
        .ok_or("CJK range terminator")?
        .0;
    let mut got = Vec::new();
    for line in ranges.lines().filter(|l| l.contains('(')) {
        let vals: Vec<_> = line.split('\'').collect();
        if vals.len() != 5 {
            return Err("CJK range grammar".into());
        }
        got.push((
            u32::from_str_radix(vals[1], 16).map_err(|_| "CJK integer")?,
            u32::from_str_radix(vals[3], 16).map_err(|_| "CJK integer")?,
        ));
    }
    if got != CJK {
        return Err("CJK reference drift".into());
    }
    let tables = Tables::parse(
        &inputs["cpython/Objects/unicodetype_db.h"],
        &inputs["cpython/Modules/unicodename_db.h"],
        &inputs["cpython/Lib/re/_casefix.py"],
    )?;
    let mut notices = String::from(
        "Rust translation: immutable compressed tables and bounded lookup translated from CPython 3.11.15. Numeric/type/name ordering retained; no normalization database or numeric-value function translated. Original files below are preserved verbatim.\n\n",
    );
    for file in [
        "cpython/LICENSE",
        "unicode/ReadMe.txt",
        "unicode/license.txt",
    ] {
        writeln!(notices, "--- {file} ---").unwrap();
        notices.push_str(&inputs[file]);
        notices.push('\n');
    }
    for file in [
        "cpython/Objects/unicodectype.c",
        "cpython/Modules/unicodedata.c",
    ] {
        let end = inputs[file].find("*/").ok_or("copyright block")? + 2;
        notices.push_str(&inputs[file][..end]);
        notices.push('\n');
    }
    let support = generation
        .split_once("# the following support code")
        .ok_or("Secret Labs notice")?
        .1;
    for l in support.lines().take(3) {
        notices.push_str(l);
        notices.push('\n');
    }
    Ok((
        tables,
        json!({"format":1,"converter_format":1,"unicode":"14.0.0","cpython_commit":"2340a037f7450e70fccfe411e6531afb4d57a312","sources":sources}),
        notices.into_bytes(),
    ))
}
fn outputs() -> Result<BTreeMap<String, Vec<u8>>, String> {
    let (t, mut provenance, notices) = load()?;
    let table = emitted(&t);
    roundtrip(&t, &table)?;
    provenance["outputs"] = json!({"unicode14_tables.rs":digest(table.as_bytes()),"UNICODE14-NOTICES":digest(&notices)});
    provenance["inventory"] = json!({"types":505,"extended":1236,"hash_targets":35458,"aliases":470,"sequences":461,"extra_keys":50,"extra_targets":56});
    let mut m = BTreeMap::new();
    m.insert("unicode14_tables.rs".into(), table.into_bytes());
    m.insert("UNICODE14-NOTICES".into(), notices);
    m.insert(
        "UNICODE14-PROVENANCE.json".into(),
        serde_json::to_vec_pretty(&provenance).unwrap(),
    );
    Ok(m)
}
fn run() -> Result<(), String> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let m = outputs()?;
    let target = root().join("src/output_schema/python_regex");
    match args.first().map(String::as_str) {
        Some("--emit") if args.len() == 2 => {
            let dir = Path::new(&args[1]);
            if dir.exists()
                && fs::read_dir(dir)
                    .map_err(|e| e.to_string())?
                    .next()
                    .is_some()
            {
                return Err("emit needs empty directory".into());
            }
            fs::create_dir_all(dir).map_err(|e| e.to_string())?;
            for (file, data) in m {
                fs::write(dir.join(file), data).map_err(|e| e.to_string())?;
            }
        }
        Some("--check") if args.len() == 1 => {
            for (file, data) in &m {
                if fs::read(target.join(file)).map_err(|e| e.to_string())? != *data {
                    return Err(format!("generated byte mismatch {file}"));
                }
            }
            let tmp = root().join("../../.superpowers/sdd/schema-unicode-regeneration");
            fs::create_dir_all(&tmp).map_err(|e| e.to_string())?;
            for dir in ["first", "second"] {
                let p = tmp.join(dir);
                fs::create_dir_all(&p).map_err(|e| e.to_string())?;
                for (file, data) in outputs()? {
                    fs::write(p.join(file), data).map_err(|e| e.to_string())?;
                }
            }
            for (file, data) in m {
                for dir in ["first", "second"] {
                    if fs::read(tmp.join(dir).join(&file)).map_err(|e| e.to_string())? != data {
                        return Err("nondeterministic emission".into());
                    }
                }
            }
        }
        _ => return Err("use --check or --emit EMPTY_DIRECTORY".into()),
    }
    println!(
        "Unicode14 source hashes, structural invariants, all-value roundtrip and deterministic output verified"
    );
    Ok(())
}
fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(1);
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_changed_hash_and_oversized_input() {
        let hash = digest(b"approved");
        assert!(verify(b"changed", &hash, 100).is_err());
        assert!(verify(b"approved", &hash, 7).is_err());
    }
    #[test]
    fn initializer_faults() {
        for bad in [
            "static int a[]={1}; static int a[]={2};",
            "static int b[]={1};",
            "static int a[]={0xGG};",
            "static int a[]={18446744073709551616};",
            "static int a[]={1",
            "static int a[]={1,x};",
            "static int a[]={1 2};",
        ] {
            assert!(array(bad, "a").is_err(), "{bad}");
        }
        assert!(uncomment("/* no terminator", false).is_err());
        assert!(cases("_EXTRA_CASES={1:(2,),1:(3,),}").is_err());
        assert!(cases("_EXTRA_CASES={1:(2,),} extra").is_err());
        assert!(cases("_EXTRA_CASES={1:(x,),}").is_err());
    }
    #[test]
    fn structural_corruptions() {
        let (t, _, _) = load().unwrap();
        let mut c = t.clone();
        c.arrays.get_mut("TYPE_INDEX1").unwrap()[0] = 65535;
        assert!(c.validate().is_err());
        let mut c = t.clone();
        c.arrays.get_mut("TYPE_INDEX2").unwrap()[0] = 65535;
        assert!(c.validate().is_err());
        let mut c = t.clone();
        c.arrays.get_mut("LEXICON").unwrap().fill(65);
        assert!(c.validate().is_err());
        let mut c = t.clone();
        c.arrays.get_mut("ALIASES").unwrap()[0] = 0x110000;
        assert!(c.validate().is_err());
        let mut c = t.clone();
        c.sequences[0][4] = 1;
        assert!(c.validate().is_err());
        let mut c = t.clone();
        c.sequences[0][0] = 5;
        assert!(c.validate().is_err());
        let mut c = t.clone();
        c.records[0].pop();
        assert!(c.validate().is_err());
        let mut c = t.clone();
        c.arrays.get_mut("NAME_INDEX2").unwrap()[0] = i64::MAX;
        assert!(c.validate().is_err());
        let type_src = fs::read_to_string(
            root().join("tests/fixtures/unicode14/upstream/cpython/Objects/unicodetype_db.h"),
        )
        .unwrap();
        let name_src = fs::read_to_string(
            root().join("tests/fixtures/unicode14/upstream/cpython/Modules/unicodename_db.h"),
        )
        .unwrap();
        let extra = fs::read_to_string(
            root().join("tests/fixtures/unicode14/upstream/cpython/Lib/re/_casefix.py"),
        )
        .unwrap();
        assert!(
            Tables::parse(
                &type_src.replace("#define SHIFT 7", "#define SHIFT 8"),
                &name_src,
                &extra
            )
            .is_err()
        );
        assert!(Tables::parse(&type_src[..type_src.len() / 2], &name_src, &extra).is_err());
        let text = emitted(&t);
        roundtrip(&t, &text).unwrap();
        assert!(roundtrip(&t, &text.replacen("TypeRecord(0,", "TypeRecord(1,", 1)).is_err());
    }
}
