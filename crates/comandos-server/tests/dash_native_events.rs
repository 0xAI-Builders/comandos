//! Dominio B: marcas y eventos v2 por el frente nativo, y la importación
//! única de events.jsonl compartida con el Python.
mod support;

use comandos_core::json::response_dumps;
use rusqlite::Connection;
use serde_json::Value;
use support::{FakeLegacy, TestHome, Wire, dead_port, front, get, oracle::oracle, request_body};

fn parse(text: &str) -> Value {
    serde_json::from_str(text).unwrap()
}

#[tokio::test]
async fn events_and_marks_are_answered_natively() {
    let home = TestHome::new("events");
    let front = front(&home, dead_port(), home.options()).await;
    let mark = request_body(
        front.port,
        "POST",
        "/work-marks",
        "",
        r#"{"scope": "session", "key": "s", "value": "resolved", "expectedRevision": 0}"#,
    )
    .await;
    assert_eq!(mark.status, 200, "{}", mark.text());
    assert!(mark.text().starts_with(
        r#"{"mark": {"scope": "session", "key": "s", "mark": "resolved", "favorite": false, "revision": 1, "#
    ));
    let stale = request_body(
        front.port,
        "POST",
        "/work-marks",
        "",
        r#"{"scope": "session", "key": "s", "value": "frozen", "expectedRevision": 0}"#,
    )
    .await;
    // `json.dumps` del Python escapa lo no ASCII (`ensure_ascii`).
    assert_eq!(stale.status, 409, "{}", stale.text());
    assert!(
        stale
            .text()
            .starts_with(r#"{"error": "Revisi\u00f3n desactualizada", "current": {"#)
    );
    let marks = parse(&get(front.port, "/work-marks").await.text());
    assert_eq!(marks["marks"][0]["key"], "s");
    let bad = get(front.port, "/events/v2?after=x").await;
    assert_eq!(
        (bad.status, bad.text().as_str()),
        (400, r#"{"error": "after inv\u00e1lido"}"#)
    );
    let origin = format!("Origin: http://127.0.0.1:{}\r\n", front.port);
    let foreign = request_body(
        front.port,
        "POST",
        "/events/v2",
        &origin,
        r#"{"hookEvent": "Stop"}"#,
    )
    .await;
    assert_eq!(
        (foreign.status, foreign.text().as_str()),
        (
            403,
            r#"{"error": "Solo productores internos de este equipo"}"#
        )
    );
    let ignored = request_body(
        front.port,
        "POST",
        "/events/v2",
        "",
        r#"{"hookEvent": "Unknown"}"#,
    )
    .await;
    assert_eq!(
        (ignored.status, ignored.text().as_str()),
        (202, r#"{"ignored": true}"#)
    );
    let accepted = request_body(
        front.port,
        "POST",
        "/events/v2",
        "",
        r#"{"hookEvent": "UserPromptSubmit", "agent": "codex", "session": "s", "pane": "%3", "turnId": "t1", "occurredAtMs": 1000}"#,
    )
    .await;
    assert_eq!(accepted.status, 200, "{}", accepted.text());
    assert_eq!(parse(&accepted.text())["event"]["kind"], "prompt_accepted");
    let page = parse(&get(front.port, "/events/v2?turns=1").await.text());
    assert_eq!(page["latest"], 1);
    assert!(page.get("turns").is_some());
    front.stop().await;
}

#[tokio::test]
async fn legacy_timeline_is_imported_once_across_rust_and_python() {
    let home = TestHome::new("events-legacy");
    home.write(
        "events.jsonl",
        "{\"ts\": 1790000000, \"project\": \"p\", \"status\": \"done\", \"detail\": \"hola\"}\n\
         {\"ts\": 1790000060, \"project\": \"p\", \"status\": \"waiting\", \"detail\": \"permiso\"}\n",
    );
    let front = front(&home, dead_port(), home.options()).await;
    let first = parse(&get(front.port, "/events/v2").await.text());
    let imported = first["latest"].as_i64().unwrap();
    assert!(
        imported >= 1,
        "la importación heredada crea eventos: {first}"
    );
    let again = parse(&get(front.port, "/events/v2").await.text());
    assert_eq!(again["latest"], imported, "una sola importación");
    // El Python sobre la misma base ve la marca y no reimporta.
    if let Some(py) = oracle(&home).await {
        let python = get(py.port, "/events/v2").await.text();
        assert_eq!(parse(&python)["latest"], imported);
        assert_eq!(python, get(front.port, "/events/v2").await.text());
        assert_eq!(
            get(py.port, "/work-marks").await.text(),
            get(front.port, "/work-marks").await.text()
        );
    }
    front.stop().await;
}

#[tokio::test]
async fn python_written_mark_is_read_by_rust_and_vice_versa() {
    let home = TestHome::new("events-interop");
    let front = front(&home, dead_port(), home.options()).await;
    let Some(py) = oracle(&home).await else {
        front.stop().await;
        return;
    };
    let body = r#"{"scope": "session", "key": "py", "value": "frozen", "expectedRevision": 0}"#;
    assert_eq!(
        request_body(py.port, "POST", "/work-marks", "", body)
            .await
            .status,
        200
    );
    let body = r#"{"scope": "session", "key": "rs", "value": "resolved", "expectedRevision": 0}"#;
    assert_eq!(
        request_body(front.port, "POST", "/work-marks", "", body)
            .await
            .status,
        200
    );
    assert_eq!(
        get(py.port, "/work-marks").await.text(),
        get(front.port, "/work-marks").await.text()
    );
    front.stop().await;
}

/// Una consulta que el Rust no puede comparar con certeza (U+FFFD tras
/// decodificar) se reenvía sin tocar la base: ni siquiera importa el
/// `events.jsonl` heredado.
#[tokio::test]
async fn unsure_query_declines_before_importing() {
    let home = TestHome::new("events-decline");
    home.write(
        "events.jsonl",
        "{\"ts\": 1790000000, \"project\": \"p\", \"status\": \"done\"}\n",
    );
    let legacy = FakeLegacy::start().await;
    let front = front(&home, legacy.port, home.options()).await;
    let wire = get(front.port, "/events/v2?turns=%FF").await;
    assert_eq!(wire.text(), r#"{"legacy": true}"#);
    assert_eq!(legacy.requests().len(), 1);
    let page = parse(&get(front.port, "/events/v2?after=999").await.text());
    assert_eq!(page["events"], serde_json::json!([]));
    assert!(
        page["latest"].as_i64().unwrap() >= 1,
        "importa al responder"
    );
    front.stop().await;
}

/// Quita los campos que dependen del reloj o de ids aleatorios (como los
/// punteros `volatile` del arnés de paridad) y vuelve a volcar con el
/// `json.dumps` del Python, así separadores, escapes y orden siguen
/// comparándose; sin nada que quitar, se comparan los bytes del cable.
fn stable(wire: &Wire) -> (u16, Option<String>, String) {
    fn scrub(value: &mut Value) -> bool {
        match value {
            Value::Object(map) => {
                let mut changed = false;
                for (key, inner) in map.iter_mut() {
                    if matches!(
                        key.as_str(),
                        "updatedAtMs" | "receivedAtMs" | "eventId" | "lastEventId" | "receiptId"
                    ) {
                        *inner = Value::Null;
                        changed = true;
                    } else {
                        changed |= scrub(inner);
                    }
                }
                changed
            }
            Value::Array(items) => items.iter_mut().fold(false, |acc, item| scrub(item) | acc),
            _ => false,
        }
    }
    let text = match serde_json::from_slice::<Value>(&wire.body) {
        Ok(mut value) => {
            if scrub(&mut value) {
                response_dumps(&value).unwrap()
            } else {
                wire.text()
            }
        }
        Err(_) => wire.text(),
    };
    (
        wire.status,
        wire.header("content-type").map(str::to_owned),
        text,
    )
}

/// Cada ruta y cada error, en la misma secuencia contra el Python y el
/// frente, cada uno sobre su propio HOME (las escrituras no se cruzan).
#[tokio::test]
async fn every_route_and_error_matches_python_oracle() {
    let rust_home = TestHome::new("events-oracle-rs");
    let python_home = TestHome::new("events-oracle-py");
    let timeline =
        "{\"ts\": 1790000000, \"project\": \"p\", \"status\": \"done\", \"detail\": \"hola\"}\n";
    rust_home.write("events.jsonl", timeline);
    python_home.write("events.jsonl", timeline);
    let Some(py) = oracle(&python_home).await else {
        return;
    };
    let front = front(&rust_home, dead_port(), rust_home.options()).await;
    // `{port}` se sustituye por el puerto de cada lado: mismo origen, que
    // pasa la puerta y llega a la comprobación de productor interno.
    let origin = "Origin: http://127.0.0.1:{port}\r\n";
    let forwarded = "X-Forwarded-For: 10.0.0.1\r\n";
    let steps: &[(&str, &str, &str, &str)] = &[
        ("GET", "/events/v2?after=x", "", ""),
        ("GET", "/events/v2?limit=-1", "", ""),
        ("GET", "/events/v2?after=1234567890123456", "", ""),
        ("GET", "/events/v2?after=%D9%A1", "", ""),
        ("GET", "/events/v2", "", ""),
        ("GET", "/events/v2?limit=1&after=0&after=x", "", ""),
        ("GET", "/events/v2?after=&turns=1", "", ""),
        (
            "POST",
            "/work-marks",
            "",
            r#"{"scope": "session", "key": "k", "value": "resolved", "expectedRevision": 0}"#,
        ),
        (
            "POST",
            "/work-marks",
            "",
            r#"{"scope": "session", "key": "k", "value": "frozen", "expectedRevision": 0}"#,
        ),
        (
            "POST",
            "/work-marks",
            "",
            r#"{"scope": "nope", "key": "k", "value": "resolved", "expectedRevision": 0}"#,
        ),
        (
            "POST",
            "/work-marks",
            "",
            r#"{"scope": "session", "key": "k", "value": "raro", "expectedRevision": 1}"#,
        ),
        ("POST", "/work-marks", "", r#"{"key": "k"}"#),
        (
            "POST",
            "/work-marks",
            "",
            r#"{"scope": "pane", "key": "p", "value": "favorite", "expectedRevision": 0}"#,
        ),
        ("POST", "/events/v2", origin, r#"{"hookEvent": "Stop"}"#),
        ("POST", "/events/v2", forwarded, r#"{"hookEvent": "Stop"}"#),
        ("POST", "/events/v2", "", r#"{"hookEvent": "Unknown"}"#),
        ("POST", "/events/v2", "", r#"{"hookEvent": []}"#),
        ("POST", "/events/v2", "", r#"{"kind": "nope"}"#),
        (
            "POST",
            "/events/v2",
            "",
            r#"{"hookEvent": "UserPromptSubmit", "agent": "codex", "session": "s", "pane": "%3", "turnId": "t1", "occurredAtMs": 1000}"#,
        ),
        (
            "POST",
            "/events/v2",
            "",
            r#"{"hookEvent": "Stop", "agent": "codex", "session": "s", "pane": "%3", "turnId": "t1", "occurredAtMs": 2000}"#,
        ),
        ("GET", "/events/v2?after=0&limit=2&turns=1", "", ""),
        ("GET", "/events/v2?after=1", "", ""),
        ("GET", "/events/v2?limit=0&after=0", "", ""),
        ("GET", "/work-marks", "", ""),
        ("GET", "/work-marks?x=1", "", ""),
    ];
    for (method, target, extra, body) in steps {
        let call = |port| async move {
            if *method == "GET" {
                get(port, target).await
            } else {
                let extra = extra.replace("{port}", &port.to_string());
                request_body(port, method, target, &extra, body).await
            }
        };
        let (a, b) = (call(py.port).await, call(front.port).await);
        assert_eq!(stable(&a), stable(&b), "{method} {target} {body}");
    }
    front.stop().await;
}

/// Corrompe la primera fila de `events` (un BLOB donde va texto): la
/// lectura falla en el Rust y el Python responde lo suyo.
fn corrupt_first_event(home: &TestHome) {
    let conn = Connection::open(home.state_db()).unwrap();
    conn.execute("UPDATE events SET title = X'FF' WHERE sequence = 1", [])
        .unwrap();
}

#[tokio::test]
async fn unreadable_row_declines_once_imported_and_fails_on_first_import() {
    let home = TestHome::new("events-corrupt");
    home.write(
        "events.jsonl",
        "{\"ts\": 1790000000, \"project\": \"p\", \"status\": \"done\"}\n",
    );
    let legacy = FakeLegacy::start().await;
    let first_front = front(&home, legacy.port, home.options()).await;
    // La primera importación ocurre aquí (la fila aún se lee bien).
    let first = get(first_front.port, "/events/v2").await;
    assert_eq!(first.status, 200, "{}", first.text());
    corrupt_first_event(&home);
    for target in ["/events/v2", "/events/v2?turns=1", "/work-marks"] {
        let wire = get(first_front.port, target).await;
        assert_eq!(wire.text(), r#"{"legacy": true}"#, "{target}");
    }
    assert_eq!(legacy.requests().len(), 3);
    first_front.stop().await;

    // En un proceso nuevo la importación corre en la misma petición que
    // falla al leer: pudo escribir, así que no se reenvía.
    let second_front = front(&home, legacy.port, home.options()).await;
    let wire = get(second_front.port, "/events/v2").await;
    assert_eq!(
        (wire.status, wire.text().as_str()),
        (500, r#"{"error": "Error interno del tablero"}"#)
    );
    assert_eq!(legacy.requests().len(), 3);
    second_front.stop().await;
}

/// Con el Python como heredado: lo que el frente reenvía por una fila
/// ilegible es exactamente lo que el Python responde directamente.
#[tokio::test]
async fn unreadable_row_answer_is_pythons() {
    let home = TestHome::new("events-corrupt-oracle");
    home.write(
        "events.jsonl",
        "{\"ts\": 1790000000, \"project\": \"p\", \"status\": \"done\"}\n",
    );
    let Some(py) = oracle(&home).await else {
        return;
    };
    let front = front(&home, py.port, home.options()).await;
    assert_eq!(get(front.port, "/events/v2").await.status, 200);
    corrupt_first_event(&home);
    for target in ["/events/v2", "/work-marks"] {
        let (a, b) = (get(py.port, target).await, get(front.port, target).await);
        assert_eq!((a.status, a.text()), (b.status, b.text()), "{target}");
    }
    front.stop().await;
}

/// El Python ignora la consulta de GET /work-marks: U+FFFD ahí no reenvía.
#[tokio::test]
async fn work_marks_ignores_unsure_query() {
    let home = TestHome::new("events-marks-query");
    let legacy = FakeLegacy::start().await;
    let front = front(&home, legacy.port, home.options()).await;
    let wire = get(front.port, "/work-marks?x=%FF").await;
    assert_eq!(wire.status, 200, "{}", wire.text());
    assert!(
        wire.text().starts_with(r#"{"marks": []"#),
        "{}",
        wire.text()
    );
    assert!(legacy.requests().is_empty());
    front.stop().await;
}
