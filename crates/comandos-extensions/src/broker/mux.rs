//! Multiplexación JSON-RPC de varios clientes MCP sobre un único upstream. Sin E/S.
use super::translate::{Parsed, bytes, cancelled, error, error_response, parse, response, slot};
use crate::serve::normalize::SUPPORTED_PROTOCOL_VERSIONS;
use serde_json::{Map, Value};
use std::collections::{BTreeMap, HashMap};

mod logging;
mod resources;

/// Identificador de un cliente conectado al broker; lo asigna [`Mux::add_client`].
pub type ClientId = u32;

/// Mensaje que el llamador debe escribir. Cada `Vec<u8>` es una línea JSON sin `\n` final.
#[derive(Debug, PartialEq, Eq)]
pub enum Outbound {
    /// Escribir al stdin del servidor MCP upstream.
    ToUpstream(Vec<u8>),
    /// Escribir al cliente indicado.
    ToClient(ClientId, Vec<u8>),
}

#[derive(Default)]
struct Client {
    initialized: bool,
    caps: Value,
}

/// Request de cliente en vuelo hacia el upstream.
struct Pending {
    client: Option<ClientId>,
    orig: Value,
    /// Token de progreso upstream asignado a esta petición, si traía uno.
    token: Option<u64>,
    kind: PendingKind,
}

enum PendingKind {
    Ordinary,
    Resource(String),
    Logging,
}

/// Original request retained until its shared upstream change is confirmed.
struct StateCall {
    client: ClientId,
    orig: Value,
    message: Map<String, Value>,
}

impl StateCall {
    fn new(client: ClientId, message: Map<String, Value>) -> Self {
        Self {
            client,
            orig: message["id"].clone(),
            message,
        }
    }

    fn reply(&self, result: &Result<Value, Value>) -> Outbound {
        Outbound::ToClient(
            self.client,
            match result {
                Ok(value) => response(self.orig.clone(), value.clone()),
                Err(value) => error_response(self.orig.clone(), value.clone()),
            },
        )
    }
}

/// Estado del multiplexor: un upstream, N clientes. No hace E/S ni conoce sockets.
#[derive(Default)]
pub struct Mux {
    next_client: ClientId,
    next_id: u64,
    clients: BTreeMap<ClientId, Client>,
    /// id upstream -> request en vuelo. Si el cliente se va, la entrada desaparece (huérfano).
    pending: HashMap<u64, Pending>,
    /// token upstream -> (cliente, token original).
    progress: HashMap<u64, (ClientId, Value)>,
    /// Clientes cuyo `initialize` espera la primera respuesta del upstream:
    /// (cliente, id original, `params.protocolVersion` pedida).
    waiting_init: Vec<(ClientId, Value, Value)>,
    init_up_id: Option<u64>,
    init_result: Option<Value>,
    /// `notifications/initialized` ya reenviada al upstream (solo se manda una).
    upstream_notified: bool,
    /// id original del upstream (serializado) -> (cliente, id entregado al cliente).
    upstream_requests: HashMap<String, (ClientId, Value)>,
    resources: BTreeMap<String, resources::Resource>,
    logging: logging::Logging,
}

impl Mux {
    pub fn new() -> Self {
        Self::default()
    }

    /// `true` cuando el upstream ya contestó su primer `initialize` (resultado cacheado).
    pub fn upstream_initialized(&self) -> bool {
        self.init_result.is_some()
    }

    /// Registra un cliente nuevo; no está inicializado hasta recibir su `InitializeResult`.
    pub fn add_client(&mut self) -> ClientId {
        self.next_client += 1;
        self.clients.insert(self.next_client, Client::default());
        self.next_client
    }

    /// Desconecta al cliente. Devuelve `notifications/cancelled` al upstream por cada request
    /// suyo en vuelo, y un error por cada request del upstream que esperaba su respuesta.
    /// Las respuestas tardías de ese cliente se descartan. Un id desconocido no hace nada.
    pub fn remove_client(&mut self, id: ClientId) -> Vec<Outbound> {
        if self.clients.remove(&id).is_none() {
            return Vec::new();
        }
        self.waiting_init.retain(|(c, ..)| *c != id);
        let mut out = self.resource_departure(id);
        out.extend(self.logging_departure(id));
        self.progress.retain(|_, (owner, _)| *owner != id);
        let mut ups: Vec<u64> = (self.pending.iter())
            .filter(|(_, p)| p.client == Some(id) && matches!(p.kind, PendingKind::Ordinary))
            .map(|(k, _)| *k)
            .collect();
        ups.sort_unstable();
        for up in ups {
            self.drop_pending(up);
            out.push(Outbound::ToUpstream(cancelled(up, "client disconnected")));
        }
        let mut keys: Vec<String> = (self.upstream_requests.iter())
            .filter(|(_, (c, _))| *c == id)
            .map(|(k, _)| k.clone())
            .collect();
        keys.sort();
        let err = error(-32000, "client disconnected");
        for k in keys {
            self.upstream_requests.remove(&k);
            if let Ok(orig) = serde_json::from_str::<Value>(&k) {
                out.push(Outbound::ToUpstream(error_response(orig, err.clone())));
            }
        }
        out
    }

