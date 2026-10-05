use serde_json::{Value, json};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use xtask::fixtures::{
    Limits, MIME_TABLE, Resolved, mime_for, resolve, serve_listener, serve_listener_with,
};

fn tmp_root(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("xtask-fixtures-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("css")).unwrap();
    std::fs::write(dir.join("index.html"), "<p>hola</p>").unwrap();
    std::fs::write(dir.join("css/app.css"), "p{}").unwrap();
    std::fs::write(dir.join("usage_state.json"), r#"{"a":1}"#).unwrap();
    std::fs::write(dir.join("sessions.json"), r#"[1]"#).unwrap();
    std::fs::write(dir.join("sessions.get.json"), r#"[2]"#).unwrap();
    dir
}

fn file(r: Resolved) -> (PathBuf, &'static str) {
    match r {
        Resolved::File { path, mime } => (path, mime),
        Resolved::Missing => panic!("se esperaba archivo"),
    }
}

#[test]
fn existing_files_are_served_as_is_with_their_mime() {
    let root = tmp_root("files");
    let (p, mime) = file(resolve(&root, "GET", "/css/app.css?v=3"));
    assert_eq!((p, mime), (root.join("css/app.css"), "text/css"));
    let (p, mime) = file(resolve(&root, "GET", "/"));
    assert_eq!((p, mime), (root.join("index.html"), "text/html"));
}

#[test]
fn api_paths_map_to_underscored_json_and_prefer_the_method_file() {
    let root = tmp_root("api");
    assert_eq!(
        file(resolve(&root, "GET", "/usage/state")).0,
        root.join("usage_state.json")
    );
    assert_eq!(
        file(resolve(&root, "GET", "/sessions")).0,
        root.join("sessions.get.json")
    );
    assert!(matches!(
        resolve(&root, "GET", "/nada/aqui"),
        Resolved::Missing
    ));
}

#[test]
fn traversal_is_refused() {
    let root = tmp_root("trav");
    assert!(matches!(
        resolve(&root, "GET", "/../etc/passwd"),
        Resolved::Missing
    ));
    assert!(matches!(
        resolve(&root, "GET", "/css/../index.html"),
        Resolved::Missing
    ));
}

/// Copia de la tabla de `comandos-server::dash::statics::mime_for`: debe seguir igual.
#[test]
fn mime_table_matches_statics_mime_for() {
    let src = include_str!("../../crates/comandos-server/src/dash/statics.rs");
    let start = src.find("pub fn mime_for").unwrap();
    let body = &src[start..];
    let body = &body[..body.find("_ => \"application/octet-stream\"").unwrap()];
    let mut server: Vec<(String, String)> = Vec::new();
    for line in body.lines().filter(|l| l.contains("=>")) {
        let (lhs, rhs) = line.split_once("=>").unwrap();
        let mime = rhs
            .trim()
            .trim_end_matches(',')
            .trim_matches('"')
            .to_string();
        for ext in lhs.split('|') {
            server.push((ext.trim().trim_matches('"').to_string(), mime.clone()));
        }
    }
    let mut ours: Vec<(String, String)> = MIME_TABLE
        .iter()
        .flat_map(|(exts, mime)| exts.iter().map(move |e| (e.to_string(), mime.to_string())))
        .collect();
    server.sort();
    ours.sort();
    assert!(server.len() > 15, "{server:?}");
    assert_eq!(ours, server);
    assert_eq!(mime_for("a/b.WOFF2"), "font/woff2");
    assert_eq!(mime_for("sin-extension"), "application/octet-stream");
}

fn http(port: u16, raw: &str) -> String {
    let mut s = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
    s.write_all(raw.as_bytes()).unwrap();
    let mut out = String::new();
    s.read_to_string(&mut out).unwrap();
    out
}

#[test]
fn server_answers_fixtures_404s_and_records_posts() {
    let root = tmp_root("net");
    let posts = Arc::new(Mutex::new(Vec::<Value>::new()));
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let (r, p) = (root.clone(), posts.clone());
    std::thread::spawn(move || serve_listener(r, listener, p));

    let got = http(
        port,
        "GET /usage/state HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n",
    );
    assert!(got.starts_with("HTTP/1.1 200"), "{got}");
    assert!(
        got.contains("application/json") && got.ends_with(r#"{"a":1}"#),
        "{got}"
    );

    let got = http(
        port,
        "GET /falta HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n",
    );
    assert!(got.starts_with("HTTP/1.1 404"), "{got}");
    assert!(got.ends_with(r#"{"error":"fixture ausente"}"#), "{got}");

    let body = r#"{"n":5}"#;
    let got = http(
        port,
        &format!(
            "POST /pomo/start HTTP/1.1\r\nHost: x\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        ),
    );
    assert!(got.ends_with(r#"{"ok":true}"#), "{got}");
    assert_eq!(
        posts.lock().unwrap().as_slice(),
        &[json!({"method": "POST", "path": "/pomo/start", "body": {"n": 5}})]
    );
    let _ = std::fs::remove_dir_all(Path::new(&root));
}

#[test]
fn symlinks_leaving_the_root_are_refused() {
    let root = tmp_root("link");
    let outside = std::env::temp_dir().join(format!("xtask-fixtures-fuera-{}", std::process::id()));
    std::fs::create_dir_all(&outside).unwrap();
    std::fs::write(outside.join("secreto.txt"), "no").unwrap();
    std::fs::write(outside.join("api.json"), "{}").unwrap();
    std::os::unix::fs::symlink(outside.join("secreto.txt"), root.join("fuga.txt")).unwrap();
    std::os::unix::fs::symlink(outside.join("api.json"), root.join("fuga_api.json")).unwrap();
    std::os::unix::fs::symlink(&outside, root.join("dir")).unwrap();
    assert!(matches!(
        resolve(&root, "GET", "/fuga.txt"),
        Resolved::Missing
    ));
    assert!(matches!(
        resolve(&root, "GET", "/fuga/api"),
        Resolved::Missing
    ));
    assert!(matches!(
        resolve(&root, "GET", "/dir/secreto.txt"),
        Resolved::Missing
    ));
    // Un enlace que se queda dentro de root sí vale.
    std::os::unix::fs::symlink(root.join("index.html"), root.join("alias.html")).unwrap();
    assert!(matches!(
        resolve(&root, "GET", "/alias.html"),
        Resolved::File { .. }
    ));
    let _ = std::fs::remove_dir_all(&outside);
    let _ = std::fs::remove_dir_all(&root);
}

fn spawn_server(tag: &str, limits: Limits) -> (u16, Arc<Mutex<Vec<Value>>>) {
    let root = tmp_root(tag);
    let posts = Arc::new(Mutex::new(Vec::<Value>::new()));
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let p = posts.clone();
    std::thread::spawn(move || serve_listener_with(root, listener, p, limits));
    (port, posts)
}

#[test]
fn oversized_post_bodies_are_rejected_and_not_recorded() {
    let (port, posts) = spawn_server(
        "body",
        Limits {
            max_body: 16,
            max_connections: 8,
        },
    );
    let body = "x".repeat(17);
    let got = http(
        port,
        &format!(
            "POST /a HTTP/1.1\r\nHost: x\r\nConnection: close\r\nContent-Length: 17\r\n\r\n{body}"
        ),
    );
    assert!(got.starts_with("HTTP/1.1 413"), "{got}");
    assert!(posts.lock().unwrap().is_empty());
}

#[test]
fn connections_beyond_the_cap_wait_for_a_free_slot() {
    let (port, _) = spawn_server(
        "conns",
        Limits {
            max_body: 1024,
            max_connections: 1,
        },
    );
    // A ocupa el único hueco sin mandar petición.
    let idle = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(150));
    let mut b = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
    b.write_all(b"GET /usage/state HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n")
        .unwrap();
    b.set_read_timeout(Some(std::time::Duration::from_millis(300)))
        .unwrap();
    let mut buf = [0u8; 64];
    assert!(
        b.read(&mut buf).is_err(),
        "B no debería ser atendido mientras A ocupa el hueco"
    );
    drop(idle);
    b.set_read_timeout(Some(std::time::Duration::from_secs(5)))
        .unwrap();
    let mut out = String::new();
    b.read_to_string(&mut out).unwrap();
    assert!(out.starts_with("HTTP/1.1 200"), "{out}");
}
