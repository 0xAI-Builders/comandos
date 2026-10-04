//! Estáticos de `comandos dash`: qué sirve Rust (archivo regular resuelto en
//! `dash_dir`) y qué reenvía al Python, más Last-Modified/304, HEAD y MIME.
mod support;

use comandos_server::dash::{
    DashConfig, parse_args,
    router::{RouteClass, classify, static_path},
    serve_listener, statics,
};
use http::Method;
use std::{
    cell::RefCell,
    fs,
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use support::{WAIT, Wire, request};
use tokio::{net::TcpListener, sync::watch, time::timeout};

/// `Sun, 04 Oct 2026 12:00:00 GMT`.
const MTIME: u64 = 1_791_115_200;
const LAST_MODIFIED: &str = "Sun, 04 Oct 2026 12:00:00 GMT";
const APP_JS: &str = "console.log('app');\n";

fn root(tag: &str) -> PathBuf {
    let base = std::env::temp_dir().join(format!("cmd-dash-st-{tag}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&base);
    fs::create_dir_all(&base).unwrap();
    base
}

fn set_mtime(path: &Path, secs: u64, nanos: u32) {
    let file = fs::File::options().write(true).open(path).unwrap();
    file.set_modified(UNIX_EPOCH + Duration::new(secs, nanos))
        .unwrap();
}

/// `dash/` con index, app.js, asset por symlink (como `H/dash` → repo),
/// directorio vacío y un secreto fuera del directorio servido.
fn fixture(tag: &str) -> PathBuf {
    let base = root(tag);
    let dash = base.join("dash");
    let repo = base.join("repo");
    fs::create_dir_all(dash.join("assets")).unwrap();
    fs::create_dir_all(dash.join("vendor")).unwrap();
    fs::create_dir_all(&repo).unwrap();
    fs::write(
        dash.join("index.html"),
        "<!doctype html><title>ComandOS</title>",
    )
    .unwrap();
    fs::write(dash.join("app.js"), APP_JS).unwrap();
    // Fracción de segundo: Last-Modified y la comparación de 304 la truncan.
    set_mtime(&dash.join("app.js"), MTIME, 700_000_000);
    fs::write(dash.join("workspace.css"), "body{}").unwrap();
    fs::write(dash.join("with space.txt"), "hola").unwrap();
    fs::write(dash.join("manifest.webmanifest"), "{}").unwrap();
    fs::write(dash.join("blob.bin"), [0u8, 1, 2]).unwrap();
    fs::write(repo.join("x.css"), ".x{color:red}").unwrap();
    std::os::unix::fs::symlink(repo.join("x.css"), dash.join("assets/x.css")).unwrap();
    fs::write(base.join("secret"), "no-debe-salir").unwrap();
    dash
}

struct Server {
    port: u16,
    stop: watch::Sender<bool>,
    task: tokio::task::JoinHandle<std::io::Result<()>>,
}

async fn start(dash: &Path) -> Server {
    let home = dash.parent().unwrap().join("home");
    fs::create_dir_all(&home).unwrap();
    let mut cfg: DashConfig = parse_args(&[], &home, Some("1")).unwrap();
    cfg.dash_dir = dash.to_path_buf();
    cfg.token = b"token-de-prueba".to_vec();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let (stop, shutdown) = watch::channel(false);
    let task = tokio::spawn(serve_listener(listener, cfg, shutdown));
    Server { port, stop, task }
}

impl Server {
    async fn get(&self, target: &str) -> Wire {
        request(self.port, "GET", target, "").await
    }
    async fn finish(self) {
        self.stop.send(true).unwrap();
        timeout(WAIT, self.task).await.unwrap().unwrap().unwrap();
    }
}

fn is_not_found_json(wire: &Wire) {
    assert_eq!(wire.status, 404);
    assert_eq!(wire.text(), r#"{"error": "No encontrado"}"#);
}

#[test]
fn static_path_decodes_and_rejects_lexically() {
    assert_eq!(static_path("/").as_deref(), Some("/index.html"));
    assert_eq!(static_path("/?x=1").as_deref(), Some("/index.html"));
    assert_eq!(static_path("/app.js?v=3#f").as_deref(), Some("/app.js"));
    assert_eq!(static_path("/%61pp.js").as_deref(), Some("/app.js"));
    assert_eq!(
        static_path("/with%20space.txt").as_deref(),
        Some("/with space.txt")
    );
    assert_eq!(static_path("/a%2Fb.js").as_deref(), Some("/a/b.js"));
    for bad in [
        "",
        "app.js",
        "http://x/app.js",
        "//app.js",
        "/a//b.js",
        "/vendor/",
        "/..%2Fsecret",
        "/../secret",
        "/a/./b.js",
        "/%2e%2e/secret",
        "/a%00b",
        "/a%5Cb",
        "/a\\b",
        "/%ff.js",
        "/%zz.js",
        "/%4",
    ] {
        assert_eq!(static_path(bad), None, "{bad:?}");
    }
}

#[test]
fn classify_serves_only_existing_regular_files_and_forwards_the_rest() {
    let seen = RefCell::new(Vec::<String>::new());
    let files = [
        "/index.html",
        "/app.js",
        "/state.css",
        "/pomodoro.js",
        "/pomodoro",
        "/tabs.js",
        "/operator.js",
        "/workspace.css",
        "/news/media/x.png",
        "/push/key",
        "/conf.js",
        "/tmux-mouse",
    ];
    let exists = |p: &str| {
        seen.borrow_mut().push(p.to_string());
        files.contains(&p)
    };
    let get = |t: &str| classify(&Method::GET, t, &exists);
    let head = |t: &str| classify(&Method::HEAD, t, &exists);

    assert_eq!(get("/"), RouteClass::Static);
    assert_eq!(get("/app.js?v=3"), RouteClass::Static);
    assert_eq!(get("/%61pp.js"), RouteClass::Static);
    assert_eq!(head("/index.html"), RouteClass::Static);
    assert_eq!(get("/pomodoro.js"), RouteClass::Static);
    assert_eq!(get("/workspace.css"), RouteClass::Static);
    // Ausente, directorio o ruta inválida: lo decide el Python.
    assert_eq!(get("/nada.css"), RouteClass::Forward);
    assert_eq!(get("/vendor/"), RouteClass::Forward);
    assert_eq!(get("/..%2Fsecret"), RouteClass::Forward);
    assert_eq!(get("/a//app.js"), RouteClass::Forward);
    // Métodos que no son GET/HEAD nunca son estáticos.
    assert_eq!(
        classify(&Method::POST, "/app.js", &exists),
        RouteClass::Forward
    );
    assert_eq!(
        classify(&Method::DELETE, "/app.js", &exists),
        RouteClass::Forward
    );
    // Las rutas dinámicas de `_do_GET` ganan al archivo en GET, no en HEAD
    // (el `do_HEAD` del Python va directo a `send_head`).
    for dynamic in [
        "/state.css",
        "/pomodoro",
        "/tabs.js",
        "/operator.js",
        "/news/media/x.png",
        "/push/key",
        "/conf.js",
        "/tmux-mouse",
    ] {
        assert_eq!(get(dynamic), RouteClass::Forward, "GET {dynamic}");
        assert_eq!(head(dynamic), RouteClass::Static, "HEAD {dynamic}");
    }
    // `/pomodoro` es exacta en el Python: con consulta deja de ser dinámica.
    assert_eq!(get("/pomodoro?x=1"), RouteClass::Static);
    // `/tmux-mouse?` es dinámica también con consulta.
    assert_eq!(get("/tmux-mouse?on=1"), RouteClass::Forward);
    // `asset_exists` nunca ve rutas sin decodificar ni con recorrido.
    for path in seen.borrow().iter() {
        assert!(path.starts_with('/'), "{path}");
        assert!(!path.contains('%') && !path.contains("..") && !path.contains("//"));
    }
}

#[test]
fn mime_table_is_fixed_and_bare() {
    for (path, mime) in [
        ("/index.html", "text/html"),
        ("/a/b.js", "text/javascript"),
        ("/m.mjs", "text/javascript"),
        ("/x.css", "text/css"),
        ("/d.json", "application/json"),
        ("/manifest.webmanifest", "application/manifest+json"),
        ("/i.png", "image/png"),
        ("/i.svg", "image/svg+xml"),
        ("/comandos.ico", "image/vnd.microsoft.icon"),
        ("/f.woff2", "font/woff2"),
        ("/f.woff", "font/woff"),
        ("/f.ttf", "font/ttf"),
        ("/w.wasm", "application/wasm"),
        ("/a.js.map", "application/json"),
        ("/r.txt", "text/plain"),
        ("/i.webp", "image/webp"),
        ("/i.jpg", "image/jpeg"),
        ("/i.JPEG", "image/jpeg"),
        ("/i.gif", "image/gif"),
        ("/s.mp3", "audio/mpeg"),
        ("/s.ogg", "audio/ogg"),
        ("/s.wav", "audio/x-wav"),
        ("/assets/xterm/README.md", "text/markdown"),
        ("/blob.bin", "application/octet-stream"),
        ("/sin-extension", "application/octet-stream"),
        ("/comandos.desktop.in", "application/octet-stream"),
    ] {
        assert_eq!(statics::mime_for(path), mime, "{path}");
    }
}

#[tokio::test]
async fn existing_files_are_served_with_type_length_and_last_modified() {
    let dash = fixture("serve");
    let server = start(&dash).await;

    let wire = server.get("/").await;
    assert_eq!(wire.status, 200);
    assert_eq!(wire.header("content-type"), Some("text/html"));
    assert_eq!(wire.header("cache-control"), Some("no-store"));
    assert!(wire.header("last-modified").is_some());
    assert_eq!(wire.text(), "<!doctype html><title>ComandOS</title>");
    assert_eq!(
        wire.header("content-length"),
        Some(wire.body.len().to_string().as_str())
    );

    let wire = server.get("/app.js?v=3").await;
    assert_eq!(wire.status, 200);
    assert_eq!(wire.header("content-type"), Some("text/javascript"));
    assert_eq!(wire.header("last-modified"), Some(LAST_MODIFIED));
    assert_eq!(wire.header("content-length"), Some("20"));
    assert_eq!(wire.text(), APP_JS);

    let wire = server.get("/%61pp.js").await;
    assert_eq!(wire.text(), APP_JS);
    let wire = server.get("/with%20space.txt").await;
    assert_eq!(
        (wire.status, wire.header("content-type")),
        (200, Some("text/plain"))
    );
    let wire = server.get("/manifest.webmanifest").await;
    assert_eq!(
        wire.header("content-type"),
        Some("application/manifest+json")
    );
    let wire = server.get("/blob.bin").await;
    assert_eq!(
        wire.header("content-type"),
        Some("application/octet-stream")
    );
    assert_eq!(wire.body, vec![0u8, 1, 2]);

    // Symlink de `dash_dir` hacia el repositorio.
    let wire = server.get("/assets/x.css").await;
    assert_eq!(wire.status, 200);
    assert_eq!(wire.header("content-type"), Some("text/css"));
    assert_eq!(wire.text(), ".x{color:red}");

    server.finish().await;
}

#[tokio::test]
async fn if_modified_since_follows_python_send_head() {
    let dash = fixture("ims");
    let server = start(&dash).await;
    let ims = |value: &str| format!("If-Modified-Since: {value}\r\n");
    let p = server.port;

    for not_modified in [
        LAST_MODIFIED.to_string(),
        "Sun, 04 Oct 2026 12:00:01 GMT".into(),
        "Mon, 05 Oct 2026 00:00:00 GMT".into(),
        // Sin zona o con -0000 el Python asume UTC; el día de semana se ignora.
        "Sat, 04 Oct 2026 12:00:00".into(),
        "Sat, 04 Oct 2026 12:00:00 -0000".into(),
        "04 Oct 2026 12:00:00 +0000".into(),
        "sun, 4 oct 2026 12:00 utc".into(),
        "Sunday, 04-Oct-26 12:00:00 GMT".into(),
        "Sun Oct  4 12:00:00 2026".into(),
    ] {
        let wire = request(p, "GET", "/app.js", &ims(&not_modified)).await;
        assert_eq!(wire.status, 304, "{not_modified}");
        assert!(wire.body.is_empty(), "{not_modified}");
        assert_eq!(wire.header("cache-control"), Some("no-store"));
        assert_eq!(wire.header("content-type"), None);
        assert_eq!(wire.header("last-modified"), None);
        // Como el Python: un 304 no anuncia longitud.
        assert_eq!(wire.header("content-length"), None, "{:?}", wire.headers);
    }
    for modified in [
        "Sun, 04 Oct 2026 11:59:59 GMT",
        "Sun, 04 Oct 2026 12:00:00 EST",
        "Sun, 04 Oct 2026 12:00:00 +0100",
        "Sun, 04 Oct 2026 24:00:00 GMT",
        "Sun, 31 Feb 2026 12:00:00 GMT",
        "basura",
        "",
    ] {
        let wire = request(p, "GET", "/app.js", &ims(modified)).await;
        assert_eq!(wire.status, 200, "{modified:?}");
        assert_eq!(wire.text(), APP_JS);
    }
    // If-None-Match anula la comparación de fechas, como en el Python.
    let wire = request(
        p,
        "GET",
        "/app.js",
        &format!("{}If-None-Match: \"x\"\r\n", ims(LAST_MODIFIED)),
    )
    .await;
    assert_eq!(wire.status, 200);
    server.finish().await;
}

#[tokio::test]
async fn head_carries_the_file_headers_without_body() {
    let dash = fixture("head");
    let server = start(&dash).await;
    let wire = request(server.port, "HEAD", "/app.js", "").await;
    assert_eq!(wire.status, 200);
    assert!(wire.body.is_empty());
    assert_eq!(wire.header("content-type"), Some("text/javascript"));
    assert_eq!(wire.header("content-length"), Some("20"));
    assert_eq!(wire.header("last-modified"), Some(LAST_MODIFIED));
    assert_eq!(wire.header("cache-control"), Some("no-store"));
    let wire = request(
        server.port,
        "HEAD",
        "/app.js",
        &format!("If-Modified-Since: {LAST_MODIFIED}\r\n"),
    )
    .await;
    assert_eq!(wire.status, 304);
    assert!(wire.body.is_empty());
    server.finish().await;
}

#[tokio::test]
async fn directories_missing_and_invalid_paths_are_forwarded() {
    let dash = fixture("fwd");
    let exists = comandos_server::dash::asset_exists(&dash);
    for target in [
        "/vendor/",
        "/vendor",
        "/nada.css",
        "/..%2Fsecret",
        "/../secret",
    ] {
        assert_eq!(
            classify(&Method::GET, target, &*exists),
            RouteClass::Forward,
            "{target}"
        );
    }
    // Mientras no hay reenvío (tarea 4), Forward responde el 404 provisional.
    let server = start(&dash).await;
    for target in ["/vendor/", "/vendor", "/nada.css", "/..%2Fsecret"] {
        let wire = server.get(target).await;
        is_not_found_json(&wire);
        assert!(!wire.text().contains("no-debe-salir"));
    }
    server.finish().await;
}

#[tokio::test]
async fn public_assets_bypass_the_gate_and_api_routes_do_not() {
    let dash = fixture("gate");
    let server = start(&dash).await;
    let remote = "X-Forwarded-For: 203.0.113.9\r\n";
    // `/workspace` es prefijo API, pero `/workspace.css` existe: asset público.
    let wire = request(server.port, "GET", "/workspace.css", remote).await;
    assert_eq!(wire.status, 200);
    assert_eq!(wire.text(), "body{}");
    // Fuera de los prefijos API no hay puerta.
    let wire = request(server.port, "GET", "/app.js", remote).await;
    assert_eq!(wire.status, 200);
    let wire = request(server.port, "GET", "/state", remote).await;
    assert_eq!(wire.status, 401);
    server.finish().await;
}

#[test]
fn http_dates_round_trip() {
    assert_eq!(statics::http_date(MTIME as i64), LAST_MODIFIED);
    assert_eq!(statics::http_date(0), "Thu, 01 Jan 1970 00:00:00 GMT");
    assert_eq!(
        statics::http_date(951_782_400),
        "Tue, 29 Feb 2000 00:00:00 GMT"
    );
    assert_eq!(statics::http_date(-1), "Wed, 31 Dec 1969 23:59:59 GMT");
    assert_eq!(statics::parse_http_date(LAST_MODIFIED), Some(MTIME as i64));
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    assert_eq!(
        statics::parse_http_date(&statics::http_date(now)),
        Some(now)
    );
}
