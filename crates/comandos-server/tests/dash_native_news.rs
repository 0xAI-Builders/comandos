//! Lector de noticias nativo (plan 2f-4, Tareas 1 y 2) contra el `cc-dash`
//! Python con el gemelo: las ocho rutas GET y las escrituras sin agente
//! (POST `/news/saved`, `/news/notes`, `/news/chat/note`), con estado,
//! cabeceras, cuerpo y filas de app-state resultantes.
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
        EMPTY_IMAGE, LARGE_IMAGE, MISSING_IMAGE, SMALL_IMAGE, large_image, media_dir, migrated,
        seed_editions,
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
