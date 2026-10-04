use comandos_core::json::{parse_unique_value, parse_value};
use serde_json::json;

#[test]
fn reserved_number_map_keys_and_big_integers_remain_distinct() {
    for raw in [
        r#"{"$serde_json::private::Number":"123"}"#,
        r#"{"\u0024serde_json::private::Number":"123"}"#,
    ] {
        assert_eq!(
            parse_value(raw).unwrap(),
            json!({"$serde_json::private::Number":"123"})
        );
    }
    let raw = r#"{"number":340282366920938463463374607431768211456,"opaque":{"$serde_json::private::Number":"invalid-number","keep":true}}"#;
    let data = parse_value(raw).unwrap();
    assert_eq!(
        data["number"].to_string(),
        "340282366920938463463374607431768211456"
    );
    assert_eq!(
        data["opaque"]["$serde_json::private::Number"],
        "invalid-number"
    );
    assert_eq!(data["opaque"]["keep"], true);
    let raw = r#"{"$serde_json::private::RawValue":"[1,2]"}"#;
    assert_eq!(
        parse_value(raw).unwrap(),
        json!({"$serde_json::private::RawValue":"[1,2]"})
    );
}

#[test]
fn unique_objects_reject_duplicates_without_reinterpreting_numbers() {
    assert!(parse_unique_value(r#"{"nested":[{"key":1,"key":2}]}"#).is_err());
    let data = parse_unique_value(
        r#"{"number":1.0,"big":18446744073709551616,"$serde_json::private::Number":"literal"}"#,
    )
    .unwrap();
    assert_eq!(data["number"].to_string(), "1.0");
    assert_eq!(data["big"].to_string(), "18446744073709551616");
    assert_eq!(data["$serde_json::private::Number"], "literal");
    assert_eq!(parse_value(r#"{"key":1,"key":2}"#).unwrap()["key"], 2);
}

#[test]
fn nested_parser_rejects_invalid_json_and_excessive_depth() {
    for raw in ["1 trailing", "[1,]", "{", "\"\\ud800\""] {
        assert!(parse_value(raw).is_err());
    }
    let raw = format!(
        "{}{}{}",
        "[".repeat(150),
        r#"{"$serde_json::private::Number":"literal"}"#,
        "]".repeat(150)
    );
    assert!(parse_value(&raw).is_err());
}
