//! MCP resources/subscribe is a per-connection set, so the shared connection uses
//! one upstream subscription per literal URI and confirmed downstream memberships.
//! Concurrent subscribes share the ACK/error; changes behind an unsubscribe wait.
//! A failed last unsubscribe retains the owner's membership. Failed orphan cleanup
//! is not retried in a loop and cannot grant another client cached membership.
//! Cancellation removes only the local waiter. Dispatched connection changes finish
//! normally, so server-side cancellation without a reply cannot stall another client;
//! an abandoned success is cleaned up or reversed using a broker-owned request.
use super::super::translate::bytes;
use super::{ClientId, Mux, Outbound, PendingKind, StateCall, state_result};
use serde_json::{Map, Value, json};
use std::collections::{BTreeSet, VecDeque};

#[derive(Default)]
pub(super) struct Resource {
    members: BTreeSet<ClientId>,
    active: bool,
    result: Option<Value>,
    flight: Option<Flight>,
    queue: VecDeque<Call>,
}

struct Call {
    subscribe: bool,
    request: StateCall,
}

struct Flight {
    up: u64,
    subscribe: bool,
    calls: Vec<Call>,
    abandoned: bool,
}

impl Mux {
    pub(super) fn resource_request(
        &mut self,
        client: ClientId,
        subscribe: bool,
        msg: Map<String, Value>,
    ) -> Vec<Outbound> {
        let Some(uri) = msg
            .get("params")
            .and_then(|p| p.get("uri"))
            .and_then(Value::as_str)
            .map(str::to_owned)
        else {
            return self.client_request(client, msg);
        };
        let mut resource = self.resources.remove(&uri).unwrap_or_default();
        let call = Call {
            subscribe,
            request: StateCall::new(client, msg),
        };
        let out = if let Some(flight) = resource.flight.as_mut() {
            if subscribe && flight.subscribe && !flight.abandoned && resource.queue.is_empty() {
                flight.calls.push(call);
            } else {
                resource.queue.push_back(call);
            }
            Vec::new()
        } else {
            self.start_resource_call(&uri, &mut resource, call)
        };
        self.keep_resource(uri, resource);
        out
    }

    fn start_resource_call(
        &mut self,
        uri: &str,
        resource: &mut Resource,
        call: Call,
    ) -> Vec<Outbound> {
        let client = call.request.client;
        if call.subscribe && resource.active && !resource.members.is_empty() {
            resource.members.insert(client);
            return vec![
                call.request
                    .reply(&Ok(resource.result.clone().unwrap_or_else(|| json!({})))),
            ];
        }
        if !call.subscribe
            && !resource.members.is_empty()
            && (!resource.members.contains(&client) || resource.members.len() > 1)
        {
            resource.members.remove(&client);
            return vec![call.request.reply(&Ok(json!({})))];
        }
        let (up, out) = self.begin_request(
            Some(client),
            call.request.message.clone(),
            PendingKind::Resource(uri.to_owned()),
        );
        resource.flight = Some(Flight {
            up,
            subscribe: call.subscribe,
            calls: vec![call],
            abandoned: false,
        });
        vec![out]
    }

    fn internal_resource_change(
        &mut self,
        uri: &str,
        resource: &mut Resource,
        subscribe: bool,
    ) -> Outbound {
        let method = if subscribe {
            "resources/subscribe"
        } else {
            "resources/unsubscribe"
        };
        let msg = json!({"jsonrpc":"2.0","id":null,"method":method,"params":{"uri":uri}});
        let (up, out) = self.begin_request(
            None,
            msg.as_object().cloned().unwrap_or_default(),
            PendingKind::Resource(uri.to_owned()),
        );
        resource.flight = Some(Flight {
            up,
            subscribe,
            calls: Vec::new(),
            abandoned: false,
        });
        out
    }

