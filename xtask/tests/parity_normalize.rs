//! Normalización y comparación del arnés `parity`, sin servidores.
#![allow(dead_code)]

#[path = "../src/parity.rs"]
mod parity;

use parity::{
    Expect, Outcome, Resp, compare, normalize, outside_links, parse_response, sanitize_links,
};
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
fn same_without_volatile_compares_raw_bytes_so_key_order_is_a_diff() {
    let h = [
        ("content-type", "application/json"),
        ("content-length", "17"),
    ];
    let a = resp(200, &h, r#"{"a": 1, "b": 2}"#);
    let b = resp(200, &h, r#"{"b": 2, "a": 1}"#);
    assert!(matches!(
        compare(&a, &b, Expect::Same, &[]),
        Outcome::Diff(m) if m.contains("cuerpo")
    ));
    // Tampoco el espaciado: sin volatile son bytes.
    let c = resp(200, &h, r#"{"a":1,"b":2}"#);
    assert!(matches!(
        compare(&a, &c, Expect::StaticAccepted, &[]),
        Outcome::Diff(_)
    ));
}

#[test]
fn volatile_mode_reserializes_in_order_and_skips_raw_content_length() {
    let a = resp(
        200,
        &[
            ("content-type", "application/json"),
            ("content-length", "27"),
        ],
        r#"{"t": 1, "k": "x"}"#,
    );
    let b = resp(
        200,
        &[
            ("content-type", "application/json"),
            ("content-length", "20"),
        ],
        r#"{"t":22222,"k":"x"}"#,
    );
    let v = vol(&["/t"]);
    // Espaciado distinto y longitudes crudas distintas: tras reserializar es lo mismo.
    assert_eq!(compare(&a, &b, Expect::Same, &v), Outcome::Ok);
    // El orden de claves sigue contando también en este modo.
    let c = resp(
        200,
        &[("content-type", "application/json")],
        r#"{"k":"x","t":1}"#,
    );
    assert!(matches!(
        compare(&a, &c, Expect::StaticAccepted, &v),
        Outcome::Diff(_)
    ));
}

#[test]
fn connection_header_and_duplicates_are_compared() {
    let a = resp(413, &[("Connection", "close")], "x");
    let b = resp(413, &[], "x");
    assert!(matches!(
        compare(&a, &b, Expect::Same, &[]),
        Outcome::Diff(m) if m.contains("connection")
    ));
    let d1 = resp(
        200,
        &[("Cache-Control", "no-store"), ("Cache-Control", "x")],
        "x",
    );
    let d2 = resp(200, &[("Cache-Control", "no-store")], "x");
    assert!(matches!(
        compare(&d1, &d2, Expect::Same, &[]),
        Outcome::Diff(m) if m.contains("cache-control")
    ));
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
    // Cuerpo más corto que Content-Length: error, no recorte silencioso.
    assert!(parse_response(b"HTTP/1.1 200 OK\r\nContent-Length: 10\r\n\r\ncorto").is_err());
    assert!(
        parse_response(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n4\r\nho").is_err()
    );
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

#[test]
fn sanitize_links_replaces_or_removes_external_symlinks() {
    use std::{fs, os::unix::fs::symlink};
    let base = std::env::temp_dir().join(format!("xtask-sanitize-{}", std::process::id()));
    let _ = fs::remove_dir_all(&base);
    let (copy, repo, outside) = (base.join("copy"), base.join("repo"), base.join("outside"));
    for d in [&copy, &repo, &outside, &outside.join("dir")] {
        fs::create_dir_all(d).unwrap();
    }
    fs::write(repo.join("asset.css"), "ok").unwrap();
    fs::write(outside.join("bin"), "binario").unwrap();
    fs::write(copy.join("local"), "l").unwrap();
    symlink(repo.join("asset.css"), copy.join("to-repo")).unwrap(); // permitido
    symlink("local", copy.join("rel-inside")).unwrap(); // relativo interno
    symlink(outside.join("bin"), copy.join("to-file")).unwrap(); // se copia
    symlink(outside.join("dir"), copy.join("to-dir")).unwrap(); // se borra
    symlink(outside.join("nope"), copy.join("dangling")).unwrap(); // se borra
    fs::create_dir_all(copy.join("sub")).unwrap();
    symlink(outside.join("bin"), copy.join("sub/deep")).unwrap();

    let allowed = vec![copy.canonicalize().unwrap(), repo.canonicalize().unwrap()];
    assert_eq!(outside_links(&copy, &allowed).len(), 4);
    assert_eq!(sanitize_links(&copy, &allowed).unwrap(), 4);
    assert!(outside_links(&copy, &allowed).is_empty());
    assert_eq!(fs::read_to_string(copy.join("to-file")).unwrap(), "binario");
    assert!(
        !fs::symlink_metadata(copy.join("to-file"))
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(
        fs::read_to_string(copy.join("sub/deep")).unwrap(),
        "binario"
    );
    assert!(fs::symlink_metadata(copy.join("to-dir")).is_err());
    assert!(fs::symlink_metadata(copy.join("dangling")).is_err());
    assert!(
        fs::symlink_metadata(copy.join("to-repo"))
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert!(
        fs::symlink_metadata(copy.join("rel-inside"))
            .unwrap()
            .file_type()
            .is_symlink()
    );
    fs::remove_dir_all(&base).unwrap();
}