    /// Procesa una línea del cliente `id`. Línea no JSON -> error -32700 a ese cliente;
    /// JSON que no es objeto (p. ej. un lote) o cliente desconocido -> se descarta.
    pub fn from_client(&mut self, id: ClientId, line: &[u8]) -> Vec<Outbound> {
        if !self.clients.contains_key(&id) {
            return Vec::new();
        }
        let msg = match parse(line) {
            Parsed::Object(m) => m,
            Parsed::NotJson => {
                let e = error(-32700, "Parse error");
                return vec![Outbound::ToClient(id, error_response(Value::Null, e))];
            }
            Parsed::NotObject => return Vec::new(),
        };
        let method = msg.get("method").and_then(Value::as_str).map(str::to_owned);
        match method.as_deref() {
            Some("initialize") => self.client_initialize(id, msg),
            Some("notifications/initialized") => {
                if std::mem::replace(&mut self.upstream_notified, true) {
                    return Vec::new();
                }
                vec![Outbound::ToUpstream(bytes(msg))]
            }
            Some("notifications/cancelled") => self.client_cancel(id, msg),
            Some("resources/subscribe") if msg.contains_key("id") => {
                self.resource_request(id, true, msg)
            }
            Some("resources/unsubscribe") if msg.contains_key("id") => {
                self.resource_request(id, false, msg)
            }
            Some("logging/setLevel") if msg.contains_key("id") => self.logging_request(id, msg),
            Some(_) if msg.contains_key("id") => self.client_request(id, msg),
            Some(_) => vec![Outbound::ToUpstream(bytes(msg))],
            None => self.client_response(id, msg),
        }
    }

    /// Procesa una línea del upstream. Las respuestas van solo a su dueño con el id original;
    /// los requests del upstream a un cliente con la capacidad; `progress` solo a su dueño;
    /// el resto de notificaciones se difunden a los clientes inicializados.
    pub fn from_upstream(&mut self, line: &[u8]) -> Vec<Outbound> {
        let Parsed::Object(mut msg) = parse(line) else {
            return Vec::new(); // el upstream roto no tiene a quién avisar
        };
        if let Some(method) = msg.get("method").and_then(Value::as_str).map(str::to_owned) {
            if msg.contains_key("id") {
                return self.upstream_request(&method, msg);
            }
            return match method.as_str() {
                "notifications/progress" => self.upstream_progress(msg),
                "notifications/cancelled" => self.upstream_cancel(msg),
                "notifications/resources/updated" => self.resource_notification(msg),
                "notifications/message" => self.logging_notification(msg),
                _ => {
                    let b = bytes(msg);
                    (self.clients.iter())
                        .filter(|(_, c)| c.initialized)
                        .map(|(id, _)| Outbound::ToClient(*id, b.clone()))
                        .collect()
                }
            };
        }
        let Some(up) = msg.get("id").and_then(Value::as_u64) else {
            return Vec::new();
        };
        if self.init_up_id == Some(up) {
            return self.init_response(&msg);
        }
        let Some(p) = self.drop_pending(up) else {
            return Vec::new(); // huérfana o desconocida
        };
        match p.kind {
            PendingKind::Resource(uri) => return self.resource_response(up, &uri, msg),
            PendingKind::Logging => return self.logging_response(up, msg),
            PendingKind::Ordinary => {}
        }
        let Some(client) = p.client else {
            return Vec::new();
        };
        msg.insert("id".into(), p.orig);
        vec![Outbound::ToClient(client, bytes(msg))]
    }

