//! Actor por nombre de servidor: único dueño del `Mux` y del upstream. Los clientes le
//! hablan por un canal acotado; él escribe a cada cliente por su propio canal acotado.
use super::{
    mux::{ClientId, Mux, Outbound},
    upstream::{self, Upstream},
};
use std::{collections::HashMap, time::Duration};
use tokio::{
    sync::{mpsc, oneshot, watch},
    task::JoinHandle,
    time::Instant,
};

/// Mensajes de clientes pendientes de procesar por el actor.
const INBOX: usize = 256;
/// Líneas pendientes hacia un cliente; si se llena, el cliente es lento y se desconecta.
const CLIENT_QUEUE: usize = 1024;
/// Hueco mínimo en el stdin del upstream para aceptar otro mensaje de cliente: cada uno
/// produce como mucho una línea upstream (las cancelaciones de un `Leave`, más).
const HEADROOM: usize = 8;

/// Alta aceptada: id del cliente y su cola de salida. `None` ⇒ el actor se está cerrando
/// y el llamador debe pedir otro al registro.
pub(super) type Joined = Option<(ClientId, mpsc::Receiver<Vec<u8>>)>;

pub(super) enum Msg {
    Join(oneshot::Sender<Joined>),
    Line(ClientId, Vec<u8>),
    Leave(ClientId),
}

pub(super) struct Handle {
    pub tx: mpsc::Sender<Msg>,
    pub task: JoinHandle<()>,
}

struct Actor {
    name: String,
    mux: Mux,
    clients: HashMap<ClientId, mpsc::Sender<Vec<u8>>>,
    stdin: mpsc::Sender<Vec<u8>>,
}

/// Arranca el actor de `name`. Termina por fin del upstream, inactividad (`idle` sin
/// clientes) o parada global (`stop`); en todos los casos cierra el upstream.
pub(super) fn start(
    name: &str,
    up: Upstream,
    idle: Duration,
    stop: watch::Receiver<bool>,
) -> Handle {
    let (tx, rx) = mpsc::channel(INBOX);
    let actor = Actor {
        name: name.into(),
        mux: Mux::new(),
        clients: HashMap::new(),
        stdin: up.stdin.clone(),
    };
    let task = tokio::spawn(actor.run(rx, up, idle, stop));
    Handle { tx, task }
}

impl Actor {
    async fn run(
        mut self,
        mut rx: mpsc::Receiver<Msg>,
        mut up: Upstream,
        idle: Duration,
        mut stop: watch::Receiver<bool>,
    ) {
        // El primer cliente llega justo tras el arranque; si no, cuenta como inactividad.
        let mut deadline = Some(Instant::now() + idle);
        let reason = loop {
            let room = self.stdin.capacity() >= HEADROOM;
            let wake = deadline.unwrap_or_else(|| Instant::now() + idle);
            tokio::select! {
                biased;
                _ = stop.changed() => break "parada del broker",
                line = up.lines.recv() => match line {
                    Some(line) => {
                        let out = self.mux.from_upstream(&line);
                        self.dispatch(out);
                    }
                    None => break "fin del upstream",
                },
                msg = rx.recv(), if room => match msg {
                    Some(msg) => self.handle(msg),
                    None => break "registro cerrado",
                },
                _ = tokio::time::sleep_until(wake), if deadline.is_some() => break "inactividad",
            }
            // Con clientes no hay plazo; al quedarse sin ellos empieza a contar.
            deadline = match (self.clients.is_empty(), deadline) {
                (false, _) => None,
                (true, None) => Some(Instant::now() + idle),
                (true, kept) => kept,
            };
        };
        // Altas que llegaron tras decidir el cierre: que pidan un actor nuevo.
        rx.close();
        while let Ok(msg) = rx.try_recv() {
            if let Msg::Join(reply) = msg {
                let _ = reply.send(None);
            }
        }
        let clients = self.clients.len();
        self.clients.clear(); // EOF a todos sus clientes.
        let status = upstream::terminate(&mut up).await;
        eprintln!(
            "broker: exit {} pid {} ({reason}; {clients} clientes; estado {status})",
            self.name, up.pid
        );
    }

    fn handle(&mut self, msg: Msg) {
        match msg {
            Msg::Join(reply) => {
                let id = self.mux.add_client();
                let (tx, rx) = mpsc::channel(CLIENT_QUEUE);
                if reply.send(Some((id, rx))).is_ok() {
                    self.clients.insert(id, tx);
                    let n = self.clients.len();
                    eprintln!("broker: attach {} cliente {id} ({n} clientes)", self.name);
                } else {
                    self.mux.remove_client(id);
                }
            }
            Msg::Line(id, line) => {
                if self.clients.contains_key(&id) {
                    let out = self.mux.from_client(id, &line);
                    self.dispatch(out);
                }
            }
            Msg::Leave(id) => self.detach(id, "desconectado"),
        }
    }

    fn detach(&mut self, id: ClientId, why: &str) {
        if self.clients.remove(&id).is_some() {
            let n = self.clients.len();
            eprintln!(
                "broker: detach {} cliente {id} {why} ({n} clientes)",
                self.name
            );
            let out = self.mux.remove_client(id);
            self.dispatch(out);
        }
    }

    /// Nunca espera: el stdin tiene hueco garantizado por `HEADROOM` salvo ráfagas de
    /// cancelaciones (que se descartan, son de mejor esfuerzo); un cliente con la cola
    /// llena o cerrada se desconecta.
    fn dispatch(&mut self, out: Vec<Outbound>) {
        let mut slow = Vec::new();
        for o in out {
            match o {
                Outbound::ToUpstream(line) => {
                    if self.stdin.try_send(line).is_err() {
                        eprintln!(
                            "broker: {}: stdin del upstream lleno, línea descartada",
                            self.name
                        );
                    }
                }
                Outbound::ToClient(id, line) => {
                    let sent = self.clients.get(&id).map(|tx| tx.try_send(line).is_ok());
                    if sent == Some(false) {
                        slow.push(id);
                    }
                }
            }
        }
        for id in slow {
            self.detach(id, "sin leer su salida");
        }
    }
}
