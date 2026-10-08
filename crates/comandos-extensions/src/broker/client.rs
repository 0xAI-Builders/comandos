//! Cliente fino de `serve`: se registra en el broker y reenvía líneas stdin⇄socket. Toda la
//! traducción la hace el daemon. Antes de leer stdin, `connect` puede autorizar el proxy
//! directo si no hay daemon o la sesión no pertenece al broker.
//!
//! Si el daemon se reinicia con la sesión viva (`systemctl restart`, fallo), el cliente se
//! vuelve a conectar sin que la sesión lo note salvo por un error en sus peticiones en vuelo:
//! repite el `attach` y el `initialize` original (y descarta su respuesta) y reenvía
//! `notifications/initialized`. Un cierre decidido por un daemon vivo (el upstream terminó,
//! cliente lento) sigue siendo definitivo.
//!
//! Durante attach el actor conserva el arranque. Una cola saturada, un EOF ambiguo o un
//! error de un daemon antiguo sin `fallback_safe` se reintentan sin leer stdin MCP. Si ese
//! daemon sigue vivo pero no se recupera, el alta continúa esperando; no se inventa una
//! limpieza ni se arranca otro servidor. El cliente que inició la sesión puede cancelarla.
use super::{check_private_dir, read_line, read_line_with, scan::IdScan};
use crate::Result;
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    fs::OpenOptions,
    os::unix::fs::{MetadataExt, OpenOptionsExt},
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
/// Plazo de la reinicialización de una sesión ya adjuntada: el daemon tiene la respuesta
/// cacheada. El attach inicial espera al actor, dueño del plazo de arranque y su limpieza.
const REPLY_TIMEOUT: Duration = Duration::from_secs(6);
/// Tras cerrar stdin, tiempo máximo esperando a que el broker cierre.
const DRAIN_TIMEOUT: Duration = Duration::from_secs(2);
/// Esperas antes de cada intento de reconexión tras un reinicio del daemon. Un reinicio
/// real tarda lo que tarde el daemon viejo en cerrar sus upstreams (hasta 5 s de gracia por
/// cada uno, ssh incluidos) más el arranque del nuevo: la suma cubre ≈ 20 s.
const RETRY: [Duration; 5] = [
    Duration::from_millis(200),
    Duration::from_secs(1),
    Duration::from_secs(3),
    Duration::from_secs(6),
    Duration::from_secs(10),
];
/// Líneas pendientes en cada sentido entre las tareas lectoras y el bucle.
const LINE_QUEUE: usize = 64;
/// Tras un EOF del daemon, cuánto se espera a que su proceso figure muerto antes de dar el
/// cierre por intencionado (ver `Live::daemon_gone_settled`), y cada cuánto se comprueba.
const GONE_GRACE: Duration = Duration::from_secs(1);
const GONE_POLL: Duration = Duration::from_millis(25);

/// Conexión adjuntada al broker. Guarda quién era el daemon (pid por `SO_PEERCRED`) y el
/// inodo del socket, para distinguir al recibir EOF un reinicio de un cierre definitivo.
pub struct Connection {
    stream: BufReader<UnixStream>,
    daemon: Option<i32>,
    socket_ino: Option<u64>,
}

/// El daemon conserva este candado mientras arranca o retira su socket. Un archivo
/// huérfano desbloqueado permite directo; una comprobación imposible conserva la espera.
pub(crate) fn broker_holds_lock(socket: &Path) -> bool {
    let Some(dir) = socket.parent() else {
        return false;
    };
    let file = match OpenOptions::new()
        .read(true)
        .custom_flags(nix::libc::O_NOFOLLOW)
        .open(dir.join("broker.lock"))
    {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return false,
        Err(_) => return true,
    };
    file.try_lock().is_err()
}

/// Conecta y envía la línea `attach` (ver [`super::attach_request`]). `Ok` solo tras
/// `{"ok":true}`. No impone otro plazo al arranque: el actor responde al estar listo o
/// después de terminar su upstream. Cortar antes permitiría un proxy directo duplicado.
pub async fn connect(socket: &Path, request: &Value) -> Result<Connection> {
    connect_owned(socket, request, None).await
}

