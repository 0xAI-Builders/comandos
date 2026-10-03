//! Private development-only frontend fixture capture and differential.
#[path = "regex_frontend/cases.rs"]
mod cases;
#[path = "regex_frontend/oracle_queries.rs"]
mod oracle_queries;
#[path = "regex_frontend/protocol.rs"]
mod protocol;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
    time::Duration,
};
fn hash(b: &[u8]) -> String {
    format!("{:x}", Sha256::digest(b))
}
fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/regex_frontend")
}
fn write(p: &Path, v: &Value) {
    fs::write(p, serde_json::to_vec_pretty(v).unwrap()).unwrap();
}
fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(1)
    }
}
fn run() -> Result<(), String> {
    let args: Vec<String> = std::env::args().collect();
    let mode = args.get(1).map(String::as_str).unwrap_or("");
    let dir = PathBuf::from(args.get(2).ok_or("evidence directory required")?);
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    match mode {
        "generate" => {
            let rows = cases::generate();
            write(&dir.join("cases.json"), &json!(rows));
            println!("{} cases", rows.len());
        }
        "capture" => {
            if fixture().join("oracle.jsonl").exists() {
                return Err("oracle already frozen; refusing overwrite".into());
            }
            let rows: Vec<Value> = serde_json::from_slice(
                &fs::read(fixture().join("cases.json")).map_err(|e| e.to_string())?,
            )
            .map_err(|e| e.to_string())?;
            let identity = protocol::run(
                "/venv/bin/python",
                &["-c".into(), oracle_queries::QUERY.into()],
                b"{\"identity\":true}",
                Duration::from_secs(10),
                &dir.join("identity"),
            )?;
            let identity: Value = serde_json::from_slice(&identity).map_err(|e| e.to_string())?;
            if !identity["version"]
                .as_str()
                .is_some_and(|s| s.starts_with("3.11.15"))
                || identity["unicode"] != "14.0.0"
                || identity["digits"] != 4300
                || identity["magic"] != 20220615
                || identity["codesize"] != 4
                || identity["maxrepeat"] != 4294967295u64
                || identity["maxgroups"] != 1073741823
            {
                return Err("identity drift".into());
            }
            for (name, expected) in [
                (
                    "_parser.py",
                    "4748e39c77d6dc14f81af80e68a62ad99031a8182d5e0b219a6666d0cfb1626f",
                ),
                (
                    "_compiler.py",
                    "c05067f8bfa4c13cbbf1eedc4d5cafc9b621bcb6ebc5771ba0518a18095af15a",
                ),
                (
                    "_constants.py",
                    "3e4463dc8ba87c4a3563be46d2d593e82ca9a0fb91768cfe5a07554ed3de82c5",
                ),
            ] {
                if !identity["files"]
                    .as_object()
                    .ok_or("files")?
                    .iter()
                    .any(|(p, h)| p.ends_with(name) && h == expected)
                {
                    return Err(format!("source drift {name}"));
                }
            }
            let old_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../.superpowers/sdd/schema-regex-frontend-capture-v1");
            let old_rows: Vec<Value> =
                serde_json::from_slice(&fs::read(old_dir.join("cases.json")).unwrap()).unwrap();
            if rows.len() < old_rows.len()
                || rows.iter().zip(&old_rows).any(|(a, b)| {
                    [
                        "id",
                        "pattern",
                        "op",
                        "search",
                        "value",
                        "schema_json",
                        "content_json",
                        "draft",
                    ]
                    .iter()
                    .any(|k| a[*k] != b[*k])
                })
            {
                return Err("frozen prefix drift".into());
            }
            let mut out = fs::read(old_dir.join("oracle.jsonl")).unwrap();
            for (i, batch) in rows[old_rows.len()..].chunks(8).enumerate() {
                let bytes = protocol::run(
                    "/venv/bin/python",
                    &["-c".into(), oracle_queries::QUERY.into()],
                    &serde_json::to_vec(&json!({"rows":batch})).unwrap(),
                    Duration::from_secs(10),
                    &dir.join(format!("batch-{i:03}")),
                )?;
                for row in protocol::records(&bytes, batch)? {
                    out.extend(serde_json::to_vec(&row).unwrap());
                    out.push(b'\n');
                }
            }
            let path = fixture().join("oracle.jsonl");
            fs::write(&path, &out).map_err(|e| e.to_string())?;
            write(
                &fixture().join("manifest.json"),
                &json!({"version":1,"rows":rows.len(),"families":cases::FAMILIES,"identity":identity,"cases_sha256":hash(&fs::read(fixture().join("cases.json")).unwrap()),"oracle_sha256":hash(&out),"protocol":{"input":8388608,"output_combined":1048576,"rows":8,"address_space":402653184,"cpu_soft":2,"cpu_hard":3,"wall_ms":10000},"first_party_python_debt":"regex_frontend/oracle_queries.rs QUERY","complete_capture":true}),
            );
            println!("captured {} rows", rows.len());
        }
        "native" => {
            let binary = args.get(3).ok_or("private test binary required")?;
            let rows: Vec<Value> =
                serde_json::from_slice(&fs::read(fixture().join("cases.json")).unwrap()).unwrap();
            let oracle: Vec<Value> = fs::read_to_string(fixture().join("oracle.jsonl"))
                .unwrap()
                .lines()
                .map(|l| serde_json::from_str(l).unwrap())
                .collect();
            let mut results = Vec::new();
            for (i, batch) in rows.chunks(8).enumerate() {
                let bytes = protocol::run(
                    binary,
                    &[
                        "--ignored".into(),
                        "--exact".into(),
                        "output_schema::python_regex::frontend_audit::audit_probe".into(),
                        "--nocapture".into(),
                        "--test-threads=1".into(),
                    ],
                    &serde_json::to_vec(batch).unwrap(),
                    Duration::from_secs(10),
                    &dir.join(format!("native-{i:03}")),
                )?;
                for (j, mut actual) in
                    protocol::native_records(&bytes, batch, "REGEX_FRONTEND=", "id")?
                        .into_iter()
                        .enumerate()
                {
                    actual["disposition"] =
                        json!(protocol::compare(&actual, &oracle[i * 8 + j], &batch[j])?);
                    results.push(actual);
                }
            }
            write(&dir.join("native-results.json"), &json!(results));
            println!("native {} rows", results.len());
        }
        "literal" => {
            let binary = args.get(3).ok_or("private test binary required")?;
            let mut rows: Vec<Value> = serde_json::from_slice(
                &fs::read(
                    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                        .join("tests/output_schema_regex_cases.json"),
                )
                .unwrap(),
            )
            .unwrap();
            for row in &mut rows {
                if row["op"] == "schema" {
                    row["oracle_checked"] = json!(true);
                }
            }
            let historical: Value = serde_json::from_slice(
                &fs::read(
                    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                        .join("../../.superpowers/sdd/schema-regex-literal-scope-full.json"),
                )
                .unwrap(),
            )
            .unwrap();
            let frozen = historical["regex"]["results"]
                .as_array()
                .ok_or("historical results")?;
            let mut results = Vec::new();
            let mut reused = 0;
            for (i, batch) in rows.chunks(8).enumerate() {
                let eligible: Vec<Value> = batch
                    .iter()
                    .filter(|r| native_selected(r, frozen))
                    .cloned()
                    .collect();
                let old = args
                    .get(4)
                    .map(|p| PathBuf::from(p).join(format!("literal-{i:03}")));
                let bytes = if let Some(old) = old.filter(|p| p.with_extension("json").exists()) {
                    let meta: Value =
                        serde_json::from_slice(&fs::read(old.with_extension("json")).unwrap())
                            .unwrap();
                    if meta["status"] == "exit status: 0"
                        && meta["reaped"] == true
                        && meta["failure"].is_null()
                        && eligible.len() == batch.len()
                    {
                        let b =
                            fs::read(old.with_extension("stdout")).map_err(|e| e.to_string())?;
                        protocol::native_records(&b, &eligible, "SCHEMA_REGEX=", "name")?;
                        reused += eligible.len();
                        b
                    } else {
                        Vec::new()
                    }
                } else {
                    Vec::new()
                };
                let bytes = if bytes.is_empty() {
                    protocol::run(
                        binary,
                        &[
                            "--ignored".into(),
                            "--exact".into(),
                            "output_schema::python_regex::tests::audit_probe".into(),
                            "--nocapture".into(),
                            "--test-threads=1".into(),
                        ],
                        &serde_json::to_vec(&eligible).unwrap(),
                        Duration::from_secs(10),
                        &dir.join(format!("literal-{i:03}")),
                    )?
                } else {
                    bytes
                };
                let actual = protocol::native_records(&bytes, &eligible, "SCHEMA_REGEX=", "name")?;
                for row in batch {
                    if let Some(r) = actual.iter().find(|r| r["name"] == row["name"]) {
                        results.push(r.clone());
                    } else {
                        let old = frozen.iter().find(|f| f["name"] == row["name"]).unwrap();
                        let mut not_run = old["native"].clone();
                        not_run["name"] = row["name"].clone();
                        not_run["retained_prerequisite"] = json!(true);
                        results.push(not_run);
                    }
                }
            }
            write(&dir.join("literal-results.json"), &json!(results));
            println!("literal {} rows; reused {reused}", results.len());
        }
        "overlay" => overlay(&dir)?,
        "verify-fixtures" => {
            let rows: Vec<Value> =
                serde_json::from_slice(&fs::read(fixture().join("cases.json")).unwrap()).unwrap();
            let oracle: Vec<Value> = fs::read_to_string(fixture().join("oracle.jsonl"))
                .unwrap()
                .lines()
                .map(|l| serde_json::from_str(l).unwrap())
                .collect();
            if rows.len() != oracle.len() {
                return Err("fixture coverage".into());
            }
            for (batch, expected) in rows.chunks(8).zip(oracle.chunks(8)) {
                protocol::records(&serde_json::to_vec(expected).unwrap(), batch)?;
            }
            println!("{} frozen rows satisfy final strict protocol", rows.len());
        }
        "prove-retention" => {
            let binary = args.get(3).ok_or("exact test binary required")?;
            let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
            let raw = fs::read(root.join("tests/output_schema_regex_cases.json")).unwrap();
            if hash(&raw) != "cda31e01c5e82427f738f1cc0b0fbe9b0df7e177e7c3e48f30f18a62446533f0" {
                return Err("historical raw fixture drift".into());
            }
            let frozen_hash=fs::read_to_string(root.join("../../.superpowers/sdd/schema-regex-frontend-native-final/native-binary-before.sha256")).unwrap();
            let binary_hash = hash(&fs::read(binary).map_err(|e| e.to_string())?);
            if frozen_hash.split_whitespace().next() != Some(binary_hash.as_str()) {
                return Err("native binary drift".into());
            }
            let mut rows: Vec<Value> = serde_json::from_slice(&raw).unwrap();
            for r in &mut rows {
                if r["op"] == "schema" {
                    r["oracle_checked"] = json!(true);
                }
            }
            let mut proof = Vec::new();
            for (i, batch) in rows[..232].chunks(8).enumerate() {
                let base = dir.join(format!("literal-{i:03}"));
                let meta: Value =
                    serde_json::from_slice(&fs::read(base.with_extension("json")).unwrap())
                        .unwrap();
                let input = serde_json::to_vec(batch).unwrap();
                let stdout = fs::read(base.with_extension("stdout")).unwrap();
                let stderr = fs::read(base.with_extension("stderr")).unwrap();
                if meta["program"] != binary.as_str()
                    || meta["input_bytes"] != input.len()
                    || meta["status"] != "exit status: 0"
                    || meta["reaped"] != true
                    || !meta["failure"].is_null()
                    || meta["caps"]["as"] != 402653184
                    || meta["caps"]["cpu_soft"] != 2
                    || meta["caps"]["cpu_hard"] != 3
                    || meta["caps"]["deadline_ms"] != 10000
                    || meta["caps"]["output_combined"] != 1048576
                    || meta["caps"]["input"] != 8388608
                    || meta["args"]
                        != json!([
                            "--ignored",
                            "--exact",
                            "output_schema::python_regex::tests::audit_probe",
                            "--nocapture",
                            "--test-threads=1"
                        ])
                {
                    return Err(format!("retained batch binding {i}"));
                }
                protocol::native_records(&stdout, batch, "SCHEMA_REGEX=", "name")?;
                proof.push(json!({"batch":i,"rows":8,"input_sha256":hash(&input),"stdout_sha256":hash(&stdout),"stderr_sha256":hash(&stderr),"metadata_sha256":hash(&serde_json::to_vec(&meta).unwrap()),"native_binary_sha256":binary_hash,"raw_fixture_sha256":hash(&raw),"complete_successful_batch":true}));
            }
            write(&dir.join("retention-proof.json"), &json!(proof));
            println!("232 complete successful rows bound exactly");
        }
        _ => return Err("expected generate/capture/native/literal/overlay".into()),
    }
    Ok(())
}
fn overlay(dir: &Path) -> Result<(), String> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let raw =
        fs::read(root.join("tests/output_schema_regex_cases.json")).map_err(|e| e.to_string())?;
    let rows: Vec<Value> = serde_json::from_slice(&raw).map_err(|e| e.to_string())?;
    let historical: Value = serde_json::from_slice(
        &fs::read(root.join("../../.superpowers/sdd/schema-regex-literal-scope-full.json"))
            .unwrap(),
    )
    .unwrap();
    let prior = historical["regex"]["results"]
        .as_array()
        .ok_or("historical rows")?;
    let native: Vec<Value> =
        serde_json::from_slice(&fs::read(dir.join("literal-results.json")).unwrap()).unwrap();
    if rows.len() != 305 || native.len() != 305 {
        return Err("literal coverage".into());
    }
    let mut overlay = Vec::new();
    for (row, actual) in rows.iter().zip(&native) {
        if row["name"] != actual["name"] {
            return Err("literal sequence".into());
        }
        let old = prior
            .iter()
            .find(|p| p["name"] == row["name"])
            .ok_or("old identity")?;
        let result = actual["result"].as_str().ok_or("literal result")?;
        let expected = match row["op"].as_str().unwrap() {
            "format" => &row["oracle"],
            "schema" => &row["oracle"]["instance"],
            "search" => {
                if row["oracle"]["compile"]["result"] == "Compiled" {
                    &row["oracle"]["search"]
                } else {
                    &row["oracle"]["compile"]
                }
            }
            "joined" => &old["native"],
            _ => return Err("unknown literal operation".into()),
        };
        let (disposition, unresolved) = if result.starts_with("not-run:") {
            if actual["result"] != old["native"]["result"] || old["unresolved"] != true {
                return Err("changed prerequisite".into());
            }
            ("retained-prerequisite", true)
        } else if result.starts_with("BudgetBoundary:") {
            if actual["result"] != old["native"]["result"]
                || actual["position"] != old["native"]["position"]
            {
                return Err("protective disposition requires controller review".into());
            }
            ("protective-preserved", true)
        } else if result == "ScopeGap:MatcherFeature" {
            ("matcher-unresolved", true)
        } else if result.starts_with("ScopeGap:") {
            ("scope-transport-unresolved", true)
        } else {
            for k in ["result", "value", "position"] {
                if actual[k] != expected[k] {
                    return Err(format!(
                        "{} {k} semantic mismatch actual={} expected={}",
                        row["name"], actual[k], expected[k]
                    ));
                }
            }
            (
                if row["op"] == "format" {
                    "format-semantic-agreement"
                } else if result == "Match" {
                    "search-agreement"
                } else if row["op"] == "schema" {
                    "schema-assertion-agreement"
                } else {
                    "error-semantic-agreement"
                },
                false,
            )
        };
        if old["unresolved"] == false && unresolved {
            return Err(format!("previous agreement regressed {}", row["name"]));
        }
        let mut input = row.clone();
        input.as_object_mut().unwrap().remove("oracle");
        input.as_object_mut().unwrap().remove("disposition");
        overlay.push(json!({"id":row["name"],"raw_fixture_file_sha256":hash(&raw),"input_encoding":"serde_json compact projection retaining exact codepoints and original schema/content string bytes; whole-file hash binds original raw JSON bytes","raw_input_sha256":hash(&serde_json::to_vec(&input).unwrap()),"oracle_sha256":hash(&serde_json::to_vec(&row["oracle"]).unwrap()),"previous_disposition":row["disposition"],"previous_native":old["native"],"previous_unresolved":old["unresolved"],"new_disposition":disposition,"unresolved":unresolved,"native":actual,"expected":expected,"reason":disposition,"evidence":"final bounded native capture, retained232complete successful rows plus57new eligible rows;16original prerequisites retained"}));
    }
    let agreements = overlay.iter().filter(|r| r["unresolved"] == false).count();
    write(
        &fixture().join("literal_dispositions.json"),
        &json!({"version":1,"base":"674c3b1431b03423c3de72fb3b4e73b7304c4709","raw_fixture_sha256":hash(&raw),"rows":305,"executed":289,"retained_prerequisites":16,"agreements":agreements,"unresolved":305-agreements,"overlay":overlay}),
    );
    println!(
        "literal overlay {agreements} agreements {} unresolved",
        305 - agreements
    );
    Ok(())
}
fn native_selected(row: &Value, frozen: &[Value]) -> bool {
    let prior = frozen
        .iter()
        .find(|p| p["name"] == row["name"])
        .expect("frozen unique ID required");
    !prior["native"]["result"]
        .as_str()
        .unwrap()
        .starts_with("not-run:")
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn generated_inputs_are_unique_and_roundtrip() {
        let rows = cases::generate();
        let bytes = serde_json::to_vec(&rows).unwrap();
        let round: Vec<Value> = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(rows, round);
        let maintained: Vec<Value> =
            serde_json::from_slice(&fs::read(fixture().join("cases.json")).unwrap()).unwrap();
        assert_eq!(
            rows, maintained,
            "final structural recipes match frozen maintained inputs"
        );
        let mut ids = std::collections::HashSet::new();
        for r in rows {
            assert!(ids.insert(r["id"].clone()));
        }
    }
}
#[cfg(test)]
mod literal_selector_regression {
    use super::*;
    #[test]
    fn original_prerequisites_never_enter_native_probe_batch() {
        let rows: Vec<Value> = serde_json::from_slice(
            &fs::read(
                PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .join("tests/output_schema_regex_cases.json"),
            )
            .unwrap(),
        )
        .unwrap();
        let historical: Value = serde_json::from_slice(
            &fs::read(
                PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .join("../../.superpowers/sdd/schema-regex-literal-scope-full.json"),
            )
            .unwrap(),
        )
        .unwrap();
        let frozen = historical["regex"]["results"].as_array().unwrap();
        let selected: Vec<_> = rows.iter().filter(|r| native_selected(r, frozen)).collect();
        assert_eq!(selected.len(), 289);
        let omitted: Vec<_> = rows
            .iter()
            .filter(|r| !native_selected(r, frozen))
            .collect();
        assert_eq!(omitted.len(), 16);
        assert_eq!(omitted.iter().filter(|r| r["op"] == "schema").count(), 15);
        assert_eq!(omitted.iter().filter(|r| r["op"] == "joined").count(), 1);
        assert!(
            !selected
                .iter()
                .any(|r| r["name"] == "schema/draft4/invalid")
        );
    }
}
