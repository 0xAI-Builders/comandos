//! Corte tabs, T2: registro y cierre de pestañas contra el Python (gemelo).
//!
//! Confinamiento: cada lado del gemelo tiene su HOME temporal corto, su tmux
//! privado (`-S`) y su `fakebin` confinado; el frente y el Python llaman a tmux
//! por el guardián que anota en `<HOME>/tmux.log`. Las sesiones `g1`/`g2` las
//! siembra la prueba con `run_tmux` (`-S`, proceso `cat` o un agente falso que
//! es una copia de `sleep`). La única orden destructiva es el `kill-session
//! -t =g2` de `close_group_recreated_member_closes_nothing`, por `run_tmux`
//! sobre la sesión que la misma prueba sembró. La limpieza es el `Drop` de
//! cada `TestHome` (`kill-server -S` y después borrar).
mod support;

use comandos_server::{
    Request,
    dash::native::{Native, NativeRoute, Outcome, files::FileLock, tabs::TabsRoute},
};
use serde_json::{Value, json};
use std::{sync::Arc, time::Duration};
use support::{
    TestHome, run_tmux,
    tabs::{REGISTRY_FILES, options_for, read_normalized, seed_registry},
    tmux_available,
    twin::{Twin, TwinRun},
};

/// Registro de partida: `g1` y `g2` vivas y abiertas como pestañas.
fn seed(h: &TestHome) {
    seed_registry(h);
    h.write("app-tabs.json", r#"{"g1": "Uno", "g2": "Dos"}"#);
    run_tmux(h, &["new-session", "-d", "-s", "g1", "cat"]);
    run_tmux(h, &["new-session", "-d", "-s", "g2", "cat"]);
}

/// Como `seed`, con un agente (copia de `sleep` llamada `codex`) en el pane de
/// `g1` y su registro de estado.
fn seed_agent(h: &TestHome) {
    seed_registry(h);
    h.write("app-tabs.json", r#"{"g1": "Uno", "g2": "Dos"}"#);
    let agent = support::fake_agent(h, "codex");
    let cmd = format!("{} 600", agent.display());
    run_tmux(h, &["new-session", "-d", "-s", "g1", &cmd]);
    run_tmux(h, &["new-session", "-d", "-s", "g2", "cat"]);
    h.write(
        "state/g1.json",
        r#"{"project": "g1", "agent": "codex", "status": "working", "ts": 1}"#,
    );
}

fn json_of(text: &str) -> Value {
    serde_json::from_str(text).unwrap_or(Value::Null)
}

/// Las llamadas a tmux de cada lado desde la última vez (y vacía los registros).
fn take_logs(t: &Twin) -> (Vec<Vec<String>>, Vec<Vec<String>>) {
    let (a, b) = (t.tmux_log_a(), t.tmux_log_b());
    for home in [&t.a, &t.b] {
        let _ = std::fs::remove_file(home.root.join("tmux.log"));
    }
    (a, b)
}

/// La misma petición a los dos lados: mismos bytes, mismos archivos del
/// registro y las mismas órdenes de tmux (lecturas incluidas).
async fn same(t: &Twin, path: &str, body: &str) -> TwinRun {
    same_logged(t, path, body).await.0
}

/// `same`, devolviendo además las órdenes de tmux (iguales en los dos lados).
async fn same_logged(t: &Twin, path: &str, body: &str) -> (TwinRun, Vec<Vec<String>>) {
    take_logs(t);
    let run = t.post(path, body).await;
    run.assert_same();
    t.files_equal(&REGISTRY_FILES).unwrap();
    let (a, b) = take_logs(t);
    assert_eq!(a, b, "órdenes de tmux de {path} {body}");
    (run, a)
}

fn argv(calls: &[&[&str]]) -> Vec<Vec<String>> {
    calls
        .iter()
        .map(|call| call.iter().map(|a| (*a).to_owned()).collect())
        .collect()
}

fn session_alive(h: &TestHome, sess: &str) -> bool {
    h.tmux_command()
        .args(["has-session", "-t", &format!("={sess}")])
        .status()
        .unwrap()
        .success()
}

#[tokio::test]
async fn tab_routes_match_python() {
    let Some(t) = Twin::start("tabs", seed).await else {
        return;
    };
    let (_, log) = same_logged(
        &t,
        "/tab-register",
        r#"{"session":"g1","label":"Uno","kind":"project","cwd":"/tmp"}"#,
    )
    .await;
    assert_eq!(log, argv(&[&["has-session", "-t", "=g1"]]));
    for (path, body) in [
        ("/tab-register", r#"{"session":"nope","label":"X"}"#),
        ("/tab-register", r#"{"session":"bad name"}"#),
        ("/tab-register", r#"{"label":"sin sesión"}"#),
        ("/tab-register", r#"{"session":5}"#),
        ("/tab-register", r#"{"session":null}"#),
        (
            "/tab-register",
            r#"{"session":"g2","label":7,"kind":false,"host":0,"cwd":[]}"#,
        ),
        (
            "/tab-register",
            r#"{"session":"g2","label":"","kind":"rara"}"#,
        ),
        ("/tab-register", r#"{"session":"local","label":"L"}"#),
        (
            "/tab-metadata",
            r#"{"session":"g2","kind":"ssh","host":"x"}"#,
        ),
        ("/tab-metadata", r#"{"session":"g2","kind":"rara"}"#),
        ("/tab-metadata", r#"{"session":"g2","kind":true}"#),
        (
            "/tab-metadata",
            r#"{"session":"g1","kind":"shell","cwd":"relativa"}"#,
        ),
        ("/tab-metadata-remove", r#"{"session":"g2"}"#),
        ("/tab-metadata-remove", r#"{"session":"nadie"}"#),
        ("/tab-close", r#"{"session":"g1"}"#),
        ("/tab-close", r#"{"session":"g1"}"#),
        ("/tab-close", r#"{"session":"local"}"#),
        ("/tab-close", r#"{"session":"g2","ephemeral":true}"#),
        ("/tab-close", r#"{"session":"g2","ephemeral":1}"#),
        ("/tab-close", r#"{"session":"nadie"}"#),
        (
            "/tab-close",
            r#"{"session":"comandos-e2e-x","ephemeral":true}"#,
        ),
    ] {
        same(&t, path, body).await;
    }
    for home in [&t.a, &t.b] {
        assert!(
            session_alive(home, "g1"),
            "cerrar la pestaña no mata la sesión"
        );
        assert!(session_alive(home, "g2"));
    }
}

/// Un `str()` que el frente no reproduce (flotante o contenedor verdadero)
/// declina ANTES de leer nada: el heredado recibe la petición original.
#[tokio::test]
async fn unrepresentable_str_declines_before_effects() {
    if !tmux_available() {
        return;
    }
    let home = TestHome::new_short("tabs-decline");
    seed(&home);
    let legacy = support::FakeLegacy::start().await;
    let fr = support::front(&home, legacy.port, options_for(&home)).await;
    for body in [
        r#"{"session":"g1","label":1.5}"#,
        r#"{"session":"g1","kind":["shell"]}"#,
        r#"{"session":"g1","cwd":{"a":1}}"#,
    ] {
        let _ = support::request_body(fr.port, "POST", "/tab-register", "", body).await;
    }
    let _ = support::request_body(
        fr.port,
        "POST",
        "/tab-metadata",
        "",
        r#"{"session":"g1","kind":"shell","host":2.0}"#,
    )
    .await;
    assert_eq!(legacy.requests().len(), 4, "{:?}", legacy.requests());
    // Ni una orden de tmux ni una escritura del registro.
    assert!(support::twin::tmux_log(&home).is_empty());
    assert!(!support::tabs::exists(&home, "app-tab-open.json"));
    fr.stop().await;
}

/// Cerrar la pestaña de un pane con un agente: el historial lleva su agente y
/// el pane (y su proceso) siguen vivos.
#[tokio::test]
async fn closing_an_agent_tab_keeps_the_agent() {
    let Some(t) = Twin::start("tabs-agent", seed_agent).await else {
        return;
    };
    let (_, log) = same_logged(&t, "/tab-close", r#"{"session":"g1"}"#).await;
    assert_eq!(
        log,
        argv(&[
            &["list-sessions", "-F", "#{session_name}"],
            &[
                "display-message",
                "-p",
                "-t",
                "=g1:",
                "#{pane_current_path}"
            ],
        ])
    );
    for home in [&t.a, &t.b] {
        let history = read_normalized(home, "app-tabs-history.json").unwrap();
        assert!(history.contains(r#""agent": "codex""#), "{history}");
        let cmd = run_tmux(
            home,
            &["list-panes", "-t", "=g1", "-F", "#{pane_current_command}"],
        );
        assert_eq!(cmd.trim(), "codex", "el agente sigue en su pane");
    }
}

/// Dos cierres concurrentes de la misma pestaña: los dos responden, el estado
/// final es el de un cierre, igual que el Python con dos hilos.
#[tokio::test]
async fn concurrent_close_matches_python() {
    let Some(t) = Twin::start("tabs-conc", seed).await else {
        return;
    };
    let body = r#"{"session":"g1"}"#;
    let (x, y) = tokio::join!(t.post("/tab-close", body), t.post("/tab-close", body));
    for run in [&x, &y] {
        run.assert_same();
        assert_eq!(run.front.status, 200);
    }
    t.files_equal(&REGISTRY_FILES).unwrap();
    for home in [&t.a, &t.b] {
        let tabs = json_of(&read_normalized(home, "app-tabs.json").unwrap());
        assert_eq!(tabs, json!({"g2": "Dos"}));
        assert!(session_alive(home, "g1"));
    }
    assert_eq!(t.tmux_mutations_a(), Vec::<Vec<String>>::new());
    assert_eq!(t.tmux_mutations_b(), Vec::<Vec<String>>::new());
}

/// El grupo `g` con `g1` y `g2` en los dos lados (POST `/workspace` con el
/// documento de GET `/workspace` reorganizado). Devuelve la vista previa.
async fn group_g(t: &Twin) -> Value {
    let current = t.get("/workspace").await;
    current.assert_same();
    let current = json_of(&current.front.text());
    let rev = current["revision"].clone();
    let mut doc = current.clone();
    let doc_map = doc.as_object_mut().unwrap();
    doc_map.remove("revision");
    doc_map.remove("ready");
    doc["groups"] = json!([
        {"id": "group-local", "tree": {"type": "tab", "tabId": "local"}},
        {"id": "g", "tree": {"type": "split", "axis": "x", "ratio": 0.5,
            "first": {"type": "tab", "tabId": "g1"}, "second": {"type": "tab", "tabId": "g2"}}},
    ]);
    let body = json!({"document": doc, "expectedRevision": rev, "requestId": "r-grupo"});
    let saved = t.post("/workspace", &body.to_string()).await;
    saved.assert_same();
    assert_eq!(saved.front.status, 200, "{}", saved.front.text());
    let preview = t.get("/workspace/close-group?groupId=g").await;
    preview.assert_same();
    json_of(&preview.front.text())
}

/// `{"groupId","expectedRevision","members","requestId"}` desde la vista previa.
fn close_group_body(preview: &Value, rid: &str) -> String {
    json!({
        "groupId": preview["groupId"],
        "expectedRevision": preview["revision"],
        "members": preview["members"],
        "requestId": rid,
    })
    .to_string()
}

#[tokio::test]
async fn close_group_recreated_member_closes_nothing() {
    let Some(t) = Twin::start("cg", seed).await else {
        return;
    };
    let preview = group_g(&t).await;
    assert_eq!(preview["members"].as_array().map(Vec::len), Some(2));
    // g2 recreada: otro session_id con el mismo nombre (en el tmux privado de cada lado).
    for h in [&t.a, &t.b] {
        run_tmux(h, &["kill-session", "-t", "=g2"]); // socket privado -S de la prueba, sesión sembrada aquí
        run_tmux(h, &["new-session", "-d", "-s", "g2", "cat"]);
    }
    let run = same(
        &t,
        "/workspace/close-group",
        &close_group_body(&preview, "req-cg-1"),
    )
    .await;
    assert_eq!(run.front.status, 400, "{}", run.front.text());
    assert!(
        run.front
            .text()
            .contains("cambi\\u00f3. No se ha cerrado nada"),
        "{}",
        run.front.text()
    );
    let tabs = json_of(&read_normalized(&t.a, "app-tabs.json").unwrap());
    assert_eq!(tabs, json!({"g1": "Uno", "g2": "Dos"}), "no se cerró nada");
    assert!(!support::tabs::exists(&t.a, "app-tab-close.json"));
}

#[tokio::test]
async fn close_group_closes_members_like_python() {
    let Some(t) = Twin::start("cg-ok", seed_agent).await else {
        return;
    };
    let preview = group_g(&t).await;
    let body = close_group_body(&preview, "req-cg-ok");
    let (run, log) = same_logged(&t, "/workspace/close-group", &body).await;
    assert_eq!(run.front.status, 200, "{}", run.front.text());
    // Identidad de cada miembro y, por miembro, las lecturas del cierre. Ni un
    // `kill-*`: cerrar el grupo cierra pestañas, no sesiones.
    assert_eq!(
        log,
        argv(&[
            &["display-message", "-p", "-t", "=g1:", "#{session_id}"],
            &["display-message", "-p", "-t", "=g2:", "#{session_id}"],
            &["list-sessions", "-F", "#{session_name}"],
            &[
                "display-message",
                "-p",
                "-t",
                "=g1:",
                "#{pane_current_path}"
            ],
            &["list-sessions", "-F", "#{session_name}"],
            &[
                "display-message",
                "-p",
                "-t",
                "=g2:",
                "#{pane_current_path}"
            ],
        ])
    );
    assert_eq!(
        json_of(&run.front.text())["closed"],
        json!(["g1", "g2"]),
        "{}",
        run.front.text()
    );
    let signal = json_of(&std::fs::read_to_string(t.a.hooks().join("app-tab-close.json")).unwrap());
    assert_eq!(signal["sessions"], json!(["g1", "g2"]));
    assert_eq!(signal["session"], "g2");
    // El resultado guardado para la repetición: el mismo texto en las dos bases.
    let meta = |home: &TestHome| -> String {
        rusqlite::Connection::open(home.state_db())
            .unwrap()
            .query_row(
                "SELECT value FROM workspace_meta WHERE key = 'close-group:req-cg-ok'",
                [],
                |row| row.get(0),
            )
            .unwrap()
    };
    assert_eq!(meta(&t.a), meta(&t.b));
    // El workspace queda igual en los dos lados.
    t.get("/workspace").await.assert_same();
    // Repetición del mismo `requestId`: el resultado guardado y la señal de nuevo.
    for home in [&t.a, &t.b] {
        std::fs::remove_file(home.hooks().join("app-tab-close.json")).unwrap();
    }
    let replay = same(&t, "/workspace/close-group", &body).await;
    assert_eq!(replay.front.text(), run.front.text());
    assert!(support::tabs::exists(&t.a, "app-tab-close.json"));
    // Ninguna sesión ni pane murió; el agente sigue en el suyo.
    for home in [&t.a, &t.b] {
        assert!(session_alive(home, "g1") && session_alive(home, "g2"));
        let cmd = run_tmux(
            home,
            &["list-panes", "-t", "=g1", "-F", "#{pane_current_command}"],
        );
        assert_eq!(cmd.trim(), "codex");
    }
    assert_eq!(t.tmux_mutations_a(), Vec::<Vec<String>>::new());
}

#[tokio::test]
async fn close_group_errors_match_python() {
    let Some(t) = Twin::start("cg-err", seed).await else {
        return;
    };
    let preview = group_g(&t).await;
    let rev = preview["revision"].clone();
    let members = preview["members"].clone();
    let mut relabeled = members.clone();
    relabeled[1]["label"] = json!(5);
    relabeled[1]["sessionId"] = json!("$999");
    let mut wrong_session = members.clone();
    wrong_session[0]["session"] = json!("otra");
    for body in [
        json!({"expectedRevision": true, "members": []}),
        json!({"expectedRevision": 1.0, "members": []}),
        json!({"expectedRevision": "1", "members": []}),
        json!({"expectedRevision": rev, "members": {}}),
        json!({"expectedRevision": rev}),
        json!({"groupId": "g", "expectedRevision": rev, "members": members}),
        json!({"groupId": "g", "expectedRevision": rev, "members": members, "requestId": 5}),
        json!({"groupId": "g", "expectedRevision": rev, "members": members, "requestId": "x".repeat(201)}),
        json!({"groupId": "nope", "expectedRevision": rev, "members": members, "requestId": "r1"}),
        json!({"groupId": 7, "expectedRevision": rev, "members": members, "requestId": "r2"}),
        json!({"groupId": "g", "expectedRevision": 999, "members": members, "requestId": "r3"}),
        json!({"groupId": "g", "expectedRevision": rev, "members": [], "requestId": "r4"}),
        json!({"groupId": "g", "expectedRevision": rev, "members": [members[0]], "requestId": "r5"}),
        json!({"groupId": "g", "expectedRevision": rev, "members": [members[0], 3], "requestId": "r6"}),
        json!({"groupId": "nope", "expectedRevision": rev, "members": [{"tabId": ["g1"]}], "requestId": "r7"}),
        json!({"groupId": "g", "expectedRevision": rev, "members": [{"tabId": {"a": 1}}, {"tabId": []}], "requestId": "r8"}),
        json!({"groupId": "g", "expectedRevision": 999, "members": [{"tabId": []}], "requestId": "r9"}),
        json!({"groupId": "g", "expectedRevision": rev, "members": relabeled, "requestId": "r10"}),
        json!({"groupId": "g", "expectedRevision": rev, "members": wrong_session, "requestId": "r11"}),
    ] {
        let run = same(&t, "/workspace/close-group", &body.to_string()).await;
        assert_ne!(run.front.status, 200, "{body}: {}", run.front.text());
    }
    let tabs = json_of(&read_normalized(&t.a, "app-tabs.json").unwrap());
    assert_eq!(tabs, json!({"g1": "Uno", "g2": "Dos"}), "no se cerró nada");
}

fn post_request(target: &str, body: &str) -> Request {
    Request {
        method: http::Method::POST,
        target: target.into(),
        peer: "127.0.0.1:12345".parse().unwrap(),
        headers: vec![],
        data: Some(serde_json::from_str(body).unwrap()),
        body: bytes::Bytes::copy_from_slice(body.as_bytes()),
        internal_producer: false,
    }
}

/// Espera (hasta 10 s) a que `<hooks>/<name>` exista.
async fn wait_for(home: &TestHome, name: &str) -> bool {
    for _ in 0..200 {
        if support::tabs::exists(home, name) {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    false
}

/// Regla de cancelación (revisión de la T1): una petición que se suelta a
/// mitad (aquí, mientras «cc-app» tiene el `flock` del espejo tras escribirse
/// el historial) termina igual. Estado final = el de un cierre completo en
/// otro HOME idéntico.
#[tokio::test]
async fn dropped_close_still_completes() {
    if !tmux_available() {
        return;
    }
    let (a, b) = (
        TestHome::new_short("tabs-drop-a"),
        TestHome::new_short("tabs-drop-b"),
    );
    seed(&a);
    seed(&b);
    let (na, nb) = (
        Arc::new(Native::new(options_for(&a))),
        Arc::new(Native::new(options_for(&b))),
    );
    let route = NativeRoute::Tabs(TabsRoute::Close);
    let request = post_request("/tab-close", r#"{"session":"g1"}"#);
    match nb.dispatch(route, &request).await.unwrap() {
        Outcome::Reply(reply) => assert_eq!(reply.status, http::StatusCode::OK),
        Outcome::Decline => panic!("declinó"),
    }
    let lock = FileLock::acquire(&a.hooks().join("app-tabs.json")).unwrap();
    let cut = tokio::time::timeout(Duration::from_millis(300), na.dispatch(route, &request)).await;
    assert!(cut.is_err(), "la petición se soltó a mitad del cierre");
    drop(lock);
    assert!(
        wait_for(&a, "app-tab-close.json").await,
        "el cierre no terminó"
    );
    for name in REGISTRY_FILES {
        assert_eq!(
            read_normalized(&a, name),
            read_normalized(&b, name),
            "{name}"
        );
    }
    na.shutdown().await;
    nb.shutdown().await;
}

/// La misma regla en el cierre de grupo: la señal agregada se escribe aunque
/// la petición se suelte mientras el worker espera el `flock` del espejo.
#[tokio::test]
async fn dropped_close_group_still_signals() {
    let Some(t) = Twin::start("cg-drop", seed).await else {
        return;
    };
    let preview = group_g(&t).await;
    let body = close_group_body(&preview, "req-cg-drop");
    let lock = FileLock::acquire(&t.a.hooks().join("app-tabs.json")).unwrap();
    let front = t.front.port;
    let cut = tokio::time::timeout(
        Duration::from_millis(300),
        support::request_body(front, "POST", "/workspace/close-group", "", &body),
    )
    .await;
    assert!(cut.is_err(), "la petición se soltó a mitad del cierre");
    drop(lock);
    let oracle = t.post_oracle("/workspace/close-group", &body).await;
    assert_eq!(oracle.status, 200);
    // La señal del frente es la agregada (con `sessions`).
    let mut aggregated = false;
    for _ in 0..200 {
        if read_normalized(&t.a, "app-tab-close.json").is_some_and(|s| s.contains("sessions")) {
            aggregated = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(aggregated, "sin señal agregada");
    t.files_equal(&REGISTRY_FILES).unwrap();
    // Y la repetición devuelve lo mismo que el Python.
    t.post("/workspace/close-group", &body).await.assert_same();
}

#[tokio::test]
async fn tab_close_waits_for_cc_app_lock() {
    if !tmux_available() {
        return;
    }
    let home = TestHome::new_short("tabs-lock");
    seed(&home);
    let lock = FileLock::acquire(&home.hooks().join("app-tabs.json")).unwrap();
    let legacy = support::FakeLegacy::start().await;
    let fr = support::front(&home, legacy.port, options_for(&home)).await;
    let pending = tokio::spawn({
        let port = fr.port;
        async move { support::request_body(port, "POST", "/tab-close", "", r#"{"session":"g1"}"#).await }
    });
    tokio::time::sleep(Duration::from_millis(300)).await;
    // Mientras espera el candado, otra ruta nativa responde (runtime libre).
    let other = support::get(fr.port, "/snippets").await;
    assert_eq!(other.status, 200);
    assert!(!pending.is_finished());
    // «cc-app» escribe y suelta.
    std::fs::write(
        home.hooks().join("app-tabs.json"),
        r#"{"g1":"Uno","g2":"Dos","cc-app":"suya"}"#,
    )
    .unwrap();
    drop(lock);
    let wire = pending.await.unwrap();
    assert_eq!(wire.status, 200, "{}", wire.text());
    let tabs = std::fs::read_to_string(home.hooks().join("app-tabs.json")).unwrap();
    assert!(
        tabs.contains("cc-app") && !tabs.contains("\"g1\""),
        "{tabs}"
    );
    assert!(legacy.requests().is_empty());
    fr.stop().await;
}
