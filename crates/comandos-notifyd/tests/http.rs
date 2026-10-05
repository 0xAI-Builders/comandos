//! POST /notify contra el Handler del Python (gi sustituido), ambos en puertos efímeros.
mod support;

use support::{Daemon, Wire, python_eval, python_notifyd, rust_notifyd, tempdir};

/// (método, ruta, cabeceras, cuerpo).
type Case<'a> = (&'a str, &'a str, &'a [(&'a str, &'a str)], &'a str);

fn summary(w: &Wire) -> (u16, Option<String>, String) {
    (
        w.status,
        w.content_type(),
        String::from_utf8_lossy(&w.body).into_owned(),
    )
}

#[tokio::test]
async fn notify_responses_match_python() {
    let hooks = tempdir();
    std::fs::write(hooks.path().join("cc-notify.conf"), "POPUPS=1\n").unwrap();
    let Some(py) = python_notifyd(hooks.path()) else {
        return;
    };
    let rs = rust_notifyd(hooks.path());
    let cases: &[Case] = &[
        (
            "POST",
            "/notify",
            &[("Content-Type", "application/json")],
            r#"{"title":"Hola ñ","body":"b","session":"s","pane":"%12345678901"}"#,
        ),
        (
            "POST",
            "/notify",
            &[("Origin", "https://evil.example")],
            "{}",
        ),
        (
            "POST",
            "/notify",
            &[("Origin", "http://localhost:4777")],
            "{}",
        ),
        ("POST", "/otra", &[], "{}"),
        ("POST", "/notify", &[], "no es json"),
        ("POST", "/notify", &[("Content-Length", "200001")], ""),
        ("GET", "/notify", &[], ""),
    ];
    for (method, path, headers, body) in cases {
        let a: Wire = rs.request(method, path, headers, body).await;
        let b: Wire = py.request(method, path, headers, body).await;
        assert_eq!(
            (a.status, a.content_type(), a.body.clone()),
            (b.status, b.content_type(), b.body.clone()),
            "{method} {path} {headers:?}"
        );
    }
    let accepted = rs.accepted();
    assert_eq!(accepted.len(), 2, "{accepted:?}");
    assert_eq!(
        accepted,
        py.accepted(),
        "mismos avisos entregados (recortes incluidos)"
    );
}

