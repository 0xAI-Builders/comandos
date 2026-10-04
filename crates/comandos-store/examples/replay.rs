//! Development replay against an explicitly named, already migrated database.
//! No default paths, session discovery, migrations, hooks or service control.
use comandos_store::intake::{Reception, record};
use rusqlite::{Connection, OpenFlags};
use serde_json::{Value, json};
use std::io::{self, BufRead, Read, Write};

fn replay(conn: &Connection, request: &Value) -> Result<Value, String> {
    let reception = Reception {
        now_ms: request["nowMs"].as_u64().ok_or("nowMs inválido")?,
        event_id: request["eventId"].as_str().ok_or("eventId inválido")?,
        receipt_id: request["receiptId"].as_str().ok_or("receiptId inválido")?,
        process_start: request["processStart"].as_str(),
    };
    record(conn, &request["payload"], &reception)
        .map(|v| v.unwrap_or(Value::Null))
        .map_err(|e| e.to_string())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args_os().skip(1);
    let database = args
        .next()
        .ok_or("usage: replay <explicit-existing-test-database>")?;
    if args.next().is_some() {
        return Err("usage: replay <explicit-existing-test-database>".into());
    }
    let conn = Connection::open_with_flags(database, OpenFlags::SQLITE_OPEN_READ_WRITE)?;
    conn.busy_timeout(std::time::Duration::from_millis(5000))?;
    conn.pragma_update(None, "foreign_keys", true)?;
    let mut input = io::stdin().lock();
    let mut output = io::BufWriter::new(io::stdout().lock());
    let mut line = String::new();
    loop {
        line.clear();
        if input.by_ref().take(1024 * 1024 + 1).read_line(&mut line)? == 0 {
            break;
        }
        if line.len() > 1024 * 1024 {
            return Err("input exceeds 1 MiB".into());
        }
        let result = comandos_core::json::parse_value(&line)
            .map_err(|e| e.to_string())
            .and_then(|request| replay(&conn, &request));
        serde_json::to_writer(
            &mut output,
            &result.unwrap_or_else(|error| json!({"error":error})),
        )?;
        writeln!(output)?;
        output.flush()?;
    }
    Ok(())
}
