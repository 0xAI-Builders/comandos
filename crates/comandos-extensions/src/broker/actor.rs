//! Actor por upstream: único dueño del `Mux` y del proceso. Al arrancar inicializa el
//! upstream él mismo (un cliente interno de calentamiento) y solo da altas cuando el
//! `initialize` tiene respuesta; así el cliente no lee stdin hasta saber que el broker sirve.
use super::{
    Key,
    mux::{ClientId, Mux, Outbound},
    upstream::{self, Upstream},
};
use std::{
    collections::{HashMap, VecDeque},
    sync::{Arc, Mutex, PoisonError},
    time::Duration,
};
use tokio::{
    sync::{mpsc, oneshot, watch},
    task::JoinHandle,
    time::Instant,
};

/// Capacidad de cada canal de entrada del actor.
const INBOX: usize = 256;
/// Líneas pendientes hacia un cliente; si se llena, el cliente es lento y se desconecta.
const CLIENT_QUEUE: usize = 1024;
/// Hueco mínimo en el stdin del upstream para leer otra línea de cliente.
const HEADROOM: usize = 8;
/// Líneas del upstream por vuelta antes de atender a los clientes.
const BATCH: usize = 64;
/// Espera máxima para encolar en el stdin una línea que no puede perderse.
const STDIN_WAIT: Duration = Duration::from_secs(2);
/// Si muere antes de este plazo, cuenta como fallo de arranque aunque contestara.
const EARLY_DEATH: Duration = Duration::from_millis(500);
/// Plazo para que el upstream conteste el `initialize` de calentamiento.
const INIT_TIMEOUT: Duration = Duration::from_secs(30);
/// Tras un fallo de arranque, toda alta recibe el mismo error durante este tiempo.
pub(super) const COOLDOWN: Duration = Duration::from_secs(30);

const WARM_INIT: &[u8] = br#"{"jsonrpc":"2.0","id":0,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"comandos-broker","version":"1"}}}"#;
const WARM_DONE: &[u8] = br#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#;

/// Fallos de arranque recientes: clave → (hasta cuándo, mensaje).
pub(super) type Failures = Arc<Mutex<HashMap<Key, (Instant, String)>>>;

pub(super) enum JoinError {
    /// El actor se está cerrando: pedir otro al registro.
    Retry,
    /// El upstream no arrancó: devolver este error al cliente.
    Failed(String),
}

pub(super) type Joined = Result<(ClientId, mpsc::Receiver<Vec<u8>>), JoinError>;

/// Altas y bajas: nunca se frenan por un stdin lleno.
pub(super) enum Ctl {
    Join(oneshot::Sender<Joined>),
    Leave(ClientId),
}

pub(super) struct Handle {
    pub ctl: mpsc::Sender<Ctl>,
    pub lines: mpsc::Sender<(ClientId, Vec<u8>)>,
    pub task: JoinHandle<()>,
}

/// Contexto compartido de todos los actores.
#[derive(Clone)]
pub(super) struct Shared {
    pub idle: Duration,
    pub stop: watch::Receiver<bool>,
    pub failures: Failures,
}

enum End {
    Stop,
    Idle,
    Eof,
    Closed,
    InitTimeout,
    InitRejected,
}

struct Actor {
    key: Key,
    mux: Mux,
    clients: HashMap<ClientId, mpsc::Sender<Vec<u8>>>,
    stdin: mpsc::Sender<Vec<u8>>,
    warm: ClientId,
    warm_answered: bool,
    ready: bool,
    waiting: Vec<oneshot::Sender<Joined>>,
}

pub(super) fn start(key: Key, up: Upstream, shared: Shared) -> Handle {
    let (ctl, ctl_rx) = mpsc::channel(INBOX);
    let (lines, lines_rx) = mpsc::channel(INBOX);
    let mut mux = Mux::new();
    let warm = mux.add_client();
    let actor = Actor {
        key,
        mux,
        clients: HashMap::new(),
        stdin: up.stdin.clone(),
        warm,
        warm_answered: false,
        ready: false,
        waiting: Vec::new(),
    };
    let task = tokio::spawn(actor.run(ctl_rx, lines_rx, up, shared));
    Handle { ctl, lines, task }
}

