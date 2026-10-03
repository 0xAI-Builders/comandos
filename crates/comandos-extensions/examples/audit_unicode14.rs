//! Development capture/audit, explicitly invoked and never installed.
#[path = "unicode14/audit_protocol.rs"]
mod audit_protocol;
#[path = "unicode14/oracle_queries.rs"]
mod oracle_queries;
#[path = "unicode14/records.rs"]
mod records;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
    time::Duration,
};
#[path = "../src/output_schema/python_regex/unicode14.rs"]
mod unicode14;
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
fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/unicode14")
}
fn evidence() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../.superpowers/sdd/schema-unicode-capture-v2")
}
fn digest(data: &[u8]) -> String {
    format!("{:x}", Sha256::digest(data))
}
fn query(req: &Value, label: &str) -> Result<Value, String> {
    let bytes = serde_json::to_vec(req).unwrap();
    let result = audit_protocol::run(
        "/venv/bin/python",
        &["-c".into(), oracle_queries::QUERY.into()],
        &bytes,
        Duration::from_secs(10),
        &evidence().join(label),
    )?;
    serde_json::from_slice(&result).map_err(|e| e.to_string())
}
fn write_json(path: &Path, v: &Value) -> Result<(), String> {
    fs::write(path, serde_json::to_vec_pretty(v).unwrap()).map_err(|e| e.to_string())
}
fn capture() -> Result<(), String> {
    fs::create_dir_all(evidence()).map_err(|e| e.to_string())?;
    let root = fixture();
    let manifest = root.join("oracle_manifest.json");
    if manifest.exists() {
        return Err("capture already frozen; refusing overwrite".into());
    }
    let mut sources = Vec::new();
    for &(file, url, hash) in SOURCES {
        let data = fs::read(root.join("upstream").join(file)).map_err(|e| e.to_string())?;
        if digest(&data) != hash {
            return Err(format!("source hash {file}"));
        }
        sources.push(
            json!({"file":file,"url":url,"sha256":hash,"bytes":data.len(),"max_bytes":4*1024*1024}),
        );
    }
    let mut m = json!({"format":records::FORMAT,"domain":[0,records::DOMAIN],"width":records::WIDTH,"rows_per_child":8,"primitive":{"record_bytes":16,"layout":"LE u32 cp/lower/upper; LE u16 flags; u8 decimal (255 absent); u8 zero","flags":["decimal","alnum","word","space","linebreak","regex_iscased","xid_start","xid_continue","identifier_start","identifier_continue"]},"canonical":{"layout":"LE u32 cp; LE u16 ASCII byte length (65535 missing); ASCII bytes","coverage":"every codepoint in ascending order"},"lookup":{"layout":"JSONL preserving u32 input vector, measured lookup class/value, ordinary/class re.compile classes"},"sources":sources,"complete":false});
    write_json(&manifest, &m)?;
    let identity = query(&json!({"op":"identity"}), "identity")?;
    for (suffix, hash) in [
        (
            "_casefix.py",
            "41572ac50cf96b04496e676d8a6708898bb8e752e06dad34ed4c50c5d8f1fe40",
        ),
        (
            "_parser.py",
            "4748e39c77d6dc14f81af80e68a62ad99031a8182d5e0b219a6666d0cfb1626f",
        ),
    ] {
        let files = identity["files"].as_object().ok_or("identity files")?;
        if !files.iter().any(|(p, h)| p.ends_with(suffix) && h == hash) {
            return Err(format!("installed source mismatch {suffix}"));
        }
    }
    m["identity"] = identity;
    let mut inventories = json!({});
    for (op, filename) in [
        ("primitive", "primitive_records.bin"),
        ("canonical", "canonical_names.bin"),
    ] {
        let mut output =
            io::BufWriter::new(fs::File::create(root.join(filename)).map_err(|e| e.to_string())?);
        let mut hash = Sha256::new();
        let mut count = 0u32;
        let mut batches = 0;
        for start in (0..records::DOMAIN).step_by((records::WIDTH * 8) as usize) {
            let rows: Vec<_> = (start..(start + records::WIDTH * 8).min(records::DOMAIN))
                .step_by(records::WIDTH as usize)
                .map(|a| (a, (a + records::WIDTH).min(records::DOMAIN)))
                .collect();
            if rows[0].0 != count {
                return Err("coverage gap".into());
            }
            let result = query(&json!({"op":op,"rows":rows}), &format!("{op}-{batches:04}"))?;
            let data = records::packed_rows(&result, &rows, op)?;
            output.write_all(&data).map_err(|e| e.to_string())?;
            hash.update(&data);
            count = rows.last().unwrap().1;
            batches += 1;
        }
        output.flush().map_err(|e| e.to_string())?;
        if count != records::DOMAIN {
            return Err("incomplete domain".into());
        }
        inventories[filename] = json!({"sha256":format!("{:x}",hash.finalize()),"records":count,"rows":records::DOMAIN/records::WIDTH,"batches":batches,"bytes":fs::metadata(root.join(filename)).unwrap().len()});
        println!("{op}: {count} records, {batches} batches");
    }
    m["inventories"] = inventories;
    m["complete"] = json!(true);
    m["remaining_first_party_python"] = json!({"file":"examples/unicode14/oracle_queries.rs","constant":"QUERY","purpose":"temporary external installed measurement only"});
    write_json(&manifest, &m)
}
fn points(v: &Value) -> Result<Vec<u32>, String> {
    if let Some(s) = v.as_str() {
        Ok(s.chars().map(|c| c as u32).collect())
    } else {
        serde_json::from_value(v.clone()).map_err(|e| e.to_string())
    }
}
fn native_lookup(input: &[u32], work: &mut usize) -> Result<Value, String> {
    use unicode14::{CodePoint, NameFailure, NameValue};
    let cp: Vec<_> = input
        .iter()
        .map(|n| CodePoint::new(*n).ok_or("invalid input codepoint"))
        .collect::<Result<_, _>>()?;
    let mut charge = |n: usize| {
        *work = work.checked_add(n).ok_or(NameFailure::BudgetBoundary)?;
        if *work > 8 * 1024 * 1024 {
            Err(NameFailure::BudgetBoundary)
        } else {
            Ok(())
        }
    };
    Ok(match unicode14::lookup_name(&cp, &mut charge) {
        Ok(NameValue::Character(c)) => json!({"class":"Character","value":[c.value()]}),
        Ok(NameValue::Sequence { index, len }) => {
            let value: Vec<_> = unicode14::sequence(index)
                .map_err(|e| format!("{e:?}"))?
                .iter()
                .map(|n| *n as u32)
                .collect();
            if value.len() != len as usize {
                return Err("sequence length mismatch".into());
            }
            json!({"class":"Sequence","value":value})
        }
        Err(e) => {
            json!({"class":match e{NameFailure::Missing=>"KeyError",NameFailure::UnicodeEncodeError=>"UnicodeEncodeError",NameFailure::BudgetBoundary=>"BudgetBoundary",NameFailure::InternalTable=>"InternalTable"}})
        }
    })
}
fn primitive(cp: u32) -> [u8; 16] {
    use unicode14::*;
    let c = CodePoint::new(cp).unwrap();
    let d = decimal(c);
    let flags = [
        d.is_some(),
        is_alnum(c),
        is_word(c),
        is_space(c),
        is_linebreak(c),
        regex_iscased(c),
        xid_start(c),
        xid_continue(c),
        identifier_start(c),
        identifier_continue(c),
    ];
    let flags = flags
        .iter()
        .enumerate()
        .fold(0u16, |a, (i, b)| a | ((*b as u16) << i));
    let mut r = [0u8; 16];
    r[..4].copy_from_slice(&cp.to_le_bytes());
    r[4..8].copy_from_slice(&lower(c).value().to_le_bytes());
    r[8..12].copy_from_slice(&upper(c).value().to_le_bytes());
    r[12..14].copy_from_slice(&flags.to_le_bytes());
    r[14] = d.unwrap_or(255);
    r
}
fn native() -> Result<(), String> {
    use base64::Engine;
    use std::io::Read;
    let mut input = Vec::new();
    io::stdin()
        .take((audit_protocol::INPUT_CAP + 1) as u64)
        .read_to_end(&mut input)
        .map_err(|e| e.to_string())?;
    if input.len() > audit_protocol::INPUT_CAP {
        return Err("input overflow".into());
    }
    let req: Value = serde_json::from_slice(&input).map_err(|e| e.to_string())?;
    let op = req["op"].as_str().ok_or("operation")?;
    let mut output = Vec::new();
    if op == "lookup" {
        let rows = req["rows"].as_array().ok_or("rows")?;
        if rows.len() > 8 {
            return Err("nine rows".into());
        }
        let mut work = 0;
        for row in rows {
            let names = row.as_array().ok_or("name row")?;
            if names.len() > 128 {
                return Err("name row work cap".into());
            }
            let mut results = Vec::new();
            for v in names {
                let input = points(v)?;
                if input.len() > 1024 {
                    return Err("name input work cap".into());
                }
                results.push(json!({"input":input,"lookup":native_lookup(&input,&mut work)?}));
            }
            output.push(json!(results));
        }
    } else {
        let rows: Vec<(u32, u32)> =
            serde_json::from_value(req["rows"].clone()).map_err(|e| e.to_string())?;
        records::rows_valid(&rows)?;
        for &(start, end) in &rows {
            let mut data = Vec::new();
            let mut buf = [0u8; 257];
            for cp in start..end {
                if op == "primitive" {
                    data.extend(primitive(cp));
                } else if op == "canonical" {
                    let len =
                        unicode14::canonical_name(unicode14::CodePoint::new(cp).unwrap(), &mut buf)
                            .map_err(|e| format!("{e:?}"))?;
                    data.extend(cp.to_le_bytes());
                    data.extend(len.map_or(65535, |n| n as u16).to_le_bytes());
                    if let Some(n) = len {
                        data.extend(&buf[..n]);
                    }
                } else {
                    return Err("unknown native operation".into());
                }
            }
            output.push(json!({"v":records::FORMAT,"op":op,"start":start,"end":end,"count":end-start,"data":base64::engine::general_purpose::STANDARD.encode(data)}));
        }
    }
    let bytes = serde_json::to_vec(&output).unwrap();
    if bytes.len() > audit_protocol::OUTPUT_CAP {
        return Err("output overflow".into());
    }
    io::stdout().write_all(&bytes).map_err(|e| e.to_string())?;
    Ok(())
}
fn native_query(req: &Value, label: &str) -> Result<Value, String> {
    let directory = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../.superpowers/sdd/schema-unicode-native-combined");
    fs::create_dir_all(&directory).map_err(|e| e.to_string())?;
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let output = audit_protocol::run(
        exe.to_str().ok_or("executable encoding")?,
        &["--native".into()],
        &serde_json::to_vec(req).unwrap(),
        Duration::from_secs(10),
        &directory.join(label),
    )?;
    serde_json::from_slice(&output).map_err(|e| e.to_string())
}
type CanonicalEntry = (u32, Option<Vec<u8>>);
fn canonical_entries(data: &[u8]) -> Result<Vec<CanonicalEntry>, String> {
    let mut entries = Vec::new();
    let mut p = 0;
    for cp in 0..records::DOMAIN {
        let head = data.get(p..p + 6).ok_or("canonical truncation")?;
        if u32::from_le_bytes(head[..4].try_into().unwrap()) != cp {
            return Err("canonical order".into());
        }
        let n = u16::from_le_bytes([head[4], head[5]]);
        p += 6;
        let name = if n == 65535 {
            None
        } else {
            let name = data
                .get(p..p + n as usize)
                .ok_or("name truncation")?
                .to_vec();
            p += n as usize;
            Some(name)
        };
        entries.push((cp, name));
    }
    if p != data.len() {
        return Err("canonical trailing bytes".into());
    }
    Ok(entries)
}
fn compare_domain() -> Result<(), String> {
    let root = fixture();
    let manifest: Value = serde_json::from_slice(
        &fs::read(root.join("oracle_manifest.json")).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    if manifest["complete"] != true {
        return Err("unfinished capture".into());
    }
    let mut report = json!({"format":1,"mismatches":0,"surrogates":2048,"payload_bytes":unicode14::payload_bytes(),"binaries":{"audit":digest(&fs::read(std::env::current_exe().unwrap()).map_err(|e|e.to_string())?)},"operations":{}});
    for (op, filename) in [
        ("primitive", "primitive_records.bin"),
        ("canonical", "canonical_names.bin"),
    ] {
        let expected = fs::read(root.join(filename)).map_err(|e| e.to_string())?;
        if manifest["inventories"][filename]["sha256"] != digest(&expected) {
            return Err("frozen capture hash mismatch".into());
        }
        let mut p = 0;
        let mut count = 0;
        let mut batches = 0;
        let mut native_hash = Sha256::new();
        for start in (0..records::DOMAIN).step_by((records::WIDTH * 8) as usize) {
            let rows: Vec<_> = (start..(start + records::WIDTH * 8).min(records::DOMAIN))
                .step_by(records::WIDTH as usize)
                .map(|a| (a, (a + records::WIDTH).min(records::DOMAIN)))
                .collect();
            if rows[0].0 != count {
                return Err("native coverage gap".into());
            }
            let output =
                native_query(&json!({"op":op,"rows":rows}), &format!("{op}-{batches:04}"))?;
            let actual = records::packed_rows(&output, &rows, op)?;
            records::exact(
                expected
                    .get(p..p + actual.len())
                    .ok_or("native length exceeds frozen")?,
                &actual,
            )
            .map_err(|e| format!("{op} range {start}: {e}"))?;
            p += actual.len();
            native_hash.update(&actual);
            count = rows.last().unwrap().1;
            batches += 1;
        }
        if count != records::DOMAIN || p != expected.len() {
            return Err("full coverage mismatch".into());
        }
        report["operations"][op] = json!({"records":count,"bytes":p,"rows":records::DOMAIN/records::WIDTH,"batches":batches,"expected_sha256":digest(&expected),"native_sha256":format!("{:x}",native_hash.finalize())});
        println!("{op}: exact agreement for {count} records");
    }
    let extra = manifest["identity"]["extra"]
        .as_object()
        .ok_or("extra dictionary")?;
    for cp in 0..records::DOMAIN {
        let expected: Vec<u32> = extra
            .get(&cp.to_string())
            .map(|v| serde_json::from_value(v.clone()).unwrap())
            .unwrap_or_default();
        if unicode14::extra_cases(unicode14::CodePoint::new(cp).unwrap()) != expected {
            return Err(format!("extra case mismatch U+{cp:04X}"));
        }
    }
    report["extra_cases"] = json!({"checked_domain":records::DOMAIN,"keys":extra.len(),"targets":extra.values().map(|v|v.as_array().unwrap().len()).sum::<usize>()});
    write_json(
        &root.join("../../../../../.superpowers/sdd/schema-unicode-exhaustive-combined.json"),
        &report,
    )
}
fn input_value(input: &[u32]) -> Value {
    input
        .iter()
        .map(|n| char::from_u32(*n))
        .collect::<Option<String>>()
        .map_or_else(|| json!(input), |s| json!(s))
}
fn lookup_inventory() -> Result<Vec<(Vec<u32>, String)>, String> {
    let entries = canonical_entries(
        &fs::read(fixture().join("canonical_names.bin")).map_err(|e| e.to_string())?,
    )?;
    let mut names = Vec::new();
    fn append(names: &mut Vec<(Vec<u32>, String)>, name: &[u8], cohort: &str) {
        for mode in 0..3 {
            let value = name
                .iter()
                .enumerate()
                .map(|(i, b)| match mode {
                    1 => b.to_ascii_lowercase(),
                    2 if i % 2 == 0 => b.to_ascii_lowercase(),
                    _ => *b,
                })
                .map(|b| b as u32)
                .collect();
            names.push((value, format!("{cohort}/{mode}")));
        }
    }
    for (cp, name) in entries {
        if let Some(name) = name {
            append(&mut names, &name, "canonical");
            if cp < 0x10000 && name.starts_with(b"CJK UNIFIED IDEOGRAPH-") {
                let s = format!("CJK UNIFIED IDEOGRAPH-{cp:05X}");
                append(&mut names, s.as_bytes(), "leading-zero");
            }
        }
    }
    let mut buf = [0u8; 257];
    for (a, b, cohort) in [(0xf0000, 0xf01d6, "alias"), (0xf0200, 0xf03cd, "sequence")] {
        for cp in a..b {
            let n = unicode14::synthetic_name(unicode14::CodePoint::new(cp).unwrap(), &mut buf)
                .map_err(|e| format!("{e:?}"))?
                .ok_or("missing synthetic name")?;
            append(&mut names, &buf[..n], cohort);
        }
    }
    for input in directed_names() {
        names.push((input, "directed".into()));
    }
    Ok(names)
}
fn directed_names() -> Vec<Vec<u32>> {
    let mut names: Vec<Vec<u32>> = vec![
        "",
        "LATIN CAPITAL LETTER A\0",
        "\0",
        " LATIN CAPITAL LETTER A",
        "LATIN CAPITAL LETTER A ",
        "LATIN  CAPITAL LETTER A",
        "LATIN-CAPITAL LETTER A",
        "HANGUL SYLLABLE ",
        "HANGUL SYLLABLE G",
        "HANGUL SYLLABLE GAX",
        "HANGUL SYLLABLE GGG",
        "HANGUL SYLLABLE GGWA",
        "HANGUL SYLLABLE GAGS",
        "HANGUL SYLLABLE GA\0",
        "CJK UNIFIED IDEOGRAPH-",
        "CJK UNIFIED IDEOGRAPH-0004E00",
        "CJK UNIFIED IDEOGRAPH-4e00",
        "CJK UNIFIED IDEOGRAPH-04e00",
        "TANGUT IDEOGRAPH-17000",
        "NUSHU CHARACTER-1B170",
        "KHITAN SMALL SCRIPT CHARACTER-18B00",
        "é",
        "☃",
    ]
    .into_iter()
    .map(|s| s.chars().map(|c| c as u32).collect())
    .collect();
    for n in [255, 256, 257] {
        names.push(vec![65; n]);
    }
    for n in [127, 128, 129] {
        names.push(vec![0xe9; n]);
    }
    for input in [vec![0xd800], vec![0xdfff], vec![65, 0xd800, 66], {
        let mut s = vec![65; 257];
        s.push(0xd800);
        s
    }] {
        names.push(input);
    }
    for (a, b) in [
        (0x3400, 0x4dbf),
        (0x4e00, 0x9fff),
        (0x20000, 0x2a6df),
        (0x2a700, 0x2b738),
        (0x2b740, 0x2b81d),
        (0x2b820, 0x2cea1),
        (0x2ceb0, 0x2ebe0),
        (0x30000, 0x3134a),
    ] {
        for cp in [a - 1, a, a + 1, b - 1, b, b + 1] {
            names.push(
                format!("CJK UNIFIED IDEOGRAPH-{cp:X}")
                    .bytes()
                    .map(u32::from)
                    .collect(),
            );
        }
    }
    names
}
fn capture_lookups() -> Result<(), String> {
    fs::create_dir_all(evidence()).map_err(|e| e.to_string())?;
    let root = fixture();
    if root.join("lookup_cases.jsonl").exists() {
        return Err("lookup capture frozen; refusing overwrite".into());
    }
    let names = lookup_inventory()?;
    let mut manifest: Value = serde_json::from_slice(
        &fs::read(root.join("oracle_manifest.json")).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    manifest["lookup"]["layout"] = json!(
        "JSONL id/cohort/input (scalar string or surrogate u32 vector)/lookup class and value/ordinary and class re.compile classes"
    );
    manifest["lookup"]["planned_records"] = json!(names.len());
    manifest["lookup"]["complete"] = json!(false);
    write_json(&root.join("oracle_manifest.json"), &manifest)?;
    let mut file = io::BufWriter::new(
        fs::File::create(root.join("lookup_cases.jsonl")).map_err(|e| e.to_string())?,
    );
    let mut directed = Vec::new();
    let mut hash = Sha256::new();
    let mut count = 0;
    let mut cohorts = std::collections::BTreeMap::<String, usize>::new();
    for (batch, chunk) in names.chunks(1024).enumerate() {
        let rows: Vec<_> = chunk
            .chunks(128)
            .map(|r| r.iter().map(|(cp, _)| cp).collect::<Vec<_>>())
            .collect();
        let output = query(
            &json!({"op":"lookup","rows":rows}),
            &format!("lookup-{batch:04}"),
        )?;
        let rows_out = output.as_array().ok_or("lookup rows")?;
        if rows_out.len() != rows.len() {
            return Err("lookup row count".into());
        }
        for (row_out, row_in) in rows_out.iter().zip(&rows) {
            if row_out.as_array().ok_or("lookup row")?.len() != row_in.len() {
                return Err("lookup count".into());
            }
        }
        for (mut result, (input, cohort)) in rows_out
            .iter()
            .flat_map(|r| r.as_array().unwrap())
            .cloned()
            .zip(chunk)
        {
            if points(&result["input"])? != *input {
                return Err("lookup identity/order".into());
            }
            result["input"] = input_value(input);
            result["id"] = json!(count);
            result["cohort"] = json!(cohort);
            if cohort == "directed" {
                directed.push(result.clone());
            }
            *cohorts.entry(cohort.clone()).or_default() += 1;
            let mut bytes = serde_json::to_vec(&result).unwrap();
            bytes.push(b'\n');
            file.write_all(&bytes).map_err(|e| e.to_string())?;
            hash.update(bytes);
            count += 1;
        }
    }
    file.flush().map_err(|e| e.to_string())?;
    if count != names.len() {
        return Err("lookup inventory coverage".into());
    }
    let primitive_data = fs::read(root.join("primitive_records.bin")).map_err(|e| e.to_string())?;
    let canonical =
        canonical_entries(&fs::read(root.join("canonical_names.bin")).map_err(|e| e.to_string())?)?;
    let directed_points:Vec<_>=[0x1c,0x1d,0x1e,0x1f,0x661,0xff11,0xb2,0xdf,0xfb00,0x130,0x131,0x17f,0x212a,0x3a3,0x3c2,0x3c3,0x301,0xd800,0xdfff,0x11f00,0x1e4f0,0x3134a,0x3134b,0x31350,0x17000].iter().map(|&cp|json!({"cp":cp,"primitive_hex":primitive_data[cp as usize*16..][..16].iter().map(|b|format!("{b:02x}")).collect::<String>(),"canonical":canonical[cp as usize].1.as_ref().map(|n|String::from_utf8(n.clone()).unwrap())})).collect();
    write_json(
        &root.join("directed_cases.json"),
        &json!({"format":1,"lookup":directed,"points":directed_points,"upper_equivalence":"str.upper on one character uses full-upper sequence whose first entry equals _PyUnicode_ToUppercase; captured sharp s and ligatures independently"}),
    )?;
    manifest["lookup"]["complete"] = json!(true);
    manifest["lookup"]["records"] = json!(count);
    manifest["lookup"]["cohorts"] = json!(cohorts);
    manifest["lookup"]["sha256"] = json!(format!("{:x}", hash.finalize()));
    manifest["lookup"]["batches"] = json!(names.len().div_ceil(1024));
    manifest["lookup"]["directed_sha256"] =
        json!(digest(&fs::read(root.join("directed_cases.json")).unwrap()));
    write_json(&root.join("oracle_manifest.json"), &manifest)?;
    println!("lookup capture: {count} records");
    Ok(())
}
fn compare_lookups() -> Result<(), String> {
    use std::io::BufRead;
    let root = fixture();
    let manifest: Value = serde_json::from_slice(
        &fs::read(root.join("oracle_manifest.json")).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    let frozen = fs::read(root.join("lookup_cases.jsonl")).map_err(|e| e.to_string())?;
    if manifest["lookup"]["sha256"] != digest(&frozen) {
        return Err("lookup capture hash".into());
    }
    let inventory = lookup_inventory()?;
    let mut rows = Vec::new();
    let mut count = 0;
    let mut batches = 0;
    let mut re_sequence_rejections = 0;
    let mut cohorts = std::collections::BTreeMap::<String, usize>::new();
    let process = |rows: &[Value], batch: usize| -> Result<(), String> {
        let inputs: Vec<_> = rows
            .chunks(128)
            .map(|r| r.iter().map(|v| v["input"].clone()).collect::<Vec<_>>())
            .collect();
        let actual = native_query(
            &json!({"op":"lookup","rows":inputs}),
            &format!("lookup-{batch:04}"),
        )?;
        let a = actual.as_array().ok_or("native lookup array")?;
        if a.len() != inputs.len() {
            return Err("native lookup row count".into());
        }
        for (a, i) in a.iter().zip(&inputs) {
            if a.as_array().ok_or("native lookup row")?.len() != i.len() {
                return Err("native lookup count".into());
            }
        }
        for (actual, expected) in a.iter().flat_map(|r| r.as_array().unwrap()).zip(rows) {
            if points(&actual["input"])? != points(&expected["input"])?
                || actual["lookup"] != expected["lookup"]
            {
                return Err(format!(
                    "lookup mismatch id {} input {} expected {} actual {}",
                    expected["id"], expected["input"], expected["lookup"], actual["lookup"]
                ));
            }
        }
        Ok(())
    };
    for line in io::Cursor::new(&frozen).lines() {
        let record: Value =
            serde_json::from_str(&line.map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        if record["id"] != count
            || inventory.get(count).is_none_or(|(cp, cohort)| {
                points(&record["input"]).ok().as_ref() != Some(cp) || record["cohort"] != *cohort
            })
        {
            return Err("frozen lookup inventory/id/cohort".into());
        }
        let cohort = record["cohort"].as_str().ok_or("cohort")?.to_string();
        *cohorts.entry(cohort.clone()).or_default() += 1;
        if cohort.starts_with("sequence") && record["lookup"]["class"] == "Sequence" {
            if record["re"] != json!(["error", "error"]) {
                return Err("installed regex accepted named sequence".into());
            }
            re_sequence_rejections += 1;
        }
        rows.push(record);
        count += 1;
        if rows.len() == 1024 {
            process(&rows, batches)?;
            rows.clear();
            batches += 1;
        }
    }
    if !rows.is_empty() {
        process(&rows, batches)?;
        batches += 1;
    }
    if count != inventory.len() {
        return Err("lookup inventory omitted rows".into());
    }
    let report = json!({"format":1,"records":count,"batches":batches,"cohorts":cohorts,"exact_lookup_mismatches":0,"installed_re_sequence_rejections_both_contexts":re_sequence_rejections,"negative_space":"directed only; not exhaustive","frozen_sha256":digest(&frozen),"audit_binary_sha256":digest(&fs::read(std::env::current_exe().unwrap()).unwrap())});
    write_json(
        &root.join("../../../../../.superpowers/sdd/schema-unicode-names-combined.json"),
        &report,
    )?;
    println!("lookup: exact agreement for {count} names and variants");
    Ok(())
}
const HOST_ROOT: &str = "/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration";
fn task_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}
fn inventory_files(dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), String> {
    for entry in fs::read_dir(dir).map_err(|e| e.to_string())? {
        let p = entry.map_err(|e| e.to_string())?.path();
        if p.is_dir() {
            inventory_files(&p, out)?;
        } else if p.is_file() {
            out.push(p);
        }
    }
    Ok(())
}
fn resources(dir: &Path) -> Result<Value, String> {
    let mut max_cpu = 0f64;
    let mut max_rss = 0u64;
    let mut batches = 0;
    let mut max_wall = 0u64;
    let mut max_out = 0u64;
    let mut max_in = 0u64;
    for entry in fs::read_dir(dir).map_err(|e| e.to_string())? {
        let path = entry.map_err(|e| e.to_string())?.path();
        if path.extension().and_then(|p| p.to_str()) == Some("stderr") {
            let s = fs::read_to_string(&path).map_err(|e| e.to_string())?;
            for line in s.lines().filter(|l| l.starts_with("RESOURCE ")) {
                let mut cpu = 0f64;
                let mut rss = 0;
                for part in line.split_whitespace() {
                    if let Some((key, val)) = part.split_once('=') {
                        match key {
                            "cpu_user" | "cpu_system" => {
                                cpu += val.parse::<f64>().map_err(|_| "CPU measurement")?
                            }
                            "rss_kib" => rss = val.parse::<u64>().map_err(|_| "RSS measurement")?,
                            _ => {}
                        }
                    }
                }
                max_cpu = max_cpu.max(cpu);
                max_rss = max_rss.max(rss);
                batches += 1;
            }
            let meta: Value = serde_json::from_slice(
                &fs::read(path.with_extension("json")).map_err(|e| e.to_string())?,
            )
            .map_err(|e| e.to_string())?;
            if meta["stdout_bytes"].as_u64().ok_or("stdout count")?
                + meta["stderr_bytes"].as_u64().ok_or("stderr count")?
                > audit_protocol::OUTPUT_CAP as u64
            {
                return Err("retained combined output cap exceeded".into());
            }
            if meta["failure"] != Value::Null
                || meta["status"] != "exit status: 0"
                || meta["reaped"] != true
            {
                return Err("unexpected productive child failure".into());
            }
            max_wall = max_wall.max(meta["elapsed_ms"].as_u64().ok_or("wall measurement")?);
            max_out = max_out.max(meta["stdout_bytes"].as_u64().ok_or("output measurement")?);
            max_in = max_in.max(meta["input_bytes"].as_u64().ok_or("input measurement")?);
        }
    }
    Ok(
        json!({"children":batches,"max_cpu_seconds":max_cpu,"cpu_precision_seconds":0.01,"max_rss_kib":max_rss,"max_wall_ms":max_wall,"max_stdout_bytes":max_out,"max_input_bytes":max_in}),
    )
}
fn freeze() -> Result<(), String> {
    let root = task_root();
    let sdd = root.join(".superpowers/sdd");
    let baseline = sdd.join("schema-regex-literal-count-frozen.json");
    let data = fs::read(&baseline).map_err(|e| e.to_string())?;
    if digest(&data) != "9d7b60f17550aec2ed68d2b87cf6bcfbd4dbd43a5de1c02d5e73e73b9cefb87f" {
        return Err("approved literal manifest drift".into());
    }
    let baseline: Value = serde_json::from_slice(&data).map_err(|e| e.to_string())?;
    let mut preserved = json!({});
    for (host, info) in baseline["files"]
        .as_object()
        .ok_or("baseline file inventory")?
    {
        let rel = host
            .strip_prefix(HOST_ROOT)
            .ok_or("historical foreign path")?
            .trim_start_matches('/');
        let mut data = fs::read(root.join(rel)).map_err(|e| format!("{rel}: {e}"))?;
        if rel == "crates/comandos-extensions/src/output_schema/python_regex.rs" {
            let tail = b"#[path = \"python_regex/unicode14.rs\"]\nmod unicode14;\n";
            if !data.ends_with(tail) {
                return Err("parent declaration source boundary".into());
            }
            data.truncate(data.len() - tail.len());
        }
        if info["sha256"] != digest(&data) {
            return Err(format!("historical source/evidence drift {host}"));
        }
        preserved[host] = info.clone();
    }
    let scope = sdd.join("schema-regex-literal-scope-full.json");
    let scope_bytes = fs::read(&scope).map_err(|e| e.to_string())?;
    if digest(&scope_bytes) != "aea199933a2274cfc507059a2777852d922b73a668af1482b1f838c499fa3ccf" {
        return Err("305 literal rows drift".into());
    }
    let scope: Value = serde_json::from_slice(&scope_bytes).map_err(|e| e.to_string())?;
    if scope["regex"]["results"]
        .as_array()
        .ok_or("literal results")?
        .len()
        != 305
    {
        return Err("literal retained count".into());
    }
    let mut files = Vec::new();
    for dir in [
        "crates/comandos-extensions/tests/fixtures/unicode14",
        "crates/comandos-extensions/examples/unicode14",
    ] {
        inventory_files(&root.join(dir), &mut files)?;
    }
    for file in [
        "crates/comandos-extensions/examples/generate_unicode14.rs",
        "crates/comandos-extensions/examples/audit_unicode14.rs",
        "crates/comandos-extensions/src/output_schema/python_regex/unicode14.rs",
        "crates/comandos-extensions/src/output_schema/python_regex/unicode14_tables.rs",
        "crates/comandos-extensions/src/output_schema/python_regex/UNICODE14-PROVENANCE.json",
        "crates/comandos-extensions/src/output_schema/python_regex/UNICODE14-NOTICES",
        "crates/comandos-extensions/src/output_schema/python_regex.rs",
        ".migration-build/target/debug/examples/audit_unicode14",
        ".migration-build/target/debug/examples/generate_unicode14",
        ".migration-build/target/debug/deps/comandos_extensions-e067645736155fb7",
        ".migration-build/target/release/comandos-extensions",
    ] {
        files.push(root.join(file));
    }
    for e in fs::read_dir(&sdd).map_err(|e| e.to_string())? {
        let p = e.map_err(|e| e.to_string())?.path();
        let name = p.file_name().unwrap().to_string_lossy();
        if name.starts_with("schema-unicode-")
            && !name.contains("freeze")
            && !name.ends_with("brief.md")
            && !name.contains("proposal")
            && !name.contains("controller-notes")
        {
            if p.is_dir() {
                inventory_files(&p, &mut files)?;
            } else {
                files.push(p);
            }
        }
    }
    files.sort();
    files.dedup();
    let mut hashes = json!({});
    for p in files {
        let data = fs::read(&p).map_err(|e| e.to_string())?;
        let canonical = fs::canonicalize(&p).map_err(|e| e.to_string())?;
        let rel = canonical
            .strip_prefix(fs::canonicalize(&root).unwrap())
            .map_err(|e| e.to_string())?
            .to_string_lossy();
        let host = format!("{HOST_ROOT}/{rel}");
        hashes[host] = json!({"sha256":digest(&data),"bytes":data.len()});
    }
    let manifest: Value = serde_json::from_slice(
        &fs::read(fixture().join("oracle_manifest.json")).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    let result = json!({"format":1,"approved_base":"527636949a301f09ebd47e1c3d3efbf927b7ae8a","checkpoint":"/home/someguy/codebase/0xJesus/ComandOS/.scratch/comandos-rust/checkpoints/20261003T101043922859Z","preserved_literal_files":preserved,"retained_historical":baseline["retained_historical"],"retained_305_literal":{"rows":305,"agreements":236,"unresolved":69,"sha256":digest(&scope_bytes),"source_only_preservation":true},"files":hashes,"oracle":manifest,"resources":{"retained_oracle":resources(&sdd.join("schema-unicode-capture-v2"))?,"retained_native":resources(&sdd.join("schema-unicode-native"))?,"native_combined":resources(&sdd.join("schema-unicode-native-combined"))?},"output_cap_combined":audit_protocol::OUTPUT_CAP,"payload_bytes":unicode14::payload_bytes(),"review_threshold_bytes":1.1f64*1024f64*1024f64,"remaining_first_party_python":{"file":format!("{HOST_ROOT}/crates/comandos-extensions/examples/unicode14/oracle_queries.rs"),"constant":"QUERY","scope":"temporary installed external measurement, not production or generator; removal remains migration obligation"},"runtime_private_and_unwired":true,"ownership_release":"source/Cargo/oracle/test ownership released only with committed delivery","self_hash":"external; file excludes itself to avoid circular digest"});
    let target = sdd.join("schema-unicode-frozen-combined.json");
    if target.exists() {
        return Err("final freeze already exists".into());
    }
    write_json(&target, &result)?;
    println!(
        "source/evidence hashes, retained complete literal classifications and resources frozen"
    );
    Ok(())
}
fn emit_output() -> Result<(), String> {
    let args: Vec<_> = std::env::args().skip(2).collect();
    if args.len() != 2 {
        return Err("two output sizes required".into());
    }
    let sizes: Vec<usize> = args
        .iter()
        .map(|s| s.parse().map_err(|_| "output size"))
        .collect::<Result<_, _>>()?;
    if sizes.iter().any(|n| *n > audit_protocol::OUTPUT_CAP) {
        return Err("test helper stream size".into());
    }
    io::stdout()
        .write_all(&vec![b'O'; sizes[0]])
        .map_err(|e| e.to_string())?;
    io::stdout().flush().map_err(|e| e.to_string())?;
    io::stderr()
        .write_all(&vec![b'E'; sizes[1]])
        .map_err(|e| e.to_string())?;
    io::stderr().flush().map_err(|e| e.to_string())?;
    Ok(())
}
fn main() {
    let result = match std::env::args().nth(1).as_deref() {
        Some("--capture") => capture(),
        Some("--capture-lookups") => capture_lookups(),
        Some("--native") => native(),
        Some("--compare-domain") => compare_domain(),
        Some("--compare-lookups") => compare_lookups(),
        Some("--freeze") => freeze(),
        Some("--emit-output") => emit_output(),
        _ => Err(
            "use --capture/--capture-lookups/--compare-domain/--compare-lookups/--native".into(),
        ),
    };
    if let Err(e) = result {
        eprintln!("{e}");
        std::process::exit(1);
    }
}
