//! Reenvío de `comandos dash` al `cc-dash` heredado, contra un heredado falso
//! de cable crudo que graba lo que recibe (método, target, cabeceras en orden y
//! cuerpo) y responde según la ruta, como lo haría el Python.
use comandos_server::{
    dash::{DashConfig, forward::is_hop_by_hop, parse_args, transport_config},
    serve,
};
use std::{
    fs,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::watch,
    time::{sleep, timeout},
};

const TOKEN: &str = "token-de-prueba";
const WAIT: Duration = Duration::from_secs(10);
const BIG: usize = 5 * 1024 * 1024;

#[derive(Debug, Clone)]
struct Recorded {
    method: String,
    target: String,
    /// Nombre tal como llegó por el cable (el cliente hyper los manda en minúsculas).
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

/// Heredado falso: una petición por conexión, como hace el frente.
struct Legacy {
    port: u16,
    seen: Arc<Mutex<Vec<Recorded>>>,
}

async fn legacy() -> Legacy {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = seen.clone();
    tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                return;
            };
            tokio::spawn(answer(stream, log.clone()));
        }
    });
    Legacy { port, seen }
}

async fn read_request(stream: &mut TcpStream) -> Option<Recorded> {
    let mut raw = Vec::new();
    let mut buf = [0u8; 4096];
    let split = loop {
        if let Some(i) = raw.windows(4).position(|w| w == b"\r\n\r\n") {
            break i;
        }
        let n = stream.read(&mut buf).await.ok()?;
        if n == 0 {
            return None;
        }
        raw.extend_from_slice(&buf[..n]);
    };
    let head = String::from_utf8_lossy(&raw[..split]).into_owned();
    let mut lines = head.split("\r\n");
    let mut first = lines.next()?.split(' ');
    let method = first.next()?.to_string();
    let target = first.next()?.to_string();
    let headers: Vec<(String, String)> = lines
        .filter_map(|l| l.split_once(':'))
        .map(|(k, v)| (k.to_string(), v.trim().to_string()))
        .collect();
    let length = headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("content-length"))
        .map_or(0, |(_, v)| v.parse::<usize>().unwrap());
    let mut body = raw[split + 4..].to_vec();
    while body.len() < length {
        let n = stream.read(&mut buf).await.ok()?;
        if n == 0 {
            return None;
        }
        body.extend_from_slice(&buf[..n]);
    }
    Some(Recorded {
        method,
        target,
        headers,
        body,
    })
}

async fn answer(mut stream: TcpStream, log: Arc<Mutex<Vec<Recorded>>>) {
    let Some(request) = read_request(&mut stream).await else {
        return;
    };
    log.lock().unwrap().push(request.clone());
    let path = request.target.split('?').next().unwrap_or("").to_string();
    let _ = match path.as_str() {
        "/eco" => {
            let echo = serde_json::json!({
                "method": request.method,
                "target": request.target,
                "headers": request.headers,
                "body": String::from_utf8_lossy(&request.body),
            })
            .to_string();
            let head = format!(
                "HTTP/1.1 200 OK\r\nServer: Falso/1\r\nContent-Type: application/json\r\nCache-Control: no-store\r\nX-Orden: 1\r\nContent-Length: {}\r\nX-Orden: 2\r\n\r\n",
                echo.len()
            );
            stream.write_all(format!("{head}{echo}").as_bytes()).await
        }
        // Cabeceras al momento y el cuerpo 3 s después (el frente tiene
        // handler_timeout y write_timeout de 1 s).
        "/lento" => {
            let r = stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: 10\r\n\r\ninicio")
                .await;
            sleep(Duration::from_secs(3)).await;
            r.and(stream.write_all(b"-fin").await)
        }
        // Las cabeceras tardan más que el handler_timeout: 504 del frente.
        "/lento-cabeceras" => {
            sleep(Duration::from_secs(3)).await;
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok")
                .await
        }
        // Como los flujos del Python: sin longitud, cuerpo hasta el cierre.
        "/cierra" => {
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nConnection: close\r\n\r\ncerrado")
                .await
        }
        "/grande" => {
            let head =
                format!("HTTP/1.1 200 OK\r\nContent-Type: application/octet-stream\r\nContent-Length: {BIG}\r\n\r\n");
            let mut r = stream.write_all(head.as_bytes()).await;
            let chunk: Vec<u8> = (0..65536u32).map(|i| (i % 251) as u8).collect();
            for _ in 0..BIG / chunk.len() {
                r = r.and(stream.write_all(&chunk).await);
            }
            r
        }
        // Promete 100 bytes, manda 10 y corta.
        "/corta" => {
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: 100\r\n\r\n0123456789")
                .await
        }
        _ => {
            stream
                .write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n")
                .await
        }
    };
    let _ = stream.shutdown().await;
}

