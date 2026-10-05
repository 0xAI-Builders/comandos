//! Servidor de fixtures para el arnés de capturas: sirve una carpeta de
//! archivos y respuestas JSON fijas en 127.0.0.1, sin tocar el tablero real.
//!
//! - `root/<ruta>` si es un archivo existente: tal cual, con su tipo MIME.
//! - Si no, `GET /a/b` → `root/a_b.get.json` o, si falta, `root/a_b.json`.
//! - `POST` se registra en `posts` y responde `root/a_b.post.json` o `{"ok":true}`.
//! - Lo demás: `404 {"error":"fixture ausente"}` (el arnés lo cuenta como fallo).
use bytes::Bytes;
use http_body_util::{BodyExt as _, Full};
use hyper::{
    Request, Response, StatusCode, body::Incoming, server::conn::http1, service::service_fn,
};
use hyper_util::rt::TokioIo;
use serde_json::{Value, json};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// Copia de la tabla de `comandos-server::dash::statics::mime_for` (xtask no
/// depende de comandos-server). La prueba `mime_table_matches_statics_mime_for`
/// de `xtask/tests/fixtures.rs` lee `statics.rs` y exige que sigan iguales.
pub const MIME_TABLE: &[(&[&str], &str)] = &[
    (&["html"], "text/html"),
    (&["js", "mjs"], "text/javascript"),
    (&["css"], "text/css"),
    (&["json", "map"], "application/json"),
    (&["webmanifest"], "application/manifest+json"),
    (&["png"], "image/png"),
    (&["svg"], "image/svg+xml"),
    (&["ico"], "image/vnd.microsoft.icon"),
    (&["woff2"], "font/woff2"),
    (&["woff"], "font/woff"),
    (&["ttf"], "font/ttf"),
    (&["wasm"], "application/wasm"),
    (&["txt"], "text/plain"),
    (&["md"], "text/markdown"),
    (&["webp"], "image/webp"),
    (&["jpg", "jpeg"], "image/jpeg"),
    (&["gif"], "image/gif"),
    (&["mp3"], "audio/mpeg"),
    (&["ogg"], "audio/ogg"),
    (&["wav"], "audio/x-wav"),
];

/// Tipo MIME por extensión, igual que `statics::mime_for`.
pub fn mime_for(path: &str) -> &'static str {
    let name = path.rsplit('/').next().unwrap_or(path);
    let Some((_, ext)) = name.rsplit_once('.') else {
        return "application/octet-stream";
    };
    let ext = ext.to_ascii_lowercase();
    MIME_TABLE
        .iter()
        .find(|(exts, _)| exts.contains(&ext.as_str()))
        .map_or("application/octet-stream", |(_, mime)| mime)
}

/// Qué responde una petición.
#[derive(Debug, PartialEq, Eq)]
pub enum Resolved {
    File { path: PathBuf, mime: &'static str },
    Missing,
}

/// Ruta relativa segura (sin `..`, `.`, `\` ni segmentos vacíos); `/` → `index.html`.
fn relative(target: &str) -> Option<String> {
    let path = target.split(['?', '#']).next().unwrap_or("");
    let path = path.trim_start_matches('/');
    if path.is_empty() {
        return Some("index.html".into());
    }
    let ok = path
        .split('/')
        .all(|s| !s.is_empty() && s != "." && s != ".." && !s.contains('\\'));
    ok.then(|| path.to_string())
}

/// Resuelve `method target` dentro de `root`.
pub fn resolve(root: &Path, method: &str, target: &str) -> Resolved {
    let Some(rel) = relative(target) else {
        return Resolved::Missing;
    };
    let direct = root.join(&rel);
    if method != "POST" && direct.is_file() {
        return Resolved::File {
            mime: mime_for(&rel),
            path: direct,
        };
    }
    let stem = rel.replace('/', "_");
    let method = method.to_ascii_lowercase();
    let mut candidates = vec![root.join(format!("{stem}.{method}.json"))];
    if method != "post" {
        candidates.push(root.join(format!("{stem}.json")));
    }
    candidates
        .into_iter()
        .find(|p| p.is_file())
        .map_or(Resolved::Missing, |path| Resolved::File {
            path,
            mime: "application/json",
        })
}

fn reply(status: StatusCode, mime: &str, body: Vec<u8>) -> Response<Full<Bytes>> {
    let mut res = Response::new(Full::new(Bytes::from(body)));
    *res.status_mut() = status;
    if let Ok(v) = mime.parse() {
        res.headers_mut().insert(hyper::header::CONTENT_TYPE, v);
    }
    res.headers_mut().insert(
        hyper::header::CACHE_CONTROL,
        hyper::header::HeaderValue::from_static("no-store"),
    );
    res
}

async fn handle(
    root: Arc<PathBuf>,
    posts: Arc<Mutex<Vec<Value>>>,
    req: Request<Incoming>,
) -> Result<Response<Full<Bytes>>, std::convert::Infallible> {
    let method = req.method().as_str().to_string();
    let target = req.uri().path_and_query().map_or_else(
        || req.uri().path().to_string(),
        |pq| pq.as_str().to_string(),
    );
    if method == "POST" {
        let bytes = req
            .into_body()
            .collect()
            .await
            .map(|b| b.to_bytes())
            .unwrap_or_default();
        let body = serde_json::from_slice::<Value>(&bytes)
            .unwrap_or_else(|_| Value::String(String::from_utf8_lossy(&bytes).into_owned()));
        let path = target.split('?').next().unwrap_or("").to_string();
        if let Ok(mut p) = posts.lock() {
            p.push(json!({"method": "POST", "path": path, "body": body}));
        }
    }
    let res = match resolve(&root, &method, &target) {
        Resolved::File { path, mime } => match std::fs::read(&path) {
            Ok(bytes) => reply(StatusCode::OK, mime, bytes),
            Err(e) => reply(
                StatusCode::INTERNAL_SERVER_ERROR,
                "application/json",
                json!({"error": format!("{}: {e}", path.display())})
                    .to_string()
                    .into_bytes(),
            ),
        },
        Resolved::Missing if method == "POST" => reply(
            StatusCode::OK,
            "application/json",
            br#"{"ok":true}"#.to_vec(),
        ),
        Resolved::Missing => reply(
            StatusCode::NOT_FOUND,
            "application/json",
            br#"{"error":"fixture ausente"}"#.to_vec(),
        ),
    };
    Ok(res)
}

/// Sirve `root` en `127.0.0.1:port` hasta que el proceso termine.
pub fn serve(root: &Path, port: u16, posts: Arc<Mutex<Vec<Value>>>) -> io::Result<()> {
    let listener = std::net::TcpListener::bind(("127.0.0.1", port))?;
    serve_listener(root.to_path_buf(), listener, posts)
}

/// Igual que `serve` sobre un socket ya abierto (pruebas con puerto 0).
pub fn serve_listener(
    root: PathBuf,
    listener: std::net::TcpListener,
    posts: Arc<Mutex<Vec<Value>>>,
) -> io::Result<()> {
    listener.set_nonblocking(true)?;
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_io()
        .build()?;
    let root = Arc::new(root);
    rt.block_on(async move {
        let listener = tokio::net::TcpListener::from_std(listener)?;
        loop {
            let (stream, _) = listener.accept().await?;
            let (root, posts) = (root.clone(), posts.clone());
            tokio::spawn(async move {
                let service = service_fn(move |req| handle(root.clone(), posts.clone(), req));
                let _ = http1::Builder::new()
                    .serve_connection(TokioIo::new(stream), service)
                    .await;
            });
        }
    })
}
