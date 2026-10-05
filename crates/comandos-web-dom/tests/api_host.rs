use comandos_web_dom::api::{DEFAULT_ERROR, error_message, js_string_of};
use serde_json::{Value, json};

#[test]
fn api_error_rules_match_the_inline_api_function() {
    assert_eq!(
        error_message("Bad", &json!({"error": "x"}), false, false).as_deref(),
        Some("x")
    );
    assert_eq!(
        error_message("Bad", &json!({"message": "m"}), false, false).as_deref(),
        Some("m")
    );
    assert_eq!(error_message("", &json!({}), true, true), None);
    assert_eq!(
        error_message("", &json!({"ok": false}), true, true).as_deref(),
        Some("La acción no se completó")
    );
    assert_eq!(
        error_message("", &json!({"ok": false}), false, true),
        None,
        "GET no mira ok:false"
    );
}

#[test]
fn error_chain_follows_js_truthiness() {
    // `j.error || j.message || r.statusText || "La acción no se completó"`.
    let e = |j: Value| error_message("Not Found", &j, false, false);
    assert_eq!(
        e(json!({"error": "", "message": "m"})).as_deref(),
        Some("m")
    );
    assert_eq!(
        e(json!({"error": 0, "message": ""})).as_deref(),
        Some("Not Found")
    );
    assert_eq!(e(json!({"error": false})).as_deref(), Some("Not Found"));
    assert_eq!(e(json!({"error": null})).as_deref(), Some("Not Found"));
    assert_eq!(e(json!({})).as_deref(), Some("Not Found"));
    assert_eq!(
        error_message("", &json!({}), false, false).as_deref(),
        Some(DEFAULT_ERROR)
    );
    // Valores verdaderos que no son cadena: `new Error(v)` hace `String(v)`.
    assert_eq!(e(json!({"error": 5})).as_deref(), Some("5"));
    assert_eq!(e(json!({"error": true})).as_deref(), Some("true"));
    assert_eq!(e(json!({"error": {}})).as_deref(), Some("[object Object]"));
    assert_eq!(
        e(json!({"error": []})).as_deref(),
        Some(""),
        "[] es verdadero"
    );
    assert_eq!(
        e(json!({"error": [1, null, "a", [2, 3]]})).as_deref(),
        Some("1,,a,2,3")
    );
}

#[test]
fn post_checks_ok_false_only_strictly() {
    let p = |j: Value| error_message("", &j, true, true);
    assert_eq!(p(json!({"ok": true})), None);
    assert_eq!(p(json!({"ok": 0})), None, "=== false, no falsy");
    assert_eq!(p(json!({"ok": null})), None);
    assert_eq!(p(json!({"ok": false, "error": "e"})).as_deref(), Some("e"));
    // Un HTTP fallido es error con o sin cuerpo y en GET o POST.
    assert_eq!(
        error_message("Bad Gateway", &json!({"ok": true}), true, false).as_deref(),
        Some("Bad Gateway")
    );
}

#[test]
fn non_object_bodies_have_no_fields() {
    // `r.json()` puede dar un primitivo o una lista: `j.error` vale undefined.
    for j in [json!(5), json!("texto"), json!([1, 2]), json!(true)] {
        assert_eq!(error_message("", &j, true, true), None, "{j}");
        assert_eq!(
            error_message("S", &j, false, false).as_deref(),
            Some("S"),
            "{j}"
        );
    }
}

#[test]
fn null_body_reproduces_the_type_error() {
    // `j.error` / `j.ok` con `j === null` lanza un TypeError; texto de Chromium.
    assert_eq!(
        error_message("", &Value::Null, false, false).as_deref(),
        Some("Cannot read properties of null (reading 'error')")
    );
    assert_eq!(
        error_message("", &Value::Null, true, true).as_deref(),
        Some("Cannot read properties of null (reading 'ok')")
    );
    assert_eq!(error_message("", &Value::Null, false, true), None);
}

#[test]
fn numbers_keep_their_js_text() {
    // `arbitrary_precision`: el texto del número es el que escribió
    // `JSON.stringify`, que para números finitos es `String(n)`.
    let v: Value = serde_json::from_str(r#"{"error": 1e+21, "b": 0.10}"#).unwrap();
    assert_eq!(js_string_of(&v["error"]), "1e+21");
    assert_eq!(
        v["b"].to_string(),
        "0.10",
        "sin arbitrary_precision sería 0.1"
    );
}

#[test]
fn preserve_order_keeps_js_key_order() {
    let v: Value = serde_json::from_str(r#"{"1":0,"b":0,"a":0}"#).unwrap();
    let keys: Vec<&str> = v.as_object().unwrap().keys().map(String::as_str).collect();
    assert_eq!(keys, ["1", "b", "a"]);
}

#[test]
fn post_is_get_when_the_body_is_falsy() {
    use comandos_web_dom::api::js_truthy;
    for v in [json!(null), json!(false), json!(0), json!(""), json!(0.0)] {
        assert!(!js_truthy(&v), "{v}");
    }
    for v in [json!({}), json!([]), json!("0"), json!(1), json!(true)] {
        assert!(js_truthy(&v), "{v}");
    }
    // Números por su texto JSON, como los deja `JSON.stringify`.
    let num = |t: &str| serde_json::from_str::<Value>(t).unwrap();
    for t in ["0", "-0", "0.0", "0.000", "0e5", "-0.0E-3"] {
        assert!(!js_truthy(&num(t)), "{t}");
    }
    for t in ["1", "-1", "0.5", "1e-7", "10", "0.01", "5e0"] {
        assert!(js_truthy(&num(t)), "{t}");
    }
}
