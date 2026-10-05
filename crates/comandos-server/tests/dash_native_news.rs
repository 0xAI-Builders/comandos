//! Lector de noticias nativo (plan 2f-4, Tareas 1 a 3) contra el `cc-dash`
//! Python con el gemelo: las ocho rutas GET, las escrituras sin agente
//! (POST `/news/saved`, `/news/notes`, `/news/chat/note`) y el chat y la
//! traducción con agentes (POST `/news/chat`, `/news/translate`), con estado,
//! cabeceras, cuerpo y filas de app-state resultantes.
//!
//! Agentes (Tarea 3): el único agente es `FakeAcp` (guion `sh` generado en el
//! HOME de cada lado); el registro de prueba que leen el frente
//! (`opts.repo_root`) y el oráculo (`FakeAcp::prelude`) hace de TODO agente
//! ACP ese guion. Cada prueba que pregunta pasa antes por `FakeAcp::canary`.
//!
//! Confinamiento: sin tmux ni procesos propios de estas rutas. app-state,
//! `news-media` (`XDG_STATE_HOME` = HOME temporal en los dos lados) y
//! `~/.claude/hooks/news-*.json` viven en el HOME de cada lado. El oráculo
//! corre sin red y sin bucles de fondo (sin planificador de ediciones).
//! Efectos en vivo: las lecturas, ninguno; las escrituras, las mismas filas de
//! `news_saved` y `news_notes` del app-state que el Python (cada lado en su
//! HOME; el reloj de los dos es `NOW_MS`).
mod support;

use std::os::unix::fs::symlink;
use support::{
    FakeLegacy, NOW_MS, TestHome, front, get,
    news::{
        EMPTY_IMAGE, FAKE_CHAIN, FakeAcp, LARGE_IMAGE, MISSING_IMAGE, SMALL_IMAGE, large_image,
        media_dir, migrated, seed_editions,
    },
    request_body,
    twin::{Twin, TwinOpts},
};

/// La misma petición GET a los dos lados: estado, `Content-Type`,
/// `Content-Length`, `Cache-Control` y cuerpo (bytes) iguales.
async fn same(t: &Twin, path: &str) -> u16 {
    let run = t.get(path).await;
    let (f, o) = (&run.front, &run.oracle);
    assert_eq!(f.status, o.status, "{path}: {} / {}", f.text(), o.text());
    for header in ["content-type", "content-length", "cache-control"] {
        assert_eq!(f.header(header), o.header(header), "{path}: {header}");
    }
    assert!(f.body == o.body, "{path}: {} / {}", f.text(), o.text());
    f.status
}

#[tokio::test]
async fn news_reads_match_python() {
    let Some(t) = Twin::start("news", seed_editions).await else {
        return;
    };
    for (path, status) in [
        ("/news/latest", 200),
        ("/news/latest?x=1", 200),
        ("/news/latestX", 200),
        ("/news/editions", 200),
        ("/news/editions?x=1", 200),
        ("/news/edition", 400),
        ("/news/edition?id=", 400),
        ("/news/edition?id=latest", 200),
        ("/news/edition?id=x", 400),
        ("/news/edition?id=a&id=b", 400),
        ("/news/edition?id=2026-10-03@09:00", 200),
        ("/news/edition?id=2026-10-03%4015:00", 200),
        ("/news/edition?id=2026-10-03@21:00", 200),
        ("/news/edition?id=2030-01-01@09:00", 200),
        ("/news/edition?id=2026-01-01@09:00", 404),
        ("/news/edition?id=2026-10-03@09:00%0A", 404),
        ("/news/edition?id=2026-10-03@09:00&id=", 200),
        ("/news/source?id=1", 200),
        ("/news/source?id=3", 200),
        ("/news/source?id=4", 200),
        ("/news/source?id=999", 404),
        ("/news/source?id=abc", 404),
        ("/news/source?id=", 404),
        ("/news/source", 404),
        ("/news/source?id=+1", 200),
        ("/news/source?id=%201%20", 200),
        ("/news/source?id=0_1", 200),
        ("/news/source?id=-1", 404),
        ("/news/source?id=9007199254740992", 404),
        ("/news/chat?story=10", 200),
        ("/news/chat?story=11", 200),
        ("/news/chat?story=99", 404),
        ("/news/chat?story=x", 404),
        ("/news/chat", 404),
        ("/news/notes", 200),
        ("/news/notes?story=10", 200),
        ("/news/notes?story=1&q=palabra", 200),
        ("/news/notes?q=PALABRA", 200),
        ("/news/notes?q=%C3%81RBOL", 200),
        ("/news/notes?q=%25", 200),
        ("/news/notes?story=x&q=+", 200),
        ("/news/saved", 200),
        ("/news/saved?x=1", 200),
        (&format!("/news/media/{SMALL_IMAGE}"), 200),
        (&format!("/news/media/{SMALL_IMAGE}?v=2"), 200),
        (&format!("/news/media/{LARGE_IMAGE}"), 200),
        (&format!("/news/media/{EMPTY_IMAGE}"), 404),
        (&format!("/news/media/{MISSING_IMAGE}"), 404),
        ("/news/media/nada.png", 404),
        ("/news/media/../x", 404),
        ("/news/media/%2e%2e%2fx", 404),
        ("/news/media/", 404),
    ] {
        assert_eq!(same(&t, path).await, status, "{path}");
    }
    // La imagen grande llega entera por trozos, con su longitud.
    let run = t.get(&format!("/news/media/{LARGE_IMAGE}")).await;
    assert_eq!(run.front.body, large_image());
    assert_eq!(run.front.header("content-type"), Some("image/jpeg"));

    // La configuración se relee en cada petición: con la clave en el entorno
    // de los dos lados (`HOME`) y sin ella.
    for config in [
        r#"{"enabled": true, "summarizer": {"kind": "anthropic-messages", "model": "m", "inputUsdPerMTok": 3, "outputUsdPerMTok": 15, "apiKeyEnv": "HOME"}}"#,
        r#"{"enabled": true, "summarizer": {"kind": "anthropic-messages", "model": "m", "inputUsdPerMTok": 3, "outputUsdPerMTok": 15, "apiKeyEnv": "NO_EXISTE_EN_LA_PRUEBA"}}"#,
        r#"{"enabled": true, "summarizer": {"kind": "acp", "agent": "opencode", "model": 0}, "policy": {"slots": ["09:00", "15:00\n", "23:59"], "budgetUsd": 0.5}}"#,
        r#"{"enabled": true, "summarizer": {"kind": "chain", "steps": {"agent": "x"}}}"#,
        r#"{"enabled": false}"#,
        "[1]",
        "{roto",
    ] {
        t.a.write("news-editions.json", config);
        t.b.write("news-editions.json", config);
        assert_eq!(same(&t, "/news/editions").await, 200, "{config}");
    }
    // `steps` ausente en una cadena: `for st in None` → el 500 del tablero.
    let broken = r#"{"enabled": true, "summarizer": {"kind": "chain"}}"#;
    t.a.write("news-editions.json", broken);
    t.b.write("news-editions.json", broken);
    assert_eq!(same(&t, "/news/editions").await, 500);
    for watch in ["[]", "0", "[1]", "{roto"] {
        t.a.write("news-watch.json", watch);
        t.b.write("news-watch.json", watch);
        assert_eq!(same(&t, "/news/latest").await, 200, "{watch}");
    }
}

