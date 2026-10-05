//! Dominio G2: POST /pane/type por el frente, con su caché por requestId.
//! Esta ruta teclea en los panes vivos del usuario: lo que más importa es que
//! un reintento con el mismo `requestId` nunca teclee dos veces, ni en Rust ni
//! al pasar entre Rust y el Python.
mod support;

use comandos_server::{
    ReplyBody, Request,
    dash::native::{
        Native, NativeRoute, Outcome,
        tmux::{Program, Tmux},
    },
};
use std::{ffi::OsString, time::Duration};
use support::{
    FakeLegacy, TestHome, Wire, dead_port, front, get, oracle::oracle, request_body, tmux_available,
};

fn tmux(home: &TestHome, args: &[&str]) -> String {
    // `-S` al socket privado: nunca el servidor del usuario.
    let out = home.tmux_command().args(args).output().unwrap();
    String::from_utf8(out.stdout).unwrap()
}

/// Sesión `s1` con dos paneles `cat` (%0 y %1): el eco de la terminal deja
/// ver exactamente lo tecleado.
fn cat_session(home: &TestHome) {
    tmux(
        home,
        &[
            "new-session",
            "-d",
            "-s",
            "s1",
            "-x",
            "200",
            "-y",
            "40",
            "cat",
        ],
    );
    tmux(home, &["split-window", "-h", "-t", "=s1:", "cat"]);
}

fn seen(wire: &Wire) -> (u16, String) {
    (wire.status, wire.text())
}

async fn typed_in(home: &TestHome, pane: &str) -> String {
    tokio::time::sleep(Duration::from_millis(200)).await;
    tmux(home, &["capture-pane", "-p", "-J", "-t", pane])
        .trim()
        .to_owned()
}

async fn typed_text(home: &TestHome) -> String {
    typed_in(home, "%0").await
}

/// `durationMs` difiere entre dos tecleos: se iguala a 0 antes de comparar.
fn masked(text: &str) -> String {
    let Some(start) = text.find(r#""durationMs": "#) else {
        return text.to_owned();
    };
    let digits = start + r#""durationMs": "#.len();
    let end = text[digits..]
        .find(|c: char| !c.is_ascii_digit())
        .map_or(text.len(), |n| digits + n);
    format!("{}0{}", &text[..digits], &text[end..])
}

fn post_request(body: &str) -> Request {
    Request {
        method: http::Method::POST,
        target: "/pane/type".into(),
        peer: "127.0.0.1:12345".parse().unwrap(),
        headers: vec![],
        data: Some(serde_json::from_str(body).unwrap()),
        body: bytes::Bytes::copy_from_slice(body.as_bytes()),
        internal_producer: false,
    }
}

