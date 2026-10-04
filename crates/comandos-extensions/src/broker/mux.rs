//! Multiplexación JSON-RPC de varios clientes MCP sobre un único upstream. Sin E/S.
use serde_json::{Map, Value};
use std::collections::{BTreeMap, HashMap};

pub type ClientId = u32;

#[derive(Debug, PartialEq, Eq)]
pub enum Outbound {
    ToUpstream(Vec<u8>),
    ToClient(ClientId, Vec<u8>),
}

#[derive(Default)]
struct Client {
    initialized: bool,
    caps: Value,
}

#[derive(Default)]
pub struct Mux {
    next_client: ClientId,
    next_id: u64,
    clients: BTreeMap<ClientId, Client>,
    /// id upstream -> (cliente, id original). Si el cliente se va, la entrada desaparece (huérfano).
    pending: HashMap<u64, (ClientId, Value)>,
    /// Clientes cuyo `initialize` espera la primera respuesta del upstream.
    waiting_init: Vec<(ClientId, Value)>,
    init_up_id: Option<u64>,
    init_result: Option<Value>,
    /// `notifications/initialized` ya reenviada al upstream (solo se manda una).
    upstream_notified: bool,
    /// id original del upstream (serializado) -> (cliente, id entregado al cliente).
    upstream_requests: HashMap<String, (ClientId, Value)>,
}

impl Mux {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn upstream_initialized(&self) -> bool {
        self.init_result.is_some()
    }

    pub fn add_client(&mut self) -> ClientId {
        self.next_client += 1;
        self.clients.insert(self.next_client, Client::default());
        self.next_client
    }

    /// Cancela lo pendiente del cliente; los requests del upstream que esperaban su
    /// respuesta reciben un error para que no queden colgados.
    pub fn remove_client(&mut self, id: ClientId) -> Vec<Outbound> {
        self.clients.remove(&id);
        self.pending.retain(|_, (c, _)| *c != id);
        self.waiting_init.retain(|(c, _)| *c != id);
        let orphans: Vec<String> = self
            .upstream_requests
            .iter()
            .filter(|(_, (c, _))| *c == id)
            .map(|(k, _)| k.clone())
            .collect();
        let err = serde_json::json!({"code": -32000, "message": "client disconnected"});
        orphans
            .into_iter()
            .filter_map(|k| {
                self.upstream_requests.remove(&k);
                let orig = serde_json::from_str::<Value>(&k).ok()?;
                Some(Outbound::ToUpstream(error_response(orig, err.clone())))
            })
            .collect()
    }

    pub fn from_client(&mut self, id: ClientId, line: &[u8]) -> Vec<Outbound> {
        let Ok(Value::Object(mut msg)) = serde_json::from_slice::<Value>(line) else {
            return Vec::new();
        };
        let method = msg.get("method").and_then(Value::as_str).map(str::to_owned);
        match method.as_deref() {
            Some("initialize") => self.client_initialize(id, msg),
            Some("notifications/initialized") => {
                if self.upstream_notified {
                    return Vec::new();
                }
                self.upstream_notified = true;
                vec![Outbound::ToUpstream(bytes(msg))]
            }
            Some("notifications/cancelled") => {
                // El requestId del cliente hay que traducirlo al id upstream.
                let Some(orig) = msg.get("params").and_then(|p| p.get("requestId")).cloned() else {
                    return Vec::new();
                };
                let Some(up) = self
                    .pending
                    .iter()
                    .find(|(_, (c, o))| *c == id && *o == orig)
                    .map(|(k, _)| *k)
                else {
                    return Vec::new();
                };
                if let Some(Value::Object(p)) = msg.get_mut("params") {
                    p.insert("requestId".into(), Value::from(up));
                }
                vec![Outbound::ToUpstream(bytes(msg))]
            }
            Some(_) if msg.contains_key("id") => {
                let orig = msg["id"].clone();
                let up = self.alloc(id, orig);
                msg.insert("id".into(), Value::from(up));
                vec![Outbound::ToUpstream(bytes(msg))]
            }
            Some(_) => vec![Outbound::ToUpstream(bytes(msg))],
            None => {
                // Respuesta del cliente a un request del upstream.
                let Some(given) = msg.get("id").cloned() else {
                    return Vec::new();
                };
                let Some(key) = self
                    .upstream_requests
                    .iter()
                    .find(|(_, (c, g))| *c == id && *g == given)
                    .map(|(k, _)| k.clone())
                else {
                    return Vec::new();
                };
                self.upstream_requests.remove(&key);
                let Ok(orig) = serde_json::from_str::<Value>(&key) else {
                    return Vec::new();
                };
                msg.insert("id".into(), orig);
                vec![Outbound::ToUpstream(bytes(msg))]
            }
        }
    }

