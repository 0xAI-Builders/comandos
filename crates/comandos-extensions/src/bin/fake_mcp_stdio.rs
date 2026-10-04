//! Servidor MCP stdio mínimo, solo para las pruebas del broker: responde a cada línea con
//! `id` con un `InitializeResult` cuyo `serverInfo.version` es su pid y que devuelve los
//! `params` recibidos en `echo`. Variables: `FAKE_MCP_PIDFILE` (añade su pid al arrancar),
//! `FAKE_MCP_DUMP_PATH` (escribe su `PATH` en ese archivo al arrancar),
//! `FAKE_MCP_DIE=1` (sale nada más arrancar), `FAKE_MCP_HANG=1` (lee pero nunca contesta),
//! `FAKE_MCP_MAX_PROTOCOL` (versión máxima, por omisión `2025-11-25`: contesta
//! `min(params.protocolVersion, máxima)` en orden de fecha, o la máxima si no pide ninguna).
//! `FAKE_MCP_DUMP_ENV` (escribe su entorno, `CLAVE=valor` por línea, en ese archivo al arrancar),
//! `FAKE_MCP_HUGE=<bytes>`: cada `tools/call` se contesta con un texto de ese tamaño, con el
//! `id` al final del objeto (como el SDK de TypeScript: `{"result":…,"jsonrpc","id"}`).
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
    if let Some(path) = std::env::var_os("FAKE_MCP_DUMP_ENV") {
        let text: String = std::env::vars_os()
            .map(|(k, v)| format!("{}={}\n", k.to_string_lossy(), v.to_string_lossy()))
            .collect();
        let _ = std::fs::write(path, text);
    }
    if knob("FAKE_MCP_DIE") {
        std::process::exit(3);
    }
    let hang = knob("FAKE_MCP_HANG");
    let huge: Option<usize> = std::env::var("FAKE_MCP_HUGE")
        .ok()
        .and_then(|v| v.parse().ok());
    let max = std::env::var("FAKE_MCP_MAX_PROTOCOL").unwrap_or_else(|_| "2025-11-25".into());
    let mut out = std::io::stdout().lock();
    for line in std::io::stdin().lock().lines().map_while(Result::ok) {
        let Ok(msg) = serde_json::from_str::<serde_json::Value>(&line) else {
            continue;
        };
        let Some(id) = msg.get("id").filter(|_| !hang) else {
            continue;
        };
        if let Some(size) = huge.filter(|_| msg["method"] == "tools/call") {
            if write_huge(&mut out, id, size).is_err() {
                break;
            }
            continue;
        }
        let asked = msg["params"]["protocolVersion"].as_str();
        let version = asked.map_or(max.as_str(), |a| a.min(max.as_str()));
        let result = serde_json::json!({"protocolVersion":version,"capabilities":{},"serverInfo":{"name":"eco","version":pid.to_string()},"echo":msg.get("params")});
        let reply = serde_json::json!({"jsonrpc":"2.0","id":id,"result":result});
        if writeln!(out, "{reply}").and_then(|()| out.flush()).is_err() {
            break;
        }
    }
}

/// Respuesta a `tools/call` con un texto de `size` bytes, escrita por trozos.
fn write_huge(out: &mut impl Write, id: &serde_json::Value, size: usize) -> std::io::Result<()> {
    out.write_all(br#"{"result":{"content":[{"type":"text","text":""#)?;
    let chunk = vec![b'x'; 1024 * 1024];
    let mut left = size;
    while left > 0 {
        let n = left.min(chunk.len());
        out.write_all(&chunk[..n])?;
        left -= n;
    }
    write!(out, "\"}}]}},\"jsonrpc\":\"2.0\",\"id\":{id}}}\n")?;
    out.flush()
}
