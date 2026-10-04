//! Servidor MCP stdio mínimo, solo para las pruebas del broker: responde a cada línea
//! con `id` con un `InitializeResult` cuyo `serverInfo.version` es su propio pid.
//! Si existe `FAKE_MCP_PIDFILE`, añade su pid a ese archivo al arrancar.
use std::io::{BufRead, Write};

fn main() {
    let pid = std::process::id();
    if let Some(path) = std::env::var_os("FAKE_MCP_PIDFILE") {
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path);
        if let Ok(f) = file.as_mut() {
            let _ = writeln!(f, "{pid}");
        }
    }
    let mut out = std::io::stdout().lock();
    for line in std::io::stdin().lock().lines().map_while(Result::ok) {
        let Ok(msg) = serde_json::from_str::<serde_json::Value>(&line) else {
            continue;
        };
        let Some(id) = msg.get("id") else { continue };
        let result = serde_json::json!({"protocolVersion":"2025-06-18","capabilities":{},"serverInfo":{"name":"eco","version":pid.to_string()}});
        let reply = serde_json::json!({"jsonrpc":"2.0","id":id,"result":result});
        if writeln!(out, "{reply}").and_then(|()| out.flush()).is_err() {
            break;
        }
    }
}
