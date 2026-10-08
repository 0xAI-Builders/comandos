use comandos_store::domains::caller::CallerAccess;
use serde_json::{Value, json};
use std::{
    collections::BTreeSet,
    env,
    io::{Read, Write},
    net::TcpStream,
    path::PathBuf,
    time::Duration,
};

const INVENTORY: &str = include_str!("../../../xtask/web/inventory.json");
const SELECTION_DOC: &str = "hooks/comandos-web.json";

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
    if !matches!(mode, "on" | "shadow" | "off") {
        return Err((2, usage()));
    }
    let (access, path) = selection_access()?;
    let mut lock = path.as_os_str().to_owned();
    lock.push(".lock");
    access
        .update_document(
            SELECTION_DOC,
            "ui-docs",
            &path,
            &PathBuf::from(lock),
            chrono::Utc::now().timestamp_millis(),
            |old| {
                let mut on = BTreeSet::new();
                let mut shadow = BTreeSet::new();
                if let Some(value) = old
                    .and_then(|bytes| std::str::from_utf8(bytes).ok())
                    .and_then(|text| serde_json::from_str::<Value>(text).ok())
                {
                    on = strings(&value, "on");
                    shadow = strings(&value, "shadow");
                }
                on.remove(id);
                shadow.remove(id);
                if mode == "on" {
                    on.insert(id.to_string());
                } else if mode == "shadow" {
                    shadow.insert(id.to_string());
                }
                let value = json!({
                    "on": on.into_iter().collect::<Vec<_>>(),
                    "shadow": shadow.into_iter().collect::<Vec<_>>(),
                });
                serde_json::to_vec(&value).map(Some).map_err(|e| {
                    comandos_store::Error::Validation(format!(
                        "comandos web: no se pudo serializar selección: {e}"
                    ))
                })
            },
            |body| {
                let text = std::str::from_utf8(body)
                    .map_err(|e| comandos_store::Error::Validation(e.to_string()))?;
                comandos_server::dash::native::files::write_text_atomic(&path, text)
                    .map_err(comandos_store::Error::Io)
            },
        )
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
            let (home, path) = selection_location()?;
            let text = comandos_store::domains::DomainStore { home: &home }
                .document(SELECTION_DOC, "ui-docs", path.clone())
                .read_readonly()
                .map_err(|e| (1, format!("{}: {e}", path.display())))?
                .and_then(|bytes| String::from_utf8(bytes).ok())
                .unwrap_or_else(|| "{\"on\":[],\"shadow\":[]}".into());
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

fn selection_access() -> Result<(CallerAccess, PathBuf), (i32, String)> {
    let (home, path) = selection_location()?;
    let access =
        CallerAccess::open(&home, "ui-docs").map_err(|e| (1, format!("comandos web: {e}")))?;
    Ok((access, path))
}

fn selection_location() -> Result<(PathBuf, PathBuf), (i32, String)> {
    let home = env::var_os("HOME")
        .filter(|h| !h.is_empty())
        .map(PathBuf::from)
        .ok_or_else(|| (2, "HOME no está definido".to_string()))?;
    let path = home.join(".claude/hooks/comandos-web.json");
    Ok((home, path))
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
