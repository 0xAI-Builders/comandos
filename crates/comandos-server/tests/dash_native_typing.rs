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
use support::{FakeLegacy, TestHome, Wire, dead_port, front, get, request_body, tmux_available};

struct TypedOriginal<'a> {
    source: Option<support::oracle::Oracle>,
    home: &'a TestHome,
    family: &'static str,
    sequence: std::sync::atomic::AtomicUsize,
}
impl<'a> TypedOriginal<'a> {
    async fn start(home: &'a TestHome, family: &'static str) -> Self {
        let source = if matches!(
            std::env::var("COMANDOS_ORACLE").as_deref(),
            Ok("record" | "check")
        ) {
            let root = support::frozen::reference(&home.root).unwrap();
            let opts = support::oracle::OracleOpts {
                python_prelude: format!("dash.time.time=lambda: {}", support::NOW_MS / 1000),
                extra_env: vec![(
                    "COMANDOS_ORACLE_REFERENCE_ROOT".into(),
                    root.display().to_string(),
                )],
                ..Default::default()
            };
            Some(
                support::oracle::oracle_with(home, opts)
                    .await
                    .expect("record/check original"),
            )
        } else {
            None
        };
        Self {
            source,
            home,
            family,
            sequence: std::sync::atomic::AtomicUsize::new(0),
        }
    }
    fn expected(
        &self,
        operation: &str,
        args: serde_json::Value,
        actual: Option<serde_json::Value>,
    ) -> serde_json::Value {
        let roots = [("<HOME>", self.home.root.as_path())];
        let input = serde_json::json!({"source_commit":support::frozen::SOURCE_COMMIT,
            "source_sha256":"4e4e26305485b4926bd2c77618a4a68eb8da9ea425825c57a9a0fea6847a6f24",
            "python":"CPython 3.10.12","fixture":{"session":"s1","panes":["%0","%1"],"command":"cat","size":[200,40]},
            "sequence":self.sequence.fetch_add(1,std::sync::atomic::Ordering::SeqCst),
            "operation":operation,"args":args,"normalize":"original durationMs only"});
        let output = comandos_oracle::oracle_at(
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden"),
            self.family,
            &input,
            || {
                Ok(comandos_oracle::normalize(
                    &serde_json::to_vec(&actual.expect("record/check original observation"))
                        .unwrap(),
                    &roots,
                ))
            },
        );
        serde_json::from_slice(&comandos_oracle::restore(&output, &roots)).unwrap()
    }
    async fn request(&self, body: &str) -> Wire {
        let actual = if let Some(source) = &self.source {
            let mut wire = request_body(source.port, "POST", "/pane/type", "", body).await;
            wire.body = masked(&wire.text()).into_bytes();
            for (name, value) in &mut wire.headers {
                if name == "content-length" {
                    *value = wire.body.len().to_string();
                }
            }
            Some(serde_json::json!({"status":wire.status,"body":wire.body,"headers":wire.headers}))
        } else {
            None
        };
        let value = self.expected("request", serde_json::json!(body), actual);
        Wire {
            status: value["status"].as_u64().unwrap().try_into().unwrap(),
            body: serde_json::from_value(value["body"].clone()).unwrap(),
            headers: serde_json::from_value(value["headers"].clone()).unwrap(),
        }
    }
    async fn capture(&self, pane: &str) -> String {
        let actual = if self.source.is_some() {
            Some(serde_json::json!(typed_in(self.home, pane).await))
        } else {
            None
        };
        self.expected("capture", serde_json::json!(pane), actual)
            .as_str()
            .unwrap()
            .into()
    }
}

