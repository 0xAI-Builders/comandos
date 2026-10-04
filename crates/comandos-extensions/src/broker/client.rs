//! Cliente fino de `serve`: se registra en el broker y reenvía líneas stdin⇄socket. Toda la
//! traducción la hace el daemon. Si el broker no sirve, `connect` (o `relay`, mientras no
//! se haya leído nada de stdin) falla y `serve` vuelve al proxy directo.
//!
//! Si el daemon se reinicia con la sesión viva (`systemctl restart`, fallo), el cliente se
//! vuelve a conectar sin que la sesión lo note salvo por un error en sus peticiones en vuelo:
//! repite el `attach` y el `initialize` original (y descarta su respuesta) y reenvía
//! `notifications/initialized`. Un cierre decidido por un daemon vivo (el upstream terminó,
//! cliente lento) sigue siendo definitivo.
use super::{check_private_dir, read_line, read_line_with, scan::IdScan};
use crate::Result;
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    os::unix::fs::MetadataExt,
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader, WriteHalf},
    net::UnixStream,
    sync::mpsc,
    task::JoinHandle,
    time::Instant,
};

/// Plazo para conectar con el socket del broker.
const CONNECT_TIMEOUT: Duration = Duration::from_millis(300);
/// Plazo para la respuesta al `attach`: algo más que la espera del daemon por el
/// `initialize` de un upstream nuevo, para recibir su error en vez de cortar antes.
const REPLY_TIMEOUT: Duration = Duration::from_secs(6);
/// Tras cerrar stdin, tiempo máximo esperando a que el broker cierre.
const DRAIN_TIMEOUT: Duration = Duration::from_secs(2);
/// Esperas antes de cada intento de reconexión tras un reinicio del daemon.
const RETRY: [Duration; 3] = [
    Duration::from_millis(200),
    Duration::from_secs(1),
    Duration::from_secs(3),
];
/// Líneas pendientes en cada sentido entre las tareas lectoras y el bucle.
const LINE_QUEUE: usize = 64;

/// Conexión adjuntada al broker. Guarda quién era el daemon (pid por `SO_PEERCRED`) y el
/// inodo del socket, para distinguir al recibir EOF un reinicio de un cierre definitivo.
pub struct Connection {
    stream: BufReader<UnixStream>,
    daemon: Option<i32>,
    socket_ino: Option<u64>,
}

/// Conecta y envía la línea `attach` (ver [`super::attach_request`]). `Ok` solo tras
/// `{"ok":true}`; cualquier otra respuesta, plazo vencido o error es `Err`.
pub async fn connect(socket: &Path, request: &Value) -> Result<Connection> {
    let dir = socket.parent().ok_or("Ruta de socket inválida")?;
    check_private_dir(dir).map_err(|e| e.to_string())?;
    let stream = tokio::time::timeout(CONNECT_TIMEOUT, UnixStream::connect(socket))
        .await
        .map_err(|_| "El broker no responde")?
        .map_err(|e| e.to_string())?;
    let daemon = stream.peer_cred().ok().and_then(|c| c.pid());
    let socket_ino = std::fs::metadata(socket).ok().map(|m| m.ino());
    let mut stream = BufReader::new(stream);
    let attach = format!("{request}\n");
    let sent = stream.write_all(attach.as_bytes()).await;
    sent.map_err(|e| e.to_string())?;
    let mut line = Vec::new();
    let read = tokio::time::timeout(REPLY_TIMEOUT, read_line(&mut stream, &mut line)).await;
    if !matches!(read, Ok(Ok(true))) {
        return Err("El broker no respondió al attach".into());
    }
    let reply: Value =
        serde_json::from_slice(&line).map_err(|_| "Respuesta del broker inválida")?;
    if reply["ok"] == true {
        return Ok(Connection {
            stream,
            daemon,
            socket_ino,
        });
    }
    let error = reply["error"]
        .as_str()
        .unwrap_or("El broker rechazó el attach");
    Err(error.into())
}

/// Conexión en uso: una tarea lee las líneas del daemon; el bucle escribe.
struct Live {
    lines: mpsc::Receiver<Vec<u8>>,
    writer: WriteHalf<BufReader<UnixStream>>,
    /// Una escritura falló: el daemon cerró; se espera su EOF por el lado de lectura.
    broken: bool,
    reader: JoinHandle<()>,
    daemon: Option<i32>,
    socket_ino: Option<u64>,
}