#[tokio::test]
async fn news_without_editions_match_python() {
    let Some(t) = Twin::start("news-empty", |home| {
        migrated(home);
    })
    .await
    else {
        return;
    };
    for (path, status) in [
        ("/news/latest", 200),
        ("/news/editions", 200),
        ("/news/edition?id=latest", 404),
        ("/news/notes", 200),
        ("/news/saved", 200),
        (&format!("/news/media/{SMALL_IMAGE}"), 404),
    ] {
        assert_eq!(same(&t, path).await, status, "{path}");
    }
}

/// Review Focus 3: un nombre que no casa o un archivo fuera de `media_dir()`
/// (por enlace simbólico) es «Imagen no encontrada», sin leer nada de fuera.
#[tokio::test]
async fn media_rejects_traversal() {
    let home = TestHome::new("news-media");
    seed_editions(&home);
    let secret = home.root.join("secreto.png");
    std::fs::write(&secret, b"no se lee").unwrap();
    let outside = "22222222222222222222222222222222.png";
    symlink(&secret, media_dir(&home).join(outside)).unwrap();
    // Un enlace dentro de la carpeta sí se sirve.
    let inside = "33333333333333333333333333333333.png";
    symlink(
        media_dir(&home).join(SMALL_IMAGE),
        media_dir(&home).join(inside),
    )
    .unwrap();
    // Un directorio con nombre válido.
    std::fs::create_dir(media_dir(&home).join("44444444444444444444444444444444.gif")).unwrap();
    let legacy = FakeLegacy::start().await;
    let f = front(&home, legacy.port, home.options()).await;
    for path in [
        format!("/news/media/{outside}"),
        "/news/media/../../secreto.png".to_owned(),
        "/news/media/%2e%2e/%2e%2e/secreto.png".to_owned(),
        "/news/media/..%2f..%2fsecreto.png".to_owned(),
        "/news/media/44444444444444444444444444444444.gif".to_owned(),
    ] {
        let wire = get(f.port, &path).await;
        assert_eq!(wire.status, 404, "{path}");
        assert_eq!(
            wire.text(),
            r#"{"error": "Imagen no encontrada"}"#,
            "{path}"
        );
    }
    let wire = get(f.port, &format!("/news/media/{inside}")).await;
    assert_eq!(wire.status, 200);
    assert_eq!(wire.body, b"\x89PNG\r\n\x1a\nfalsa");
    // Nada se reenvió al heredado.
    assert!(legacy.requests().is_empty(), "{:?}", legacy.requests());
    f.stop().await;
}

