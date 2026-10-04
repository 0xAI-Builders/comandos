use comandos_core::{allocation as a, json::workspace_loads};
use serde_json::{Value, json};
#[test]
fn duration_rounds_minutes_hours_and_days_with_python_ties() {
    for (n, want) in [
        (90., "2 min"),
        (150., "2 min"),
        (4500., "1 h"),
        (9000., "2 h"),
        (165600., "1d 22h"),
        (-1., "0 min"),
    ] {
        assert_eq!(a::fmt_duration(&json!(n)).unwrap(), want);
    }
}
#[test]
fn quota_projection_preserves_individual_rows_and_bilingual_copy() {
    let rows = vec![
        json!({"provider":"claude","account":"main","window":"7d","percent":63.,"resets_at":1800000000.+85.5*3600.,"extra":{"keep":1}}),
        json!({"provider":"claude","account":"work","window":"7d","percent":0,"resets_at":1800000000.+3600.}),
    ];
    let original = rows.clone();
    let es = a::enrich_limits(&rows, 1800000000., "es").unwrap();
    let en = a::enrich_limits(&rows, 1800000000., "en").unwrap();
    assert_eq!(rows, original);
    assert_eq!(es.len(), 2);
    assert_eq!(es[0]["extra"], rows[0]["extra"]);
    assert_eq!(es[0]["reachesReset"], false);
    assert!(
        es[0]["verdict"]
            .as_str()
            .unwrap()
            .starts_with("se acaba en ")
    );
    assert!(
        en[0]["verdict"]
            .as_str()
            .unwrap()
            .starts_with("runs out in ")
    );
    assert_eq!(es[1]["verdict"], "llega al reset");
    assert_eq!(en[1]["verdict"], "lasts to reset");
}
#[test]
fn early_exits_preserve_bad_unused_percent_but_validate_reset() {
    let out = a::enrich_limits(
        &[
            json!({"window":"bad","percent":"broken","resets_at":0}),
            json!({"window":"5h","percent":null,"resets_at":123}),
        ],
        100.,
        "en",
    )
    .unwrap();
    assert_eq!(out[0]["windowSeconds"], Value::Null);
    assert_eq!(out[1]["windowSeconds"], 18000);
    assert_eq!(out[0]["burn"], Value::Null);
    assert!(a::enrich_limits(&[json!({"window":"bad","resets_at":"broken"})], 100., "es").is_err());
}
#[test]
fn quota_fixed_source_reference_numeric_cases() {
    let fixture = workspace_loads(include_str!("allocation_fixture.json")).unwrap();
    for row in fixture.as_array().unwrap() {
        let result = if row["kind"] == "duration" {
            a::fmt_duration(&row["input"]).map(Value::String)
        } else {
            a::enrich_limits(
                row["limits"].as_array().unwrap(),
                row["now"].as_f64().unwrap(),
                row["lang"].as_str().unwrap(),
            )
            .map(Value::Array)
        };
        if let Some(error) = row["error"].as_str() {
            assert_eq!(result.unwrap_err().kind, error, "{row}");
        } else {
            assert_json_close(&result.unwrap(), &row["expected"], "root");
        }
    }
}
fn assert_json_close(actual: &Value, expected: &Value, path: &str) {
    match (actual, expected) {
        (Value::Number(a), Value::Number(b)) => {
            if a.as_str() == b.as_str() {
                return;
            }
            if !a.as_str().contains(['.', 'e', 'E']) && !b.as_str().contains(['.', 'e', 'E']) {
                assert_eq!(a.as_str(), b.as_str(), "{path}");
                return;
            }
            let x = a.as_str().parse::<f64>().unwrap();
            let y = b.as_str().parse::<f64>().unwrap();
            assert!(
                (x.is_nan() && y.is_nan())
                    || x == y
                    || (x.is_finite() && y.is_finite() && (x - y).abs() <= 1e-12 * y.abs().max(1.)),
                "{path}: {actual} != {expected}"
            );
        }
        (Value::Array(a), Value::Array(b)) => {
            assert_eq!(a.len(), b.len(), "{path}");
            for (i, (x, y)) in a.iter().zip(b).enumerate() {
                assert_json_close(x, y, &format!("{path}/{i}"));
            }
        }
        (Value::Object(a), Value::Object(b)) => {
            assert_eq!(a.len(), b.len(), "{path}");
            for (k, v) in b {
                assert_json_close(a.get(k).unwrap(), v, &format!("{path}/{k}"));
            }
        }
        _ => assert_eq!(actual, expected, "{path}"),
    }
}