impl Live {
    fn start(conn: Connection) -> Self {
        let (reader, writer) = tokio::io::split(conn.stream);
        let (tx, lines) = mpsc::channel(LINE_QUEUE);
        let reader = tokio::spawn(async move {
            // La mitad de lectura no es `AsyncBufRead`; los bytes que el `BufReader` interior
            // ya tuviera tras la respuesta al `attach` salen primero por `poll_read`.
            let mut reader = BufReader::new(reader);
            let mut buf = Vec::new();
            while let Ok(true) = read_line_with(&mut reader, &mut buf, false).await {
                if tx.send(std::mem::take(&mut buf)).await.is_err() {
                    break;
                }
            }
        });
        Self {
            lines,
            writer,
            broken: false,
            reader,
            daemon: conn.daemon,
            socket_ino: conn.socket_ino,
        }
    }

    async fn send(&mut self, line: &[u8]) {
        if self.broken {
            return;
        }
        let mut bytes = Vec::with_capacity(line.len() + 1);
        bytes.extend_from_slice(line);
        bytes.push(b'\n');
        if self.writer.write_all(&bytes).await.is_err() {
            self.broken = true;
        }
    }

    /// `true` si el daemon de esta conexión ya no está: su proceso murió o el socket fue
    /// borrado o reemplazado (un daemon parado con SIGTERM borra el socket antes de cerrar a
    /// sus clientes). Si sigue vivo con el mismo socket, el cierre fue decisión suya.
    fn daemon_gone(&self, socket: &Path) -> bool {
        let ino = std::fs::metadata(socket).ok().map(|m| m.ino());
        let replaced = ino.is_none() || ino != self.socket_ino;
        replaced || self.daemon.is_some_and(|pid| !alive(pid))
    }
}

impl Drop for Live {
    fn drop(&mut self) {
        self.reader.abort();
    }
}

/// Proceso vivo (no zombi), según `/proc/<pid>/stat`.
fn alive(pid: i32) -> bool {
    std::fs::read_to_string(format!("/proc/{pid}/stat")).is_ok_and(|s| {
        s.rsplit(')')
            .next()
            .is_some_and(|r| !r.trim_start().starts_with('Z'))
    })
}

/// Lo que la sesión ya negoció y lo que tiene en vuelo, para sobrevivir a un reinicio.
#[derive(Default)]
struct Session {
    /// Línea `initialize` original y su id (serializado).
    init: Option<(Vec<u8>, String)>,
    /// El `initialize` ya tuvo respuesta: se repite al reconectar.
    init_done: bool,
    notified: bool,
    /// Peticiones del cliente sin respuesta: id serializado → id.
    inflight: HashMap<String, Value>,
}

impl Session {
    fn track_client(&mut self, line: &[u8]) {
        let scan = IdScan::of(line);
        let Some(method) = scan.method() else {
            return; // respuesta del cliente a un request del upstream
        };
        if method == "notifications/initialized" {
            self.notified = true;
        }
        let Some(id) = scan.id() else {
            return;
        };
        let key = id.to_string();
        if method == "initialize" && self.init.is_none() {
            self.init = Some((line.to_vec(), key.clone()));
        }
        self.inflight.insert(key, id);
    }

    fn track_broker(&mut self, line: &[u8]) {
        if self.inflight.is_empty() {
            return;
        }
        let scan = IdScan::of(line);
        if scan.has_method() {
            return; // request o notificación del upstream
        }
        let Some(key) = scan.id().map(|id| id.to_string()) else {
            return;
        };
        if self.inflight.remove(&key).is_some()
            && self.init.as_ref().is_some_and(|(_, k)| *k == key)
        {
            self.init_done = true;
        }
    }

    /// Error -32603 para cada petición en vuelo, que el daemon caído ya no contestará.
    fn lost(&mut self) -> Vec<Vec<u8>> {
        (self.inflight.drain())
            .map(|(_, id)| {
                let e = json!({"jsonrpc":"2.0","id":id,"error":{"code":-32603,"message":"broker reiniciado"}});
                e.to_string().into_bytes()
            })
            .collect()
    }

