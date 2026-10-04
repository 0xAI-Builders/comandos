use comandos_core::mcp_descriptions;
use serde_json::{Value, json};

#[test]
fn configuration_controls_whitespace_and_unicode_limit() {
    assert_eq!(
        mcp_descriptions::metadata("gmail", &json!("  hola\0\t\u{85} mundo\u{2028} !\u{7f}")),
        json!({"description":"hola mundo !", "descriptionSource":"configuration"})
    );
    let text = format!("{}tail", "🦀".repeat(600));
    assert_eq!(
        mcp_descriptions::metadata("gmail", &json!(text))["description"],
        "🦀".repeat(600)
    );
}

#[test]
fn catalog_casefold_and_underscore_keys() {
    let expected = json!({"description":"Busca y gestiona tareas de Jira, páginas de Confluence y componentes de Compass.", "descriptionSource":"catalog"});
    for name in ["atlassian", "ATLASSIAN", "atlaßian", "atlaſſian"] {
        assert_eq!(
            mcp_descriptions::metadata(name, &Value::Null),
            expected,
            "{name}"
        );
    }
    assert_eq!(
        mcp_descriptions::metadata("CHROME_BG", &Value::Null)["descriptionSource"],
        "catalog"
    );
}

#[test]
fn empty_or_nonstring_configuration_falls_back_without_inference() {
    for value in [
        Value::Null,
        json!(false),
        json!(42),
        json!(["description"]),
        json!({"description":"x"}),
        json!("\t\n\0 \u{1f}"),
    ] {
        assert_eq!(
            mcp_descriptions::metadata("gmail", &value)["descriptionSource"],
            "catalog"
        );
    }
    assert_eq!(
        mcp_descriptions::metadata("unknown-custom", &Value::Null),
        json!({"description":"", "descriptionSource":"unavailable"})
    );
    assert_eq!(
        mcp_descriptions::metadata(" gmail ", &Value::Null)["descriptionSource"],
        "unavailable"
    );
}

#[test]
fn preserved_python_reference_rows() {
    let fixture: Value =
        serde_json::from_str(include_str!("mcp_descriptions_fixture.json")).unwrap();
    for row in fixture["rows"].as_array().unwrap() {
        assert_eq!(
            mcp_descriptions::metadata(row["name"].as_str().unwrap(), &row["description"]),
            row["expected"],
            "{row}"
        );
    }
}