impl Actor {
    async fn run(
        mut self,
        mut ctl: mpsc::Receiver<Ctl>,
        mut lines: mpsc::Receiver<(ClientId, Vec<u8>)>,
        mut up: Upstream,
        mut sh: Shared,
    ) {
        let spawned = Instant::now();
        let warm = self.mux.from_client(self.warm, WARM_INIT);
        self.dispatch(warm).await;
        let mut idle_at = Some(spawned + sh.idle);
        let end = loop {
            let room = self.stdin.capacity() >= HEADROOM;
            let wake = idle_at.unwrap_or_else(|| Instant::now() + sh.idle);
            tokio::select! {
                biased;
                _ = sh.stop.changed() => break End::Stop,
                line = up.lines.recv() => {
                    let Some(line) = line else { break End::Eof };
                    self.upstream_line(&line).await;
                    for _ in 1..BATCH {
                        let Ok(line) = up.lines.try_recv() else { break };
                        self.upstream_line(&line).await;
                    }
                    // Turno de los clientes tras cada tanda, aunque el upstream no calle.
                    for _ in 0..BATCH {
                        match ctl.try_recv() {
                            Ok(msg) => self.control(msg).await,
                            Err(_) => break,
                        }
                    }
                    for _ in 0..BATCH {
                        if self.stdin.capacity() < HEADROOM {
                            break;
                        }
                        let Ok((id, line)) = lines.try_recv() else { break };
                        self.client_line(id, &line).await;
                    }
                }
                msg = ctl.recv() => match msg {
                    Some(msg) => self.control(msg).await,
                    None => break End::Closed,
                },
                Some((id, line)) = lines.recv(), if room => self.client_line(id, &line).await,
                _ = tokio::time::sleep_until(spawned + INIT_TIMEOUT), if !self.ready => {
                    break End::InitTimeout;
                }
                _ = tokio::time::sleep_until(wake), if idle_at.is_some() => break End::Idle,
            }
            if self.warm_answered && !self.ready {
                break End::InitRejected;
            }
            let unused = self.ready && self.clients.is_empty() && self.waiting.is_empty();
            idle_at = match (unused, idle_at) {
                (false, _) => None,
                (true, None) => Some(Instant::now() + sh.idle),
                (true, kept) => kept,
            };
        };
        // Edad al terminar, antes de los 5 s de gracia de `terminate`.
        let lived = spawned.elapsed();
        let early = !(self.ready && lived >= EARLY_DEATH);
        // Si el upstream murió solo, su estado de salida va en el mensaje; en los demás
        // casos se cierra después de rechazar las altas, para no hacerlas esperar.
        let mut status = None;
        if matches!(end, End::Eof) {
            status = Some(upstream::terminate(&mut up).await);
        }
        let reason = match end {
            End::Stop => "parada del broker".to_string(),
            End::Idle => "inactividad".into(),
            End::Closed => "registro cerrado".into(),
            End::Eof if !early => "fin del upstream".into(),
            End::Eof => format!(
                "upstream murió al arrancar: {}",
                status.as_deref().unwrap_or("desconocido")
            ),
            End::InitTimeout => "upstream no respondió a initialize".into(),
            End::InitRejected => "upstream rechazó initialize".into(),
        };
        let failed = match end {
            End::Eof if early => Some(reason.clone()),
            End::InitTimeout | End::InitRejected => Some(reason.clone()),
            _ => None,
        };
        if let Some(msg) = &failed {
            // Antes de cerrar los canales: un reintento debe encontrar ya el fallo.
            let mut map = sh.failures.lock().unwrap_or_else(PoisonError::into_inner);
            map.insert(self.key.clone(), (Instant::now() + COOLDOWN, msg.clone()));
            eprintln!(
                "broker: fallo {}: {msg} (espera {} s)",
                self.key,
                COOLDOWN.as_secs()
            );
        }
        let refuse = || match &failed {
            Some(msg) => JoinError::Failed(msg.clone()),
            None => JoinError::Retry,
        };
        ctl.close();
        lines.close();
        for reply in self.waiting.drain(..) {
            let _ = reply.send(Err(refuse()));
        }
        while let Ok(msg) = ctl.try_recv() {
            if let Ctl::Join(reply) = msg {
                let _ = reply.send(Err(refuse()));
            }
        }
        let clients = self.clients.len();
        self.clients.clear(); // EOF a todos sus clientes.
        let status = match status {
            Some(status) => status,
            None => upstream::terminate(&mut up).await,
        };
        eprintln!(
            "broker: exit {} pid {} ({reason}; {clients} clientes; estado {status})",
            self.key, up.pid
        );
    }