/// Revisión de la Tarea 1: `news-editions.json` convertido en FIFO no detiene
/// el worker de app-state (la configuración se lee en el pool de bloqueo,
/// sin bloquear al abrir) y la ruta responde «sin configurar». El Python se
/// quedaría colgado en el `open`; el frente sigue sirviendo el resto.
#[tokio::test]
async fn editions_config_fifo_does_not_stall_state() {
    let home = TestHome::new("news-fifo");
    seed_editions(&home);
    let config = home.hooks().join("news-editions.json");
    std::fs::remove_file(&config).unwrap();
    nix::unistd::mkfifo(
        &config,
        nix::sys::stat::Mode::S_IRUSR | nix::sys::stat::Mode::S_IWUSR,
    )
    .unwrap();
    let legacy = FakeLegacy::start().await;
    let f = front(&home, legacy.port, home.options()).await;
    let wait = std::time::Duration::from_secs(10);
    let Ok(wire) = tokio::time::timeout(wait, get(f.port, "/news/editions")).await else {
        // Se suelta al lector colgado (O_RDWR no espera pareja en Linux) para
        // que el frente pueda apagarse y la prueba falle en vez de colgarse.
        let _release = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&config);
        panic!("/news/editions se quedó colgada en el FIFO");
    };
    assert_eq!(wire.status, 200, "{}", wire.text());
    assert!(
        wire.text()
            .starts_with(r#"{"configured": false, "reason": "sin configurar", "latest": "#),
        "{}",
        wire.text()
    );
    // El worker de la base sigue libre para otra ruta.
    let saved = tokio::time::timeout(wait, get(f.port, "/news/saved"))
        .await
        .expect("/news/saved esperó al FIFO");
    assert_eq!(saved.status, 200);
    assert!(legacy.requests().is_empty(), "{:?}", legacy.requests());
    f.stop().await;
}

/// Las filas de las tablas que tocan las escrituras, como texto comparable.
fn news_rows(home: &TestHome) -> Vec<String> {
    let conn = rusqlite::Connection::open(home.state_db()).unwrap();
    let mut out = Vec::new();
    for table in ["news_notes", "news_saved", "news_chat"] {
        let mut stmt = conn
            .prepare(&format!("SELECT * FROM {table} ORDER BY rowid"))
            .unwrap();
        let width = stmt.column_count();
        let mut rows = stmt.query([]).unwrap();
        while let Some(row) = rows.next().unwrap() {
            let cells: Vec<String> = (0..width)
                .map(|i| format!("{:?}", row.get_ref(i).unwrap()))
                .collect();
            out.push(format!("{table}: {}", cells.join(" | ")));
        }
    }
    out
}

/// POST a los dos lados: estado, cabeceras y bytes iguales.
async fn same_post(t: &Twin, path: &str, body: &str) -> u16 {
    let run = t.post(path, body).await;
    let (f, o) = (&run.front, &run.oracle);
    assert_eq!(
        f.status,
        o.status,
        "{path} {body}: {} / {}",
        f.text(),
        o.text()
    );
    for header in ["content-type", "content-length", "cache-control"] {
        assert_eq!(
            f.header(header),
            o.header(header),
            "{path} {body}: {header}"
        );
    }
    assert!(
        f.body == o.body,
        "{path} {body}: {} / {}",
        f.text(),
        o.text()
    );
    f.status
}