    fn client_initialize(&mut self, id: ClientId, mut msg: Map<String, Value>) -> Vec<Outbound> {
        let Some(orig) = msg.get("id").cloned() else {
            let e = error(-32600, "initialize requires an id");
            return vec![Outbound::ToClient(id, error_response(Value::Null, e))];
        };
        let caps = slot(&mut msg, &["params", "capabilities"]).map(|v| v.clone());
        let asked =
            slot(&mut msg, &["params", "protocolVersion"]).map_or(Value::Null, |v| v.clone());
        if let Some(c) = self.clients.get_mut(&id) {
            c.caps = caps.unwrap_or(Value::Null);
        }
        if let Some(result) = &self.init_result {
            let result = for_client(result, &asked);
            if let Some(c) = self.clients.get_mut(&id) {
                c.initialized = true;
            }
            return vec![Outbound::ToClient(id, response(orig, result))];
        }
        if self.waiting_init.iter().any(|(c, ..)| *c == id) {
            return Vec::new(); // initialize duplicado: ya se contestará una vez
        }
        self.waiting_init.push((id, orig, asked));
        if self.init_up_id.is_some() {
            return Vec::new();
        }
        let up = self.fresh_id();
        self.init_up_id = Some(up);
        msg.insert("id".into(), Value::from(up));
        vec![Outbound::ToUpstream(bytes(msg))]
    }

    fn init_response(&mut self, msg: &Map<String, Value>) -> Vec<Outbound> {
        self.init_up_id = None;
        let waiting = std::mem::take(&mut self.waiting_init);
        let result = msg.get("result").cloned();
        let err = msg
            .get("error")
            .cloned()
            .unwrap_or_else(|| error(-32603, "Internal error"));
        if let Some(r) = &result {
            self.init_result = Some(r.clone());
        }
        let mut out = Vec::new();
        for (c, oid, asked) in waiting {
            let Some(client) = self.clients.get_mut(&c) else {
                continue;
            };
            client.initialized = result.is_some();
            out.push(Outbound::ToClient(
                c,
                match &result {
                    Some(r) => response(oid, for_client(r, &asked)),
                    None => error_response(oid, err.clone()),
                },
            ));
        }
        out
    }

    fn client_request(&mut self, client: ClientId, msg: Map<String, Value>) -> Vec<Outbound> {
        vec![
            self.begin_request(Some(client), msg, PendingKind::Ordinary)
                .1,
        ]
    }

    fn begin_request(
        &mut self,
        client: Option<ClientId>,
        mut msg: Map<String, Value>,
        kind: PendingKind,
    ) -> (u64, Outbound) {
        let orig = msg["id"].clone();
        let up = self.fresh_id();
        let mut token = None;
        if let Some(client) = client
            && let Some(s) = slot(&mut msg, &["params", "_meta", "progressToken"])
        {
            let t = self.fresh_id();
            let o = std::mem::replace(s, Value::from(t));
            self.progress.insert(t, (client, o));
            token = Some(t);
        }
        self.pending.insert(
            up,
            Pending {
                client,
                orig,
                token,
                kind,
            },
        );
        msg.insert("id".into(), Value::from(up));
        (up, Outbound::ToUpstream(bytes(msg)))
    }

    /// Traduce `requestId` al id upstream y olvida la petición (su respuesta tardía se descarta).
    fn client_cancel(&mut self, id: ClientId, mut msg: Map<String, Value>) -> Vec<Outbound> {
        let Some(original) = msg.get("params").and_then(|p| p.get("requestId")).cloned() else {
            return Vec::new();
        };
        if let Some(out) = self.resource_cancel(id, &original) {
            return out;
        }
        if let Some(out) = self.logging_cancel(id, &original) {
            return out;
        }
        let Some(rid) = slot(&mut msg, &["params", "requestId"]) else {
            return Vec::new();
        };
        let Some(up) = (self.pending.iter())
            .find(|(_, p)| {
                p.client == Some(id) && p.orig == *rid && matches!(p.kind, PendingKind::Ordinary)
            })
            .map(|(k, _)| *k)
        else {
            return Vec::new();
        };
        *rid = Value::from(up);
        self.drop_pending(up);
        vec![Outbound::ToUpstream(bytes(msg))]
    }