struct Front {
    port: u16,
    stop: watch::Sender<bool>,
    task: tokio::task::JoinHandle<std::io::Result<()>>,
}

fn home(tag: &str) -> PathBuf {
    let base = std::env::temp_dir().join(format!("cmd-dash-fwd-{tag}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&base);
    fs::create_dir_all(base.join("dash")).unwrap();
    base
}

/// Frente con `handler_timeout` y `write_timeout` de 1 s apuntando a `legacy_port`.
async fn front(tag: &str, legacy_port: u16) -> Front {
    let h = home(tag);
    let mut cfg: DashConfig = parse_args(&[], &h, Some(&legacy_port.to_string())).unwrap();
    cfg.dash_dir = h.join("dash");
    cfg.token = TOKEN.as_bytes().to_vec();
    let mut config = transport_config(cfg);
    config.limits.handler_timeout = Duration::from_secs(1);
    config.limits.write_timeout = Duration::from_secs(1);
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let (stop, shutdown) = watch::channel(false);
    let task = tokio::spawn(serve(listener, config, shutdown));
    Front { port, stop, task }
}

impl Front {
    async fn finish(self) {
        self.stop.send(true).unwrap();
        timeout(WAIT, self.task).await.unwrap().unwrap().unwrap();
    }
}

#[derive(Debug)]
struct Wire {
    status: u16,
    headers: Vec<(String, String)>,
    /// Cuerpo ya sin el troceado `chunked`.
    body: Vec<u8>,
    chunked: bool,
    /// Un cuerpo `chunked` sin su trozo final de cierre.
    truncated: bool,
}

impl Wire {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }
    fn all(&self, name: &str) -> Vec<&str> {
        self.headers
            .iter()
            .filter(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
            .collect()
    }
}

/// Escribe la petición tal cual y lee hasta que el frente cierra.
async fn send(port: u16, raw: &[u8]) -> Wire {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    stream.write_all(raw).await.unwrap();
    let mut out = Vec::new();
    // Un corte del servidor puede llegar como RST: lo leído hasta ahí vale.
    let _ = timeout(WAIT, stream.read_to_end(&mut out))
        .await
        .expect("el frente debe cerrar la conexión");
    parse(&out)
}

fn parse(raw: &[u8]) -> Wire {
    let split = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .expect("respuesta sin fin de cabeceras");
    let head = String::from_utf8_lossy(&raw[..split]).into_owned();
    let mut lines = head.split("\r\n");
    let status = lines
        .next()
        .and_then(|l| l.split(' ').nth(1))
        .and_then(|s| s.parse().ok())
        .expect("línea de estado");
    let headers: Vec<(String, String)> = lines
        .filter_map(|l| l.split_once(':'))
        .map(|(k, v)| (k.trim().to_ascii_lowercase(), v.trim().to_owned()))
        .collect();
    let rest = &raw[split + 4..];
    let chunked = headers
        .iter()
        .any(|(k, v)| k == "transfer-encoding" && v.eq_ignore_ascii_case("chunked"));
    if !chunked {
        return Wire {
            status,
            headers,
            body: rest.to_vec(),
            chunked,
            truncated: false,
        };
    }
    let (mut body, mut at) = (Vec::new(), 0);
    let mut truncated = true;
    while let Some(eol) = rest[at..].windows(2).position(|w| w == b"\r\n") {
        let size =
            usize::from_str_radix(std::str::from_utf8(&rest[at..at + eol]).unwrap().trim(), 16)
                .unwrap();
        at += eol + 2;
        if size == 0 {
            truncated = false;
            break;
        }
        if rest.len() < at + size {
            body.extend_from_slice(&rest[at..]);
            break;
        }
        body.extend_from_slice(&rest[at..at + size]);
        at += size + 2;
    }
    Wire {
        status,
        headers,
        body,
        chunked,
        truncated,
    }
}

fn get(path: &str, port: u16) -> Vec<u8> {
    format!("GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n")
        .into_bytes()
}

#[test]
fn hop_by_hop_names() {
    for name in [
        "connection",
        "Keep-Alive",
        "transfer-encoding",
        "te",
        "trailer",
        "upgrade",
        "proxy-authorization",
        "Proxy-Connection",
    ] {
        assert!(is_hop_by_hop(name), "{name}");
    }
    for name in [
        "host",
        "content-length",
        "x-forwarded-for",
        "cookie",
        "trailers",
    ] {
        assert!(!is_hop_by_hop(name), "{name}");
    }
}

/// (a) Cabeceras, Host, query y cuerpo llegan intactos; no se añade XFF ni Via.
#[tokio::test]
async fn post_reaches_legacy_with_headers_host_query_and_body_intact() {
    let legacy = legacy().await;
    let front = front("eco", legacy.port).await;
    let body = br#"{"a":1}"#;
    let raw = format!(
        "POST /eco?x=1 HTTP/1.1\r\nHost: foo.ts.net\r\nX-Forwarded-For: 100.64.0.9\r\nX-Comandos-Token: {TOKEN}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        std::str::from_utf8(body).unwrap()
    );
    let wire = send(front.port, raw.as_bytes()).await;
    assert_eq!(wire.status, 200, "{wire:?}");

    let seen = legacy.seen.lock().unwrap().clone();
    assert_eq!(seen.len(), 1);
    let got = &seen[0];
    assert_eq!(got.method, "POST");
    assert_eq!(got.target, "/eco?x=1");
    assert_eq!(got.body, body);
    let names: Vec<String> = got
        .headers
        .iter()
        .map(|(k, _)| k.to_ascii_lowercase())
        .collect();
    assert_eq!(
        names,
        [
            "host",
            "x-forwarded-for",
            "x-comandos-token",
            "content-type",
            "content-length"
        ],
        "{:?}",
        got.headers
    );
    let value = |n: &str| {
        got.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(n))
            .map(|(_, v)| v.as_str())
    };
    assert_eq!(value("host"), Some("foo.ts.net"));
    assert_eq!(value("x-forwarded-for"), Some("100.64.0.9"));
    assert_eq!(value("x-comandos-token"), Some(TOKEN));
    assert_eq!(value("content-length"), Some("7"));

    // La respuesta del heredado sale con sus cabeceras y su longitud.
    let echo: serde_json::Value = serde_json::from_slice(&wire.body).unwrap();
    assert_eq!(echo["target"], "/eco?x=1");
    assert_eq!(echo["body"], r#"{"a":1}"#);
    assert_eq!(wire.header("server"), Some("Falso/1"));
    assert_eq!(wire.header("content-type"), Some("application/json"));
    assert_eq!(wire.header("cache-control"), Some("no-store"));
    assert_eq!(wire.all("cache-control").len(), 1);
    assert_eq!(wire.all("x-orden"), ["1", "2"]);
    assert_eq!(
        wire.header("content-length"),
        Some(wire.body.len().to_string().as_str())
    );
    assert!(!wire.chunked);
    front.finish().await;
}

/// (a') Una petición local directa no gana `X-Forwarded-For`, `Via` ni `X-Real-IP`.
#[tokio::test]
async fn local_request_gains_no_proxy_headers() {
    let legacy = legacy().await;
    let front = front("local", legacy.port).await;
    let wire = send(front.port, &get("/eco", front.port)).await;
    assert_eq!(wire.status, 200);
    let seen = legacy.seen.lock().unwrap().clone();
    let names: Vec<String> = seen[0]
        .headers
        .iter()
        .map(|(k, _)| k.to_ascii_lowercase())
        .collect();
    assert_eq!(names, ["host"], "{:?}", seen[0].headers);
    assert_eq!(seen[0].headers[0].1, format!("127.0.0.1:{}", front.port));
    front.finish().await;
}

/// (b) El cuerpo que tarda 3 s llega entero pese a `handler_timeout` de 1 s.
#[tokio::test]
async fn slow_body_outlives_the_handler_timeout() {
    let legacy = legacy().await;
    let front = front("lento", legacy.port).await;
    let started = Instant::now();
    let wire = send(front.port, &get("/lento", front.port)).await;
    assert_eq!(wire.status, 200, "{wire:?}");
    assert_eq!(wire.body, b"inicio-fin");
    assert_eq!(wire.header("content-length"), Some("10"));
    assert!(started.elapsed() >= Duration::from_millis(2900));
    front.finish().await;
}

/// (b') `handler_timeout` cubre hasta las cabeceras del heredado.
#[tokio::test]
async fn slow_headers_hit_the_handler_timeout() {
    let legacy = legacy().await;
    let front = front("lento-cab", legacy.port).await;
    let started = Instant::now();
    let wire = send(front.port, &get("/lento-cabeceras", front.port)).await;
    assert_eq!(wire.status, 504);
    assert!(started.elapsed() < Duration::from_millis(2500));
    front.finish().await;
}

/// (c) Sin heredado: 502 con el JSON exacto y `no-store`, en menos de 1 s.
#[tokio::test]
async fn missing_legacy_answers_502_quickly() {
    let closed = {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        l.local_addr().unwrap().port()
    };
    let front = front("sin", closed).await;
    let started = Instant::now();
    let wire = send(front.port, &get("/eco", front.port)).await;
    assert!(started.elapsed() < Duration::from_secs(1));
    assert_eq!(wire.status, 502);
    assert_eq!(
        String::from_utf8(wire.body.clone()).unwrap(),
        r#"{"error": "Servidor heredado no disponible"}"#
    );
    assert_eq!(wire.header("content-type"), Some("application/json"));
    assert_eq!(wire.header("cache-control"), Some("no-store"));
    front.finish().await;
}

/// (d) 5 MiB llegan íntegros.
#[tokio::test]
async fn large_body_arrives_whole() {
    let legacy = legacy().await;
    let front = front("grande", legacy.port).await;
    let wire = send(front.port, &get("/grande", front.port)).await;
    assert_eq!(wire.status, 200);
    assert_eq!(wire.body.len(), BIG);
    assert!(
        wire.body
            .iter()
            .enumerate()
            .all(|(i, b)| *b == ((i % 65536) % 251) as u8)
    );
    assert_eq!(
        wire.header("content-length"),
        Some(BIG.to_string().as_str())
    );
    front.finish().await;
}

/// (e) `Connection: close` del heredado se conserva; el cuerpo sin longitud
/// llega entero y bien terminado.
#[tokio::test]
async fn connection_close_is_kept() {
    let legacy = legacy().await;
    let front = front("cierra", legacy.port).await;
    // Sin `Connection: close` del cliente: la cabecera viene del heredado.
    let raw = format!(
        "GET /cierra HTTP/1.1\r\nHost: 127.0.0.1:{}\r\n\r\n",
        front.port
    );
    let wire = send(front.port, raw.as_bytes()).await;
    assert_eq!(wire.status, 200);
    assert_eq!(wire.header("connection"), Some("close"));
    assert_eq!(wire.body, b"cerrado");
    // Sin longitud del heredado, el frente trocea (`chunked`) y cierra bien.
    assert!(wire.chunked, "{wire:?}");
    assert!(!wire.truncated, "{wire:?}");
    front.finish().await;
}

/// Corte a media respuesta: el cliente ve la conexión cerrada antes de tiempo.
#[tokio::test]
async fn cut_response_is_cut_for_the_client() {
    let legacy = legacy().await;
    let front = front("corta", legacy.port).await;
    let wire = send(front.port, &get("/corta", front.port)).await;
    assert_eq!(wire.status, 200);
    assert_eq!(wire.header("content-length"), Some("100"));
    assert!(wire.body.len() < 100, "{}", wire.body.len());
    front.finish().await;
}

/// (f) Remoto sin token: 401 del frente y el heredado no recibe nada.
#[tokio::test]
async fn remote_without_token_never_reaches_legacy() {
    let legacy = legacy().await;
    let front = front("puerta", legacy.port).await;
    let raw = "POST /eco HTTP/1.1\r\nHost: foo.ts.net\r\nX-Forwarded-For: 100.64.0.9\r\nContent-Type: application/json\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}";
    let wire = send(front.port, raw.as_bytes()).await;
    assert_eq!(wire.status, 401);
    let raw = "GET /state HTTP/1.1\r\nHost: foo.ts.net\r\nX-Forwarded-For: 100.64.0.9\r\nConnection: close\r\n\r\n";
    let wire = send(front.port, raw.as_bytes()).await;
    assert_eq!(wire.status, 401);
    sleep(Duration::from_millis(200)).await;
    assert!(legacy.seen.lock().unwrap().is_empty());
    front.finish().await;
}

/// Heredado que escucha con backlog 5, como `socketserver` en Python: una ráfaga de 32
/// peticiones a la vez no puede traducirse en SYN encolados (≈ 1 s de retransmisión);
/// el frente limita las conexiones en curso y todas responden en bien menos de un segundo.
#[tokio::test]
async fn a_burst_against_a_backlog_5_legacy_has_no_syn_retransmit_stalls() {
    let socket = socket2::Socket::new(
        socket2::Domain::IPV4,
        socket2::Type::STREAM,
        Some(socket2::Protocol::TCP),
    )
    .unwrap();
    socket
        .bind(
            &"127.0.0.1:0"
                .parse::<std::net::SocketAddr>()
                .unwrap()
                .into(),
        )
        .unwrap();
    socket.listen(5).unwrap();
    socket.set_nonblocking(true).unwrap();
    let std_listener: std::net::TcpListener = socket.into();
    let listener = TcpListener::from_std(std_listener).unwrap();
    let port = listener.local_addr().unwrap().port();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = seen.clone();
    tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                return;
            };
            // Acepta despacio, como un hilo por conexión que arranca: el backlog se llena.
            sleep(Duration::from_millis(5)).await;
            tokio::spawn(answer(stream, log.clone()));
        }
    });
    let front = front("burst", port).await;
    let started = Instant::now();
    let mut tasks = Vec::new();
    for i in 0..32 {
        let p = front.port;
        tasks.push(tokio::spawn(async move {
            send(p, &get(&format!("/eco?i={i}"), p)).await
        }));
    }
    for task in tasks {
        let wire = task.await.unwrap();
        assert_eq!(wire.status, 200);
    }
    let elapsed = started.elapsed();
    assert!(
        elapsed < Duration::from_millis(900),
        "la ráfaga tardó {elapsed:?}: hay SYN esperando retransmisión"
    );
    assert_eq!(seen.lock().unwrap().len(), 32);
    front.finish().await;
}