    pub(super) fn resource_response(
        &mut self,
        up: u64,
        uri: &str,
        msg: Map<String, Value>,
    ) -> Vec<Outbound> {
        let Some(mut resource) = self.resources.remove(uri) else {
            return Vec::new();
        };
        let Some(flight) = resource.flight.take() else {
            return Vec::new();
        };
        debug_assert_eq!(flight.up, up);
        let result = state_result(&msg);
        let orphan_failure = result.is_err() && flight.calls.is_empty();
        if let Ok(value) = &result {
            resource.active = flight.subscribe;
            resource.result = flight.subscribe.then(|| value.clone());
            for call in &flight.calls {
                if flight.subscribe && self.clients.contains_key(&call.request.client) {
                    resource.members.insert(call.request.client);
                } else if !flight.subscribe {
                    resource.members.remove(&call.request.client);
                }
            }
        }
        let mut out: Vec<_> = flight
            .calls
            .iter()
            .filter(|call| self.clients.contains_key(&call.request.client))
            .map(|call| call.request.reply(&result))
            .collect();
        while resource.flight.is_none() {
            let Some(call) = resource.queue.pop_front() else {
                break;
            };
            if self.clients.contains_key(&call.request.client) {
                out.extend(self.start_resource_call(uri, &mut resource, call));
            }
        }
        if resource.flight.is_none() && !orphan_failure {
            if resource.active && resource.members.is_empty() {
                out.push(self.internal_resource_change(uri, &mut resource, false));
            } else if !resource.active && !resource.members.is_empty() {
                // A cancelled unsubscribe may nevertheless complete at the server.
                out.push(self.internal_resource_change(uri, &mut resource, true));
            }
        }
        self.keep_resource(uri.to_owned(), resource);
        out
    }

    pub(super) fn resource_notification(&self, msg: Map<String, Value>) -> Vec<Outbound> {
        let Some(uri) = msg
            .get("params")
            .and_then(|p| p.get("uri"))
            .and_then(Value::as_str)
        else {
            return Vec::new();
        };
        let Some(resource) = self.resources.get(uri).filter(|r| r.active) else {
            return Vec::new();
        };
        let line = bytes(msg);
        resource
            .members
            .iter()
            .filter(|id| self.clients.get(*id).is_some_and(|c| c.initialized))
            .map(|id| Outbound::ToClient(*id, line.clone()))
            .collect()
    }

    pub(super) fn resource_departure(&mut self, client: ClientId) -> Vec<Outbound> {
        let uris: Vec<_> = self.resources.keys().cloned().collect();
        let mut out = Vec::new();
        for uri in uris {
            let Some(mut resource) = self.resources.remove(&uri) else {
                continue;
            };
            let removed = resource.members.remove(&client);
            resource.queue.retain(|call| call.request.client != client);
            if let Some(flight) = resource.flight.as_mut() {
                let count = flight.calls.len();
                flight.calls.retain(|call| call.request.client != client);
                if count > 0 && flight.calls.is_empty() {
                    flight.abandoned = true;
                }
            } else if removed && resource.members.is_empty() && resource.active {
                out.push(self.internal_resource_change(&uri, &mut resource, false));
            }
            self.keep_resource(uri, resource);
        }
        out
    }

    pub(super) fn resource_cancel(
        &mut self,
        client: ClientId,
        original: &Value,
    ) -> Option<Vec<Outbound>> {
        let uri = self
            .resources
            .iter()
            .find(|(_, r)| {
                r.queue
                    .iter()
                    .any(|c| c.request.client == client && c.request.orig == *original)
                    || r.flight.as_ref().is_some_and(|f| {
                        f.calls
                            .iter()
                            .any(|c| c.request.client == client && c.request.orig == *original)
                    })
            })
            .map(|(uri, _)| uri.clone())?;
        let mut resource = self.resources.remove(&uri)?;
        if let Some(index) = resource
            .queue
            .iter()
            .position(|c| c.request.client == client && c.request.orig == *original)
        {
            resource.queue.remove(index);
        } else if let Some(flight) = resource.flight.as_mut() {
            flight
                .calls
                .retain(|c| !(c.request.client == client && c.request.orig == *original));
            if self
                .pending
                .get(&flight.up)
                .is_some_and(|p| p.client == Some(client) && p.orig == *original)
            {
                self.forget_progress(flight.up);
            }
            if flight.calls.is_empty() {
                flight.abandoned = true;
            }
        }
        // Keep the upstream response ticket: a late success can still require cleanup.
        self.keep_resource(uri, resource);
        Some(Vec::new())
    }

    fn keep_resource(&mut self, uri: String, resource: Resource) {
        if resource.active
            || !resource.members.is_empty()
            || resource.flight.is_some()
            || !resource.queue.is_empty()
        {
            self.resources.insert(uri, resource);
        }
    }
}
