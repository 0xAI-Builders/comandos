use serde_json::{Value, json};
use std::{
    collections::BTreeSet,
    env, fs, io::{Read, Write}, net::TcpStream, path::PathBuf, time::Duration,
};

const INVENTORY: &str = include_str!("../../../xtask/web/inventory.json");

pub fn main(args: &[String]) -> i32 {
    match run(args) {
        Ok(()) => 0,
        Err((code, message)) => {
            eprintln!("{message}");
            code
        }
    }
}

fn run(args: &[String]) -> Result<(), (i32, String)> {
    match args {
        [cmd, id, mode] if cmd == "set" => set(id, mode),
        [cmd] if cmd == "status" => status(),
        _ => Err((2, usage())),
    }
}

fn set(id: &str, mode: &str) -> Result<(), (i32, String)> {
    if !known_ids().contains(id) {
        return Err((2, format!("comandos web: componente desconocido: {id}")));
    }
    let path = selection_path()?;
    let mut on = BTreeSet::new();
    let mut shadow = BTreeSet::new();
    if let Ok(text) = fs::read_to_string(&path)
        && let Ok(value) = serde_json::from_str::<Value>(&text)
    {
        on = strings(&value, "on");
        shadow = strings(&value, "shadow");
    }
    on.remove(id);
    shadow.remove(id);
    match mode {
        "on" => {
            on.insert(id.to_string());
        }
        "shadow" => {
            shadow.insert(id.to_string());
        }
        "off" => {}
        _ => return Err((2, usage())),
    }
    let value = json!({
        "on": on.into_iter().collect::<Vec<_>>(),
        "shadow": shadow.into_iter().collect::<Vec<_>>(),
    });
    let text = serde_json::to_string(&value)
        .map_err(|e| (1, format!("comandos web: no se pudo serializar selección: {e}")))?;
    comandos_server::dash::native::files::write_text_atomic(&path, &text)
        .map_err(|e| (1, format!("{}: {e}", path.display())))?;
    Ok(())
}

fn status() -> Result<(), (i32, String)> {
    match get_status() {
        Some(text) => {
            println!("{text}");
            Ok(())
        }
        None => {
            let path = selection_path()?;
            let text = fs::read_to_string(&path).unwrap_or_else(|_| "{\"on\":[],\"shadow\":[]}".into());
            println!("{text}");
            Ok(())
        }
    }
}

fn get_status() -> Option<String> {
    let mut stream = TcpStream::connect("127.0.0.1:4777").ok()?;
    let _ = stream.set_read_timeout(Some(Duration::from_millis(500)));
    stream
        .write_all(b"GET /web/status HTTP/1.1\r\nHost: 127.0.0.1:4777\r\nConnection: close\r\n\r\n")
        .ok()?;
    let mut raw = String::new();
    stream.read_to_string(&mut raw).ok()?;
    let (head, body) = raw.split_once("\r\n\r\n")?;
    if !head.starts_with("HTTP/1.1 200 ") && !head.starts_with("HTTP/1.0 200 ") {
        return None;
    }
    serde_json::from_str::<Value>(body).ok()?;
    Some(body.to_string())
}

fn selection_path() -> Result<PathBuf, (i32, String)> {
    let home = env::var_os("HOME")
        .filter(|h| !h.is_empty())
        .map(PathBuf::from)
        .ok_or_else(|| (2, "HOME no está definido".to_string()))?;
    Ok(home.join(".claude/hooks/comandos-web.json"))
}

fn known_ids() -> BTreeSet<String> {
    let mut ids = BTreeSet::new();
    if let Ok(value) = serde_json::from_str::<Value>(INVENTORY)
        && let Some(units) = value.get("units").and_then(Value::as_array)
    {
        for id in units
            .iter()
            .filter_map(|u| u.get("component").and_then(Value::as_str))
        {
            ids.insert(id.to_string());
        }
    }
    ids.insert("quick-terminal".into());
    ids
}

fn strings(value: &Value, key: &str) -> BTreeSet<String> {
    value
        .get(key)
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_string)
        .collect()
}

fn usage() -> String {
    "uso: comandos web set <id> on|off|shadow | comandos web status".into()
}