    /// Respuesta de un cliente a un request del upstream: se restaura el id original.
    fn client_response(&mut self, id: ClientId, mut msg: Map<String, Value>) -> Vec<Outbound> {
        let Some(given) = msg.get("id") else {
            return Vec::new();
        };
        let Some(key) = (self.upstream_requests.iter())
            .find(|(_, (c, g))| *c == id && g == given)
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

    /// Request del upstream. `roots/list`, `sampling/createMessage` y `elicitation/create`
    /// exigen un cliente inicializado que declare la capacidad (el más reciente); si no hay,
    /// -32601 al upstream. Cualquier otro método va al cliente inicializado más reciente.
    fn upstream_request(&mut self, method: &str, mut msg: Map<String, Value>) -> Vec<Outbound> {
        let orig = msg["id"].clone();
        let key = orig.to_string();
        if self.upstream_requests.contains_key(&key) {
            let e = error(-32600, "duplicate request id");
            return vec![Outbound::ToUpstream(error_response(orig, e))];
        }
        let cap = match method {
            "roots/list" => Some("roots"),
            "sampling/createMessage" => Some("sampling"),
            "elicitation/create" => Some("elicitation"),
            _ => None,
        };
        let target = (self.clients.iter().rev())
            .filter(|(_, c)| c.initialized)
            .find(|(_, c)| cap.is_none_or(|k| c.caps.get(k).is_some()))
            .map(|(id, _)| *id);
        let Some(target) = target else {
            let e = error(-32601, "no client available for this request");
            return vec![Outbound::ToUpstream(error_response(orig, e))];
        };
        let given = Value::from(self.fresh_id());
        self.upstream_requests.insert(key, (target, given.clone()));
        msg.insert("id".into(), given);
        vec![Outbound::ToClient(target, bytes(msg))]
    }

    /// `progress` es por petición: solo al dueño del token, con su token original.
    fn upstream_progress(&mut self, mut msg: Map<String, Value>) -> Vec<Outbound> {
        let Some(tok) = slot(&mut msg, &["params", "progressToken"]) else {
            return Vec::new();
        };
        let Some((client, orig)) = tok.as_u64().and_then(|k| self.progress.get(&k).cloned()) else {
            return Vec::new();
        };
        *tok = orig;
        vec![Outbound::ToClient(client, bytes(msg))]
    }

    /// El upstream cancela un request suyo pendiente: solo al cliente al que se lo enviamos.
    fn upstream_cancel(&mut self, mut msg: Map<String, Value>) -> Vec<Outbound> {
        let Some(rid) = slot(&mut msg, &["params", "requestId"]) else {
            return Vec::new();
        };
        let Some((client, given)) = self.upstream_requests.remove(&rid.to_string()) else {
            return Vec::new();
        };
        *rid = given;
        vec![Outbound::ToClient(client, bytes(msg))]
    }

    fn fresh_id(&mut self) -> u64 {
        self.next_id += 1;
        self.next_id
    }

    /// Quita un request en vuelo junto con su token de progreso.
    fn drop_pending(&mut self, up: u64) -> Option<Pending> {
        let p = self.pending.remove(&up)?;
        if let Some(t) = p.token {
            self.progress.remove(&t);
        }
        Some(p)
    }

    fn forget_progress(&mut self, up: u64) {
        if let Some(token) = self.pending.get_mut(&up).and_then(|p| p.token.take()) {
            self.progress.remove(&token);
        }
    }
}

/// State changes require an object result, not merely receipt of a response id.
/// Error/malformed replies never establish a subscription or logging preference.
fn state_result(msg: &Map<String, Value>) -> Result<Value, Value> {
    if let Some(e) = msg.get("error") {
        return Err(if e.is_object() {
            e.clone()
        } else {
            error(-32603, "Invalid upstream state response")
        });
    }
    msg.get("result")
        .filter(|v| v.is_object())
        .cloned()
        .ok_or_else(|| error(-32603, "Invalid upstream state response"))
}

/// Versión que el upstream (que contestó `upstream`) negociaría con un cliente que pide
/// `asked`: la pedida si es conocida y no posterior a la del upstream; si no, la del upstream.
/// Las versiones `YYYY-MM-DD` se ordenan por fecha comparando cadenas.
pub fn negotiate<'a>(asked: &'a Value, upstream: &'a str) -> &'a str {
    asked
        .as_str()
        .filter(|a| SUPPORTED_PROTOCOL_VERSIONS.contains(a) && *a <= upstream)
        .unwrap_or(upstream)
}

/// Copia del `InitializeResult` cacheado con la `protocolVersion` negociada para este cliente.
/// Si el upstream no dio una versión en cadena, el resultado va tal cual.
fn for_client(cached: &Value, asked: &Value) -> Value {
    let mut result = cached.clone();
    if let Some(upstream) = cached["protocolVersion"].as_str() {
        result["protocolVersion"] = Value::from(negotiate(asked, upstream));
    }
    result
}
