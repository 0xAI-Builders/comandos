//! Normalización y comparación del arnés `parity`, sin servidores.
#![allow(dead_code)]

#[path = "../src/parity.rs"]
mod parity;

use parity::{Expect, Outcome, Resp, compare, normalize, parse_response};
use serde_json::json;

fn resp(status: u16, headers: &[(&str, &str)], body: &str) -> Resp {
    Resp {
        status,
        headers: headers
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect(),
        body: body.as_bytes().to_vec(),
    }
}

fn vol(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

#[test]
fn normalize_replaces_plain_pointer() {
    let mut v = json!({"generated_at": 5, "a": {"b": 1, "c": 2}});
    normalize(&mut v, &vol(&["/generated_at", "/a/b"]));
    assert_eq!(
        v,
        json!({"generated_at": "<volátil>", "a": {"b": "<volátil>", "c": 2}})
    );
}

#[test]
fn normalize_wildcard_over_array_indexes() {
    let mut v = json!({"arr": [{"t": 1, "k": 1}, {"t": 2, "k": 2}, {"k": 3}]});
    normalize(&mut v, &vol(&["/arr/*/t"]));
    // Un elemento sin la clave no se rellena: la ausencia también es dato.
    assert_eq!(
        v,
        json!({"arr": [{"t": "<volátil>", "k": 1}, {"t": "<volátil>", "k": 2}, {"k": 3}]})
    );
}

#[test]
fn normalize_ignores_missing_paths() {
    let mut v = json!({"a": 1});
    normalize(&mut v, &vol(&["/zzz/y", "/a/b/c"]));
    assert_eq!(v, json!({"a": 1}));
}

#[test]
fn same_equal_responses_are_ok() {
    let h = [
        ("Content-Type", "application/json"),
        ("Content-Length", "13"),
        ("Cache-Control", "no-store"),
    ];
    let a = resp(403, &h, r#"{"error":"x"}"#);
    let b = resp(403, &h, r#"{"error":"x"}"#);
    assert_eq!(compare(&a, &b, Expect::Same, &[]), Outcome::Ok);
}

#[test]
fn same_json_whitespace_is_semantic_but_header_length_is_not() {
    // Python usa `", "`; el JSON equivale, pero Content-Length ya difiere.
    let a = resp(
        400,
        &[
            ("content-type", "application/json"),
            ("content-length", "17"),
        ],
        r#"{"a": 1, "b": 2}"#,
    );
    let b = resp(
        400,
        &[
            ("content-type", "application/json"),
            ("content-length", "13"),
        ],
        r#"{"a":1,"b":2}"#,
    );
    match compare(&a, &b, Expect::Same, &[]) {
        Outcome::Diff(m) => assert!(m.contains("content-length"), "{m}"),
        o => panic!("{o:?}"),
    }
    assert_eq!(compare(&a, &b, Expect::StaticAccepted, &[]), Outcome::Ok);
}

#[test]
fn same_detects_status_header_and_body_diffs() {
    let h = [("content-type", "application/json")];
    let base = resp(200, &h, r#"{"a":1}"#);
    assert!(matches!(
        compare(&base, &resp(201, &h, r#"{"a":1}"#), Expect::Same, &[]),
        Outcome::Diff(m) if m.contains("status")
    ));
    assert!(matches!(
        compare(&base, &resp(200, &[("content-type", "text/html")], r#"{"a":1}"#), Expect::Same, &[]),
        Outcome::Diff(m) if m.contains("content-type")
    ));
    assert!(matches!(
        compare(&base, &resp(200, &h, r#"{"a":2}"#), Expect::Same, &[]),
        Outcome::Diff(m) if m.contains("cuerpo")
    ));
}

#[test]
fn same_ignores_irrelevant_headers_and_case() {
    let a = resp(
        200,
        &[
            ("Content-Type", "text/plain"),
            ("Server", "BaseHTTP/0.6"),
            ("Date", "x"),
        ],
        "hola",
    );
    let b = resp(
        200,
        &[("content-type", "text/plain"), ("Server", "comandos")],
        "hola",
    );
    assert_eq!(compare(&a, &b, Expect::Same, &[]), Outcome::Ok);
}

#[test]
fn volatile_pointers_are_masked_before_comparing() {
    let h = [("content-type", "application/json")];
    let a = resp(
        200,
        &h,
        r#"{"generated_at": 1, "panes": [{"last_seen": 5, "n": 1}]}"#,
    );
    let b = resp(
        200,
        &h,
        r#"{"generated_at": 2, "panes": [{"last_seen": 9, "n": 1}]}"#,
    );
    let v = vol(&["/generated_at", "/panes/*/last_seen"]);
    assert_eq!(compare(&a, &b, Expect::StaticAccepted, &v), Outcome::Ok);
    let c = resp(
        200,
        &h,
        r#"{"generated_at": 2, "panes": [{"last_seen": 9, "n": 2}]}"#,
    );
    assert!(matches!(
        compare(&a, &c, Expect::StaticAccepted, &v),
        Outcome::Diff(_)
    ));
}

#[test]
fn non_json_bodies_compare_as_bytes() {
    let a = resp(200, &[], "<html>a</html>");
    let b = resp(200, &[], "<html>a</html>\n");
    assert!(matches!(
        compare(&a, &b, Expect::StaticAccepted, &[]),
        Outcome::Diff(m) if m.contains("cuerpo")
    ));
    assert_eq!(compare(&a, &a, Expect::StaticAccepted, &[]), Outcome::Ok);
}

#[test]
fn static_accepted_ignores_headers_but_not_status() {
    let a = resp(200, &[("content-type", "text/css; charset=utf-8")], "x{}");
    let b = resp(200, &[("content-type", "text/css")], "x{}");
    assert_eq!(compare(&a, &b, Expect::StaticAccepted, &[]), Outcome::Ok);
    let c = resp(404, &[("content-type", "text/css")], "x{}");
    assert!(matches!(
        compare(&a, &c, Expect::StaticAccepted, &[]),
        Outcome::Diff(_)
    ));
}

#[test]
fn skip_never_compares() {
    let a = resp(200, &[], "a");
    let b = resp(500, &[], "b");
    assert!(matches!(
        compare(&a, &b, Expect::Skip, &[]),
        Outcome::Skip(_)
    ));
}

#[test]
fn parse_response_content_length_and_chunked() {
    let raw = b"HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: 5\r\n\r\nhola!EXTRA";
    let r = parse_response(raw).unwrap();
    assert_eq!((r.status, r.body.as_slice()), (200, &b"hola!"[..]));

    let raw = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n4\r\nhola\r\n4;x=1\r\n que\r\n0\r\n\r\n";
    let r = parse_response(raw).unwrap();
    assert_eq!(r.body, b"hola que");

    let raw = b"HTTP/1.0 404 Not Found\r\nX: y\r\n\r\ncuerpo hasta EOF";
    let r = parse_response(raw).unwrap();
    assert_eq!(
        (r.status, r.body.as_slice()),
        (404, &b"cuerpo hasta EOF"[..])
    );
    assert!(parse_response(b"basura").is_err());
}

#[test]
fn status_only_ignores_body_and_headers() {
    let a = resp(404, &[("content-type", "text/html")], "<html>Error</html>");
    let b = resp(
        404,
        &[("content-type", "application/json")],
        r#"{"error":"No encontrado"}"#,
    );
    assert_eq!(compare(&a, &b, Expect::StatusOnly, &[]), Outcome::Ok);
    let c = resp(200, &[], "");
    assert!(matches!(
        compare(&a, &c, Expect::StatusOnly, &[]),
        Outcome::Diff(_)
    ));
}