    async fn upstream_line(&mut self, line: &[u8]) {
        let out = self.mux.from_upstream(line);
        self.dispatch(out).await;
        if !self.ready && self.mux.upstream_initialized() {
            self.ready = true;
            let done = self.mux.from_client(self.warm, WARM_DONE);
            self.dispatch(done).await;
            let gone = self.mux.remove_client(self.warm);
            self.dispatch(gone).await;
            eprintln!("broker: listo {}", self.key);
            for reply in std::mem::take(&mut self.waiting) {
                self.join(reply);
            }
        }
    }

    async fn control(&mut self, msg: Ctl) {
        match msg {
            Ctl::Join(reply) if self.ready => self.join(reply),
            Ctl::Join(reply) => self.waiting.push(reply),
            Ctl::Leave(id) => self.detach(id, "desconectado").await,
        }
    }

    fn join(&mut self, reply: oneshot::Sender<Joined>) {
        let id = self.mux.add_client();
        let (tx, rx) = mpsc::channel(CLIENT_QUEUE);
        if reply.send(Ok((id, rx))).is_ok() {
            self.clients.insert(id, tx);
            let n = self.clients.len();
            eprintln!(
                "broker: attach {} cliente {id} ({n} clientes)",
                self.key.name
            );
        } else {
            // Nadie espera ya (plazo del attach vencido): sin estado que limpiar.
            self.mux.remove_client(id);
        }
    }

    async fn client_line(&mut self, id: ClientId, line: &[u8]) {
        if self.clients.contains_key(&id) {
            let out = self.mux.from_client(id, line);
            self.dispatch(out).await;
        }
    }

    async fn detach(&mut self, id: ClientId, why: &str) {
        if self.clients.remove(&id).is_some() {
            let n = self.clients.len();
            eprintln!(
                "broker: detach {} cliente {id} {why} ({n} clientes)",
                self.key.name
            );
            let out = self.mux.remove_client(id);
            self.dispatch(out).await;
        }
    }

    /// Hacia el upstream nada se descarta salvo tras 2 s de stdin lleno; un cliente con la
    /// cola llena o cerrada se desconecta (y sus cancelaciones siguen el mismo camino).
    async fn dispatch(&mut self, out: Vec<Outbound>) {
        let mut queue = VecDeque::from(out);
        while let Some(o) = queue.pop_front() {
            match o {
                Outbound::ToUpstream(line) => self.send_upstream(line).await,
                Outbound::ToClient(id, _) if id == self.warm => self.warm_answered = true,
                Outbound::ToClient(id, line) => {
                    let sent = self.clients.get(&id).map(|tx| tx.try_send(line).is_ok());
                    if sent == Some(false) && self.clients.remove(&id).is_some() {
                        let n = self.clients.len();
                        eprintln!(
                            "broker: detach {} cliente {id} sin leer su salida ({n} clientes)",
                            self.key.name
                        );
                        queue.extend(self.mux.remove_client(id));
                    }
                }
            }
        }
    }

    async fn send_upstream(&mut self, line: Vec<u8>) {
        use mpsc::error::TrySendError;
        let line = match self.stdin.try_send(line) {
            Ok(()) => return,
            Err(TrySendError::Closed(_)) => {
                eprintln!(
                    "broker: {}: el escritor del upstream terminó",
                    self.key.name
                );
                return;
            }
            Err(TrySendError::Full(line)) => line,
        };
        match tokio::time::timeout(STDIN_WAIT, self.stdin.send(line)).await {
            Ok(Ok(())) => {}
            Ok(Err(_)) => eprintln!(
                "broker: {}: el escritor del upstream terminó",
                self.key.name
            ),
            Err(_) => eprintln!(
                "broker: {}: stdin del upstream lleno 2 s, línea descartada",
                self.key.name
            ),
        }
    }
}
