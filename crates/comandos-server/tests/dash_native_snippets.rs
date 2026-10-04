//! Dominio F1: snippets y registro de uso respondidos por Rust, con el mismo
//! `flock` que el Python. El heredado es un puerto muerto salvo en las
//! pruebas de declinar (heredado falso que responde `{"legacy": true}`).
mod support;

use serde_json::Value;
use std::{fs, sync::Arc};
use support::{FakeLegacy, TestHome, Wire, dead_port, front, get, oracle::oracle, request_body};

const MESSY: &str = r#"[
  {"id": "0123456789abcdef", "name": " Saludo ", "body": "echo hola", "tags": ["a", " b "], "updated_at": 7.9},
  {"id": "0123456789ABCDEF", "name": "mayúsculas", "body": "x"},
  {"id": "fedcba9876543210", "name": null, "body": true, "tags": null},
  {"id": "1111111111111111", "name": "", "body": "x"},
  {"id": "2222222222222222", "name": "ok", "body": "x", "tags": ["", "y"]},
  "no-objeto",
  {"id": "3333333333333333", "name": "ñandú", "body": "y", "tags": []}
]"#;

fn seen(wire: &Wire) -> (u16, Option<String>, String) {
    (
        wire.status,
        wire.header("content-type").map(str::to_owned),
        wire.text(),
    )
}

#[tokio::test]
async fn snippets_list_normalizes_like_python() {
    let home = TestHome::new("snip-list");
    home.write("snippets.json", MESSY);
    let front = front(&home, dead_port(), home.options()).await;
    let wire = get(front.port, "/snippets").await;
    assert_eq!(wire.status, 200);
    assert_eq!(
        wire.text(),
        r#"[{"id": "0123456789abcdef", "name": " Saludo ", "body": "echo hola", "tags": ["a", " b "], "updated_at": 7}, {"id": "fedcba9876543210", "name": "None", "body": "True", "tags": [], "updated_at": 0}, {"id": "3333333333333333", "name": "\u00f1and\u00fa", "body": "y", "tags": [], "updated_at": 0}]"#
    );
    if let Some(py) = oracle(&home).await {
        assert_eq!(seen(&get(py.port, "/snippets").await), seen(&wire));
    }
    front.stop().await;
}

