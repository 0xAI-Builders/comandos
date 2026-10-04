//! Servidor MCP stdio mínimo, solo para las pruebas del broker: responde a cada línea con
//! `id` con un `InitializeResult` cuyo `serverInfo.version` es su pid y que devuelve los
//! `params` recibidos en `echo`. Variables: `FAKE_MCP_PIDFILE` (añade su pid al arrancar),
//! `FAKE_MCP_DUMP_PATH` (escribe su `PATH` en ese archivo al arrancar),
//! `FAKE_MCP_DIE=1` (sale nada más arrancar), `FAKE_MCP_HANG=1` (lee pero nunca contesta),
//! `FAKE_MCP_MAX_PROTOCOL` (versión máxima, por omisión `2025-11-25`: contesta
//! `min(params.protocolVersion, máxima)` en orden de fecha, o la máxima si no pide ninguna).
//! Al arrancar escribe [`STDERR_MARKER`] en stderr (el broker debe mandarlo a `/dev/null`).
use std::io::{BufRead, Write};

const STDERR_MARKER: &str = "fake_mcp_stdio-stderr-marker";

fn main() {
    let pid = std::process::id();
    eprintln!("{STDERR_MARKER}");
    let knob = |k: &str| std::env::var_os(k).is_some_and(|v| v == "1");
    if let Some(path) = std::env::var_os("FAKE_MCP_PIDFILE") {
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path);
        if let Ok(f) = file.as_mut() {
            let _ = writeln!(f, "{pid}");
        }
    }
    if let Some(path) = std::env::var_os("FAKE_MCP_DUMP_PATH") {
        let _ = std::fs::write(
            path,
            std::env::var_os("PATH")
                .unwrap_or_default()
                .as_encoded_bytes(),
        );
    }
    if knob("FAKE_MCP_DIE") {
        std::process::exit(3);
    }
    let hang = knob("FAKE_MCP_HANG");
    let max = std::env::var("FAKE_MCP_MAX_PROTOCOL").unwrap_or_else(|_| "2025-11-25".into());
    let mut out = std::io::stdout().lock();
    for line in std::io::stdin().lock().lines().map_while(Result::ok) {
        let Ok(msg) = serde_json::from_str::<serde_json::Value>(&line) else {
            continue;
        };
        let Some(id) = msg.get("id").filter(|_| !hang) else {
            continue;
        };
        let asked = msg["params"]["protocolVersion"].as_str();
        let version = asked.map_or(max.as_str(), |a| a.min(max.as_str()));
        let result = serde_json::json!({"protocolVersion":version,"capabilities":{},"serverInfo":{"name":"eco","version":pid.to_string()},"echo":msg.get("params")});
        let reply = serde_json::json!({"jsonrpc":"2.0","id":id,"result":result});
        if writeln!(out, "{reply}").and_then(|()| out.flush()).is_err() {
            break;
        }
    }
}
