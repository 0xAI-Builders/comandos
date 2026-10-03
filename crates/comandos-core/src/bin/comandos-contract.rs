//! Bounded JSONL adapter for migration contract checks. It performs no state I/O.
use comandos_core::{event, hook, turn};
use serde_json::{Value, json};
use std::io::{self, BufRead, Read, Write};

fn dispatch(request: &Value) -> Result<Value, String> {
    match request["op"].as_str() {
        Some("turn") => {
            let current = &request["current"];
            if !current.is_null() && !current.is_object() {
                return Err("current debe ser objeto o null".into());
            }
            let changed = turn::reduce_turn(current.as_object(), &request["event"]);
            Ok(
                json!({"changed":changed.is_some(),"state":changed.map(Value::Object).unwrap_or_else(|| current.clone())}),
            )
        }
        Some("fold") => {
            let events = request["events"]
                .as_array()
                .ok_or("events debe ser un arreglo")?;
            Ok(Value::Object(turn::turns_from_events(events)))
        }
        Some("event") => {
            let now = request["nowMs"].as_u64().ok_or("nowMs inválido")?;
            let id = request["newEventId"]
                .as_str()
                .ok_or("newEventId inválido")?;
            event::normalize(&request["event"], now, id).map(Value::Object)
        }
        Some("dedupe") => Ok(event::dedupe_key(&request["event"]).into()),
        Some("destination") => Ok(event::destination(&request["event"]).into()),
        Some("hook") => Ok(hook::normalize_hook(
            &request["payload"],
            request["processStart"].as_str(),
        )
        .map(Value::Object)
        .unwrap_or(Value::Null)),
        Some("resolve") => {
            Ok(hook::resolve_pane_key(&request["document"], &request["event"]).into())
        }
        _ => Err("operación desconocida".into()),
    }
}

fn main() -> io::Result<()> {
    const MAX_LINE: u64 = 1024 * 1024;
    let mut input = io::stdin().lock();
    let mut output = io::BufWriter::new(io::stdout().lock());
    let mut line = String::new();
    loop {
        line.clear();
        if input.by_ref().take(MAX_LINE + 1).read_line(&mut line)? == 0 {
            break;
        }
        if line.len() as u64 > MAX_LINE {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "entrada JSONL mayor a 1 MiB",
            ));
        }
        let result = comandos_core::json::parse_value(&line)
            .map_err(|e| e.to_string())
            .and_then(|request| dispatch(&request));
        let result = result.unwrap_or_else(|error| json!({"error":error}));
        serde_json::to_writer(&mut output, &result)?;
        writeln!(output)?;
        output.flush()?;
    }
    Ok(())
}
