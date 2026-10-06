use serde_json::{Value, json};
use std::{
    env,
    io::{BufRead, Write},
    process::Command,
    thread,
    time::Duration,
};

fn main() {
    let profile_arg = env::args()
        .find(|arg| arg.starts_with("--user-data-dir="))
        .unwrap_or_else(|| "--user-data-dir=/tmp/comandos-browser-fake".to_owned());
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        let Ok(msg) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        let id = msg.get("id").cloned();
        let method = msg.get("method").and_then(Value::as_str).unwrap_or("");
        if method == "initialize" {
            if let Ok(ms) = env::var("FAKE_INIT_DELAY_MS")
                && let Ok(ms) = ms.parse::<u64>()
            {
                thread::sleep(Duration::from_millis(ms));
            }
            if env::var("FAKE_INIT_FAIL").is_ok() {
                break;
            }
            write_json(
                &mut stdout,
                json!({"jsonrpc":"2.0","id":id,"result":{"protocolVersion":"2025-11-25"}}),
            );
        } else if method == "tools/call" {
            let args = msg
                .get("params")
                .and_then(|p| p.get("arguments"))
                .and_then(Value::as_object);
            if args
                .and_then(|a| a.get("notify"))
                .and_then(Value::as_bool)
                .unwrap_or(false)
            {
                write_json(
                    &mut stdout,
                    json!({"jsonrpc":"2.0","method":"notifications/message","params":{"x":1}}),
                );
            }
            if let Some(count) = args
                .and_then(|a| a.get("notify_count"))
                .and_then(Value::as_u64)
            {
                for index in 0..count {
                    write_json(
                        &mut stdout,
                        json!({"jsonrpc":"2.0","method":"notifications/message","params":{"index":index}}),
                    );
                }
            }
            if args
                .and_then(|a| a.get("spawn_orphan"))
                .and_then(Value::as_bool)
                .unwrap_or(false)
            {
                let _ = Command::new("sleep").arg("600").arg(&profile_arg).spawn();
            }
            if let Some(ms) = args.and_then(|a| a.get("sleep_ms")).and_then(Value::as_u64) {
                thread::sleep(Duration::from_millis(ms));
            }
            let name = msg
                .get("params")
                .and_then(|p| p.get("name"))
                .and_then(Value::as_str)
                .unwrap_or("tool");
            write_json(
                &mut stdout,
                json!({"jsonrpc":"2.0","id":id,"result":{"content":[{"type":"text","text":format!("ok {name}")}]}}),
            );
        }
    }
}

fn write_json(stdout: &mut std::io::Stdout, value: Value) {
    let text = comandos_core::json::response_dumps_compact(&value).unwrap();
    let _ = writeln!(stdout, "{text}");
    let _ = stdout.flush();
}