#[tokio::test]
async fn pane_type_types_literal_text() {
    if !tmux_available() {
        return;
    }
    let home = TestHome::new("type-ok");
    cat_session(&home);
    let front = front(&home, dead_port(), home.options()).await;
    let body = r#"{"session": "s1", "pane": "%0", "text": "a;b ñ", "requestId": "r1"}"#;
    let wire = request_body(front.port, "POST", "/pane/type", "", body).await;
    assert_eq!(wire.status, 200, "{}", wire.text());
    assert!(
        wire.text()
            .starts_with(r#"{"ok": true, "typed": 5, "durationMs": "#),
        "{}",
        wire.text()
    );
    assert!(wire.text().ends_with(r#", "requestId": "r1"}"#));
    assert_eq!(typed_text(&home).await, "a;b ñ");
    front.stop().await;
}

#[tokio::test]
async fn pane_type_retry_same_request_id_types_once() {
    if !tmux_available() {
        return;
    }
    let home = TestHome::new("type-retry");
    cat_session(&home);
    let front = front(&home, dead_port(), home.options()).await;
    let body = r#"{"session": "s1", "pane": "%0", "text": "xyz", "requestId": "rep"}"#;
    let first = request_body(front.port, "POST", "/pane/type", "", body).await;
    let second = request_body(front.port, "POST", "/pane/type", "", body).await;
    assert_eq!(
        seen(&first),
        seen(&second),
        "misma respuesta, mismo durationMs"
    );
    // El caché es por requestId: otro texto con el mismo id tampoco teclea.
    let other = r#"{"session": "s1", "pane": "%0", "text": "otro", "requestId": "rep"}"#;
    let third = request_body(front.port, "POST", "/pane/type", "", other).await;
    assert_eq!(seen(&first), seen(&third));
    assert_eq!(typed_text(&home).await, "xyz", "una sola vez");
    front.stop().await;
}

#[tokio::test]
async fn pane_type_errors_match_python_oracle() {
    if !tmux_available() {
        return;
    }
    let home = TestHome::new("type-oracle");
    cat_session(&home);
    let Some(py) = oracle(&home).await else {
        return;
    };
    let front = front(&home, dead_port(), home.options()).await;
    let long = "x".repeat(2001);
    let cases = [
        r#"{"session": "a b", "text": "x"}"#.to_owned(),
        r#"{"session": 5, "text": "x"}"#.to_owned(),
        r#"{"text": "x"}"#.to_owned(),
        r#"{"session": "nadie", "pane": "%0", "text": "x"}"#.to_owned(),
        r#"{"session": "s1", "text": "x"}"#.to_owned(),
        r#"{"session": "s1", "pane": "s1", "text": "x"}"#.to_owned(),
        r#"{"session": "s1", "pane": "%99", "text": "x"}"#.to_owned(),
        r#"{"session": "s1", "pane": "%0", "text": "   "}"#.to_owned(),
        r#"{"session": "s1", "pane": "%0", "text": "\u001c\u0085"}"#.to_owned(),
        r#"{"session": "s1", "pane": "%0", "text": 7}"#.to_owned(),
        r#"{"session": "s1", "pane": "%0"}"#.to_owned(),
        r#"{"session": "s1", "pane": "%0", "text": "a\nb"}"#.to_owned(),
        r#"{"session": "s1", "pane": "%0", "text": "a\u007fb"}"#.to_owned(),
        format!(r#"{{"session": "s1", "pane": "%0", "text": "{long}"}}"#),
        // Con requestId, un error nunca entra en la caché.
        r#"{"session": "s1", "pane": "%99", "text": "x", "requestId": 12}"#.to_owned(),
        r#"{"session": "s1", "pane": "%99", "text": "x", "requestId": 12}"#.to_owned(),
    ];
    for body in &cases {
        assert_eq!(
            seen(&request_body(py.port, "POST", "/pane/type", "", body).await),
            seen(&request_body(front.port, "POST", "/pane/type", "", body).await),
            "{body}"
        );
    }
    assert_eq!(typed_text(&home).await, "", "ningún error teclea");
    front.stop().await;
}

/// Mismo tecleo por el Python (en %0) y por el frente (en %1): bytes iguales
/// salvo `durationMs`, y el reintento de cada uno devuelve su respuesta
/// guardada sin volver a teclear.
#[tokio::test]
async fn pane_type_and_retry_match_python_oracle() {
    if !tmux_available() {
        return;
    }
    let home = TestHome::new("type-oracle-ok");
    cat_session(&home);
    let Some(py) = oracle(&home).await else {
        return;
    };
    let front = front(&home, dead_port(), home.options()).await;
    let on = |pane: &str| {
        format!(r#"{{"session": "s1", "pane": "{pane}", "text": "dí;x", "requestId": "same"}}"#)
    };
    let py_first = request_body(py.port, "POST", "/pane/type", "", &on("%0")).await;
    let rs_first = request_body(front.port, "POST", "/pane/type", "", &on("%1")).await;
    assert_eq!(py_first.status, 200, "{}", py_first.text());
    assert_eq!(
        (py_first.status, masked(&py_first.text())),
        (rs_first.status, masked(&rs_first.text()))
    );
    assert_eq!(
        py_first.header("content-type"),
        rs_first.header("content-type")
    );
    let py_again = request_body(py.port, "POST", "/pane/type", "", &on("%0")).await;
    let rs_again = request_body(front.port, "POST", "/pane/type", "", &on("%1")).await;
    assert_eq!(seen(&py_first), seen(&py_again));
    assert_eq!(seen(&rs_first), seen(&rs_again));
    assert_eq!(typed_in(&home, "%0").await, "dí;x");
    assert_eq!(typed_in(&home, "%1").await, "dí;x");
    front.stop().await;
}

/// Dos peticiones al mismo pane: la segunda es el 409 del Python (en ambos).
#[tokio::test]
async fn pane_type_busy_pane_matches_python_oracle() {
    if !tmux_available() {
        return;
    }
    let home = TestHome::new("type-busy-oracle");
    cat_session(&home);
    let Some(py) = oracle(&home).await else {
        return;
    };
    let front = front(&home, dead_port(), home.options()).await;
    let mut busy = Vec::new();
    for (port, pane) in [(py.port, "%0"), (front.port, "%1")] {
        let long = format!(
            r#"{{"session": "s1", "pane": "{pane}", "text": "{}", "requestId": "largo"}}"#,
            "y".repeat(300)
        );
        let first =
            tokio::spawn(async move { request_body(port, "POST", "/pane/type", "", &long).await });
        tokio::time::sleep(Duration::from_millis(300)).await;
        let short =
            format!(r#"{{"session": "s1", "pane": "{pane}", "text": "z", "requestId": "corto"}}"#);
        busy.push(seen(
            &request_body(port, "POST", "/pane/type", "", &short).await,
        ));
        assert_eq!(first.await.unwrap().status, 200);
        // El 409 no se guardó: con el pane libre, ese mismo id sí teclea.
        assert_eq!(
            request_body(port, "POST", "/pane/type", "", &short)
                .await
                .status,
            200
        );
    }
    assert_eq!(busy[0], busy[1]);
    assert_eq!(
        busy[1],
        (
            409,
            r#"{"error": "Ya se est\u00e1 escribiendo en ese pane; espera a que termine.", "code": "typing_in_progress"}"#
                .to_owned()
        )
    );
    for pane in ["%0", "%1"] {
        let text = typed_in(&home, pane).await;
        assert_eq!(text.matches('y').count(), 300, "{pane}");
        assert_eq!(text.matches('z').count(), 1, "{pane}");
    }
    front.stop().await;
}

#[tokio::test]
async fn declined_request_id_stays_declined() {
    if !tmux_available() {
        return;
    }
    let home = TestHome::new("type-decline");
    cat_session(&home);
    home.write(
        "state/proyecto.json",
        r#"{"project": "s1", "session": "s1"}"#,
    );
    let legacy = FakeLegacy::start().await;
    let front = front(&home, legacy.port, home.options()).await;
    let body = r#"{"session": "s1", "pane": "%0", "text": "x", "requestId": "dup"}"#;
    assert_eq!(
        request_body(front.port, "POST", "/pane/type", "", body)
            .await
            .text(),
        r#"{"legacy": true}"#
    );
    std::fs::remove_file(home.hooks().join("state/proyecto.json")).unwrap();
    // Sin el estado ya sería nativa, pero ese requestId lo atendió el Python.
    assert_eq!(
        request_body(front.port, "POST", "/pane/type", "", body)
            .await
            .text(),
        r#"{"legacy": true}"#
    );
    // Un id nuevo sí es nativo ya.
    let fresh = r#"{"session": "s1", "pane": "%0", "text": "n", "requestId": "nuevo"}"#;
    assert_eq!(
        request_body(front.port, "POST", "/pane/type", "", fresh)
            .await
            .status,
        200
    );
    assert_eq!(typed_text(&home).await, "n", "Rust solo tecleó el id nuevo");
    assert_eq!(legacy.requests().len(), 2);
    front.stop().await;
}

/// De punta a punta con el Python real detrás: la primera vez lo atiende el
/// Python (un estado nombra la sesión), el reintento vuelve a él aunque el
/// estado ya no esté, y su caché responde sin teclear otra vez.
#[tokio::test]
async fn declined_request_id_is_answered_by_python_cache() {
    if !tmux_available() {
        return;
    }
    let home = TestHome::new("type-decline-py");
    cat_session(&home);
    home.write(
        "state/proyecto.json",
        r#"{"project": "s1", "session": "s1"}"#,
    );
    let Some(py) = oracle(&home).await else {
        return;
    };
    let front = front(&home, py.port, home.options()).await;
    let body = r#"{"session": "s1", "pane": "%0", "text": "una", "requestId": "py-1"}"#;
    let first = request_body(front.port, "POST", "/pane/type", "", body).await;
    assert_eq!(first.status, 200, "{}", first.text());
    std::fs::remove_file(home.hooks().join("state/proyecto.json")).unwrap();
    let again = request_body(front.port, "POST", "/pane/type", "", body).await;
    assert_eq!(seen(&first), seen(&again));
    assert_eq!(typed_text(&home).await, "una", "una sola vez");
    front.stop().await;
}

#[tokio::test]
async fn pane_type_busy_pane_is_409() {
    if !tmux_available() {
        return;
    }
    let home = TestHome::new("type-busy");
    cat_session(&home);
    let front = front(&home, dead_port(), home.options()).await;
    let port = front.port;
    let long = format!(
        r#"{{"session": "s1", "pane": "%0", "text": "{}"}}"#,
        "y".repeat(400)
    );
    let first =
        tokio::spawn(async move { request_body(port, "POST", "/pane/type", "", &long).await });
    tokio::time::sleep(Duration::from_millis(300)).await;
    let busy = request_body(
        port,
        "POST",
        "/pane/type",
        "",
        r#"{"session": "s1", "pane": "%0", "text": "z"}"#,
    )
    .await;
    assert_eq!(
        seen(&busy),
        (
            409,
            r#"{"error": "Ya se est\u00e1 escribiendo en ese pane; espera a que termine.", "code": "typing_in_progress"}"#
                .to_owned()
        )
    );
    // Otro pane no espera al primero.
    let other = request_body(
        port,
        "POST",
        "/pane/type",
        "",
        r#"{"session": "s1", "pane": "%1", "text": "w"}"#,
    )
    .await;
    assert_eq!(other.status, 200, "{}", other.text());
    assert_eq!(first.await.unwrap().status, 200);
    front.stop().await;
}

/// Si la petición se suelta a medio teclear (cliente que corta, plazo del
/// transporte), el tecleo termina, el candado se libera y la respuesta entra
/// en la caché: el reintento la recibe sin teclear otra vez.
#[tokio::test]
async fn abandoned_request_still_caches_and_releases_the_pane() {
    if !tmux_available() {
        return;
    }
    let home = TestHome::new("type-abandon");
    cat_session(&home);
    let native = Native::new(home.options());
    let body = format!(
        r#"{{"session": "s1", "pane": "%0", "text": "{}", "requestId": "suelta"}}"#,
        "q".repeat(200)
    );
    let request = post_request(&body);
    let cut = tokio::time::timeout(
        Duration::from_millis(250),
        native.dispatch(NativeRoute::PaneType, &request),
    )
    .await;
    assert!(cut.is_err(), "la petición se soltó a medio teclear");
    let mut answered = None;
    for _ in 0..100 {
        tokio::time::sleep(Duration::from_millis(100)).await;
        match native.dispatch(NativeRoute::PaneType, &request).await {
            Ok(Outcome::Reply(reply)) if reply.status == http::StatusCode::OK => {
                answered = Some(reply);
                break;
            }
            Ok(Outcome::Reply(reply)) => {
                assert_eq!(
                    reply.status,
                    http::StatusCode::CONFLICT,
                    "solo 409 mientras teclea"
                );
            }
            _ => panic!("ni declinar ni fallar"),
        }
    }
    let reply = answered.expect("el reintento recibe la respuesta guardada");
    let ReplyBody::Bytes(bytes) = reply.body else {
        panic!("cuerpo en memoria")
    };
    let text = String::from_utf8(bytes.to_vec()).unwrap();
    assert!(text.ends_with(r#", "requestId": "suelta"}"#), "{text}");
    assert_eq!(typed_text(&home).await.matches('q').count(), 200);
    native.shutdown().await;
}

/// Un requestId que el frente tecleó se responde de su caché aunque después se
/// apague el conjunto nativo (base más nueva): reenviarlo haría teclear al Python.
#[tokio::test]
async fn typed_request_id_survives_native_shutdown() {
    if !tmux_available() {
        return;
    }
    let home = TestHome::new("type-disabled");
    cat_session(&home);
    let legacy = FakeLegacy::start().await;
    let front = front(&home, legacy.port, home.options()).await;
    let body = r#"{"session": "s1", "pane": "%0", "text": "uno", "requestId": "antes"}"#;
    let first = request_body(front.port, "POST", "/pane/type", "", body).await;
    assert_eq!(first.status, 200, "{}", first.text());
    let known = *comandos_server::dash::native::state::known_versions()
        .iter()
        .max()
        .unwrap();
    let conn = rusqlite::Connection::open(home.state_db()).unwrap();
    conn.execute(
        "INSERT INTO schema_migrations (version, name, applied_at) VALUES (?, 'futuro', 0)",
        [known + 1],
    )
    .unwrap();
    drop(conn);
    // Una ruta de la base descubre el esquema nuevo y apaga el conjunto.
    assert_eq!(
        get(front.port, "/notices/prefs").await.text(),
        r#"{"legacy": true}"#
    );
    let again = request_body(front.port, "POST", "/pane/type", "", body).await;
    assert_eq!(seen(&first), seen(&again));
    // Lo demás ya va al heredado.
    let fresh = r#"{"session": "s1", "pane": "%0", "text": "dos", "requestId": "despues"}"#;
    assert_eq!(
        request_body(front.port, "POST", "/pane/type", "", fresh)
            .await
            .text(),
        r#"{"legacy": true}"#
    );
    assert_eq!(typed_text(&home).await, "uno");
    front.stop().await;
}

/// El mismo `requestId` mientras el primero aún teclea: el pane está tomado y
/// la respuesta es el 409 del Python (en ambos); el reintento de después sale
/// de la caché y el texto se tecleó una sola vez.
#[tokio::test]
async fn same_request_id_while_typing_matches_python_oracle() {
    if !tmux_available() {
        return;
    }
    let home = TestHome::new("type-same-rid");
    cat_session(&home);
    let Some(py) = oracle(&home).await else {
        return;
    };
    let front = front(&home, dead_port(), home.options()).await;
    let mut seen_busy = Vec::new();
    for (port, pane) in [(py.port, "%0"), (front.port, "%1")] {
        let body = format!(
            r#"{{"session": "s1", "pane": "{pane}", "text": "{}", "requestId": "doble"}}"#,
            "k".repeat(300)
        );
        let again = body.clone();
        let first =
            tokio::spawn(async move { request_body(port, "POST", "/pane/type", "", &again).await });
        tokio::time::sleep(Duration::from_millis(300)).await;
        seen_busy.push(seen(
            &request_body(port, "POST", "/pane/type", "", &body).await,
        ));
        let first = first.await.unwrap();
        assert_eq!(first.status, 200, "{}", first.text());
        let retry = request_body(port, "POST", "/pane/type", "", &body).await;
        assert_eq!(seen(&first), seen(&retry), "{pane}: la respuesta guardada");
        assert_eq!(
            typed_in(&home, pane).await.matches('k').count(),
            300,
            "{pane}"
        );
    }
    assert_eq!(seen_busy[0], seen_busy[1]);
    assert_eq!(seen_busy[1].0, 409);
    front.stop().await;
}

/// La carrera de la revisión: B no encuentra la caché, se queda en
/// `has-session` (un tmux lento) mientras A teclea y termina, y luego toma el
/// pane libre. Con el candado tomado vuelve a mirar la caché: responde lo de A
/// y no teclea otra vez.
#[tokio::test]
async fn same_request_id_finishing_during_resolution_types_once() {
    if !tmux_available() {
        return;
    }
    let home = TestHome::new("type-race");
    cat_session(&home);
    let mut opts = home.options();
    let mut program = Program::named("sh");
    program.prefix = vec![
        OsString::from("-c"),
        OsString::from(
            r#"[ "$1" = has-session ] && sleep 1; exec tmux -f /dev/null -S "$SOCKET" "$@""#,
        ),
        OsString::from("sh"),
    ];
    // Socket explícito: solo `TMUX_TMPDIR` cae en el servidor real si el
    // directorio desaparece.
    program.env.push((
        "SOCKET".into(),
        support::private_socket_of(&home.tmux_dir()).into_os_string(),
    ));
    program
        .env
        .push(("TMUX_TMPDIR".into(), home.tmux_dir().into_os_string()));
    program.env_remove.push("TMUX".into());
    opts.tmux = Tmux {
        program,
        timeout: Duration::from_secs(5),
    };
    let front = front(&home, dead_port(), opts).await;
    let port = front.port;
    let body = r#"{"session": "s1", "pane": "%0", "text": "ab", "requestId": "carrera"}"#;
    let a = tokio::spawn(async move { request_body(port, "POST", "/pane/type", "", body).await });
    tokio::time::sleep(Duration::from_millis(500)).await;
    let b = request_body(port, "POST", "/pane/type", "", body).await;
    let a = a.await.unwrap();
    assert_eq!(a.status, 200, "{}", a.text());
    assert_eq!(seen(&a), seen(&b), "B recibe la respuesta de A");
    assert_eq!(typed_text(&home).await, "ab", "una sola vez");
    front.stop().await;
}