    pub fn from_upstream(&mut self, line: &[u8]) -> Vec<Outbound> {
        let Ok(Value::Object(mut msg)) = serde_json::from_slice::<Value>(line) else {
            return Vec::new();
        };
        if let Some(method) = msg.get("method").and_then(Value::as_str).map(str::to_owned) {
            return if msg.contains_key("id") {
                self.upstream_request(&method, msg)
            } else {
                let b = bytes(msg);
                self.clients
                    .iter()
                    .filter(|(_, c)| c.initialized)
                    .map(|(id, _)| Outbound::ToClient(*id, b.clone()))
                    .collect()
            };
        }
        let Some(up) = msg.get("id").and_then(Value::as_u64) else {
            return Vec::new();
        };
        if self.init_up_id == Some(up) {
            return self.init_response(msg);
        }
        let Some((client, orig)) = self.pending.remove(&up) else {
            return Vec::new(); // huérfana o desconocida
        };
        msg.insert("id".into(), orig);
        vec![Outbound::ToClient(client, bytes(msg))]
    }

    fn client_initialize(&mut self, id: ClientId, mut msg: Map<String, Value>) -> Vec<Outbound> {
        let orig = msg.get("id").cloned().unwrap_or(Value::Null);
        if let Some(c) = self.clients.get_mut(&id) {
            c.caps = msg
                .get("params")
                .and_then(|p| p.get("capabilities"))
                .cloned()
                .unwrap_or(Value::Null);
        }
        if let Some(result) = self.init_result.clone() {
            if let Some(c) = self.clients.get_mut(&id) {
                c.initialized = true;
            }
            return vec![Outbound::ToClient(id, response(orig, result))];
        }
        self.waiting_init.push((id, orig));
        if self.init_up_id.is_some() {
            return Vec::new();
        }
        let up = self.fresh_id();
        self.init_up_id = Some(up);
        msg.insert("id".into(), Value::from(up));
        vec![Outbound::ToUpstream(bytes(msg))]
    }

    fn init_response(&mut self, msg: Map<String, Value>) -> Vec<Outbound> {
        self.init_up_id = None;
        let waiting = std::mem::take(&mut self.waiting_init);
        let result = msg.get("result").cloned();
        if let Some(r) = &result {
            self.init_result = Some(r.clone());
        }
        let error = msg.get("error").cloned().unwrap_or(Value::Null);
        let mut out = Vec::new();
        for (c, oid) in waiting {
            let Some(client) = self.clients.get_mut(&c) else {
                continue;
            };
            out.push(match &result {
                Some(r) => {
                    client.initialized = true;
                    Outbound::ToClient(c, response(oid, r.clone()))
                }
                None => Outbound::ToClient(c, error_response(oid, error.clone())),
            });
        }
        out
    }

    /// Request del upstream: al cliente inicializado más reciente que declare la capacidad.
    fn upstream_request(&mut self, method: &str, mut msg: Map<String, Value>) -> Vec<Outbound> {
        let orig = msg["id"].clone();
        let cap = match method {
            "roots/list" => Some("roots"),
            "sampling/createMessage" => Some("sampling"),
            "elicitation/create" => Some("elicitation"),
            _ => None,
        };
        let ready = || self.clients.iter().rev().filter(|(_, c)| c.initialized);
        let target = ready()
            .find(|(_, c)| cap.is_none_or(|k| c.caps.get(k).is_some()))
            .or_else(|| ready().next())
            .map(|(id, _)| *id);
        let Some(target) = target else {
            let err = serde_json::json!({"code": -32601, "message": "no client available"});
            return vec![Outbound::ToUpstream(error_response(orig, err))];
        };
        let given = Value::from(self.fresh_id());
        self.upstream_requests
            .insert(orig.to_string(), (target, given.clone()));
        msg.insert("id".into(), given);
        vec![Outbound::ToClient(target, bytes(msg))]
    }

    fn fresh_id(&mut self) -> u64 {
        self.next_id += 1;
        self.next_id
    }

    fn alloc(&mut self, client: ClientId, orig: Value) -> u64 {
        let id = self.fresh_id();
        self.pending.insert(id, (client, orig));
        id
    }
}

fn bytes(m: Map<String, Value>) -> Vec<u8> {
    serde_json::to_vec(&Value::Object(m)).unwrap_or_default()
}

fn envelope(id: Value, key: &str, body: Value) -> Vec<u8> {
    let mut m = Map::new();
    m.insert("jsonrpc".into(), "2.0".into());
    m.insert("id".into(), id);
    m.insert(key.into(), body);
    bytes(m)
}

fn response(id: Value, result: Value) -> Vec<u8> {
    envelope(id, "result", result)
}

fn error_response(id: Value, error: Value) -> Vec<u8> {
    envelope(id, "error", error)
}