#[tokio::test]
async fn popups_off_answers_popup_false() {
    let hooks = tempdir();
    std::fs::write(hooks.path().join("cc-notify.conf"), "POPUPS=0\n").unwrap();
    let rs = rust_notifyd(hooks.path());
    let wire = rs.request("POST", "/notify", &[], r#"{"title":"x"}"#).await;
    assert_eq!(wire.status, 200);
    assert_eq!(wire.body, br#"{"ok": true, "popup": false}"#);
    assert_eq!(wire.header("Content-Length").as_deref(), Some("28"));
    assert!(rs.accepted().is_empty());
}

/// Cuerpos con tipos y tamaños variados: `str()` de Python y recortes por carácter.
#[tokio::test]
async fn payload_conversions_match_python() {
    let hooks = tempdir();
    std::fs::write(hooks.path().join("cc-notify.conf"), "POPUPS=1\n").unwrap();
    let Some(py) = python_notifyd(hooks.path()) else {
        return;
    };
    let rs = rust_notifyd(hooks.path());
    let long = "ñ".repeat(70_000);
    let big_int = "1".repeat(4300);
    let huge_int = "1".repeat(4301);
    let bodies: Vec<String> = vec![
        String::new(),
        "{}".into(),
        "  ".into(),
        r#"{"title": null, "body": true, "session": false, "kind": 1, "project": -0, "options": 1.5, "full": 1e400, "pane": -1e-7}"#.into(),
        r#"{"title": ["a", "it's", "\"q\"", 'x'], "body": {"k": [1, 2.50, null, true]}, "options": {"a\nb": "\t\u0001é\u00ad\u2028😀"}}"#.replace('\'', "\""),
        r#"{"title": 12345678901234567890123, "body": 1E16, "session": 0.0001, "kind": 0.00001, "project": 123456789.123456789}"#.into(),
        r#"{"title": NaN, "body": -Infinity, "session": Infinity}"#.into(),
        r#"{"title": "a", "title": "b"}"#.into(),
        format!(r#"{{"title": "{long}", "body": "{long}", "project": "{long}", "options": "{long}", "full": "{long}", "session": "{long}", "kind": "{long}", "pane": "{long}"}}"#),
        format!(r#"{{"title": {big_int}}}"#),
        format!(r#"{{"title": {huge_int}}}"#),
        format!(r#"{{"x": [{huge_int}]}}"#),
        format!(r#"{{"title": {huge_int}.5}}"#),
        "[1, 2]".into(),
        "null".into(),
        "\"texto\"".into(),
        "{\"title\": \"a\u{0}b\"}".into(),
        "{\"title\": \"ctl\\u0000\\u001f\\u007f\\u0085\\u00a0\"}".into(),
        r#"{"title": {"nested": {"deep": ["x", {"y": "z'\"w"}]}}}"#.into(),
        r#"{"title": "x"} trailing"#.into(),
    ];
    for body in &bodies {
        let length = body.len().to_string();
        let headers = [("Content-Length", length.as_str())];
        let a = rs.request("POST", "/notify", &headers, body).await;
        let b = py.request("POST", "/notify", &headers, body).await;
        assert_eq!(summary(&a), summary(&b), "{}", &body[..body.len().min(120)]);
    }
    // Codificaciones que `json.loads(bytes)` detecta: BOM UTF-8 y UTF-16/32.
    let mut encoded: Vec<Vec<u8>> = Vec::new();
    let text = r#"{"title": "ü16"}"#;
    encoded.push([&[0xef, 0xbb, 0xbf][..], text.as_bytes()].concat());
    encoded.push(text.encode_utf16().flat_map(u16::to_le_bytes).collect());
    encoded.push(text.encode_utf16().flat_map(u16::to_be_bytes).collect());
    encoded.push(
        [0xff, 0xfe]
            .into_iter()
            .chain(text.encode_utf16().flat_map(u16::to_le_bytes))
            .collect(),
    );
    encoded.push(
        text.chars()
            .flat_map(|c| (c as u32).to_le_bytes())
            .collect(),
    );
    encoded.push(vec![0xff, 0xfe, 0xfd]);
    for bytes in &encoded {
        let mut raw = format!(
            "POST /notify HTTP/1.0\r\nContent-Length: {}\r\n\r\n",
            bytes.len()
        )
        .into_bytes();
        raw.extend_from_slice(bytes);
        let a = rs.raw(&raw).await;
        let b = py.raw(&raw).await;
        assert_eq!(summary(&a), summary(&b), "{bytes:?}");
    }
    let accepted = rs.accepted();
    assert!(accepted.len() >= 10, "{}", accepted.len());
    assert_eq!(accepted, py.accepted());
}

/// Líneas de petición, cabeceras y `Content-Length` raros: mismo estado, tipo y cuerpo.
#[tokio::test]
async fn raw_requests_match_python() {
    let hooks = tempdir();
    std::fs::write(hooks.path().join("cc-notify.conf"), "POPUPS=1\n").unwrap();
    let Some(py) = python_notifyd(hooks.path()) else {
        return;
    };
    let rs = rust_notifyd(hooks.path());
    let mut requests: Vec<Vec<u8>> = [
        "HEAD /notify HTTP/1.0\r\n\r\n",
        "PUT /notify HTTP/1.0\r\n\r\n",
        "post /notify HTTP/1.0\r\n\r\n",
        "OPTIONS * HTTP/1.1\r\n\r\n",
        "GET /notify\r\n\r\n",
        "POST /notify\r\n\r\n",
        "GARBAGE\r\n\r\n",
        "A B C D\r\n\r\n",
        "A B C HTTP/1.0\r\n\r\n",
        "POST /notify HTTP/2.0\r\n\r\n",
        "POST /notify HTTP/0.9\r\nContent-Length: 2\r\n\r\n{}",
        "POST /notify HTTP/0.9\r\nOrigin: https://evil\r\n\r\n",
        "POST /notify HTTP/x\r\n\r\n",
        "POST /notify HTTP/1.0.1\r\n\r\n",
        "POST /notify HTTP/+1._0\r\n\r\n",
        "POST /notify FTP/1.0\r\n\r\n",
        "<b> /notify HTTP/1.0\r\n\r\n",
        "POST //notify HTTP/1.0\r\nContent-Length: 2\r\n\r\n{}",
        "POST ///notify HTTP/1.1\r\nContent-Length: 2\r\n\r\n{}",
        "POST /notify?x=1 HTTP/1.0\r\nContent-Length: 2\r\n\r\n{}",
        "POST /notify HTTP/1.0\r\nContent-Length: +2\r\n\r\n{}",
        "POST /notify HTTP/1.0\r\nContent-Length:  2 \r\n\r\n{}",
        "POST /notify HTTP/1.0\r\nContent-Length: 0_2\r\n\r\n{}",
        "POST /notify HTTP/1.0\r\nContent-Length: 1__2\r\n\r\n{}",
        "POST /notify HTTP/1.0\r\nContent-Length: abc\r\n\r\n{}",
        "POST /notify HTTP/1.0\r\nContent-Length: -1\r\n\r\n{\"title\":\"eof\"}",
        "POST /notify HTTP/1.0\r\nContent-Length: 99\r\n\r\n{\"title\":\"short\"}",
        "POST /notify HTTP/1.0\r\nContent-Length: 0\r\n\r\n{\"title\":\"x\"}",
        "POST /notify HTTP/1.0\r\ncontent-length: 2\r\nContent-Length: 50\r\n\r\n{}",
        "POST /notify HTTP/1.0\r\nContent-Length: 200000\r\n\r\n{}",
        "POST /notify HTTP/1.0\r\nOrigin: http://127.0.0.1\r\nContent-Length: 2\r\n\r\n{}",
        "POST /notify HTTP/1.0\r\nOrigin: https://localhost:1\r\nContent-Length: 2\r\n\r\n{}",
        "POST /notify HTTP/1.0\r\nOrigin: http://localhost:\r\nContent-Length: 2\r\n\r\n{}",
        "POST /notify HTTP/1.0\r\nOrigin: http://localhost:4777 \r\nContent-Length: 2\r\n\r\n{}",
        "POST /notify HTTP/1.0\r\nOrigin:\r\nContent-Length: 2\r\n\r\n{}",
        "POST /notify HTTP/1.0\r\nOrigin: http://localhost.evil\r\nContent-Length: 2\r\n\r\n{}",
        "POST /notify HTTP/1.0\r\nOrigin: http://localhost\r\n :1\r\nContent-Length: 2\r\n\r\n{}",
        "POST /notify HTTP/1.0\r\nOrigin: HTTP://localhost\r\nContent-Length: 2\r\n\r\n{}",
        "POST /notify HTTP/1.0\r\nOrigin: null\r\nContent-Length: 2\r\n\r\n{}",
        "POST /notify HTTP/1.0\r\nbroken line\r\nOrigin: https://evil\r\nContent-Length: 2\r\n\r\n{}",
        "POST /notify HTTP/1.0\r\n Origin: https://evil\r\nContent-Length: 2\r\n\r\n{}",
        "POST /notify HTTP/1.0\r\n:x\r\nOrigin: https://evil\r\nContent-Length: 2\r\n\r\n{}",
        "POST /notify HTTP/1.0\r\nFrom someone\r\nOrigin: https://evil\r\nContent-Length: 2\r\n\r\n{}",
        "POST /notify HTTP/1.0\nOrigin: https://evil\nContent-Length: 2\n\n{}",
        "POST /notify HTTP/1.0\r\nX: a\rOrigin: https://evil\r\nContent-Length: 2\r\n\r\n{}",
        "\r\n",
        "",
    ]
    .iter()
    .map(|s| s.as_bytes().to_vec())
    .collect();
    // Bytes latin-1 en cabeceras y línea de petición.
    requests.push(b"POST /notify HTTP/1.0\r\nContent-Length: \xa02\x85\r\n\r\n{}".to_vec());
    requests.push(b"POST /notify HTTP/1.0\r\nContent-Length: \x1c2\r\n\r\n{}".to_vec());
    requests.push(b"P\xd1ST /notify HTTP/1.0\r\n\r\n".to_vec());
    requests.push(b"POST\x85/notify\xa0HTTP/1.0\r\nContent-Length: 2\r\n\r\n{}".to_vec());
    requests.push(b"PO'ST /notify HTTP/1.0\r\n\r\n".to_vec());
    requests.push(b"PO\"'ST /notify HTTP/1.0\r\n\r\n".to_vec());
    // Límites de BaseHTTPRequestHandler / http.client.
    let mut long_line = b"POST /".to_vec();
    long_line.extend(std::iter::repeat_n(b'a', 70_000));
    long_line.extend_from_slice(b" HTTP/1.0\r\n\r\n");
    requests.push(long_line);
    let mut long_header = b"POST /notify HTTP/1.0\r\nX: ".to_vec();
    long_header.extend(std::iter::repeat_n(b'a', 70_000));
    long_header.extend_from_slice(b"\r\n\r\n");
    requests.push(long_header);
    for (method, count) in [("POST", 98), ("POST", 99), ("POST", 100), ("HEAD", 100)] {
        let mut many = format!("{method} /notify HTTP/1.0\r\n").into_bytes();
        for i in 0..count {
            many.extend_from_slice(format!("X-{i}: v\r\n").as_bytes());
        }
        many.extend_from_slice(b"Content-Length: 2\r\n\r\n{}");
        requests.push(many);
    }
    for raw in &requests {
        let a = rs.raw(raw).await;
        let b = py.raw(raw).await;
        assert_eq!(
            summary(&a),
            summary(&b),
            "{:?}",
            String::from_utf8_lossy(&raw[..raw.len().min(160)])
        );
    }
    let accepted = rs.accepted();
    assert!(accepted.len() >= 10, "{}", accepted.len());
    assert_eq!(accepted, py.accepted());
}

/// `POPUPS` se lee en cada petición con las reglas de `_conf_value`.
#[tokio::test]
async fn popups_conf_matches_python() {
    let confs = [
        "POPUPS=1\n",
        "POPUPS=0\n",
        "POPUPS=\"1\"\n",
        "  POPUPS= '1' \n",
        "POPUPS = 1\n",
        "POPUPS=0\nPOPUPS=1\n",
        "# x\r\nPOPUPS=1\r\n",
        "A=1\rPOPUPS=1\r",
        "POPUPS=1\u{2028}\n",
        "POPUPS=\u{a0}1\u{1c}\n",
        "",
    ];
    for conf in confs {
        let hooks = tempdir();
        std::fs::write(hooks.path().join("cc-notify.conf"), conf).unwrap();
        let Some(py) = python_notifyd(hooks.path()) else {
            return;
        };
        let rs = rust_notifyd(hooks.path());
        let a = rs.request("POST", "/notify", &[], "{}").await;
        let b = py.request("POST", "/notify", &[], "{}").await;
        assert_eq!(summary(&a), summary(&b), "{conf:?}");
    }
    // Sin archivo y con bytes no UTF-8.
    let hooks = tempdir();
    let rs = rust_notifyd(hooks.path());
    let off = rs.request("POST", "/notify", &[], "{}").await;
    assert_eq!(off.body, br#"{"ok": true, "popup": false}"#);
    std::fs::write(hooks.path().join("cc-notify.conf"), b"POPUPS=1\n\xff\n").unwrap();
    let rs: Daemon = rust_notifyd(hooks.path());
    let bad = rs.request("POST", "/notify", &[], "{}").await;
    if let Some(py) = python_notifyd(hooks.path()) {
        let b = py.request("POST", "/notify", &[], "{}").await;
        assert_eq!(summary(&bad), summary(&b));
    }
}

/// `conf_value` y `ui_lang` contra `_conf_value` y `_ui_lang` del Python.
#[test]
fn conf_and_lang_match_python() {
    use comandos_notifyd::notice::{Lang, conf_value, ui_lang};
    let cases: &[(&str, Option<&str>)] = &[
        ("CC_LANG=es\n", Some("en_US.UTF-8")),
        ("CC_LANG=en\n", Some("es_MX.UTF-8")),
        ("CC_LANG=auto\n", Some("es_MX.UTF-8")),
        ("CC_LANG=auto\n", Some("ES")),
        ("CC_LANG=fr\n", Some("en_US")),
        ("CC_LANG=es\nCC_LANG=en\n", None),
        ("CC_LANG=en\nCC_LANG=xx\n", Some("es")),
        (" CC_LANG = es\n", Some("en")),
        ("CC_LANG=\"es\"\n", Some("")),
        ("", None),
    ];
    for (conf, lang) in cases {
        let hooks = tempdir();
        std::fs::write(hooks.path().join("cc-notify.conf"), conf).unwrap();
        let env: Vec<(&str, &str)> = lang.iter().map(|l| ("LANG", *l)).collect();
        let Some(expected) = python_eval(hooks.path(), &env, "mod.UI_LANG") else {
            return;
        };
        let got = match ui_lang(&hooks.path().join("cc-notify.conf"), *lang) {
            Lang::Es => "es",
            Lang::En => "en",
        };
        assert_eq!(serde_json::json!(got), expected, "{conf:?} {lang:?}");
    }
    let conf = "A=1\nPOPUPS = 2\nPOPUPS='x'\nPOPUPS=y\nB=\"\"\n";
    let hooks = tempdir();
    let path = hooks.path().join("cc-notify.conf");
    std::fs::write(&path, conf).unwrap();
    for (key, default) in [("POPUPS", "0"), ("A", "d"), ("B", "d"), ("C", "d")] {
        let expr = format!("mod._conf_value({key:?}, {default:?})");
        let Some(expected) = python_eval(hooks.path(), &[], &expr) else {
            return;
        };
        assert_eq!(
            serde_json::json!(conf_value(&path, key, default)),
            expected,
            "{key}"
        );
    }
    assert_eq!(conf_value(&hooks.path().join("nada"), "A", "d"), "d");
}

/// `py_str` contra `str()` de Python sobre los mismos valores JSON.
#[test]
fn py_str_matches_python() {
    use comandos_notifyd::notice::py_str;
    let samples = r#"[null, true, false, 0, -0, 7, -12, 123456789012345678901234567890,
        1.5, 2.50, 1e16, 1e15, 0.0001, 0.00001, -0.0, 1e400, -1e400, 1.7976931348623157e308,
        5e-324, 0.1, 100.0, 3.14159265358979,
        "s", "it's", "\"", "'\"", "\\", "\u0000\u001f\u007f\u0080\u009f\u00a0\u00ad",
        "\u0378\u2028\u2029\u200b\ue000\ufeff\ufffe", "😀\ud83d\ude00", "\u0300",
        [], {}, [1, [2, [3]]], {"a": {"b": [true, null]}}, ["it's", "\"x\""], {"k'": "v\""}]"#;
    let hooks = tempdir();
    let expr = format!(
        "[str(v) for v in json.loads({})]",
        serde_json::to_string(samples).unwrap()
    );
    let Some(expected) = python_eval(hooks.path(), &[], &expr) else {
        return;
    };
    let values = comandos_core::json::workspace_loads(samples).unwrap();
    let got: Vec<String> = values.as_array().unwrap().iter().map(py_str).collect();
    assert_eq!(serde_json::json!(got), expected);
}
