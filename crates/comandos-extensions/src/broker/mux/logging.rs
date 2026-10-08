//! The shared upstream level is the minimum of explicit, ACKed client levels.
//! The eight MCP severities are ordered from debug to emergency in every supported
//! protocol version. Changes are serialized, and errors retain confirmed state.
//! Clients without setLevel receive all messages actually emitted by upstream;
//! MCP has no reset-to-default method, so no remote default is invented.
//! Departure maintenance has no downstream owner and never loops on an error.
//! Cancelling a dispatched change discards its local waiter, then reconciles the
//! eventual ACK to remaining preferences. Ordinary request cancellation is unchanged.
use super::super::translate::{bytes, error, error_response};
use super::{ClientId, Mux, Outbound, PendingKind, StateCall, state_result};
use serde_json::{Map, Value, json};
use std::collections::{BTreeMap, VecDeque};

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Level {
    Debug,
    Info,
    Notice,
    Warning,
    Error,
    Critical,
    Alert,
    Emergency,
}

impl Level {
    fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "debug" => Self::Debug,
            "info" => Self::Info,
            "notice" => Self::Notice,
            "warning" => Self::Warning,
            "error" => Self::Error,
            "critical" => Self::Critical,
            "alert" => Self::Alert,
            "emergency" => Self::Emergency,
            _ => return None,
        })
    }

    fn name(self) -> &'static str {
        match self {
            Self::Debug => "debug",
            Self::Info => "info",
            Self::Notice => "notice",
            Self::Warning => "warning",
            Self::Error => "error",
            Self::Critical => "critical",
            Self::Alert => "alert",
            Self::Emergency => "emergency",
        }
    }
}

struct Call {
    request: StateCall,
    level: Level,
}
struct Flight {
    up: u64,
    level: Level,
    call: Option<Call>,
}

#[derive(Default)]
pub(super) struct Logging {
    levels: BTreeMap<ClientId, Level>,
    current: Option<Level>,
    flight: Option<Flight>,
    queue: VecDeque<Call>,
    dirty: bool,
}

impl Mux {
    pub(super) fn logging_request(
        &mut self,
        client: ClientId,
        msg: Map<String, Value>,
    ) -> Vec<Outbound> {
        let level = msg
            .get("params")
            .and_then(|p| p.get("level"))
            .and_then(Value::as_str)
            .and_then(Level::parse);
        let Some(level) = level else {
            return vec![Outbound::ToClient(
                client,
                error_response(msg["id"].clone(), error(-32602, "Invalid log level")),
            )];
        };
        let mut logging = std::mem::take(&mut self.logging);
        logging.queue.push_back(Call {
            request: StateCall::new(client, msg),
            level,
        });
        let out = self.drain_logging(&mut logging);
        self.logging = logging;
        out
    }

    fn drain_logging(&mut self, logging: &mut Logging) -> Vec<Outbound> {
        let mut out = Vec::new();
        while logging.flight.is_none() {
            let Some(mut call) = logging.queue.pop_front() else {
                break;
            };
            if !self.clients.contains_key(&call.request.client) {
                continue;
            }
            let level = logging
                .levels
                .iter()
                .filter(|(c, _)| **c != call.request.client)
                .map(|(_, level)| *level)
                .chain(std::iter::once(call.level))
                .min()
                .unwrap_or(call.level);
            if logging.current == Some(level) {
                logging.levels.insert(call.request.client, call.level);
                logging.dirty = true;
                out.push(call.request.reply(&Ok(json!({}))));
            } else {
                call.request.message["params"]["level"] = json!(level.name());
                let (up, line) = self.begin_request(
                    Some(call.request.client),
                    call.request.message.clone(),
                    PendingKind::Logging,
                );
                logging.flight = Some(Flight {
                    up,
                    level,
                    call: Some(call),
                });
                out.push(line);
            }
        }
        if logging.flight.is_none()
            && std::mem::take(&mut logging.dirty)
            && let Some(level) = logging.levels.values().copied().min()
            && logging.current != Some(level)
        {
            let msg = json!({"jsonrpc":"2.0","id":null,"method":"logging/setLevel","params":{"level":level.name()}});
            let (up, line) = self.begin_request(
                None,
                msg.as_object().cloned().unwrap_or_default(),
                PendingKind::Logging,
            );
            logging.flight = Some(Flight {
                up,
                level,
                call: None,
            });
            out.push(line);
        }
        out
    }

    pub(super) fn logging_response(&mut self, up: u64, msg: Map<String, Value>) -> Vec<Outbound> {
        let mut logging = std::mem::take(&mut self.logging);
        let Some(flight) = logging.flight.take() else {
            self.logging = logging;
            return Vec::new();
        };
        debug_assert_eq!(flight.up, up);
        let result = state_result(&msg);
        if result.is_ok() {
            logging.current = Some(flight.level);
            logging.dirty = true;
        }
        let mut out = Vec::new();
        if let Some(call) = flight.call
            && self.clients.contains_key(&call.request.client)
        {
            if result.is_ok() {
                logging.levels.insert(call.request.client, call.level);
            }
            out.push(call.request.reply(&result));
        }
        out.extend(self.drain_logging(&mut logging));
        self.logging = logging;
        out
    }

    pub(super) fn logging_notification(&self, msg: Map<String, Value>) -> Vec<Outbound> {
        let Some(level) = msg
            .get("params")
            .and_then(|p| p.get("level"))
            .and_then(Value::as_str)
            .and_then(Level::parse)
        else {
            return Vec::new();
        };
        let line = bytes(msg);
        self.clients
            .iter()
            .filter(|(id, c)| {
                c.initialized
                    && self
                        .logging
                        .levels
                        .get(*id)
                        .is_none_or(|minimum| level >= *minimum)
            })
            .map(|(id, _)| Outbound::ToClient(*id, line.clone()))
            .collect()
    }

    pub(super) fn logging_departure(&mut self, client: ClientId) -> Vec<Outbound> {
        let mut logging = std::mem::take(&mut self.logging);
        logging.dirty |= logging.levels.remove(&client).is_some();
        logging.queue.retain(|call| call.request.client != client);
        let mut out = Vec::new();
        if let Some(flight) = logging.flight.as_mut()
            && flight
                .call
                .as_ref()
                .is_some_and(|c| c.request.client == client)
        {
            flight.call = None;
            logging.dirty = true;
        }
        out.extend(self.drain_logging(&mut logging));
        self.logging = logging;
        out
    }

    pub(super) fn logging_cancel(
        &mut self,
        client: ClientId,
        original: &Value,
    ) -> Option<Vec<Outbound>> {
        if let Some(index) = self
            .logging
            .queue
            .iter()
            .position(|c| c.request.client == client && c.request.orig == *original)
        {
            self.logging.queue.remove(index);
            return Some(Vec::new());
        }
        let flight = self.logging.flight.as_mut()?;
        if !flight
            .call
            .as_ref()
            .is_some_and(|c| c.request.client == client && c.request.orig == *original)
        {
            return None;
        }
        flight.call = None;
        let up = flight.up;
        self.logging.dirty = true;
        self.forget_progress(up);
        Some(Vec::new())
    }
}