#[tokio::test]
async fn snippets_crud_writes_python_bytes() {
    let home = TestHome::new("snip-crud");
    let front = front(&home, dead_port(), home.options()).await;
    let created = request_body(
        front.port,
        "POST",
        "/snippets",
        "",
        r#"{"name": "  Deploy  ", "body": "make deploy", "tags": [" ops ", "prod"]}"#,
    )
    .await;
    assert_eq!(created.status, 200, "{}", created.text());
    let item: Value = serde_json::from_str(&created.text()).unwrap();
    let id = item["item"]["id"].as_str().unwrap().to_owned();
    assert_eq!(id.len(), 16);
    assert_eq!(
        created.text(),
        format!(
            r#"{{"item": {{"id": "{id}", "name": "Deploy", "body": "make deploy", "tags": ["ops", "prod"], "updated_at": 1791115200}}}}"#
        )
    );
    assert_eq!(
        fs::read_to_string(home.hooks().join("snippets.json")).unwrap(),
        format!(
            r#"[{{"id": "{id}", "name": "Deploy", "body": "make deploy", "tags": ["ops", "prod"], "updated_at": 1791115200}}]"#
        )
    );
    let update = format!(r#"{{"id": "{id}", "name": "Deploy 2", "body": "make d2", "tags": []}}"#);
    let updated = request_body(front.port, "POST", "/snippets/update", "", &update).await;
    assert_eq!(
        updated.text(),
        format!(
            r#"{{"item": {{"id": "{id}", "name": "Deploy 2", "body": "make d2", "tags": [], "updated_at": 1791115200}}}}"#
        )
    );
    // Lo escrito por Rust lo lee igual el Python.
    if let Some(py) = oracle(&home).await {
        assert_eq!(
            seen(&get(py.port, "/snippets").await),
            seen(&get(front.port, "/snippets").await)
        );
    }
    let deleted = request_body(
        front.port,
        "POST",
        "/snippets/delete",
        "",
        &format!(r#"{{"id": "{id}"}}"#),
    )
    .await;
    assert_eq!(
        (deleted.status, deleted.text().as_str()),
        (200, r#"{"ok": true}"#)
    );
    let again = request_body(
        front.port,
        "POST",
        "/snippets/delete",
        "",
        &format!(r#"{{"id": "{id}"}}"#),
    )
    .await;
    assert_eq!(
        (again.status, again.text().as_str()),
        (404, r#"{"error": "snippet no encontrado"}"#)
    );
    front.stop().await;
}

#[tokio::test]
async fn snippets_errors_match_python_oracle() {
    let home = TestHome::new("snip-err");
    home.write("snippets.json", MESSY);
    let Some(py) = oracle(&home).await else {
        return;
    };
    let front = front(&home, dead_port(), home.options()).await;
    let long_name = "n".repeat(81);
    let cases = [
        ("/snippets", r#"{"name": "", "body": "x"}"#.to_owned()),
        ("/snippets", r#"{"name": "a", "body": "   "}"#.to_owned()),
        ("/snippets", format!(r#"{{"name": "{long_name}", "body": "x"}}"#)),
        ("/snippets", r#"{"name": "a", "body": "x", "tags": "uno"}"#.to_owned()),
        ("/snippets", r#"{"name": "a", "body": "x", "tags": ["1","2","3","4","5","6","7","8","9","10","11"]}"#.to_owned()),
        ("/snippets", r#"{"name": "a", "body": "x", "tags": [" "]}"#.to_owned()),
        ("/snippets", r#"{"name": "a", "body": "x", "tags": [3]}"#.to_owned()),
        ("/snippets", format!(r#"{{"name": "a", "body": "x", "tags": ["{}"]}}"#, "t".repeat(33))),
        ("/snippets/update", r#"{"id": "zz"}"#.to_owned()),
        ("/snippets/update", r#"{"id": "0123456789abcdef", "name": "", "body": "x"}"#.to_owned()),
        ("/snippets/update", r#"{"id": "9999999999999999", "name": "a", "body": "x"}"#.to_owned()),
        ("/snippets/delete", r#"{"id": 5}"#.to_owned()),
        ("/snippets/delete", r#"{"id": "9999999999999999"}"#.to_owned()),
    ];
    for (path, body) in &cases {
        assert_eq!(
            seen(&request_body(py.port, "POST", path, "", body).await),
            seen(&request_body(front.port, "POST", path, "", body).await),
            "{path} {body}"
        );
    }
    front.stop().await;
}

#[tokio::test]
async fn snippets_lock_contended_declines() {
    let home = TestHome::new("snip-lock");
    home.write("snippets.json", "[]");
    let legacy = FakeLegacy::start().await;
    let front = front(&home, legacy.port, home.options()).await;
    let holder = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(home.hooks().join("snippets.json.lock"))
        .unwrap();
    holder.lock().unwrap();
    let wire = request_body(
        front.port,
        "POST",
        "/snippets",
        "",
        r#"{"name": "a", "body": "b"}"#,
    )
    .await;
    assert_eq!(wire.text(), r#"{"legacy": true}"#, "declina al heredado");
    let wire = request_body(
        front.port,
        "POST",
        "/ui-log",
        "",
        r#"{"events": [{"k": "x"}]}"#,
    )
    .await;
    // ui-log usa su propio candado: no está tomado, se atiende en Rust.
    assert_eq!(wire.text(), r#"{"ok": true, "n": 1}"#);
    assert_eq!(
        fs::read_to_string(home.hooks().join("snippets.json")).unwrap(),
        "[]"
    );
    holder.unlock().unwrap();
    let wire = request_body(
        front.port,
        "POST",
        "/snippets",
        "",
        r#"{"name": "a", "body": "b"}"#,
    )
    .await;
    assert_eq!(wire.status, 200, "libre: nativo");
    assert_eq!(
        legacy.requests(),
        vec!["POST /snippets HTTP/1.1".to_owned()]
    );
    front.stop().await;
}

#[tokio::test]
async fn ui_log_appends_python_lines() {
    let home = TestHome::new("uilog");
    let front = front(&home, dead_port(), home.options()).await;
    let body = r#"{"events": [{"ts": 5, "k": "clicañ-muy-largo-de-mas-de-24", "n": 7, "c": true, "d": "12", "s": null}, "x", {"ts": "1.5e3"}]}"#;
    let wire = request_body(front.port, "POST", "/ui-log", "", body).await;
    assert_eq!(wire.text(), r#"{"ok": true, "n": 2}"#);
    assert_eq!(
        fs::read_to_string(home.hooks().join("ui-events.jsonl")).unwrap(),
        "{\"ts\": 5.0, \"k\": \"clicañ-muy-largo-de-mas-\", \"n\": \"7\", \"c\": \"True\", \"d\": 12, \"s\": \"\"}\n\
         {\"ts\": 1500.0, \"k\": \"\", \"n\": \"\", \"c\": \"\", \"d\": 0, \"s\": \"\"}\n"
    );
    for (body, expected) in [
        (r#"{}"#, r#"{"ok": true, "n": 0}"#),
        (r#"{"events": "abc"}"#, r#"{"ok": true, "n": 0}"#),
    ] {
        assert_eq!(
            request_body(front.port, "POST", "/ui-log", "", body)
                .await
                .text(),
            expected
        );
    }
    front.stop().await;
}

#[tokio::test]
async fn ui_log_errors_and_rotation_match_python_oracle() {
    // Dos HOME con el mismo archivo de partida: cada servidor escribe el suyo.
    let rust_home = TestHome::new("uilog-rs");
    let python_home = TestHome::new("uilog-py");
    let mut old = String::new();
    let line = format!(
        "{{\"ts\": 1.0, \"k\": \"viejo\", \"n\": \"{}\"}}\n",
        "x".repeat(60)
    );
    while old.len() < 2_000_100 {
        old.push_str(&line);
    }
    old.push_str("no es json\n\n");
    old.push_str("{\"ts\": 2000000000.0, \"k\": \"nuevo\"}\n");
    for home in [&rust_home, &python_home] {
        home.write("ui-events.jsonl", &old);
    }
    let Some(py) = oracle(&python_home).await else {
        return;
    };
    let mut opts = rust_home.options();
    opts.clock = Arc::new(comandos_server::dash::native::wall_clock_ms);
    let front = front(&rust_home, dead_port(), opts).await;
    let body = r#"{"events": [{"ts": 2000000001, "k": "a", "d": 1}]}"#;
    assert_eq!(
        seen(&request_body(py.port, "POST", "/ui-log", "", body).await),
        seen(&request_body(front.port, "POST", "/ui-log", "", body).await)
    );
    let rotated = fs::read_to_string(rust_home.hooks().join("ui-events.jsonl")).unwrap();
    assert_eq!(
        rotated,
        fs::read_to_string(python_home.hooks().join("ui-events.jsonl")).unwrap()
    );
    assert_eq!(
        rotated,
        "{\"ts\": 2000000000.0, \"k\": \"nuevo\"}\n{\"ts\": 2000000001.0, \"k\": \"a\", \"n\": \"\", \"c\": \"\", \"d\": 1, \"s\": \"\"}\n"
    );
    for body in [
        r#"{"events": {"a": 1}}"#,
        r#"{"events": 5}"#,
        r#"{"events": [{"d": "x"}]}"#,
        r#"{"events": [{"ts": "nope"}]}"#,
        r#"{"events": [{"ts": [1]}]}"#,
    ] {
        assert_eq!(
            seen(&request_body(py.port, "POST", "/ui-log", "", body).await),
            seen(&request_body(front.port, "POST", "/ui-log", "", body).await),
            "{body}"
        );
    }
    front.stop().await;
}

#[tokio::test]
async fn exotic_inputs_decline_to_legacy() {
    let home = TestHome::new("snip-exotic");
    home.write(
        "snippets.json",
        r#"[{"id": "0123456789abcdef", "name": 1.5, "body": "x"}]"#,
    );
    let legacy = FakeLegacy::start().await;
    let front = front(&home, legacy.port, home.options()).await;
    for (method, path, body) in [
        ("GET", "/snippets", ""),
        ("POST", "/snippets", r#"{"name": 2.5, "body": "x"}"#),
        ("POST", "/ui-log", r#"{"events": [{"k": 1.5}]}"#),
        (
            "POST",
            "/ui-log",
            r#"{"events": [{"d": 123456789012345678901234567890}]}"#,
        ),
    ] {
        let wire = request_body(front.port, method, path, "", body).await;
        assert_eq!(wire.text(), r#"{"legacy": true}"#, "{method} {path} {body}");
    }
    assert!(
        !home.hooks().join("ui-events.jsonl").exists(),
        "declinar no escribe"
    );
    front.stop().await;
}

#[tokio::test]
async fn snippets_odd_rows_match_python_oracle() {
    // Ids que `str()` no puede casar se saltan sin declinar; un id entero de
    // 16 dígitos sí casa; `tags` cadena en una fila inválida no importa.
    let home = TestHome::new("snip-odd");
    home.write(
        "snippets.json",
        r#"[
  {"id": 1.5, "name": 2.5, "body": "x"},
  {"id": [1], "name": "a", "body": "x"},
  {"id": 1234567890123456, "name": "num", "body": "x", "updated_at": "12"},
  {"id": "4444444444444444", "name": "", "body": "x", "tags": "abc"},
  {"id": "5555555555555555", "name": "z", "body": "x", "tags": [], "updated_at": true}
]"#,
    );
    let front = front(&home, dead_port(), home.options()).await;
    let wire = get(front.port, "/snippets").await;
    assert_eq!(
        wire.text(),
        r#"[{"id": "1234567890123456", "name": "num", "body": "x", "tags": [], "updated_at": 12}, {"id": "5555555555555555", "name": "z", "body": "x", "tags": [], "updated_at": 1}]"#
    );
    if let Some(py) = oracle(&home).await {
        assert_eq!(seen(&get(py.port, "/snippets").await), seen(&wire));
        for broken in [
            r#"[{"id": "0123456789abcdef", "name": "a", "body": "x", "updated_at": "abc"}]"#,
            r#"[{"id": "0123456789abcdef", "name": "a", "body": "x", "updated_at": NaN}]"#,
            r#"[{"id": "0123456789abcdef", "name": "a", "body": "x", "updated_at": Infinity}]"#,
        ] {
            home.write("snippets.json", broken);
            let wire = get(front.port, "/snippets").await;
            assert_eq!(wire.status, 500, "{broken}");
            assert_eq!(
                seen(&get(py.port, "/snippets").await),
                seen(&wire),
                "{broken}"
            );
            // Bajo candado también: 500 y el archivo intacto.
            let body = r#"{"name": "a", "body": "b"}"#;
            assert_eq!(
                seen(&request_body(py.port, "POST", "/snippets", "", body).await),
                seen(&request_body(front.port, "POST", "/snippets", "", body).await),
                "{broken}"
            );
            assert_eq!(
                fs::read_to_string(home.hooks().join("snippets.json")).unwrap(),
                broken
            );
        }
    }
    front.stop().await;
}

#[tokio::test]
async fn snippets_valid_row_with_string_tags_declines() {
    let home = TestHome::new("snip-strtags");
    home.write(
        "snippets.json",
        r#"[{"id": "0123456789abcdef", "name": "a", "body": "x", "tags": "abc"}]"#,
    );
    let legacy = FakeLegacy::start().await;
    let front = front(&home, legacy.port, home.options()).await;
    for (method, path, body) in [
        ("GET", "/snippets", ""),
        ("POST", "/snippets", r#"{"name": "a", "body": "b"}"#),
        ("POST", "/snippets/delete", r#"{"id": "0123456789abcdef"}"#),
    ] {
        let wire = request_body(front.port, method, path, "", body).await;
        assert_eq!(wire.text(), r#"{"legacy": true}"#, "{method} {path}");
    }
    assert_eq!(
        fs::read_to_string(home.hooks().join("snippets.json")).unwrap(),
        r#"[{"id": "0123456789abcdef", "name": "a", "body": "x", "tags": "abc"}]"#
    );
    front.stop().await;
}

#[tokio::test]
async fn ui_log_conversions_match_python_oracle() {
    let rust_home = TestHome::new("uilog-conv-rs");
    let python_home = TestHome::new("uilog-conv-py");
    let Some(py) = oracle(&python_home).await else {
        return;
    };
    let front = front(&rust_home, dead_port(), rust_home.options()).await;
    let big = format!("1{}", "0".repeat(400));
    let cases = [
        r#"{"events": [{"ts": true, "k": 0, "n": false, "c": -3, "d": 3.9, "s": "ñ\u0001"}]}"#.to_owned(),
        r#"{"events": [{"ts": " 12 ", "d": true}, {"ts": 1e20, "d": "-7"}, {"ts": 0.5, "d": " 1_0 "}]}"#.to_owned(),
        r#"{"events": false}"#.to_owned(),
        r#"{"events": []}"#.to_owned(),
        r#"{"events": [1, null, []]}"#.to_owned(),
        r#"{"events": true}"#.to_owned(),
        format!(r#"{{"events": [{{"ts": {big}}}]}}"#),
        r#"{"events": [{"d": NaN}]}"#.to_owned(),
        r#"{"events": [{"d": Infinity}]}"#.to_owned(),
        r#"{"events": [{"d": [1]}]}"#.to_owned(),
    ];
    for body in &cases {
        assert_eq!(
            seen(&request_body(py.port, "POST", "/ui-log", "", body).await),
            seen(&request_body(front.port, "POST", "/ui-log", "", body).await),
            "{body}"
        );
    }
    let read = |home: &TestHome| {
        fs::read_to_string(home.hooks().join("ui-events.jsonl")).unwrap_or_default()
    };
    assert_eq!(read(&rust_home), read(&python_home));
    assert!(read(&rust_home).contains("\"s\": \"ñ\\u0001\""));
    front.stop().await;
}

#[tokio::test]
async fn ui_log_rotation_aborts_like_python() {
    // Una línea con `ts` no comparable o que no es objeto aborta la rotación
    // en silencio: el archivo queda con lo anexado. `\r` cuenta como salto.
    for (tag, bad) in [("str", "{\"ts\": \"x\"}\r"), ("list", "[1, 2]\r\n")] {
        let rust_home = TestHome::new(&format!("uilog-abort-{tag}-rs"));
        let python_home = TestHome::new(&format!("uilog-abort-{tag}-py"));
        let mut old = String::new();
        let line = format!(
            "{{\"ts\": 1.0, \"k\": \"viejo\", \"n\": \"{}\"}}\r",
            "x".repeat(60)
        );
        while old.len() < 2_000_100 {
            old.push_str(&line);
        }
        old.push_str(bad);
        for home in [&rust_home, &python_home] {
            home.write("ui-events.jsonl", &old);
        }
        let Some(py) = oracle(&python_home).await else {
            return;
        };
        let mut opts = rust_home.options();
        opts.clock = Arc::new(comandos_server::dash::native::wall_clock_ms);
        let front = front(&rust_home, dead_port(), opts).await;
        let body = r#"{"events": [{"ts": 2000000001, "k": "a"}]}"#;
        assert_eq!(
            seen(&request_body(py.port, "POST", "/ui-log", "", body).await),
            seen(&request_body(front.port, "POST", "/ui-log", "", body).await)
        );
        let rotated = fs::read_to_string(rust_home.hooks().join("ui-events.jsonl")).unwrap();
        assert!(rotated.starts_with(&old), "{tag}: sin rotar");
        assert_eq!(
            rotated,
            fs::read_to_string(python_home.hooks().join("ui-events.jsonl")).unwrap(),
            "{tag}"
        );
        front.stop().await;
    }
}

#[tokio::test]
async fn ui_log_uncertain_rotation_declines_without_writing() {
    let home = TestHome::new("uilog-unsure");
    let mut old = String::new();
    let line = format!("{{\"ts\": 1.0, \"n\": \"{}\"}}\n", "x".repeat(60));
    while old.len() < 2_000_100 {
        old.push_str(&line);
    }
    // El `json` del Python lee el sustituto suelto; el parser Rust no.
    old.push_str("{\"ts\": 1.0, \"k\": \"\\udc80\"}\n");
    home.write("ui-events.jsonl", &old);
    let legacy = FakeLegacy::start().await;
    let front = front(&home, legacy.port, home.options()).await;
    let wire = request_body(
        front.port,
        "POST",
        "/ui-log",
        "",
        r#"{"events": [{"k": "a"}]}"#,
    )
    .await;
    assert_eq!(wire.text(), r#"{"legacy": true}"#);
    assert_eq!(
        fs::read_to_string(home.hooks().join("ui-events.jsonl")).unwrap(),
        old
    );
    // Pequeño y sin rotar: la misma línea no se lee y se anexa en Rust.
    home.write("ui-events.jsonl", "{\"k\": \"\\udc80\"}\n");
    let wire = request_body(
        front.port,
        "POST",
        "/ui-log",
        "",
        r#"{"events": [{"k": "a"}]}"#,
    )
    .await;
    assert_eq!(wire.text(), r#"{"ok": true, "n": 1}"#);
    assert_eq!(legacy.requests(), vec!["POST /ui-log HTTP/1.1".to_owned()]);
    front.stop().await;
}

#[tokio::test]
async fn snippets_non_scalar_id_is_400_like_python() {
    let home = TestHome::new("snip-id-kind");
    home.write("snippets.json", MESSY);
    let legacy = FakeLegacy::start().await;
    let front = front(&home, legacy.port, home.options()).await;
    let cases = [
        (
            "/snippets/update",
            r#"{"id": 1.5, "name": "a", "body": "x"}"#,
        ),
        (
            "/snippets/update",
            r#"{"id": [1], "name": 2.5, "body": "x"}"#,
        ),
        ("/snippets/update", r#"{"id": {"a": 1}}"#),
        ("/snippets/update", r#"{"id": 1e16}"#),
        ("/snippets/delete", r#"{"id": 1.5}"#),
        ("/snippets/delete", r#"{"id": ["0123456789abcdef"]}"#),
        ("/snippets/delete", r#"{"id": {}}"#),
        ("/snippets/delete", r#"{"id": null}"#),
        ("/snippets/delete", r#"{}"#),
    ];
    for (path, body) in cases {
        let wire = request_body(front.port, "POST", path, "", body).await;
        assert_eq!(
            (wire.status, wire.text().as_str()),
            (400, r#"{"error": "id invalido"}"#),
            "{path} {body}"
        );
    }
    assert!(legacy.requests().is_empty(), "{:?}", legacy.requests());
    if let Some(py) = oracle(&home).await {
        for (path, body) in cases {
            assert_eq!(
                seen(&request_body(py.port, "POST", path, "", body).await),
                seen(&request_body(front.port, "POST", path, "", body).await),
                "{path} {body}"
            );
        }
    }
    assert_eq!(
        fs::read_to_string(home.hooks().join("snippets.json")).unwrap(),
        MESSY
    );
    front.stop().await;
}

#[tokio::test]
async fn ui_log_lock_contended_declines_without_writing() {
    let home = TestHome::new("uilog-lock");
    let old = "{\"ts\": 1.0, \"k\": \"previo\"}\n";
    home.write("ui-events.jsonl", old);
    let legacy = FakeLegacy::start().await;
    let front = front(&home, legacy.port, home.options()).await;
    let holder = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(home.hooks().join("ui-events.jsonl.lock"))
        .unwrap();
    holder.lock().unwrap();
    let body = r#"{"events": [{"k": "a"}]}"#;
    let wire = request_body(front.port, "POST", "/ui-log", "", body).await;
    assert_eq!(wire.text(), r#"{"legacy": true}"#, "declina al heredado");
    assert_eq!(
        fs::read_to_string(home.hooks().join("ui-events.jsonl")).unwrap(),
        old,
        "el archivo queda intacto"
    );
    assert_eq!(legacy.requests(), vec!["POST /ui-log HTTP/1.1".to_owned()]);
    holder.unlock().unwrap();
    let wire = request_body(front.port, "POST", "/ui-log", "", body).await;
    assert_eq!(wire.text(), r#"{"ok": true, "n": 1}"#, "libre: nativo");
    front.stop().await;
}

/// `"updated_at": <n>` → `0`, para comparar archivos escritos a horas distintas.
fn no_times(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(at) = rest.find("\"updated_at\": ") {
        let start = at + "\"updated_at\": ".len();
        out.push_str(&rest[..start]);
        out.push('0');
        let digits = rest[start..].bytes().take_while(u8::is_ascii_digit).count();
        rest = &rest[start + digits..];
    }
    out.push_str(rest);
    out
}

#[tokio::test]
async fn snippets_update_replaces_every_duplicate_id() {
    let start = r#"[{"id": "0123456789abcdef", "name": "uno", "body": "x"}, {"id": "aaaaaaaaaaaaaaaa", "name": "otro", "body": "y", "tags": [], "updated_at": 3}, {"id": "0123456789abcdef", "name": "dos", "body": "z", "tags": ["t"]}]"#;
    let rust_home = TestHome::new("snip-dup-rs");
    let python_home = TestHome::new("snip-dup-py");
    for home in [&rust_home, &python_home] {
        home.write("snippets.json", start);
    }
    let front = front(&rust_home, dead_port(), rust_home.options()).await;
    let body = r#"{"id": "0123456789abcdef", "name": " nuevo ", "body": "b", "tags": [" k "]}"#;
    let wire = request_body(front.port, "POST", "/snippets/update", "", body).await;
    assert_eq!(
        wire.text(),
        r#"{"item": {"id": "0123456789abcdef", "name": "nuevo", "body": "b", "tags": ["k"], "updated_at": 1791115200}}"#
    );
    let written = fs::read_to_string(rust_home.hooks().join("snippets.json")).unwrap();
    assert_eq!(
        written,
        r#"[{"id": "0123456789abcdef", "name": "nuevo", "body": "b", "tags": ["k"], "updated_at": 1791115200}, {"id": "aaaaaaaaaaaaaaaa", "name": "otro", "body": "y", "tags": [], "updated_at": 3}, {"id": "0123456789abcdef", "name": "nuevo", "body": "b", "tags": ["k"], "updated_at": 1791115200}]"#
    );
    if let Some(py) = oracle(&python_home).await {
        let a = request_body(py.port, "POST", "/snippets/update", "", body).await;
        assert_eq!(
            (a.status, no_times(&a.text())),
            (wire.status, no_times(&wire.text()))
        );
        assert_eq!(
            no_times(&fs::read_to_string(python_home.hooks().join("snippets.json")).unwrap()),
            no_times(&written)
        );
    }
    front.stop().await;
}

#[tokio::test]
async fn new_snippets_file_and_lock_are_private() {
    use std::os::unix::fs::PermissionsExt;
    let home = TestHome::new("snip-mode");
    let front = front(&home, dead_port(), home.options()).await;
    let wire = request_body(
        front.port,
        "POST",
        "/snippets",
        "",
        r#"{"name": "a", "body": "b"}"#,
    )
    .await;
    assert_eq!(wire.status, 200, "{}", wire.text());
    for name in ["snippets.json", "snippets.json.lock"] {
        let mode = fs::metadata(home.hooks().join(name))
            .unwrap()
            .permissions()
            .mode()
            & 0o7777;
        assert_eq!(mode, 0o600, "{name}: {mode:o}");
    }
    front.stop().await;
}
