use comandos_web_dom::api::{
    DEFAULT_ERROR, Failure, Node, error_message, from_tree, js_string_of, utf16_lossy,
};
use serde_json::{Value, json};

/// La regla como texto, para las pruebas que solo miran mensajes.
fn msg(f: Option<Failure>) -> Option<String> {
    f.map(|f| match f {
        Failure::Message(m) => m,
        Failure::ReadNull(k) => format!("<lee {k} de null>"),
    })
}

#[test]
fn api_error_rules_match_the_inline_api_function() {
    assert_eq!(
        msg(error_message("Bad", &json!({"error": "x"}), false, false)).as_deref(),
        Some("x")
    );
    assert_eq!(
        msg(error_message("Bad", &json!({"message": "m"}), false, false)).as_deref(),
        Some("m")
    );
    assert_eq!(error_message("", &json!({}), true, true), None);
    assert_eq!(
        msg(error_message("", &json!({"ok": false}), true, true)).as_deref(),
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
    let e = |j: Value| msg(error_message("Not Found", &j, false, false));
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
        msg(error_message("", &json!({}), false, false)).as_deref(),
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
    let p = |j: Value| msg(error_message("", &j, true, true));
    assert_eq!(p(json!({"ok": true})), None);
    assert_eq!(p(json!({"ok": 0})), None, "=== false, no falsy");
    assert_eq!(p(json!({"ok": null})), None);
    assert_eq!(p(json!({"ok": false, "error": "e"})).as_deref(), Some("e"));
    // Un HTTP fallido es error con o sin cuerpo y en GET o POST.
    assert_eq!(
        msg(error_message(
            "Bad Gateway",
            &json!({"ok": true}),
            true,
            false
        ))
        .as_deref(),
        Some("Bad Gateway")
    );
}

#[test]
fn non_object_bodies_have_no_fields() {
    // `r.json()` puede dar un primitivo o una lista: `j.error` vale undefined.
    for j in [json!(5), json!("texto"), json!([1, 2]), json!(true)] {
        assert_eq!(error_message("", &j, true, true), None, "{j}");
        assert_eq!(
            msg(error_message("S", &j, false, false)).as_deref(),
            Some("S"),
            "{j}"
        );
    }
}

#[test]
fn null_body_is_a_read_the_engine_must_do() {
    // `j.error` / `j.ok` con `j === null` lanza un TypeError cuyo texto es del
    // motor (Chromium y WebKitGTK difieren): la regla solo dice qué se lee.
    assert_eq!(
        error_message("", &Value::Null, false, false),
        Some(Failure::ReadNull("error"))
    );
    assert_eq!(
        error_message("S", &Value::Null, true, false),
        Some(Failure::ReadNull("error"))
    );
    assert_eq!(
        error_message("", &Value::Null, true, true),
        Some(Failure::ReadNull("ok"))
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

/// Árbol de prueba con la forma de un valor de JS ya analizado por el motor.
#[derive(Clone)]
enum J {
    Undef,
    Null,
    Num(&'static str),
    Str(String),
    Arr(Vec<J>),
    Obj(Vec<(&'static str, J)>),
    Fun,
}

fn walk(j: J) -> Value {
    from_tree(j, |j: &J| -> Result<Node<J>, String> {
        Ok(match j {
            J::Undef | J::Fun => Node::Skip,
            J::Null => Node::Null,
            J::Num(t) => Node::Number(t.parse().map_err(|_| "número".to_string())?),
            J::Str(s) => Node::Str(s.clone()),
            J::Arr(a) => Node::Array(a.clone()),
            J::Obj(o) => Node::Object(o.iter().map(|(k, v)| (k.to_string(), v.clone())).collect()),
        })
    })
    .unwrap()
}

#[test]
fn tree_conversion_keeps_order_numbers_and_json_stringify_rules() {
    let v = walk(J::Obj(vec![
        ("2", J::Num("1e+21")),
        ("b", J::Arr(vec![J::Undef, J::Fun, J::Null, J::Num("0.1")])),
        ("a", J::Undef),
        ("f", J::Fun),
        ("s", J::Str("x".into())),
    ]));
    // Como `JSON.stringify`: `undefined`/funciones se omiten en objetos y son
    // `null` en listas; el texto de cada número es el de JS.
    assert_eq!(
        serde_json::to_string(&v).unwrap(),
        r#"{"2":1e+21,"b":[null,null,null,0.1],"s":"x"}"#
    );
    assert_eq!(walk(J::Undef), Value::Null, "la raíz `undefined` es null");
    assert_eq!(walk(J::Arr(vec![])), json!([]));
    assert_eq!(walk(J::Obj(vec![])), json!({}));
}

#[test]
fn deep_nesting_beyond_serde_limit_converts() {
    // `serde_json::from_str` corta a 128 niveles; el motor no. Revisión B1, I2.
    let depth = 2000;
    let mut j = J::Num("7");
    for _ in 0..depth {
        j = J::Arr(vec![j]);
    }
    let mut v = &walk(j);
    let mut n = 0;
    while let Value::Array(a) = v {
        v = &a[0];
        n += 1;
    }
    assert_eq!((n, v.to_string()), (depth, "7".to_string()));
}

#[test]
fn errors_from_the_engine_are_returned_not_turned_into_null() {
    let r = from_tree(
        J::Arr(vec![J::Num("x")]),
        |j: &J| -> Result<Node<J>, String> {
            match j {
                J::Arr(a) => Ok(Node::Array(a.clone())),
                _ => Err("getter lanzó".into()),
            }
        },
    );
    assert_eq!(r, Err("getter lanzó".to_string()));
}

#[test]
fn lone_surrogates_become_u_fffd_unlike_js() {
    // Un `String` de Rust no puede guardar un sustituto suelto: pasa a U+FFFD
    // (el JS lo conservaría; se pinta igual pero el dato difiere, ver la doc
    // de `api`), nunca a `null` ni a error.
    assert_eq!(utf16_lossy(&[0x61, 0xD800, 0x62]), "a\u{FFFD}b");
    assert_eq!(utf16_lossy(&[0xD83C, 0xDF45]), "🍅");
    let v = walk(J::Obj(vec![("t", J::Str(utf16_lossy(&[0x78, 0xDC00])))]));
    assert_eq!(v["t"], "x\u{FFFD}");
}

#[test]
fn post_error_requires_literal_false_for_all_value_kinds() {
    for flag in [
        json!(null),
        json!(false),
        json!(true),
        json!(0),
        json!(-0.0),
        json!("false"),
        json!([]),
        json!([false]),
        json!({}),
        json!({"ok":false}),
    ] {
        let expected = if flag == Value::Bool(false) {
            Some(Failure::Message(DEFAULT_ERROR.into()))
        } else {
            None
        };
        assert_eq!(error_message("", &json!({"ok":flag}), true, true), expected);
    }
    assert_eq!(error_message("", &json!({}), true, true), None);
}
