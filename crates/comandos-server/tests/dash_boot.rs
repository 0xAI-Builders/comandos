//! Arranque de `comandos dash`: argumentos, token, directorio de estáticos,
//! clasificación de rutas y la puerta de seguridad sobre un puerto libre.
use comandos_server::dash::{
    DashConfig, dash_dir, load_token, parse_args,
    router::{RouteClass, classify},
    serve_listener,
};
use http::Method;
use std::{fs, os::unix::fs::PermissionsExt, path::PathBuf, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::watch,
    time::timeout,
};

const WAIT: Duration = Duration::from_secs(3);

fn home(tag: &str) -> PathBuf {
    let h = std::env::temp_dir().join(format!("cmd-dash-{tag}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&h);
    fs::create_dir_all(h.join(".claude/hooks/dash")).unwrap();
    h
}

#[test]
fn args_default_to_4777_and_legacy_4781_and_ignore_no_open() {
    let cfg = parse_args(&["--no-open".into()], &home("args"), None).unwrap();
    assert_eq!((cfg.port, cfg.legacy_port), (4777, 4781));
    let cfg = parse_args(
        &[
            "4790".into(),
            "--no-open".into(),
            "--legacy-port".into(),
            "4791".into(),
        ],
        &home("args2"),
        None,
    )
    .unwrap();
    assert_eq!((cfg.port, cfg.legacy_port), (4790, 4791));
    let cfg = parse_args(&[], &home("args3"), Some("4799")).unwrap();
    assert_eq!(cfg.legacy_port, 4799, "COMANDOS_DASH_LEGACY_PORT");
}

#[test]
fn args_flag_beats_env_and_bad_values_are_errors() {
    let h = home("args4");
    let cfg = parse_args(&["--legacy-port=4792".into()], &h, Some("4799")).unwrap();
    assert_eq!(cfg.legacy_port, 4792, "la bandera gana a la variable");
    // Como el Python: el primer argumento numérico es el puerto, el resto se ignora.
    let cfg = parse_args(&["4793".into(), "4794".into()], &h, None).unwrap();
    assert_eq!(cfg.port, 4793);
    assert!(parse_args(&["--legacy-port".into()], &h, None).is_err());
    assert!(parse_args(&["--legacy-port".into(), "x".into()], &h, None).is_err());
    assert!(parse_args(&["70000".into()], &h, None).is_err());
    assert!(parse_args(&[], &h, Some("no")).is_err());
    assert!(
        parse_args(&["4781".into()], &h, None).is_err(),
        "el tablero no puede reenviarse a sí mismo"
    );
    let cfg = parse_args(&[], &h, Some("")).unwrap();
    assert_eq!(cfg.legacy_port, 4781, "variable vacía = sin definir");
    assert_eq!(cfg.home, h);
    assert_eq!(cfg.dash_dir, h.join(".claude/hooks/dash"));
}

#[test]
fn token_is_created_once_with_0600_and_43_urlsafe_chars() {
    let h = home("token");
    let t = load_token(&h).unwrap();
    let path = h.join(".claude/hooks/dash-token");
    assert_eq!(path.metadata().unwrap().permissions().mode() & 0o777, 0o600);
    assert_eq!(t.len(), 43);
    assert!(
        t.iter()
            .all(|b| b.is_ascii_alphanumeric() || *b == b'-' || *b == b'_')
    );
    assert_eq!(load_token(&h).unwrap(), t, "estable");
    fs::write(&path, "  fijo-123 \n").unwrap();
    assert_eq!(
        load_token(&h).unwrap(),
        b"fijo-123",
        "recortado como el Python"
    );
    fs::write(&path, " \n").unwrap();
    let regenerated = load_token(&h).unwrap();
    assert_eq!(regenerated.len(), 43, "vacío se regenera");
    assert_ne!(regenerated, t);
}

#[test]
fn dash_dir_prefers_a_readable_override_and_rejects_a_bad_one() {
    let h = home("dir");
    assert_eq!(dash_dir(&h, None).unwrap(), h.join(".claude/hooks/dash"));
    assert_eq!(
        dash_dir(&h, Some("")).unwrap(),
        h.join(".claude/hooks/dash")
    );
    let other = h.join("otro");
    fs::create_dir_all(&other).unwrap();
    assert_eq!(dash_dir(&h, Some(other.to_str().unwrap())).unwrap(), other);
    let dotted = format!("{}/./otro/../otro", h.display());
    assert_eq!(dash_dir(&h, Some(&dotted)).unwrap(), other, "abspath");
    let err = dash_dir(&h, Some("/definitivamente/no")).unwrap_err();
    assert!(err.starts_with("COMANDOS_DASH_DIR no es un directorio legible: "));
    assert!(err.ends_with("/definitivamente/no"));
    let file = h.join("archivo");
    fs::write(&file, "x").unwrap();
    assert!(dash_dir(&h, Some(file.to_str().unwrap())).is_err());
}

#[test]
fn classify_serves_existing_files_and_forwards_the_rest() {
    let none = |_: &str| false;
    let files = |p: &str| ["/index.html", "/app.js", "/workspace.css", "/state"].contains(&p);
    // Estático solo si el archivo existe (los casos finos están en dash_statics).
    assert_eq!(classify(&Method::GET, "/", &files), RouteClass::Static);
    assert_eq!(classify(&Method::GET, "/", &none), RouteClass::Forward);
    assert_eq!(
        classify(&Method::GET, "/app.js?v=3", &files),
        RouteClass::Static
    );
    assert_eq!(
        classify(&Method::HEAD, "/index.html", &files),
        RouteClass::Static
    );
    // Las rutas API de `_do_GET` se reenvían aunque exista un archivo homónimo.
    assert_eq!(
        classify(&Method::GET, "/state", &files),
        RouteClass::Forward
    );
    assert_eq!(
        classify(&Method::GET, "/usage/state?x=1", &files),
        RouteClass::Forward
    );
    assert_eq!(classify(&Method::HEAD, "/tabs", &none), RouteClass::Forward);
    // `/workspace` es prefijo API, pero `/workspace.css` existente es un archivo.
    assert_eq!(
        classify(&Method::GET, "/workspace.css", &files),
        RouteClass::Static
    );
    assert_eq!(
        classify(&Method::GET, "/workspace.css", &none),
        RouteClass::Forward
    );
    assert_eq!(classify(&Method::POST, "/", &files), RouteClass::Forward);
    assert_eq!(
        classify(&Method::DELETE, "/workspace.css", &files),
        RouteClass::Forward
    );
}

struct Server {
    port: u16,
    stop: watch::Sender<bool>,
    task: tokio::task::JoinHandle<std::io::Result<()>>,
}

async fn start(tag: &str) -> Server {
    let h = home(tag);
    let mut cfg: DashConfig = parse_args(&[], &h, Some("1")).unwrap();
    cfg.token = b"token-de-prueba".to_vec();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let (stop, shutdown) = watch::channel(false);
    let task = tokio::spawn(serve_listener(listener, cfg, shutdown));
    Server { port, stop, task }
}

async fn raw(port: u16, request: String) -> String {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    stream.write_all(request.as_bytes()).await.unwrap();
    let mut out = Vec::new();
    timeout(WAIT, stream.read_to_end(&mut out))
        .await
        .unwrap()
        .unwrap();
    String::from_utf8(out).unwrap()
}

fn get(method: &str, path: &str, port: u16, extra: &str) -> String {
    format!(
        "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n{extra}Connection: close\r\n\r\n"
    )
}

fn split(wire: &str) -> (&str, &str) {
    wire.split_once("\r\n\r\n").unwrap()
}

#[tokio::test]
async fn unrouted_paths_answer_404_json_behind_the_ported_gate() {
    let server = start("serve").await;
    let p = server.port;
    let wire = raw(p, get("GET", "/no-existe", p, "")).await;
    let (head, body) = split(&wire);
    assert!(head.starts_with("HTTP/1.1 404"), "{head}");
    assert!(
        head.to_ascii_lowercase()
            .contains("cache-control: no-store")
    );
    assert_eq!(body, r#"{"error": "No encontrado"}"#);

    // Ruta API desde un proxy no local sin token: la puerta portada responde 401.
    let wire = raw(
        p,
        get("GET", "/state", p, "X-Forwarded-For: 203.0.113.9\r\n"),
    )
    .await;
    assert!(wire.starts_with("HTTP/1.1 401"), "{wire}");
    // Con el token, la petición pasa la puerta (aún sin reenvío: 404).
    let wire = raw(
        p,
        get(
            "GET",
            "/state",
            p,
            "X-Forwarded-For: 203.0.113.9\r\nX-Comandos-Token: token-de-prueba\r\n",
        ),
    )
    .await;
    assert!(wire.starts_with("HTTP/1.1 404"), "{wire}");
    // Host ajeno: 403 antes de cualquier ruta.
    let wire = raw(
        p,
        "GET / HTTP/1.1\r\nHost: evil.example\r\nConnection: close\r\n\r\n".into(),
    )
    .await;
    assert!(wire.starts_with("HTTP/1.1 403"), "{wire}");
    // Métodos no admitidos siguen siendo 501.
    let wire = raw(p, get("PUT", "/", p, "Content-Length: 0\r\n")).await;
    assert!(wire.starts_with("HTTP/1.1 501"), "{wire}");

    server.stop.send(true).unwrap();
    timeout(WAIT, server.task).await.unwrap().unwrap().unwrap();
}

#[tokio::test]
async fn head_is_admitted_like_get_and_never_carries_a_body() {
    let server = start("head").await;
    let p = server.port;
    for path in ["/no-existe", "/state"] {
        let wire = raw(p, get("HEAD", path, p, "")).await;
        let (head, body) = split(&wire);
        assert!(head.starts_with("HTTP/1.1 404"), "{path}: {head}");
        assert_eq!(body, "", "{path}: HEAD sin cuerpo");
    }
    // HEAD a una ruta API pasa por la misma puerta que GET.
    let wire = raw(
        p,
        get("HEAD", "/state", p, "X-Forwarded-For: 203.0.113.9\r\n"),
    )
    .await;
    assert!(wire.starts_with("HTTP/1.1 401"), "{wire}");
    server.stop.send(true).unwrap();
    timeout(WAIT, server.task).await.unwrap().unwrap().unwrap();
}