async fn busy_sequence(home: &TestHome, port: u16, pane: &str, same_id: bool) -> serde_json::Value {
    let (letter, id) = if same_id {
        ('k', "doble")
    } else {
        ('y', "largo")
    };
    let long = format!(
        r#"{{"session":"s1","pane":"{pane}","text":"{}","requestId":"{id}"}}"#,
        letter.to_string().repeat(300)
    );
    let initial = long.clone();
    let first =
        tokio::spawn(async move { request_body(port, "POST", "/pane/type", "", &initial).await });
    tokio::time::sleep(Duration::from_millis(300)).await;
    let short = if same_id {
        long
    } else {
        format!(r#"{{"session":"s1","pane":"{pane}","text":"z","requestId":"corto"}}"#)
    };
    let busy = seen(&request_body(port, "POST", "/pane/type", "", &short).await);
    let first = first.await.unwrap();
    assert_eq!(first.status, 200);
    let retry = request_body(port, "POST", "/pane/type", "", &short).await;
    assert_eq!(retry.status, 200);
    if same_id {
        assert_eq!(
            seen(&first),
            seen(&retry),
            "actual cache preserves durationMs"
        );
    }
    let text = typed_in(home, pane).await;
    assert_eq!(text.matches(letter).count(), 300, "{pane}");
    if !same_id {
        assert_eq!(text.matches('z').count(), 1, "{pane}");
    }
    serde_json::json!({"busy":busy,"first_status":first.status,"retry_status":retry.status,"capture":text})
}

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
    let py = TypedOriginal::start(&home, "server-typing-errors").await;
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
            seen(&py.request(body).await),
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
    let py = TypedOriginal::start(&home, "server-typing-retry").await;
    let front = front(&home, dead_port(), home.options()).await;
    let on = |pane: &str| {
        format!(r#"{{"session": "s1", "pane": "{pane}", "text": "dí;x", "requestId": "same"}}"#)
    };
    let py_first = py.request(&on("%0")).await;
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
    let py_again = py.request(&on("%0")).await;
    let rs_again = request_body(front.port, "POST", "/pane/type", "", &on("%1")).await;
    assert_eq!(seen(&py_first), seen(&py_again));
    assert_eq!(seen(&rs_first), seen(&rs_again));
    assert_eq!(py.capture("%0").await, "dí;x");
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
    let py = TypedOriginal::start(&home, "server-typing-busy").await;
    let front = front(&home, dead_port(), home.options()).await;
    let actual = if let Some(source) = &py.source {
        Some(busy_sequence(&home, source.port, "%0", false).await)
    } else {
        None
    };
    let original = py.expected(
        "busy-sequence",
        serde_json::json!({"same_id":false,"pane":"%0"}),
        actual,
    );
    let native = busy_sequence(&home, front.port, "%1", false).await;
    assert_eq!(native, original);
    assert_eq!(
        native["busy"],
        serde_json::json!([
            409,
            "{\"error\": \"Ya se est\\u00e1 escribiendo en ese pane; espera a que termine.\", \"code\": \"typing_in_progress\"}"
        ])
    );
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
    let py = TypedOriginal::start(&home, "server-typing-forwarded-cache").await;
    let actor_home = TestHome::new("type-fallback-rust");
    let mut actor_options = actor_home.options();
    actor_options.tmux = home.options().tmux;
    let actor = if py.source.is_none() {
        Some(front(&actor_home, dead_port(), actor_options).await)
    } else {
        None
    };
    let backend = py
        .source
        .as_ref()
        .map_or_else(|| actor.as_ref().unwrap().port, |source| source.port);
    let front = front(&home, backend, home.options()).await;
    let body = r#"{"session": "s1", "pane": "%0", "text": "una", "requestId": "py-1"}"#;
    let first = request_body(front.port, "POST", "/pane/type", "", body).await;
    assert_eq!(first.status, 200, "{}", first.text());
    std::fs::remove_file(home.hooks().join("state/proyecto.json")).unwrap();
    let again = request_body(front.port, "POST", "/pane/type", "", body).await;
    assert_eq!(seen(&first), seen(&again));
    let capture = typed_text(&home).await;
    let actual=py.source.as_ref().map(|_|serde_json::json!({"status":first.status,"body":masked(&first.text()),"capture":capture}));
    let expected = py.expected(
        "forwarded-cache",
        serde_json::json!({"body":body,"retry_after_state_removed":true}),
        actual,
    );
    assert_eq!(
        serde_json::json!({"status":first.status,"body":masked(&first.text()),"capture":capture}),
        expected
    );
    assert_eq!(capture, "una", "una sola vez");
    if let Some(actor) = actor {
        actor.stop().await;
    }
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
    let native = std::sync::Arc::new(Native::new(home.options()));
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
    let py = TypedOriginal::start(&home, "server-typing-concurrent-id").await;
    let front = front(&home, dead_port(), home.options()).await;
    let actual = if let Some(source) = &py.source {
        Some(busy_sequence(&home, source.port, "%0", true).await)
    } else {
        None
    };
    let original = py.expected(
        "busy-sequence",
        serde_json::json!({"same_id":true,"pane":"%0"}),
        actual,
    );
    let native = busy_sequence(&home, front.port, "%1", true).await;
    assert_eq!(native, original);
    assert_eq!(native["busy"][0], 409);
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
