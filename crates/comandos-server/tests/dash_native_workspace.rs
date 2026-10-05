//! Dominio C: workspace por el frente nativo, interoperando con el Python.
mod support;

use serde_json::{Value, json};
use std::{process::Stdio, sync::Arc};
use support::{
    FakeLegacy, TestHome, Wire, dead_port, front, get, oracle::oracle, request_body, tmux_available,
};

fn parse(text: &str) -> Value {
    serde_json::from_str(text).unwrap()
}

/// El documento de un payload: sin `revision` ni `ready`.
fn document_of(payload: &Value) -> Value {
    let mut doc = payload.as_object().unwrap().clone();
    doc.remove("revision");
    doc.remove("ready");
    Value::Object(doc)
}

/// Un parche con borradores cuyos `updatedAt` mezclan tipos: el `sorted` de
/// `_patch` en el Python lanza `TypeError` (500, sin escribir).
const MIXED_STAMPS: &str = r#"{"deviceId": "d1", "drafts": {"a": {"text": "x", "updatedAt": "x"}, "b": {"text": "y", "updatedAt": 1}}, "draftsPatch": {}}"#;

/// `sync-<rev>-<sha256(json.dumps(wanted, sort_keys=True))[:24]>` del Python.
fn sync_request_id(revision: i64, wanted: &Value) -> String {
    use sha2::{Digest, Sha256};
    let encoded = comandos_core::json::workspace_dumps_with_options(wanted, true, false).unwrap();
    let digest: String = Sha256::digest(encoded.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    format!("sync-{revision}-{}", &digest[..24])
}

fn revisions(home: &TestHome) -> i64 {
    let conn = rusqlite::Connection::open(home.state_db()).unwrap();
    conn.query_row(
        "SELECT COALESCE(MAX(revision), 0) FROM workspace_current",
        [],
        |r| r.get(0),
    )
    .unwrap_or(0)
}

/// Sesiones en el tmux privado del HOME (nunca el del usuario), en este orden:
/// los `session_id` (`$0`, `$1`…) salen iguales en dos HOME distintos.
fn tmux_sessions(home: &TestHome, names: &[&str]) -> bool {
    if !tmux_available() {
        eprintln!("tmux no está instalado: se salta la identidad de sesión");
        return false;
    }
    names.iter().all(|name| {
        home.tmux_command()
            .args(["new-session", "-d", "-s", name])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
    })
}

#[tokio::test]
async fn workspace_get_syncs_registry_once() {
    let home = TestHome::new("ws-get");
    home.write("app-tabs.json", r#"{"s1": "Uno"}"#);
    let front = front(&home, dead_port(), home.options()).await;
    let first = parse(&get(front.port, "/workspace").await.text());
    assert_eq!(first["revision"], 1);
    assert_eq!(first["ready"], true);
    let keys: Vec<&str> = first
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(&keys[keys.len() - 2..], ["revision", "ready"]);
    assert_eq!(
        parse(&get(front.port, "/workspace").await.text())["revision"],
        1,
        "sin cambios no hay revisión"
    );
    home.write("app-tabs.json", r#"{"s1": "Uno", "s2": "Dos"}"#);
    assert_eq!(
        parse(&get(front.port, "/workspace").await.text())["revision"],
        2
    );
    home.write("app-tabs.json", "{roto");
    assert_eq!(
        parse(&get(front.port, "/workspace").await.text())["revision"],
        2,
        "registro ilegible: no se toca"
    );
    front.stop().await;
}

#[tokio::test]
async fn workspace_post_validation_conflict_and_replay() {
    let home = TestHome::new("ws-post");
    home.write("app-tabs.json", r#"{"s1": "Uno"}"#);
    let front = front(&home, dead_port(), home.options()).await;
    let current = parse(&get(front.port, "/workspace").await.text());
    let rev = current["revision"].as_i64().unwrap();
    let doc = document_of(&current);
    let post = |body: Value| {
        let port = front.port;
        async move { request_body(port, "POST", "/workspace", "", &body.to_string()).await }
    };
    for bad in [json!("1"), json!(true), json!(1.0)] {
        let wire = post(json!({"document": doc, "expectedRevision": bad, "requestId": "r"})).await;
        assert_eq!(
            (wire.status, wire.text().as_str()),
            (400, r#"{"error": "expectedRevision inv\u00e1lido"}"#)
        );
    }
    let wire = post(json!({"document": doc, "expectedRevision": rev})).await;
    assert_eq!(
        (wire.status, wire.text().as_str()),
        (400, r#"{"error": "requestId inv\u00e1lido"}"#)
    );
    let mut other = doc.clone();
    let tab = other["tabs"]["s1"].clone();
    other["tabs"]
        .as_object_mut()
        .unwrap()
        .insert("zz".into(), tab);
    let wire = post(json!({"document": other, "expectedRevision": rev, "requestId": "r0"})).await;
    assert_eq!(wire.status, 400);
    let saved = post(json!({"document": doc, "expectedRevision": rev, "requestId": "r1"})).await;
    assert_eq!(saved.status, 200, "{}", saved.text());
    assert_eq!(parse(&saved.text())["revision"], rev + 1);
    let replay = post(json!({"document": doc, "expectedRevision": rev, "requestId": "r1"})).await;
    assert_eq!(
        parse(&replay.text())["revision"],
        rev + 1,
        "mismo requestId: misma revisión"
    );
    let stale = post(json!({"document": doc, "expectedRevision": rev, "requestId": "r2"})).await;
    assert_eq!(stale.status, 409);
    let body = parse(&stale.text());
    assert_eq!(body["error"], "Revisión desactualizada");
    assert_eq!(body["current"]["revision"], rev + 1);
    front.stop().await;
}

#[tokio::test]
async fn sort_restore_and_close_group_preview() {
    let home = TestHome::new("ws-sort");
    let legacy = FakeLegacy::start().await;
    home.write("app-tabs.json", r#"{"s1": "Uno", "s2": "Dos"}"#);
    tmux_sessions(&home, &["s1"]);
    let front = front(&home, legacy.port, home.options()).await;
    let current = parse(&get(front.port, "/workspace").await.text());
    let ids: Vec<String> = current["groups"]
        .as_array()
        .unwrap()
        .iter()
        .map(|g| g["id"].as_str().unwrap().to_owned())
        .collect();
    assert!(ids.len() >= 2, "{current}");
    let mut reversed = ids.clone();
    reversed.reverse();
    let sorted = request_body(
        front.port,
        "POST",
        "/workspace/sort",
        "",
        &json!({"restore": reversed}).to_string(),
    )
    .await;
    assert_eq!(sorted.status, 200, "{}", sorted.text());
    let sorted = parse(&sorted.text());
    assert_eq!(sorted["previous"], json!(ids));
    let keys: Vec<&str> = sorted
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(keys.last(), Some(&"previous"));
    // `by` necesita /state: lo responde el heredado.
    let by = request_body(
        front.port,
        "POST",
        "/workspace/sort",
        "",
        r#"{"by": "name"}"#,
    )
    .await;
    assert_eq!(by.text(), r#"{"legacy": true}"#);
    let missing = get(front.port, "/workspace/close-group?groupId=nope").await;
    assert_eq!(missing.status, 404);
    let group = &sorted["groups"][0]["id"];
    let preview = parse(
        &get(
            front.port,
            &format!("/workspace/close-group?groupId={}", group.as_str().unwrap()),
        )
        .await
        .text(),
    );
    let keys: Vec<&str> = preview
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(keys, ["groupId", "members", "revision"]);
    front.stop().await;
}

#[tokio::test]
async fn workspace_client_get_and_save() {
    let home = TestHome::new("ws-client");
    let front = front(&home, dead_port(), home.options()).await;
    assert_eq!(
        get(front.port, "/workspace/client?deviceId=d1")
            .await
            .text(),
        "{}"
    );
    let saved = request_body(
        front.port,
        "POST",
        "/workspace/client",
        "",
        r#"{"deviceId": "d1", "activeTabId": "local"}"#,
    )
    .await;
    assert_eq!(saved.status, 200, "{}", saved.text());
    // El POST responde `clean` en su orden; el GET, lo guardado (claves ordenadas).
    assert_eq!(
        get(front.port, "/workspace/client?deviceId=d1")
            .await
            .text(),
        r#"{"activePaneKey": null, "activeTabId": "local", "drafts": {}, "readingAnchors": {}}"#
    );
    assert_eq!(
        saved.text(),
        r#"{"activeTabId": "local", "activePaneKey": null, "drafts": {}, "readingAnchors": {}}"#
    );
    let bad = request_body(
        front.port,
        "POST",
        "/workspace/client",
        "",
        r#"{"deviceId": 5}"#,
    )
    .await;
    assert_eq!(
        (bad.status, bad.text().as_str()),
        (400, r#"{"error": "deviceId inv\u00e1lido"}"#)
    );
    front.stop().await;
}

/// Review Focus 5: lo que el Rust no reproduce con certeza se reenvía, y
/// declinar nunca confirma una revisión ni escribe un cliente.
#[tokio::test]
async fn exotic_inputs_decline_to_legacy() {
    let home = TestHome::new("ws-exotic");
    let legacy = FakeLegacy::start().await;
    home.write("app-tabs.json", r#"{"s1": "Uno"}"#);
    // `index: true`: el Python lo acepta (bool es int); el Rust declina.
    home.write(
        "app-sessions-v2.json",
        &json!({"version": 2, "sessions": {"s1": {"windows": [
            {"index": true, "name": "w", "active": 1, "width": 80, "height": 24, "layout": "b25e,80x24,0,0,1",
             "panes": [{"id": "%1", "active": true}]}]}}})
        .to_string(),
    );
    let front = front(&home, legacy.port, home.options()).await;
    let legacy_body = r#"{"legacy": true}"#;
    assert_eq!(get(front.port, "/workspace").await.text(), legacy_body);
    assert_eq!(
        get(front.port, "/workspace/close-group?groupId=group-s1")
            .await
            .text(),
        legacy_body
    );
    assert_eq!(revisions(&home), 0, "declinar nunca confirma una revisión");
    // Layout no ASCII (el `\d` de Python casa dígitos árabes).
    home.write(
        "app-sessions-v2.json",
        &json!({"version": 2, "sessions": {"s1": {"windows": [
            {"index": 0, "name": "w", "active": 1, "width": 80, "height": 24, "layout": "b25e,80x24,0,0,١",
             "panes": [{"id": "%1", "active": true}]}]}}})
        .to_string(),
    );
    assert_eq!(get(front.port, "/workspace").await.text(), legacy_body);
    // Registro con bytes no UTF-8: depende de la codificación del proceso.
    std::fs::remove_file(home.hooks().join("app-sessions-v2.json")).unwrap();
    std::fs::write(home.hooks().join("app-tabs.json"), b"{\"s1\": \"\xff\"}").unwrap();
    assert_eq!(get(front.port, "/workspace").await.text(), legacy_body);
    assert_eq!(revisions(&home), 0);
    for body in [
        r#"{"restore": ["a", 1]}"#,
        r#"{"by": "alpha"}"#,
        r#"{"restore": "a"}"#,
    ] {
        let wire = request_body(front.port, "POST", "/workspace/sort", "", body).await;
        assert_eq!(wire.text(), legacy_body, "{body}");
    }
    for body in [
        r#"{"deviceId": "d1", "drafts": [["a", {"text": "x"}]]}"#,
        r#"{"deviceId": "d1", "readingAnchors": "ab"}"#,
        r#"{"deviceId": "d1", "drafts": {"a": 5}, "draftsPatch": {}}"#,
        MIXED_STAMPS,
    ] {
        let wire = request_body(front.port, "POST", "/workspace/client", "", body).await;
        assert_eq!(wire.text(), legacy_body, "{body}");
    }
    // Un estado de cliente guardado ilegible: el texto del ValueError es de Python.
    let conn = rusqlite::Connection::open(home.state_db()).unwrap();
    conn.execute("INSERT INTO workspace_clients VALUES ('roto', '{x', 0)", [])
        .unwrap();
    let clients: i64 = conn
        .query_row("SELECT COUNT(*) FROM workspace_clients", [], |r| r.get(0))
        .unwrap();
    assert_eq!(clients, 1, "declinar nunca escribe un cliente");
    assert_eq!(
        get(front.port, "/workspace/client?deviceId=roto")
            .await
            .text(),
        legacy_body
    );
    let wire = request_body(
        front.port,
        "POST",
        "/workspace/client",
        "",
        r#"{"deviceId": "roto"}"#,
    )
    .await;
    assert_eq!(wire.text(), legacy_body);
    assert_eq!(legacy.requests().len(), 13);
    front.stop().await;
}

#[tokio::test]
async fn workspace_sync_interop_no_duplicate_revision() {
    let home = TestHome::new("ws-interop");
    home.write("app-tabs.json", r#"{"s1": "Uno"}"#);
    let front = front(&home, dead_port(), home.options()).await;
    let Some(py) = oracle(&home).await else {
        front.stop().await;
        return;
    };
    let rs = get(front.port, "/workspace").await.text();
    let python = get(py.port, "/workspace").await.text();
    assert_eq!(rs, python, "mismos bytes y misma revisión");
    assert_eq!(parse(&rs)["revision"], 1);
    home.write("app-tabs.json", r#"{"s1": "Uno", "s2": "Dos"}"#);
    // Veinte sincronizaciones concurrentes, mitad en cada proceso.
    let mut tasks = Vec::new();
    for i in 0..20 {
        let port = if i % 2 == 0 { front.port } else { py.port };
        tasks.push(tokio::spawn(
            async move { get(port, "/workspace").await.text() },
        ));
    }
    let mut bodies = Vec::new();
    for task in tasks {
        bodies.push(task.await.unwrap());
    }
    assert!(
        bodies.iter().all(|b| *b == bodies[0]),
        "todas ven el mismo acomodo"
    );
    assert_eq!(
        parse(&bodies[0])["revision"],
        2,
        "una sola revisión nueva para el mismo inventario"
    );
    assert_eq!(revisions(&home), 2);
    // Los `requestId` de las dos sincronías son los que calcula el Python:
    // `sync-<rev>-<sha256(json.dumps(wanted, sort_keys=True))[:24]>`.
    let mut expected = vec![
        sync_request_id(0, &document_of(&parse(&rs))),
        sync_request_id(1, &document_of(&parse(&bodies[0]))),
    ];
    expected.sort();
    let conn = rusqlite::Connection::open(home.state_db()).unwrap();
    let mut stmt = conn
        .prepare("SELECT request_id FROM workspace_requests ORDER BY request_id")
        .unwrap();
    let stored: Vec<String> = stmt
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(stored, expected);
    for target in [
        "/workspace/client?deviceId=x",
        "/workspace/close-group?groupId=nope",
    ] {
        let (a, b) = (get(py.port, target).await, get(front.port, target).await);
        assert_eq!((a.status, a.text()), (b.status, b.text()), "{target}");
    }
    front.stop().await;
}

fn seen(wire: &Wire) -> (u16, Option<String>, String) {
    (
        wire.status,
        wire.header("content-type").map(str::to_owned),
        wire.text(),
    )
}

/// Un snapshot válido con claves de pane, conversaciones y un `.bak`.
fn seed(home: &TestHome) {
    home.write(
        "app-tabs.json",
        r#"{"s1": "Uno ñ", "s2": "Dos", "hub": "Oculta", "mal nombre": "x", "s3": ""}"#,
    );
    home.write("prefs.json", r#"{"favorites": ["s2"]}"#);
    let window = |key: &str, mut pane: Value| {
        pane["key"] = json!(key);
        json!({"windows": [{"index": 0, "name": "w", "active": 1, "width": 80, "height": 24,
            "layout": "b25e,80x24,0,0,1", "panes": [pane]}]})
    };
    let snapshot = json!({"version": 2, "sessions": {
        "s1": window("k-s1", json!({"id": "%1", "active": true, "resume_id": "r1", "agent": "claude", "pid": 4242, "start": 17.5})),
        "s2": window("k-s2", json!({"id": "%1", "active": 1, "acp": {"sessionId": "acp-1"}, "pid": 7})),
    }});
    home.write("app-sessions-v2.json", &snapshot.to_string());
    home.write("app-sessions-v2.json.bak", "{roto");
}

/// Cada ruta y cada error contra el oráculo, en secuencia, sobre dos HOME
/// gemelos (mismas pestañas, mismo snapshot, mismas sesiones tmux).
#[tokio::test]
async fn every_route_and_error_matches_python_oracle() {
    let (py_home, rs_home) = (TestHome::new("ws-py-seq"), TestHome::new("ws-rs-seq"));
    let mut identities = true;
    for home in [&py_home, &rs_home] {
        seed(home);
        identities &= tmux_sessions(home, &["s1", "s2"]);
    }
    let mut opts = rs_home.options();
    opts.clock = Arc::new(comandos_server::dash::native::wall_clock_ms);
    let front = front(&rs_home, dead_port(), opts).await;
    let Some(py) = oracle(&py_home).await else {
        front.stop().await;
        return;
    };
    let mut step = 0;
    let mut both = async |method: &str, path: &str, body: Option<String>| -> Value {
        step += 1;
        let (a, b) = match &body {
            None => (get(py.port, path).await, get(front.port, path).await),
            Some(body) => (
                request_body(py.port, method, path, "", body).await,
                request_body(front.port, method, path, "", body).await,
            ),
        };
        assert_eq!(seen(&a), seen(&b), "paso {step}: {method} {path} {body:?}");
        serde_json::from_str(&a.text()).unwrap_or(Value::Null)
    };
    let current = both("GET", "/workspace", None).await;
    assert_eq!(current["revision"], 1, "{current}");
    both("GET", "/workspace?x=1", None).await;
    for target in [
        "/workspace/close-group?groupId=group-s1",
        "/workspace/close-group?groupId=group-local",
        "/workspace/close-group?groupId=group-s2&groupId=group-s1",
        "/workspace/close-group?groupId=nope",
        "/workspace/close-group",
        "/workspace/client",
        "/workspace/client?deviceId=d1",
    ] {
        both("GET", target, None).await;
    }
    if identities {
        let preview = both("GET", "/workspace/close-group?groupId=group-s2", None).await;
        assert_eq!(preview["members"][0]["sessionId"], "$1", "{preview}");
    }
    for body in [
        json!({"deviceId": 5}),
        json!({"deviceId": ""}),
        json!({"deviceId": "x".repeat(201)}),
        json!({"deviceId": "d1", "activeTabId": 7}),
        json!({"deviceId": "d1", "activeTabId": "s1", "activePaneKey": "k-s1",
               "draftsPatch": {"s1": {"text": "hola", "updatedAt": 5, "selStart": 2, "selEnd": 9}}}),
        json!({"deviceId": "d1", "anchorsPatch": {"s1": {"text": "a", "updatedAt": 6, "ratio": 1}}}),
        json!({"deviceId": "d1", "draftsPatch": {"s1": null}}),
        json!({"deviceId": "d1", "draftsPatch": []}),
        json!({"deviceId": "d1", "draftsPatch": {"s1": 3}}),
        json!({"deviceId": "d1", "anchorsPatch": {"": {"text": "a"}}}),
        json!({"deviceId": "d1", "drafts": null, "readingAnchors": {}}),
    ] {
        both("POST", "/workspace/client", Some(body.to_string())).await;
    }
    both("GET", "/workspace/client?deviceId=d1", None).await;
    let current = both("GET", "/workspace", None).await;
    let rev = current["revision"].as_i64().unwrap();
    let doc = document_of(&current);
    let mut fewer = doc.clone();
    fewer["tabs"].as_object_mut().unwrap().remove("s2");
    fewer["groups"] = json!(
        doc["groups"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|g| g["id"] != "group-s2")
            .collect::<Vec<_>>()
    );
    // Sin el enlace del pane quitado, valida: solo falla la comparación de tabs.
    let mut unbound = fewer.clone();
    unbound["bindings"].as_object_mut().unwrap().remove("k-s2");
    let mut split = doc.clone();
    split["groups"] = json!([
        {"id": "group-local", "tree": {"type": "tab", "tabId": "local"}},
        {"id": "par", "tree": {"type": "split", "axis": "x", "ratio": 0.25,
            "first": {"type": "tab", "tabId": "s1"}, "second": {"type": "tab", "tabId": "s2"}}},
    ]);
    for body in [
        json!({"document": doc, "expectedRevision": "1", "requestId": "r"}),
        json!({"document": doc, "expectedRevision": 1.0, "requestId": "r"}),
        json!({"document": doc, "expectedRevision": false, "requestId": "r"}),
        json!({"expectedRevision": rev, "requestId": "r"}),
        json!({"document": {"schema": 1, "groups": [], "tabs": []}, "expectedRevision": rev, "requestId": "r"}),
        json!({"document": {"schema": 1, "groups": [{"id": "g"}], "tabs": {}}, "expectedRevision": rev, "requestId": "r"}),
        json!({"document": fewer, "expectedRevision": rev, "requestId": "r"}),
        json!({"document": unbound, "expectedRevision": rev, "requestId": "r"}),
        json!({"document": doc, "expectedRevision": rev}),
        json!({"document": doc, "expectedRevision": rev, "requestId": ""}),
        json!({"document": doc, "expectedRevision": rev, "requestId": 5}),
        json!({"document": doc, "expectedRevision": rev, "requestId": "r".repeat(201)}),
        json!({"document": split, "expectedRevision": rev + 5, "requestId": "r-split"}),
        json!({"document": split, "expectedRevision": rev, "requestId": "r-split"}),
        json!({"document": split, "expectedRevision": rev, "requestId": "r-split"}),
        json!({"document": doc, "expectedRevision": rev, "requestId": "r-split"}),
        json!({"document": doc, "expectedRevision": rev, "requestId": "r-otra"}),
    ] {
        both("POST", "/workspace", Some(body.to_string())).await;
    }
    both("GET", "/workspace", None).await;
    for body in [
        json!({"restore": ["par", "group-local"]}),
        json!({"restore": []}),
        json!({"restore": ["no", "par", "par"]}),
    ] {
        let sorted = both("POST", "/workspace/sort", Some(body.to_string())).await;
        assert!(sorted["previous"].is_array(), "{sorted}");
    }
    both("GET", "/workspace/close-group?groupId=par", None).await;
    // Cerrar una pestaña en el registro: la sincronía la quita de su grupo.
    for home in [&py_home, &rs_home] {
        home.write("app-tabs.json", r#"{"s1": "Uno ñ"}"#);
    }
    both("GET", "/workspace", None).await;
    both("GET", "/workspace/close-group?groupId=par", None).await;
    front.stop().await;
}

/// Un snapshot que el Python lee pero con el que revienta (`acp` verdadero
/// no-objeto: 500 antes de confirmar nada) y el respaldo `.bak` cuando el
/// principal no es válido: mismos bytes en los dos, sobre el mismo HOME.
#[tokio::test]
async fn snapshot_edges_match_python_oracle() {
    let home = TestHome::new("ws-snap-edges");
    home.write("app-tabs.json", r#"{"s1": "Uno"}"#);
    let pane = |extra: Value| {
        let mut pane = json!({"id": "%1", "active": true, "key": "k1"});
        pane.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        json!({"version": 2, "sessions": {"s1": {"windows": [{"index": 0, "name": "w",
            "active": 1, "width": 80, "height": 24, "layout": "b25e,80x24,0,0,1",
            "panes": [pane]}]}}})
        .to_string()
    };
    home.write("app-sessions-v2.json", &pane(json!({"acp": "x"})));
    let front = front(&home, dead_port(), home.options()).await;
    let Some(py) = oracle(&home).await else {
        front.stop().await;
        return;
    };
    let (a, b) = (
        get(py.port, "/workspace").await,
        get(front.port, "/workspace").await,
    );
    assert_eq!(seen(&a), seen(&b));
    assert_eq!(a.status, 500);
    assert_eq!(revisions(&home), 0, "el 500 llega antes de confirmar");
    // Principal inválido (checksum), respaldo válido con otra clave de pane.
    home.write(
        "app-sessions-v2.json",
        &pane(json!({"acp": {"sessionId": "a1"}})).replace("b25e", "0000"),
    );
    home.write(
        "app-sessions-v2.json.bak",
        &pane(json!({"resume_id": "r9"})),
    );
    let rs = get(front.port, "/workspace").await;
    let python = get(py.port, "/workspace").await;
    assert_eq!(seen(&rs), seen(&python));
    assert_eq!(
        parse(&rs.text())["bindings"]["k1"]["conversation"]["id"],
        "r9"
    );
    front.stop().await;
}

/// Con el Python como heredado del frente: el cuerpo de `updatedAt` mezclados
/// se declina, el Python responde su 500 y nadie escribe el cliente.
#[tokio::test]
async fn client_mixed_stamps_forward_to_python_500() {
    let home = TestHome::new("ws-mixed-stamps");
    let Some(py) = oracle(&home).await else {
        return;
    };
    let front = front(&home, py.port, home.options()).await;
    let direct = request_body(py.port, "POST", "/workspace/client", "", MIXED_STAMPS).await;
    let forwarded = request_body(front.port, "POST", "/workspace/client", "", MIXED_STAMPS).await;
    assert_eq!(direct.status, 500, "{}", direct.text());
    assert_eq!(seen(&direct), seen(&forwarded));
    let conn = rusqlite::Connection::open(home.state_db()).unwrap();
    let clients: i64 = conn
        .query_row("SELECT COUNT(*) FROM workspace_clients", [], |r| r.get(0))
        .unwrap();
    assert_eq!(clients, 0, "ni el Python ni el frente escriben");
    front.stop().await;
}