/// Tarea 2: guardar/quitar, notas (`add` vacía, `update` inexistente,
/// `delete`, acción desconocida) y notas de chat, contra el Python con el
/// mismo reloj; al final, las mismas filas en los dos app-state.
#[tokio::test]
async fn news_writes_match_python() {
    let opts = TwinOpts {
        // `news_reading._now` del oráculo = el reloj del frente.
        python_prelude: format!("import news_reading\nnews_reading._now = lambda: {NOW_MS}"),
        ..TwinOpts::default()
    };
    let Some(t) = Twin::start_with("news-w", seed_editions, opts).await else {
        return;
    };
    for (path, body, status) in [
        ("/news/saved", r#"{"storyId": 11, "saved": true}"#, 200),
        ("/news/saved", r#"{"storyId": 11, "saved": true}"#, 200),
        ("/news/saved", r#"{"storyId": "10", "saved": 0}"#, 200),
        ("/news/saved", r#"{"storyId": " 12 ", "saved": []}"#, 200),
        ("/news/saved", r#"{"storyId": 12, "saved": "sí"}"#, 200),
        ("/news/saved", r#"{"storyId": 999, "saved": true}"#, 404),
        ("/news/saved", r#"{"storyId": 11.0, "saved": true}"#, 404),
        ("/news/saved", r#"{"storyId": true, "saved": true}"#, 404),
        ("/news/saved", r#"{"storyId": [11]}"#, 404),
        ("/news/saved", r#"{"storyId": 0}"#, 404),
        ("/news/saved", r#"{"storyId": 9007199254740992}"#, 404),
        ("/news/saved", "{}", 404),
        (
            "/news/notes",
            r#"{"action": "add", "storyId": 10, "text": "  hola  "}"#,
            200,
        ),
        (
            "/news/notes",
            r#"{"action": "add", "storyId": 10, "text": ""}"#,
            400,
        ),
        ("/news/notes", r#"{"action": "add", "storyId": 10}"#, 400),
        (
            "/news/notes",
            r#"{"action": "add", "storyId": 10, "text": 7}"#,
            200,
        ),
        (
            "/news/notes",
            r#"{"action": "add", "storyId": 999, "text": "x"}"#,
            404,
        ),
        ("/news/notes", r#"{"action": "add", "text": "x"}"#, 404),
        (
            "/news/notes",
            r#"{"action": "update", "noteId": 1, "text": "otra"}"#,
            200,
        ),
        ("/news/notes", r#"{"action": "update", "noteId": 1}"#, 200),
        (
            "/news/notes",
            r#"{"action": "update", "noteId": 999, "text": "x"}"#,
            404,
        ),
        ("/news/notes", r#"{"action": "update", "text": "x"}"#, 404),
        ("/news/notes", r#"{"action": "delete", "noteId": 3}"#, 200),
        ("/news/notes", r#"{"action": "delete", "noteId": 3}"#, 200),
        ("/news/notes", r#"{"action": "delete"}"#, 200),
        ("/news/notes", r#"{"action": "x"}"#, 400),
        ("/news/notes", r#"{"action": ["add"]}"#, 400),
        ("/news/notes", "{}", 400),
        ("/news/chat/note", r#"{"chatId": 2}"#, 200),
        ("/news/chat/note", r#"{"chatId": 2}"#, 200),
        ("/news/chat/note", r#"{"chatId": 1}"#, 200),
        ("/news/chat/note", r#"{"chatId": "3"}"#, 200),
        ("/news/chat/note", r#"{"chatId": 99}"#, 404),
        ("/news/chat/note", "{}", 404),
    ] {
        assert_eq!(same_post(&t, path, body).await, status, "{path} {body}");
    }
    assert_eq!(news_rows(&t.a), news_rows(&t.b), "filas");
    // Las lecturas ven lo escrito igual en los dos lados.
    for path in ["/news/notes", "/news/saved", "/news/chat?story=10"] {
        let run = t.get(path).await;
        assert!(run.front.body == run.oracle.body, "{path}");
    }
}

/// Lo que no se puede reproducir con certeza (`str()` de un `float` como
/// texto de nota) declina ANTES de escribir: se reenvía y no hay filas nuevas.
/// Con la consulta en la ruta (`self.path in (...)` es exacto) tampoco es del
/// frente.
#[tokio::test]
async fn news_writes_decline_before_effects() {
    let home = TestHome::new("news-decline");
    seed_editions(&home);
    let before = news_rows(&home);
    let legacy = FakeLegacy::start().await;
    let f = front(&home, legacy.port, home.options()).await;
    for (path, body) in [
        (
            "/news/notes",
            r#"{"action": "add", "storyId": 10, "text": 1.5}"#,
        ),
        (
            "/news/notes",
            r#"{"action": "update", "noteId": 1, "text": [1]}"#,
        ),
        (
            "/news/notes",
            r#"{"action": "add", "storyId": 10, "text": {"a": 1}}"#,
        ),
        ("/news/notes?x=1", r#"{"action": "x"}"#),
        ("/news/saved?x=1", r#"{"storyId": 11, "saved": true}"#),
        ("/news/chat/note/", r#"{"chatId": 2}"#),
    ] {
        let wire = request_body(f.port, "POST", path, "", body).await;
        assert_eq!(wire.status, 200, "{path} {body}: {}", wire.text());
    }
    assert_eq!(legacy.requests().len(), 6, "{:?}", legacy.requests());
    assert_eq!(news_rows(&home), before);
    f.stop().await;
}

// ---------------------------------------------------------------- Tarea 3: agentes

/// La siembra de las pruebas con agente: la de siempre, el agente falso y la
/// configuración que solo nombra al agente `falso`.
fn seed_with_fake(env: &'static [(&'static str, &'static str)]) -> impl Fn(&TestHome) {
    move |home: &TestHome| {
        seed_editions(home);
        FakeAcp::install(home, env);
        home.write("news-editions.json", FAKE_CHAIN);
    }
}

/// Gemelo con el agente falso en los dos lados.
async fn fake_twin(tag: &str, env: &'static [(&'static str, &'static str)]) -> Option<Twin> {
    let opts = TwinOpts {
        python_prelude: FakeAcp::prelude(),
        front: Some(Box::new(|o| o.repo_root = Some(o.home.join("fake-repo")))),
        ..TwinOpts::default()
    };
    Twin::start_with(tag, seed_with_fake(env), opts).await
}

/// Espera (≤ 5 s) a que no quede nada pendiente en la ruta.
async fn settled(port: u16, path: &str, busy: &str) -> String {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        let text = get(port, path).await.text();
        if !text.contains(busy) {
            return text;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "{path} sigue pendiente: {text}"
        );
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
}

/// Ninguno de los falsos del `fakebin` con nombre de agente real se llamó.
fn no_named_agent_calls(home: &TestHome) {
    let log = std::fs::read(home.root.join("fakebin.log")).unwrap_or_default();
    let text = String::from_utf8_lossy(&log);
    for name in ["claude", "codex", "grok", "opencode", "agy", "acp"] {
        assert!(
            !text
                .split('\u{1e}')
                .any(|call| call.trim_start_matches('\0').starts_with(name)),
            "se llamó a {name}: {text}"
        );
    }
}

const REPLY: &str = "\"Claro: {\\\"reply\\\": \\\"Cuesta **10 USD** según «la fuente».\\\", \\\"cite\\\": \\\"openai.com · párrafo 2\\\"} listo\"";

/// Review Focus 1: sin cadena de agentes, `503` con el texto de `make_asker`
/// y ninguna fila nueva (ni el mensaje pendiente), en los dos lados. Con la
/// cadena, los errores de `start_chat`/`start_translation` (404, 400, 409)
/// son los del Python y tampoco lanzan agentes.
#[tokio::test]
async fn chat_without_chain_writes_nothing() {
    let Some(t) = fake_twin("news-nochain", &[("FAKEACP_CHUNK", "\"x\"")]).await else {
        return;
    };
    let before = (news_rows(&t.a), news_rows(&t.b));
    for config in [
        None,
        Some(r#"{"enabled": true}"#),
        Some(r#"{"summarizer": {"kind": "anthropic-messages", "model": "m"}}"#),
        Some(r#"{"summarizer": {"kind": "chain", "steps": []}}"#),
        Some(r#"{"summarizer": {"kind": "chain"}}"#),
        Some(r#"{"summarizer": 0}"#),
        Some("[1]"),
        Some("{roto"),
    ] {
        for home in [&t.a, &t.b] {
            match config {
                Some(text) => home.write("news-editions.json", text),
                None => {
                    let _ = std::fs::remove_file(home.hooks().join("news-editions.json"));
                }
            }
        }
        assert_eq!(
            same_post(&t, "/news/chat", r#"{"storyId": 10, "message": "hola"}"#).await,
            503,
            "{config:?}"
        );
        assert_eq!(
            same_post(&t, "/news/translate", r#"{"sourceId": 3}"#).await,
            503,
            "{config:?}"
        );
    }
    assert_eq!((news_rows(&t.a), news_rows(&t.b)), before);
    // Con la cadena: errores antes de prometer nada.
    for home in [&t.a, &t.b] {
        home.write("news-editions.json", FAKE_CHAIN);
        migrated(home)
            .execute_batch(
                "INSERT INTO news_chat (id, story_id, edition_id, role, state, text, created_at_ms) \
                 VALUES (50, 12, '2026-10-03@15:00', 'assistant', 'pending', '', 1)",
            )
            .unwrap();
    }
    for (path, body, status) in [
        ("/news/chat", r#"{"storyId": 999, "message": "hola"}"#, 404),
        ("/news/chat", r#"{"message": "hola"}"#, 404),
        ("/news/chat", r#"{"storyId": 10, "message": "   "}"#, 400),
        ("/news/chat", r#"{"storyId": 10}"#, 400),
        ("/news/chat", r#"{"storyId": 12, "message": "otra"}"#, 409),
        ("/news/translate", r#"{"sourceId": 4}"#, 404),
        ("/news/translate", r#"{"sourceId": 999}"#, 404),
        ("/news/translate", "{}", 404),
        ("/news/translate", r#"{"sourceId": 1}"#, 200),
    ] {
        assert_eq!(same_post(&t, path, body).await, status, "{path} {body}");
    }
    assert_eq!(news_rows(&t.a), news_rows(&t.b), "filas");
    // Ningún agente se lanzó en ningún lado.
    assert!(FakeAcp::at(&t.a).pids().is_empty());
    assert!(FakeAcp::at(&t.b).pids().is_empty());
    no_named_agent_calls(&t.a);
    no_named_agent_calls(&t.b);
}

/// Chat completo con el agente falso: `200` inmediato con el pendiente; a los
/// ≤ 5 s el historial tiene la respuesta, igual en los dos lados; el agente
/// recibió exactamente las mismas líneas JSON-RPC (prompt incluido) y su
/// petición de permiso se negó.
#[tokio::test]
async fn chat_with_fake_agent_completes() {
    static ENV: &[(&str, &str)] = &[
        ("FAKEACP_CHUNK", REPLY),
        ("FAKEACP_SLEEP", "0.5"),
        ("FAKEACP_PERMISSION", "1"),
    ];
    let Some(t) = fake_twin("news-chat", ENV).await else {
        return;
    };
    FakeAcp::at(&t.a).canary(&t.a);
    for message in ["¿Cuánto cuesta?", "¿Y en «México»?"] {
        let body = serde_json::json!({"storyId": 10, "message": message}).to_string();
        let run = t.post("/news/chat", &body).await;
        assert_eq!(run.front.status, 200, "{}", run.front.text());
        assert!(
            run.front.body == run.oracle.body,
            "{} / {}",
            run.front.text(),
            run.oracle.text()
        );
        assert!(
            run.front.text().contains(r#""state": "pending""#),
            "{}",
            run.front.text()
        );
        let busy = r#""state": "pending""#;
        let front = settled(t.front.port, "/news/chat?story=10", busy).await;
        let oracle = settled(t.oracle.port, "/news/chat?story=10", busy).await;
        assert_eq!(front, oracle);
        assert!(front.contains("Cuesta **10 USD**"), "{front}");
        assert!(
            front.contains(r#""model": "falso:predeterminado""#),
            "{front}"
        );
    }
    let (fa, fb) = (FakeAcp::at(&t.a), FakeAcp::at(&t.b));
    assert_eq!(fa.pids().len(), 2);
    assert_eq!(fb.pids().len(), 2);
    let received = fa.received(&t.a);
    assert_eq!(received, fb.received(&t.b), "lo que recibió el agente");
    assert!(
        received.iter().any(|l| l
            == r#"{"jsonrpc": "2.0", "id": 900, "result": {"outcome": {"outcome": "selected", "optionId": "no"}}}"#),
        "{received:?}"
    );
    assert!(
        received
            .iter()
            .any(|l| l.contains("Conversaci\\u00f3n previa"))
    );
    assert_eq!(news_rows(&t.a), news_rows(&t.b), "filas");
    assert!(fa.alive().is_empty());
    no_named_agent_calls(&t.a);
    no_named_agent_calls(&t.b);
}

/// Dos traducciones seguidas de la misma fuente: un solo trabajo de agente
/// (la segunda ve `running`), el mismo resultado en los dos lados.
#[tokio::test]
async fn translate_twice_starts_once() {
    static ENV: &[(&str, &str)] = &[
        (
            "FAKEACP_CHUNK",
            "\"{\\\"texts\\\": [\\\"Artículo traducido\\\"]}\"",
        ),
        ("FAKEACP_SLEEP", "0.5"),
    ];
    let Some(t) = fake_twin("news-tr", ENV).await else {
        return;
    };
    FakeAcp::at(&t.a).canary(&t.a);
    for _ in 0..2 {
        let run = t.post("/news/translate", r#"{"sourceId": 3}"#).await;
        assert_eq!(run.front.status, 200, "{}", run.front.text());
        assert!(
            run.front.body == run.oracle.body,
            "{} / {}",
            run.front.text(),
            run.oracle.text()
        );
        assert!(run.front.text().contains(r#""state": "running""#));
    }
    let busy = r#""state": "running""#;
    let front = settled(t.front.port, "/news/source?id=3", busy).await;
    let oracle = settled(t.oracle.port, "/news/source?id=3", busy).await;
    assert_eq!(front, oracle);
    assert!(front.contains("Art\\u00edculo traducido"), "{front}");
    assert_eq!(FakeAcp::at(&t.a).pids().len(), 1);
    assert_eq!(FakeAcp::at(&t.b).pids().len(), 1);
    assert_eq!(
        FakeAcp::at(&t.a).received(&t.a),
        FakeAcp::at(&t.b).received(&t.b)
    );
    // Una traducción hecha se reutiliza sin agente.
    assert_eq!(
        same_post(&t, "/news/translate", r#"{"sourceId": 3}"#).await,
        200
    );
    assert_eq!(FakeAcp::at(&t.a).pids().len(), 1);
    no_named_agent_calls(&t.a);
    no_named_agent_calls(&t.b);
}

/// Review Focus 2: cuatro chats con un agente que tarda 1 s; nunca más de dos
/// procesos `FakeAcp` vivos a la vez (contados por `/proc`) y ninguno se pierde.
#[tokio::test]
async fn agent_jobs_limited_to_two() {
    let home = TestHome::new("news-limit");
    seed_editions(&home);
    let fake = FakeAcp::install(&home, &[("FAKEACP_CHUNK", REPLY), ("FAKEACP_SLEEP", "1")]);
    home.write("news-editions.json", FAKE_CHAIN);
    migrated(&home)
        .execute_batch(
            "INSERT INTO news_stories (id, edition_id, position, story_key, category, title, summary_md, body_md) \
             VALUES (13, '2026-10-03@15:00', 1, 'k4', 'ia', 'Cuarta', 'Resumen cuatro', 'Cuerpo cuatro')",
        )
        .unwrap();
    fake.canary(&home);
    let legacy = FakeLegacy::start().await;
    let mut opts = home.options();
    opts.repo_root = Some(fake.repo.clone());
    let f = front(&home, legacy.port, opts).await;
    for story in [10, 11, 12, 13] {
        let body = serde_json::json!({"storyId": story, "message": "¿Qué pasó?"}).to_string();
        let wire = request_body(f.port, "POST", "/news/chat", "", &body).await;
        assert_eq!(wire.status, 200, "{}", wire.text());
    }
    let mut most = 0;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    loop {
        most = most.max(fake.alive().len());
        let mut pending = 0;
        for story in [10, 11, 12, 13] {
            let text = get(f.port, &format!("/news/chat?story={story}"))
                .await
                .text();
            pending += usize::from(text.contains(r#""state": "pending""#));
        }
        if pending == 0 {
            break;
        }
        assert!(std::time::Instant::now() < deadline, "chats sin terminar");
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    assert!(most <= 2, "{most} agentes a la vez");
    assert!(most >= 1);
    assert_eq!(fake.pids().len(), 4, "un agente por chat");
    for story in [10, 11, 12, 13] {
        let text = get(f.port, &format!("/news/chat?story={story}"))
            .await
            .text();
        assert!(text.contains("Cuesta **10 USD**"), "{story}: {text}");
    }
    assert!(legacy.requests().is_empty(), "{:?}", legacy.requests());
    std::thread::sleep(std::time::Duration::from_millis(100));
    assert!(fake.alive().is_empty());
    f.stop().await;
}

/// Lo que no se reproduce con certeza en la cadena (pasos que no son objetos,
/// `agent` que no es texto, `model` no textual, `summarizer` que no es objeto)
/// y la falta del checkout del heredado declinan ANTES de escribir.
#[tokio::test]
async fn agent_routes_decline_before_effects() {
    let home = TestHome::new("news-agent-decline");
    seed_editions(&home);
    let fake = FakeAcp::install(&home, &[("FAKEACP_CHUNK", "\"x\"")]);
    let before = news_rows(&home);
    let legacy = FakeLegacy::start().await;
    let mut opts = home.options();
    opts.repo_root = Some(fake.repo.clone());
    let f = front(&home, legacy.port, opts).await;
    let mut sent = 0;
    for config in [
        r#"{"summarizer": {"kind": "chain", "steps": "x"}}"#,
        r#"{"summarizer": {"kind": "chain", "steps": {"agent": "falso"}}}"#,
        r#"{"summarizer": {"kind": "chain", "steps": [{"agent": "falso"}, "x"]}}"#,
        r#"{"summarizer": {"kind": "chain", "steps": [{"model": "m"}]}}"#,
        r#"{"summarizer": {"kind": "acp", "agent": 5}}"#,
        r#"{"summarizer": {"kind": "acp", "agent": "falso", "model": 5}}"#,
        r#"{"summarizer": "acp"}"#,
    ] {
        home.write("news-editions.json", config);
        for (path, body) in [
            ("/news/chat", r#"{"storyId": 10, "message": "hola"}"#),
            ("/news/translate", r#"{"sourceId": 3}"#),
        ] {
            let wire = request_body(f.port, "POST", path, "", body).await;
            assert_eq!(wire.status, 200, "{config} {path}: {}", wire.text());
            sent += 1;
        }
    }
    // `self.path in (...)` es exacto: con consulta no es del frente.
    home.write("news-editions.json", FAKE_CHAIN);
    for path in ["/news/chat?x=1", "/news/translate/"] {
        let wire = request_body(
            f.port,
            "POST",
            path,
            "",
            r#"{"storyId": 10, "message": "hola"}"#,
        )
        .await;
        assert_eq!(wire.status, 200, "{path}: {}", wire.text());
        sent += 1;
    }
    assert_eq!(legacy.requests().len(), sent, "{:?}", legacy.requests());
    f.stop().await;
    // Sin checkout del heredado (`repo_root`), tampoco.
    let mut opts = home.options();
    opts.repo_root = None;
    let f = front(&home, legacy.port, opts).await;
    let wire = request_body(
        f.port,
        "POST",
        "/news/chat",
        "",
        r#"{"storyId": 10, "message": "hola"}"#,
    )
    .await;
    assert_eq!(wire.status, 200, "{}", wire.text());
    assert_eq!(legacy.requests().len(), sent + 1);
    f.stop().await;
    assert_eq!(news_rows(&home), before);
    assert!(fake.pids().is_empty());
}

/// Ronda 1: un agente que manda más de 1 MiB de texto deja el chat fallido
/// («respuesta demasiado larga»), queda cerrado y devuelve su puesto: dos
/// chats así seguidos y después uno normal terminan.
#[tokio::test]
async fn flooding_agent_fails_and_frees_its_slot() {
    let home = TestHome::new("news-flood");
    seed_editions(&home);
    let fake = FakeAcp::install(
        &home,
        &[("FAKEACP_CHUNK", REPLY), ("FAKEACP_FLOOD", "chunks")],
    );
    home.write("news-editions.json", FAKE_CHAIN);
    fake.canary(&home);
    let legacy = FakeLegacy::start().await;
    let mut opts = home.options();
    opts.repo_root = Some(fake.repo.clone());
    let f = front(&home, legacy.port, opts).await;
    let busy = r#""state": "pending""#;
    for story in [10, 11] {
        let body = serde_json::json!({"storyId": story, "message": "¿Qué pasó?"}).to_string();
        let wire = request_body(f.port, "POST", "/news/chat", "", &body).await;
        assert_eq!(wire.status, 200, "{}", wire.text());
        let text = settled(f.port, &format!("/news/chat?story={story}"), busy).await;
        assert!(
            text.contains(r#""state": "failed", "text": "No pude responder: ning\u00fan agente respondi\u00f3 (falso:predeterminado: respuesta demasiado larga)""#),
            "{text}"
        );
    }
    // El mismo agente sin inundar: el puesto está libre y responde.
    let fake = FakeAcp::install(&home, &[("FAKEACP_CHUNK", REPLY)]);
    let body = serde_json::json!({"storyId": 12, "message": "¿Y ahora?"}).to_string();
    let wire = request_body(f.port, "POST", "/news/chat", "", &body).await;
    assert_eq!(wire.status, 200, "{}", wire.text());
    let text = settled(f.port, "/news/chat?story=12", busy).await;
    assert!(text.contains("Cuesta **10 USD**"), "{text}");
    assert_eq!(fake.pids().len(), 3, "un agente por chat");
    std::thread::sleep(std::time::Duration::from_millis(100));
    assert!(fake.alive().is_empty());
    assert!(legacy.requests().is_empty(), "{:?}", legacy.requests());
    f.stop().await;
}

/// Revisión de la Tarea 2: TEXT que no es UTF-8 en la noticia o en la
/// burbuja de chat declina ANTES de escribir (se reenvía, sin filas nuevas).
/// Un texto de nota con un sustituto suelto (`"\ud800"`, que el `json` del
/// Python sí admite) no llega a las rutas de noticias: la puerta del
/// transporte (2b, común a todo POST) responde 400 «JSON invalido» con
/// cierre; nunca 500 y nada se escribe.
#[tokio::test]
async fn bad_text_declines_before_effects() {
    let home = TestHome::new("news-badtext");
    seed_editions(&home);
    let fake = FakeAcp::install(&home, &[("FAKEACP_CHUNK", "\"x\"")]);
    home.write("news-editions.json", FAKE_CHAIN);
    migrated(&home)
        .execute_batch(
            "INSERT INTO news_stories (id, edition_id, position, story_key, category, title, summary_md, body_md) \
             VALUES (14, '2026-10-03@15:00', 2, 'k5', 'ia', CAST(X'C3' AS TEXT), 'r', 'c'); \
             INSERT INTO news_chat (id, story_id, edition_id, role, state, text, created_at_ms) \
             VALUES (60, 10, '2026-10-03@09:00', 'user', 'done', CAST(X'FF' AS TEXT), 1);",
        )
        .unwrap();
    let before = news_rows(&home);
    let legacy = FakeLegacy::start().await;
    let mut opts = home.options();
    opts.repo_root = Some(fake.repo.clone());
    let f = front(&home, legacy.port, opts).await;
    let cases = [
        ("/news/saved", r#"{"storyId": 14, "saved": true}"#),
        (
            "/news/notes",
            r#"{"action": "add", "storyId": 14, "text": "hola"}"#,
        ),
        ("/news/chat/note", r#"{"chatId": 60}"#),
        ("/news/chat", r#"{"storyId": 14, "message": "hola"}"#),
    ];
    for (path, body) in cases {
        let wire = request_body(f.port, "POST", path, "", body).await;
        assert_eq!(wire.status, 200, "{path} {body}: {}", wire.text());
    }
    assert_eq!(
        legacy.requests().len(),
        cases.len(),
        "{:?}",
        legacy.requests()
    );
    for body in [
        r#"{"action": "add", "storyId": 10, "text": "\ud800"}"#,
        r#"{"action": "update", "noteId": 1, "text": "a\udc00b"}"#,
    ] {
        let wire = request_body(f.port, "POST", "/news/notes", "", body).await;
        assert_eq!(wire.status, 400, "{body}: {}", wire.text());
        assert_eq!(wire.text(), r#"{"error": "JSON invalido"}"#);
    }
    assert_eq!(
        legacy.requests().len(),
        cases.len(),
        "{:?}",
        legacy.requests()
    );
    assert_eq!(news_rows(&home), before);
    assert!(fake.pids().is_empty());
    f.stop().await;
}

/// Revisión de la Tarea 2: `news-editions.json` de más de 1 MiB es «sin
/// configurar» (no se lee entero) y sin cadena para el chat.
#[tokio::test]
async fn oversized_config_is_not_configured() {
    let home = TestHome::new("news-bigconf");
    seed_editions(&home);
    let fake = FakeAcp::install(&home, &[("FAKEACP_CHUNK", "\"x\"")]);
    let mut big = String::from(FAKE_CHAIN);
    big.push_str(&" ".repeat(1024 * 1024));
    home.write("news-editions.json", &big);
    let legacy = FakeLegacy::start().await;
    let mut opts = home.options();
    opts.repo_root = Some(fake.repo.clone());
    let f = front(&home, legacy.port, opts).await;
    let wire = get(f.port, "/news/editions").await;
    assert_eq!(wire.status, 200);
    assert!(
        wire.text()
            .starts_with(r#"{"configured": false, "reason": "sin configurar", "latest": "#),
        "{}",
        wire.text()
    );
    let wire = request_body(
        f.port,
        "POST",
        "/news/chat",
        "",
        r#"{"storyId": 10, "message": "hola"}"#,
    )
    .await;
    assert_eq!(wire.status, 503, "{}", wire.text());
    // Justo 1 MiB sí se lee.
    let mut fits = String::from(FAKE_CHAIN);
    fits.push_str(&" ".repeat(1024 * 1024 - FAKE_CHAIN.len()));
    home.write("news-editions.json", &fits);
    let wire = get(f.port, "/news/editions").await;
    assert!(
        wire.text()
            .starts_with(r#"{"configured": false, "reason": "falta el modelo", "latest": "#),
        "{}",
        wire.text()
    );
    assert!(legacy.requests().is_empty(), "{:?}", legacy.requests());
    assert!(fake.pids().is_empty());
    f.stop().await;
}
