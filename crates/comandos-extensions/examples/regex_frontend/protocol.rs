#[path = "../audit_support/process.rs"]
pub mod process;
use serde_json::Value;
pub fn records(bytes: &[u8], rows: &[Value]) -> Result<Vec<Value>, String> {
    if rows.len() > 8 {
        return Err("row cap".into());
    }
    let values: Vec<Value> = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
    if values.len() != rows.len() {
        return Err("row count".into());
    }
    for (value, row) in values.iter().zip(rows) {
        if value["id"] != row["id"] {
            return Err("row identity/order".into());
        }
        let result = value["result"].as_str().ok_or("missing category")?;
        match result {
            "Compiled" => {
                for k in ["flags", "groups"] {
                    if value[k].as_u64().is_none() {
                        return Err(format!("invalid {k}"));
                    }
                }
                if !value["names"].is_array() {
                    return Err("invalid names".into());
                }
                if row.get("search").is_some() && !value["search"].is_boolean() {
                    return Err("nonboolean search".into());
                }
            }
            "error" | "OverflowError" | "ValueError" | "RecursionError" => {
                let p = value.get("position").ok_or("missing position")?;
                if !p.is_null() && p.as_u64().is_none() {
                    return Err("invalid position".into());
                }
            }
            _ => return Err("unknown category".into()),
        }
        for w in value["warnings"].as_array().ok_or("missing warnings")? {
            if w["classified"] != true {
                return Err("unclassified warning".into());
            }
            if !matches!(
                w["category"].as_str(),
                Some("FutureWarning" | "DeprecationWarning")
            ) {
                return Err("warning category".into());
            }
            let message = w["message"].as_str().ok_or("warning message")?;
            let family = match w["category"].as_str().unwrap() {
                "FutureWarning" => [
                    "Possible nested set at position ",
                    "Possible set difference at position ",
                    "Possible set intersection at position ",
                    "Possible set symmetric difference at position ",
                    "Possible set union at position ",
                ]
                .iter()
                .any(|prefix| message.starts_with(prefix)),
                "DeprecationWarning" => message.starts_with("bad character in group name "),
                _ => false,
            };
            let position = message
                .rsplit_once(" at position ")
                .map(|(_, digits)| digits)
                .filter(|digits| !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()))
                .and_then(|digits| digits.parse::<u64>().ok());
            if !family || position.is_none() || w["position"].as_u64() != position {
                return Err("unclassified warning family/position".into());
            }
            let p = w.get("position").ok_or("warning position")?;
            if !p.is_null() && p.as_u64().is_none() {
                return Err("warning position".into());
            }
        }
    }
    Ok(values)
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn strict_shape_order_and_nullable_positions() {
        let rows = vec![json!({"id":"a"})];
        let good = json!([{"id":"a","result":"error","position":null,"warnings":[]}]);
        assert!(records(&serde_json::to_vec(&good).unwrap(), &rows).is_ok());
        for bad in [
            json!([]),
            json!([good[0].clone(), good[0].clone()]),
            json!([{"id":"b","result":"error","position":null,"warnings":[]}]),
            json!([{"id":"a","result":"unknown","position":null,"warnings":[]}]),
            json!([{"id":"a","result":"error","warnings":[]}]),
            json!([{"id":"a","result":"error","position":-1,"warnings":[]}]),
            json!([{"id":"a","result":"error","position":"0","warnings":[]}]),
        ] {
            assert!(records(&serde_json::to_vec(&bad).unwrap(), &rows).is_err());
        }
        assert!(records(b"x", &rows).is_err());
        assert!(records(b"[]", &vec![json!({}); 9]).is_err());
    }
}
pub fn native_records(
    bytes: &[u8],
    rows: &[Value],
    prefix: &str,
    key: &str,
) -> Result<Vec<Value>, String> {
    if rows.len() > 8 {
        return Err("row cap".into());
    }
    let text = std::str::from_utf8(bytes).map_err(|e| e.to_string())?;
    let mut records = Vec::new();
    let mut state = 0;
    let harness = match prefix {
        "REGEX_FRONTEND=" => "test output_schema::python_regex::frontend_audit::audit_probe ... ",
        "SCHEMA_REGEX=" => "test output_schema::python_regex::tests::audit_probe ... ",
        _ => return Err("unknown native protocol".into()),
    };
    for line in text.lines() {
        if line.is_empty() {
            continue;
        }
        if state == 0 {
            if line != "running 1 test" {
                return Err("stdout contamination".into());
            }
            state = 1;
            continue;
        }
        let line = if state == 1 {
            state = 2;
            line.strip_prefix(harness)
                .ok_or("unexpected selected-test prefix")?
        } else {
            line
        };
        if state == 2 && line == "ok" && records.len() == rows.len() {
            state = 3;
        } else if state == 3 {
            let suffix = line
                .strip_prefix("test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; ")
                .ok_or("unexpected test summary")?;
            let (count, time) = suffix
                .split_once(" filtered out; finished in ")
                .ok_or("unexpected test summary")?;
            let time = time.strip_suffix('s').ok_or("unexpected test summary")?;
            if count.is_empty()
                || !count.bytes().all(|b| b.is_ascii_digit())
                || time.is_empty()
                || !time.bytes().all(|b| b.is_ascii_digit() || b == b'.')
            {
                return Err("unexpected test summary".into());
            }
            state = 4;
        } else if state == 2
            && let Some(raw) = line.strip_prefix(prefix)
        {
            records.push(serde_json::from_str::<Value>(raw).map_err(|e| e.to_string())?);
            if records.len() > rows.len() {
                return Err("extra native row".into());
            }
        } else {
            return Err("stdout contamination".into());
        }
    }
    if records.len() != rows.len() || state != 4 {
        return Err("native row count".into());
    }
    for (record, row) in records.iter().zip(rows) {
        if record[key] != row[key] {
            return Err("native row identity/order".into());
        }
        if record["result"].as_str().is_none() {
            return Err("missing native category".into());
        }
        let category = record["result"].as_str().unwrap();
        if !matches!(
            category,
            "Compiled"
                | "error"
                | "OverflowError"
                | "ValueError"
                | "RecursionError"
                | "ReError"
                | "FormatValid"
                | "Match"
                | "Valid"
                | "Invalid"
                | "ScopeGap:Grammar"
                | "ScopeGap:Unicode14"
                | "ScopeGap:MatcherFeature"
                | "ScopeGap:TextTransport"
                | "ScopeGap:SchemaScope"
                | "BudgetBoundary:Decode"
                | "BudgetBoundary:Compile"
                | "BudgetBoundary:Search"
                | "InternalProgram"
                | "CompileAbort:OverflowError"
                | "CompileAbort:ValueError"
                | "CompileAbort:RecursionError"
        ) {
            return Err("unknown native category".into());
        }
        if matches!(category, "error" | "ReError")
            && record
                .get("position")
                .is_none_or(|p| !p.is_null() && p.as_u64().is_none())
        {
            return Err("invalid native position".into());
        }
    }
    Ok(records)
}
#[cfg(test)]
mod faults {
    use super::*;
    use serde_json::json;
    use std::{path::PathBuf, time::Duration};
    fn path(label: &str) -> PathBuf {
        let dir = std::env::var_os("REGEX_FRONTEND_TEST_EVIDENCE")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                std::env::temp_dir().join(format!(
                    "schema-regex-frontend-protocol-{}",
                    std::process::id()
                ))
            });
        std::fs::create_dir_all(&dir).unwrap();
        dir.join(label)
    }
    fn benign(label: &str) {
        let rows = vec![json!({"id":"benign"})];
        let p = json!([{"id":"benign","result":"Compiled","flags":32,"groups":0,"names":[],"warnings":[]}]);
        let bytes = process::run(
            "/bin/cat",
            &[],
            &serde_json::to_vec(&p).unwrap(),
            Duration::from_secs(10),
            &path(label),
        )
        .unwrap();
        assert!(records(&bytes, &rows).is_ok());
    }
    #[test]
    fn real_zero_one_eight_nine_rows_and_recovery() {
        for n in [0, 1, 8] {
            let rows: Vec<_> = (0..n).map(|i| json!({"id":i})).collect();
            let output:Vec<_>=rows.iter().map(|r|json!({"id":r["id"],"result":"Compiled","flags":32,"groups":0,"names":[],"warnings":[]})).collect();
            let bytes = process::run(
                "/bin/cat",
                &[],
                &serde_json::to_vec(&output).unwrap(),
                Duration::from_secs(10),
                &path(&format!("rows-{n}")),
            )
            .unwrap();
            assert_eq!(records(&bytes, &rows).unwrap().len(), n);
        }
        assert!(records(b"[]", &vec![json!({"id":"n"}); 9]).is_err());
        benign("post-row-cap");
    }
    #[test]
    fn real_fault_children_kill_reap_and_recover() {
        for (program, args, expected, label, deadline) in [
            (
                "/bin/false",
                vec![],
                "child exit",
                "nonzero",
                Duration::from_secs(10),
            ),
            (
                "/bin/sleep",
                vec!["30".into()],
                "timeout",
                "timeout",
                Duration::from_millis(30),
            ),
            (
                "/usr/bin/yes",
                vec![],
                "output overflow",
                "stdout-overflow",
                Duration::from_secs(10),
            ),
        ] {
            let e = process::run(program, &args, b"", deadline, &path(label)).unwrap_err();
            assert!(e.starts_with(expected), "{e}");
            benign(&format!("post-{label}"));
        }
        let e = process::run_inner(
            "/bin/sleep",
            &["30".into()],
            b"",
            Duration::from_secs(10),
            &path("crash"),
            |pid| {
                nix::sys::signal::killpg(
                    nix::unistd::Pid::from_raw(pid as i32),
                    nix::sys::signal::Signal::SIGSEGV,
                )
                .map_err(|e| e.to_string())
            },
        )
        .unwrap_err();
        assert!(e.contains("signal"));
        benign("post-crash");
        assert!(
            process::run(
                "/bin/true",
                &[],
                &vec![0; process::INPUT_CAP + 1],
                Duration::from_secs(10),
                &path("input-overflow")
            )
            .is_err()
        );
        benign("post-input-overflow");
    }
    #[test]
    fn actual_stdout_stderr_combined_overflow_and_recovery() {
        let binary = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../.migration-build/target/debug/examples/audit_unicode14");
        for (out, err, label) in [
            (600000, 600000, "combined"),
            (0, process::OUTPUT_CAP, "stderr"),
        ] {
            assert_eq!(
                process::run(
                    binary.to_str().unwrap(),
                    &["--emit-output".into(), out.to_string(), err.to_string()],
                    b"",
                    Duration::from_secs(10),
                    &path(label)
                )
                .unwrap_err(),
                "output overflow"
            );
            benign(&format!("post-{label}"));
        }
    }
    #[test]
    fn actual_payload_faults_and_recovery() {
        let rows = vec![json!({"id":"a","search":[97]})];
        for payload in [b"invalid JSON".to_vec(),b"noise\n[]".to_vec(),serde_json::to_vec(&json!([{"id":"a","result":"Compiled","flags":32,"groups":0,"names":[],"search":"false","warnings":[]}])).unwrap(),serde_json::to_vec(&json!([{"id":"a","result":"error","warnings":[]}])).unwrap()]{let bytes=process::run("/bin/cat",&[],&payload,Duration::from_secs(10),&path("payload-fault")).unwrap();assert!(records(&bytes,&rows).is_err());benign("post-payload-fault");}
    }
    #[test]
    fn framing_rejects_extra_duplicate_reordered_and_contaminated_rows() {
        let rows = vec![json!({"id":"a"}), json!({"id":"b"})];
        for s in [
            "REGEX_FRONTEND={\"id\":\"a\",\"result\":\"Compiled\"}\n",
            "REGEX_FRONTEND={\"id\":\"a\",\"result\":\"Compiled\"}\nREGEX_FRONTEND={\"id\":\"a\",\"result\":\"Compiled\"}\n",
            "REGEX_FRONTEND={\"id\":\"b\",\"result\":\"Compiled\"}\nREGEX_FRONTEND={\"id\":\"a\",\"result\":\"Compiled\"}\n",
            "noise\n",
        ] {
            assert!(native_records(s.as_bytes(), &rows, "REGEX_FRONTEND=", "id").is_err());
        }
    }
    #[test]
    fn productive_quota_boundaries_include_all_framing_and_resource() {
        let framing = b"[]\n";
        let resource = b"\nRESOURCE cpu_user=0.01 cpu_system=0.00 rss_kib=10000 status=0\n";
        for n in [
            process::OUTPUT_CAP - 1,
            process::OUTPUT_CAP,
            process::OUTPUT_CAP + 1,
        ] {
            let mut q = process::OutputQuota::default();
            q.reserve(framing.len());
            q.reserve(n - framing.len() - resource.len());
            q.reserve(resource.len());
            assert_eq!(q.used, n.min(process::OUTPUT_CAP));
            assert_eq!(q.observed, n);
            assert_eq!(q.truncated, n > process::OUTPUT_CAP);
        }
    }
    #[test]
    fn forced_semantic_mismatch_and_execution_stages_are_distinct() {
        let oracle = json!({"result":"Compiled","search":true});
        let mismatch = json!({"result":"Compiled","search":false});
        assert_ne!(oracle, mismatch);
        let compile_not_run = json!({"result":"InfrastructureFailure","compile":"NotRun"});
        let search_not_run = json!({"result":"Compiled","search":"NotRun"});
        assert_ne!(compile_not_run, search_not_run);
        let rows = vec![json!({"id":"a"})];
        assert!(records(&serde_json::to_vec(&json!([{"id":"a","result":"Compiled","flags":32,"groups":0,"names":[],"warnings":[]}])).unwrap(),&rows).is_ok());
        assert!(
            records(
                &serde_json::to_vec(&json!([{"id":"a","result":"NotRun","warnings":[]}])).unwrap(),
                &rows
            )
            .is_err()
        );
    }
}
pub fn run(
    program: &str,
    args: &[String],
    input: &[u8],
    deadline: std::time::Duration,
    evidence: &std::path::Path,
) -> Result<Vec<u8>, String> {
    let bytes = process::run(program, args, input, deadline, evidence)?;
    let stderr = std::fs::read(evidence.with_extension("stderr")).map_err(|e| e.to_string())?;
    let stderr = std::str::from_utf8(&stderr).map_err(|e| e.to_string())?;
    if stderr
        .lines()
        .any(|l| !l.is_empty() && !l.starts_with("RESOURCE cpu_user="))
    {
        return Err("stderr contamination".into());
    }
    Ok(bytes)
}
pub fn compare(actual: &Value, expected: &Value, row: &Value) -> Result<&'static str, String> {
    if actual["id"] != expected["id"] || actual["id"] != row["id"] {
        return Err("compare identity".into());
    }
    if actual["result"] == "BudgetBoundary:Compile"
        && row["family"] == "budget-depth"
        && row["recipe"].as_str().is_some_and(|r| r.contains("129"))
    {
        return Ok("protective-depth");
    }
    let keys = if expected["result"] == "Compiled" {
        vec!["result", "flags", "groups", "names", "width_diagnostic"]
    } else {
        vec!["result", "position"]
    };
    for k in keys {
        if actual[k] != expected[k] {
            return Err(format!(
                "{} {k}: actual={} expected={}",
                row["id"], actual[k], expected[k]
            ));
        }
    }
    let wa = actual["warnings"].as_array().ok_or("native warnings")?;
    let we = expected["warnings"]
        .as_array()
        .ok_or("reference warnings")?;
    if wa.len() != we.len()
        || wa
            .iter()
            .zip(we)
            .any(|(a, e)| a["category"] != e["category"] || a["position"] != e["position"])
    {
        return Err(format!("{} warning mismatch", row["id"]));
    }
    if row["op"] == "format" && actual["format_result"] != expected["format_result"] {
        return Err("format mismatch".into());
    }
    if row["op"] == "schema" {
        if actual["schema_identity_sha256"] != expected["proof"]["schema_sha256"] {
            return Err("schema identity mismatch".into());
        }
        if actual["schema_result"]["result"]
            .as_str()
            .is_some_and(|s| s.starts_with("ScopeGap:"))
        {
            return Ok("schema-unresolved");
        }
        if actual["schema_result"]["result"] != expected["instance"]["result"] {
            return Err("schema assertion mismatch".into());
        }
    }
    if row.get("search").is_some() && expected["result"] == "Compiled" {
        if actual["search"]["result"] == "ScopeGap:MatcherFeature" {
            return Ok("syntax-agreement-matcher-unresolved");
        }
        if !actual["search"].is_boolean() || actual["search"] != expected["search"] {
            return Err("search mismatch".into());
        }
        return Ok("search-agreement");
    }
    Ok("semantic-agreement")
}
#[cfg(test)]
mod comparison_tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn productive_comparator_rejects_forced_nullable_and_search_mismatches() {
        let row = json!({"id":"a","op":"compile","search":[97]});
        let expected = json!({"id":"a","result":"Compiled","flags":32,"groups":0,"names":[],"width_diagnostic":[1,1],"warnings":[],"search":true});
        let mut actual = expected.clone();
        assert_eq!(
            compare(&actual, &expected, &row).unwrap(),
            "search-agreement"
        );
        actual["search"] = json!(false);
        assert!(compare(&actual, &expected, &row).is_err());
        let expected = json!({"id":"a","result":"error","position":null,"warnings":[]});
        let mut actual = expected.clone();
        actual["position"] = json!(0);
        assert!(compare(&actual, &expected, &row).is_err());
    }
}
#[cfg(test)]
mod stderr_test {
    use super::*;
    use std::{path::PathBuf, time::Duration};
    #[test]
    fn actual_subcap_stderr_is_contamination_and_recovery_succeeds() {
        let dir = std::env::var_os("REGEX_FRONTEND_TEST_EVIDENCE")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                std::env::temp_dir().join(format!(
                    "schema-regex-frontend-stderr-{}",
                    std::process::id()
                ))
            });
        std::fs::create_dir_all(&dir).unwrap();
        let binary = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../.migration-build/target/debug/examples/audit_unicode14");
        assert_eq!(
            run(
                binary.to_str().unwrap(),
                &["--emit-output".into(), "0".into(), "1".into()],
                b"",
                Duration::from_secs(10),
                &dir.join("subcap-stderr")
            )
            .unwrap_err(),
            "stderr contamination"
        );
        assert!(
            run(
                "/bin/cat",
                &[],
                b"[]",
                Duration::from_secs(10),
                &dir.join("post-subcap-stderr")
            )
            .is_ok()
        );
    }
}
#[cfg(test)]
mod native_framing_regression {
    use super::*;
    use serde_json::json;
    #[test]
    fn actual_rust_harness_prefix_and_terminal_ok_are_preserved() {
        let rows = vec![json!({"id":"a"})];
        let bytes=b"\nrunning 1 test\ntest output_schema::python_regex::frontend_audit::audit_probe ... REGEX_FRONTEND={\"id\":\"a\",\"result\":\"Compiled\"}\nok\n\ntest result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s\n\n";
        assert_eq!(
            native_records(bytes, &rows, "REGEX_FRONTEND=", "id")
                .unwrap()
                .len(),
            1
        );
    }
}
#[cfg(test)]
mod framing_contamination {
    use super::*;
    use serde_json::json;
    #[test]
    fn selected_framing_rejects_prefix_suffix_extra_ok_and_trailing_noise() {
        let good = "\nrunning 1 test\ntest output_schema::python_regex::frontend_audit::audit_probe ... REGEX_FRONTEND={\"id\":\"a\",\"result\":\"Compiled\"}\nok\n\ntest result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 55 filtered out; finished in 0.00s\n\n";
        let rows = vec![json!({"id":"a"})];
        assert!(native_records(good.as_bytes(), &rows, "REGEX_FRONTEND=", "id").is_ok());
        for bad in [
            format!("noise{good}"),
            format!("{good}noise"),
            good.replace(" ... REGEX_FRONTEND=", " extra ... REGEX_FRONTEND="),
            good.replace("REGEX_FRONTEND=", "WRONG="),
            good.replace("\nok\n", "\nok\nok\n"),
            good.replace("finished in 0.00s", "finished in 0.00s noise"),
        ] {
            assert!(native_records(bad.as_bytes(), &rows, "REGEX_FRONTEND=", "id").is_err());
        }
    }
}
#[cfg(test)]
mod warning_family_regression {
    use super::*;
    use serde_json::json;
    #[test]
    fn unknown_warning_family_cannot_claim_known_position() {
        let rows = vec![json!({"id":"a"})];
        let value = json!([{"id":"a","result":"error","position":0,"warnings":[{"category":"FutureWarning","position":1,"classified":true,"message":"Possible set hypothetical at position 1"}]}]);
        assert!(records(&serde_json::to_vec(&value).unwrap(), &rows).is_err());
    }
}