/// Un EOF después del alta conserva al dueño anterior, incluso si su socket desaparece
/// mientras sigue cerrando upstreams. Reintentar no transfiere esa autoridad a otro daemon.
async fn connect_owned(
    socket: &Path,
    request: &Value,
    mut owner: Option<i32>,
) -> Result<Connection> {
    let dir = socket.parent().ok_or("Ruta de socket inválida")?;
    check_private_dir(dir).map_err(|e| e.to_string())?;
    let attach = format!("{request}\n");
    let mut retry = Duration::from_millis(200);
    loop {
        let connected = tokio::time::timeout(CONNECT_TIMEOUT, UnixStream::connect(socket)).await;
        // Una cola de accept llena produce EAGAIN o un plazo vencido, no confirma que el
        // daemon esté ausente. Solo un fallo definitivo permite el proxy directo.
        let busy = match &connected {
            Err(_) => true,
            Ok(Err(e)) => matches!(
                e.kind(),
                std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted
            ),
            _ => false,
        };
        if let Ok(Ok(stream)) = connected {
            let daemon = stream.peer_cred().ok().and_then(|c| c.pid());
            // Un resultado perdido no libera al dueño. Tampoco adjuntamos a otro daemon
            // mientras el anterior siga vivo: podría conservar el upstream arrancado.
            if owner.is_none_or(|pid| !alive(pid)) || owner == daemon {
                owner = daemon;
                let socket_ino = std::fs::metadata(socket).ok().map(|m| m.ino());
                let mut stream = BufReader::new(stream);
                let mut line = Vec::new();
                if stream.write_all(attach.as_bytes()).await.is_ok()
                    && matches!(read_line(&mut stream, &mut line).await, Ok(true))
                    && let Ok(reply) = serde_json::from_slice::<Value>(&line)
                {
                    if reply["ok"] == true {
                        return Ok(Connection {
                            stream,
                            daemon,
                            socket_ino,
                        });
                    }
                    // Un error antiguo no acredita la limpieza: aquellos daemons podían
                    // rechazar antes de terminar el grupo (timeout de attach o initialize).
                    // El daemon nuevo solo autoriza directo para sesiones que no puede
                    // compartir; fallos de arranque/cooldown siguen bajo el mismo registro.
                    if reply["fallback_safe"] == true
                        && let Some(error) = reply["error"].as_str()
                    {
                        return Err(error.into());
                    }
                }
            }
        }
        if !busy && owner.is_none_or(|pid| !alive(pid)) && !broker_holds_lock(socket) {
            return Err("El broker no respondió al attach".into());
        }
        // Repetir attach al mismo registro es seguro: stdin MCP aún no se ha leído.
        // Nunca se repite una llamada de herramienta por este camino.
        tokio::time::sleep(retry).await;
        retry = (retry * 2).min(Duration::from_secs(5));
    }
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

    /// Como `daemon_gone`, pero da al daemon hasta `GONE_GRACE` para terminar de morir: el
    /// EOF de un proceso que muere (SIGKILL, pánico) llega al cerrar sus descriptores, antes
    /// de que `/proc` lo marque zombi, y bajo carga ese hueco dura decenas de milisegundos.
    /// Un daemon vivo que cerró a propósito sigue vivo con el mismo socket tras la espera.
    async fn daemon_gone_settled(&self, socket: &Path) -> bool {
        let deadline = Instant::now() + GONE_GRACE;
        loop {
            if self.daemon_gone(socket) {
                return true;
            }
            if Instant::now() >= deadline {
                return false;
            }
            tokio::time::sleep(GONE_POLL).await;
        }
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
/// Una vez iniciado el lector de stdin, ningún EOF autoriza proxy directo: puede consumir
/// bytes mientras se espera reattach. El EOF previo a la primera actividad conserva al
/// dueño y reintenta; después reconecta si el daemon se reinició o termina la sesión. Al cerrar
/// stdin se cierra la mitad de escritura y se espera al broker como mucho [`DRAIN_TIMEOUT`].
pub async fn relay(conn: Connection, socket: &Path, request: &Value) -> i32 {
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
                None => return drain(live, &mut stdout).await,
            },
            line = live.lines.recv() => match line {
                Some(line) => {
                    session.track_broker(&line);
                    if emit(&mut stdout, &line).await.is_err() {
                        return 1;
                    }
                    wrote = true;
                }
                None if !wrote && !consumed.load(Ordering::Relaxed) => {
                    match reconnect(&session, socket, request, live.daemon).await {
                        Some(next) => live = next,
                        None => return 1,
                    }
                }
                None => {
                    if !live.daemon_gone_settled(socket).await {
                        return 0;
                    }
                    for error in session.lost() {
                        if emit(&mut stdout, &error).await.is_err() {
                            return 1;
                        }
                    }
                    match reconnect(&session, socket, request, live.daemon).await {
                        Some(next) => live = next,
                        None => return 0,
                    }
                }
            },
        }
    }
}

/// Hasta cinco intentos (tras 200 ms, 1 s, 3 s, 6 s y 10 s) de adjuntarse a un daemon nuevo.
async fn reconnect(
    session: &Session,
    socket: &Path,
    request: &Value,
    owner: Option<i32>,
) -> Option<Live> {
    for delay in RETRY {
        tokio::time::sleep(delay).await;
        let Ok(conn) = connect_owned(socket, request, owner).await else {
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
