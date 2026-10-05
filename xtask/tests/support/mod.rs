//! Broker MCP falso por tuberías, con el esquema real de `chrome-bg`.
#![allow(dead_code)]
use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::sync::{Arc, Mutex};
use xtask::mcp::{Client, ToolInfo};

/// `tools/list` real de `chrome-bg` (broker comandos-browser-macmini, 2026-10-05),
/// recortado a las herramientas que usa el arnés.
pub const CHROME_BG_TOOLS: &str = include_str!("../data/chrome-bg-tools.json");

pub fn real_tools() -> Vec<ToolInfo> {
    let v: Value = serde_json::from_str(CHROME_BG_TOOLS).unwrap();
    v["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| ToolInfo {
            name: t["name"].as_str().unwrap().to_string(),
            schema: t["inputSchema"].clone(),
        })
        .collect()
}

pub fn text(t: &str) -> Value {
    json!({"content": [{"type": "text", "text": t}]})
}

pub fn eval_reply(v: &Value) -> Value {
    text(&format!(
        "Script ran on page and returned:\n```json\n{v}\n```"
    ))
}

/// Lanza el broker falso: `handler(tool, args)` da el `result` de cada
/// `tools/call`, que además queda en `calls`. Antes de cada respuesta emite
/// una notificación, que el cliente debe saltar.
pub fn fake_broker<F>(calls: Arc<Mutex<Vec<Value>>>, mut handler: F) -> Client
where
    F: FnMut(&str, &Value) -> Value + Send + 'static,
{
    fake_broker_with(calls, false, move |name, args| Some(handler(name, args)))
}

/// Como `fake_broker`, pero `handler` puede devolver `None` para no responder
/// nunca (broker colgado), y con `ping` el broker manda un `ping` con id antes
/// de cada respuesta; la contestación del cliente queda en `calls` como
/// `{"response": …}`.
pub fn fake_broker_with<F>(calls: Arc<Mutex<Vec<Value>>>, ping: bool, mut handler: F) -> Client
where
    F: FnMut(&str, &Value) -> Option<Value> + Send + 'static,
{
    let (client_read, server_write) = std::io::pipe().unwrap();
    let (server_read, client_write) = std::io::pipe().unwrap();
    std::thread::spawn(move || {
        let mut out = server_write;
        let tools: Value = serde_json::from_str(CHROME_BG_TOOLS).unwrap();
        let mut pings = 0;
        for line in BufReader::new(server_read).lines() {
            let Ok(line) = line else { return };
            let msg: Value = serde_json::from_str(&line).unwrap();
            let Some(id) = msg.get("id").cloned() else {
                continue;
            };
            let Some(method) = msg.get("method").and_then(Value::as_str) else {
                calls.lock().unwrap().push(json!({"response": msg}));
                continue;
            };
            let result = match method {
                "initialize" => {
                    Some(json!({"protocolVersion": "2025-11-25", "capabilities": {"tools": {}}}))
                }
                "tools/list" => Some(tools.clone()),
                "tools/call" => {
                    calls.lock().unwrap().push(msg["params"].clone());
                    let name = msg["params"]["name"].as_str().unwrap().to_string();
                    handler(&name, &msg["params"]["arguments"])
                }
                other => panic!("método inesperado {other}"),
            };
            let Some(result) = result else { continue };
            let note = json!({"jsonrpc": "2.0", "method": "notifications/message", "params": {}});
            if ping {
                pings += 1;
                let req = json!({"jsonrpc": "2.0", "id": format!("srv-{pings}"), "method": "ping"});
                let other = json!({"jsonrpc": "2.0", "id": format!("srv-x{pings}"), "method": "sampling/createMessage", "params": {}});
                if writeln!(out, "{req}\n{other}").is_err() {
                    return;
                }
            }
            let reply = json!({"jsonrpc": "2.0", "id": id, "result": result});
            if writeln!(out, "{note}\n{reply}").is_err() {
                return;
            }
        }
    });
    Client::connect(
        Box::new(BufReader::new(client_read)),
        Box::new(client_write),
    )
    .unwrap()
}
