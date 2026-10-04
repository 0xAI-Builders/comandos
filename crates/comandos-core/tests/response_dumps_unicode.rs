//! `json.dumps(x, ensure_ascii=False)`: orden de inserción, separadores por defecto.
use comandos_core::json::{response_dumps_unicode, workspace_loads};

#[test]
fn keeps_insertion_order_and_utf8() {
    let value =
        workspace_loads(r#"{"ts": 1.0, "k": "clic ñ", "d": 3, "z": [true, null], "nan": NaN}"#)
            .unwrap();
    assert_eq!(
        response_dumps_unicode(&value).unwrap(),
        r#"{"ts": 1.0, "k": "clic ñ", "d": 3, "z": [true, null], "nan": NaN}"#
    );
}

#[test]
fn escapes_controls_like_python() {
    let value = workspace_loads(r#"{"s": "a\u0001\n\"b"}"#).unwrap();
    assert_eq!(
        response_dumps_unicode(&value).unwrap(),
        r#"{"s": "a\u0001\n\"b"}"#
    );
}