    /// Pone a una conexión nueva en el estado de la anterior: `initialize` repetido (su
    /// respuesta se descarta) y `notifications/initialized`. `false` si el daemon nuevo no
    /// contesta bien al `initialize`.
    async fn resume(&self, live: &mut Live) -> bool {
        if let Some((line, key)) = self.init.as_ref().filter(|_| self.init_done) {
            live.send(line).await;
            let deadline = Instant::now() + REPLY_TIMEOUT;
            loop {
                let Ok(Some(reply)) = tokio::time::timeout_at(deadline, live.lines.recv()).await
                else {
                    return false;
                };
                let scan = IdScan::of(&reply);
                if scan.has_method() || scan.id().map(|id| id.to_string()).as_ref() != Some(key) {
                    continue;
                }
                let ok = serde_json::from_slice::<Value>(&reply)
                    .is_ok_and(|v| v.get("result").is_some());
                if !ok {
                    return false;
                }
                break;
            }
        }
        if self.notified {
            live.send(br#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#)
                .await;
        }
        !live.broken
    }
}

/// Líneas de stdin, leídas por una tarea propia (las lecturas parciales no deben perderse al
/// cancelar un `select!`). `consumed` se marca en cuanto se saca un byte del descriptor.
fn stdin_lines(consumed: Arc<AtomicBool>) -> mpsc::Receiver<Vec<u8>> {
    let (tx, rx) = mpsc::channel(LINE_QUEUE);
    tokio::spawn(async move {
        let mut stdin = BufReader::new(tokio::io::stdin());
        let mut buf = Vec::new();
        loop {
            if stdin.fill_buf().await.is_ok_and(|c| !c.is_empty()) {
                consumed.store(true, Ordering::Relaxed);
            }
            match read_line(&mut stdin, &mut buf).await {
                Ok(true) => {
                    if tx.send(std::mem::take(&mut buf)).await.is_err() {
                        break;
                    }
                }
                _ => break,
            }
        }
    });
    rx
}

async fn emit(stdout: &mut tokio::io::Stdout, line: &[u8]) -> std::io::Result<()> {
    stdout.write_all(line).await?;
    stdout.write_all(b"\n").await?;
    stdout.flush().await
}

/// Reenvía stdin→broker y broker→stdout. Devuelve el código de salida de la sesión.
///
/// `Err` solo si el broker cerró antes de que se leyera un byte de stdin y sin haber
/// escrito nada en stdout: el llamador aún puede usar el proxy directo. Después, un EOF del
/// broker reconecta si el daemon se reinició (ver el módulo) o termina la sesión. Al cerrar
/// stdin se cierra la mitad de escritura y se espera al broker como mucho [`DRAIN_TIMEOUT`].
pub async fn relay(conn: Connection, socket: &Path, request: &Value) -> Result<i32> {
    let consumed = Arc::new(AtomicBool::new(false));
    let mut input = stdin_lines(consumed.clone());
    let mut stdout = tokio::io::stdout();
    let mut session = Session::default();
    let mut wrote = false;
    let mut live = Live::start(conn);
    loop {
        tokio::select! {
            line = input.recv() => match line {
                Some(line) => {
                    session.track_client(&line);
                    live.send(&line).await;
                }
                None => return Ok(drain(live, &mut stdout).await),
            },
            line = live.lines.recv() => match line {
                Some(line) => {
                    session.track_broker(&line);
                    if emit(&mut stdout, &line).await.is_err() {
                        return Ok(1);
                    }
                    wrote = true;
                }
                None if !wrote && !consumed.load(Ordering::Relaxed) => {
                    return Err("El broker cerró antes de empezar".into());
                }
                None => {
                    if !live.daemon_gone(socket) {
                        return Ok(0);
                    }
                    for error in session.lost() {
                        if emit(&mut stdout, &error).await.is_err() {
                            return Ok(1);
                        }
                    }
                    match reconnect(&session, socket, request).await {
                        Some(next) => live = next,
                        None => return Ok(0),
                    }
                }
            },
        }
    }
}

/// Hasta tres intentos (tras 200 ms, 1 s y 3 s) de adjuntarse a un daemon nuevo.
async fn reconnect(session: &Session, socket: &Path, request: &Value) -> Option<Live> {
    for delay in RETRY {
        tokio::time::sleep(delay).await;
        let Ok(conn) = connect(socket, request).await else {
            continue;
        };
        let mut live = Live::start(conn);
        if session.resume(&mut live).await {
            return Some(live);
        }
    }
    None
}

/// Stdin cerrado: se cierra la escritura y se reenvía lo que quede hasta que el broker
/// cierre o pasen [`DRAIN_TIMEOUT`].
async fn drain(mut live: Live, stdout: &mut tokio::io::Stdout) -> i32 {
    let _ = live.writer.shutdown().await;
    let deadline = Instant::now() + DRAIN_TIMEOUT;
    while let Ok(Some(line)) = tokio::time::timeout_at(deadline, live.lines.recv()).await {
        if emit(stdout, &line).await.is_err() {
            return 1;
        }
    }
    0
}
