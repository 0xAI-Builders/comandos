//! Direct, read-only upstream checks. No facade filters or tokenizer are involved.
use crate::{
    Result, config,
    transport::{Error, Transport},
};
use comandos_core::json::truthy;
use serde_json::{Value, json};
use std::{path::Path, time::Duration};
use tokio::{
    io::AsyncWriteExt,
    sync::{mpsc, watch},
    task::JoinSet,
};

const DEADLINE: Duration = Duration::from_secs(40);
const BROWSER: &[&str] = &[
    "chrome-bg",
    "claude-in-chrome",
    "playwright",
    "x-playwright",
    "lightpanda",
    "obscura",
    "screenwright",
    "teams",
];
fn mailbox(name: &str) -> Option<&'static str> {
    match name {
        "gmail" => Some("jesusbatallar@gmail.com"),
        "gmail-signara" => Some("jesus@signara.ai"),
        "qcdr-mail" => Some("jesus@qcdr.io"),
        "proton-mail" => Some("pdlgmcn@protonmail.com"),
        _ => None,
    }
}
fn failure(name: &str, error: Error) -> Value {
    let mut result = json!({"name":name,"status":"failed","errors":[error.category]});
    if let Some(status) = error.http_status {
        result["http_status"] = json!([status]);
    }
    result
}
fn rpc_result(response: Value) -> std::result::Result<Value, Error> {
    if response.get("error").is_some() {
        return Err(Error::category("McpError"));
    }
    response
        .get("result")
        .filter(|v| v.is_object())
        .cloned()
        .ok_or_else(|| Error::category("ValidationError"))
}
async fn request(
    transport: &Transport,
    method: &str,
    params: Value,
) -> std::result::Result<Value, Error> {
    rpc_result(
        transport
            .request(json!({"jsonrpc":"2.0","method":method,"params":params}))
            .await?,
    )
}
fn profile_json(text: &str) -> std::result::Result<Value, Error> {
    if let Ok(value) = comandos_core::json::parse_value(text) {
        return Ok(value);
    }
    // json.loads accepts NaN/Infinity and duplicate keys. Identity checks only
    // inspect string values and truthiness; all three constants are truthy and
    // cannot match an email string. Keep quoted strings unchanged.
    let tokens = regex::Regex::new(r#""(?:\\.|[^"\\])*"|-?Infinity|NaN"#)
        .map_err(|_| Error::category("JSONDecodeError"))?;
    let normalized = tokens.replace_all(text, |caps: &regex::Captures<'_>| {
        if caps[0].starts_with('"') {
            caps[0].to_owned()
        } else {
            "true".into()
        }
    });
    comandos_core::json::parse_value(&normalized).map_err(|_| Error::category("JSONDecodeError"))
}
async fn connected(transport: &Transport, name: &str) -> std::result::Result<Value, Error> {
    let initial = request(transport, "initialize", json!({"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"comandos","version":"1"}})).await?;
    if !crate::check_protocol::initialize(&initial) {
        return Err(Error::category("ValidationError"));
    }
    let version = initial["protocolVersion"].as_str().unwrap_or("");
    if !matches!(
        version,
        "2024-11-05" | "2025-03-26" | "2025-06-18" | "2025-11-25"
    ) {
        return Err(Error::category("RuntimeError"));
    }
    transport.set_protocol(version);
    transport
        .notify(json!({"jsonrpc":"2.0","method":"notifications/initialized"}))
        .await?;
    let mut count = 0usize;
    if initial["capabilities"]
        .get("tools")
        .is_some_and(|v| !v.is_null())
    {
        let mut cursor = None;
        for _ in 0..30 {
            let params = cursor
                .as_ref()
                .map(|v| json!({"cursor":v}))
                .unwrap_or(json!({}));
            let page = request(transport, "tools/list", params).await?;
            if !crate::check_protocol::tools(&page) {
                return Err(Error::category("ValidationError"));
            }
            let tools = page["tools"].as_array().unwrap();
            count += tools.len();
            cursor = match page.get("nextCursor") {
                None | Some(Value::Null) => None,
                Some(Value::String(s)) if s.is_empty() => None,
                Some(Value::String(s)) => Some(s.clone()),
                _ => return Err(Error::category("ValidationError")),
            };
            if cursor.is_none() {
                break;
            }
        }
    }
    let expected = mailbox(name);
    let tool = match name {
        "google-drive" => Some("list_recent_files"),
        "google-calendar" => Some("list-calendars"),
        _ if expected.is_some() => Some("get_profile"),
        _ => None,
    };
    if let Some(tool) = tool {
        let arguments = if name == "google-drive" {
            json!({"pageSize":1})
        } else {
            json!({})
        };
        let result = request(
            transport,
            "tools/call",
            json!({"name":tool,"arguments":arguments}),
        )
        .await?;
        let is_error = crate::check_protocol::call(&result)
            .ok_or_else(|| Error::category("ValidationError"))?;
        let content = result["content"].as_array().unwrap();
        if is_error {
            return Ok(json!({"name":name,"status":"failed","phase":"read_access","tools":count}));
        }
        if let Some(expected) = expected {
            let text = content
                .iter()
                .find(|c| c["type"] == "text")
                .ok_or_else(|| Error::category("StopIteration"))?["text"]
                .as_str()
                .ok_or_else(|| Error::category("ValidationError"))?;
            let profile = profile_json(text)?;
            if !profile.is_object() {
                return Err(Error::category("AttributeError"));
            }
            let address = profile
                .get("emailAddress")
                .filter(|v| truthy(v))
                .or_else(|| profile.get("email"));
            if address.and_then(Value::as_str) != Some(expected) {
                return Ok(
                    json!({"name":name,"status":"failed","phase":"mailbox_identity","tools":count}),
                );
            }
        }
    }
    Ok(json!({"name":name,"status":"connected","tools":count}))
}
async fn probe(
    home: &Path,
    name: &str,
    spec: &Value,
    duration: Duration,
    mut cancelled: watch::Receiver<bool>,
) -> Value {
    if !spec.get("enabled").is_none_or(truthy) {
        return json!({"name":name,"status":"disabled"});
    }
    if BROWSER.contains(&name) {
        return json!({"name":name,"status":"not_probed","reason":"interactive or browser runtime"});
    }
    let mut transport = None;
    let result = tokio::select! {
        result=tokio::time::timeout(duration, async {
            transport = Some(Transport::connect_until(home,name,spec,Some(std::time::Instant::now()+duration)).await?);
            connected(transport.as_ref().unwrap(),name).await
        }) => result.unwrap_or_else(|_| Err(Error::category("TimeoutError"))),
        _=async {if !*cancelled.borrow() {let _=cancelled.changed().await;}}=>Err(Error::category("CancelledError")),
    };
    // Keep the transport outside the timed future: timeout/cancellation still reaps children.
    if let Some(mut transport) = transport {
        transport.shutdown().await;
    }
    result.unwrap_or_else(|error| failure(name, error))
}

pub async fn run(home: &Path, catalog: &Value, requested: Vec<String>) -> Result<bool> {
    let servers = catalog["servers"].as_object().ok_or("Invalid catalog")?;
    let mut names = requested;
    if names.is_empty() {
        names = servers.keys().cloned().collect();
        names.sort();
    }
    if names.iter().any(|n| !servers.contains_key(n)) {
        return Err("Unknown server name".into());
    }
    let mut signals = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        .map_err(|_| "Signal setup failed")?;
    let (cancel, cancelled) = watch::channel(false);
    let (output, mut lines) = mpsc::channel::<Value>(8);
    let mut writer = tokio::spawn(async move {
        let mut stdout = tokio::io::stdout();
        while let Some(result) = lines.recv().await {
            let mut bytes = result.to_string().into_bytes();
            bytes.push(b'\n');
            stdout
                .write_all(&bytes)
                .await
                .map_err(|_| "Check output unavailable")?;
            stdout
                .flush()
                .await
                .map_err(|_| "Check output unavailable")?;
        }
        Ok::<_, String>(())
    });
    let mut jobs = JoinSet::new();
    let mut results = vec![Value::Null; names.len()];
    let mut next = 0;
    let mut interrupted = false;
    let mut outcome = Ok(());
    let mut writer_consumed = false;
    let mut pending_output = None;
    loop {
        while next < names.len() && jobs.len() < 4 && !interrupted && pending_output.is_none() {
            let index = next;
            next += 1;
            let name = names[index].clone();
            let spec = servers[&name].clone();
            let home = home.to_path_buf();
            let cancelled = cancelled.clone();
            jobs.spawn(
                async move { (index, probe(&home, &name, &spec, DEADLINE, cancelled).await) },
            );
        }
        if jobs.is_empty() && pending_output.is_none() {
            break;
        }
        tokio::select! {
            _=signals.recv(),if !interrupted=>{interrupted=true;pending_output=None;cancel.send_replace(true);},
            _=tokio::signal::ctrl_c(),if !interrupted=>{interrupted=true;pending_output=None;cancel.send_replace(true);},
            result=&mut writer,if !interrupted=>{writer_consumed=true;interrupted=true;pending_output=None;cancel.send_replace(true);outcome=result.unwrap_or_else(|_|Err("Check output unavailable".into()));},
            permit=output.reserve(),if pending_output.is_some()=>{
                match permit {
                    Ok(permit)=>permit.send(pending_output.take().unwrap()),
                    Err(_)=>{interrupted=true;pending_output=None;cancel.send_replace(true);outcome=Err("Check output unavailable".into());}
                }
            },
            Some(result)=jobs.join_next(),if pending_output.is_none()=>{
                match result {
                    Ok((index,result))=>{
                        if !interrupted {pending_output=Some(result.clone());}
                        results[index]=result;
                    },
                    Err(_)=>{interrupted=true;cancel.send_replace(true);outcome=Err("Check task failed".into());},
                }
            }
        }
    }
    drop(output);
    if interrupted {
        if !writer_consumed {
            writer.abort();
            let _ = writer.await;
        }
        outcome?;
        return Err("Check cancelled".into());
    }
    tokio::select! {
        result=&mut writer=>result.map_err(|_| "Check output unavailable")??,
        _=signals.recv()=>{writer.abort();let _=writer.await;return Err("Check cancelled".into());},
        _=tokio::signal::ctrl_c()=>{writer.abort();let _=writer.await;return Err("Check cancelled".into());},
    }
    let failed = results.iter().any(|r| r["status"] == "failed");
    let path = config::state_dir(home).join("last-check.json");
    tokio::task::spawn_blocking(move || config::save_json(&path, &json!(results)))
        .await
        .map_err(|_| "Check state unavailable")??;
    Ok(failed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    fn home() -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!(
            "check-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path).unwrap();
        path
    }
    fn gone(path: &Path) {
        let pid: i32 = std::fs::read_to_string(path).unwrap().parse().unwrap();
        assert_eq!(
            nix::sys::signal::kill(nix::unistd::Pid::from_raw(pid), None),
            Err(nix::errno::Errno::ESRCH)
        );
    }
    #[tokio::test(flavor = "current_thread")]
    async fn authenticated_deadline_still_closes_session_without_refresh() {
        use std::{
            io::{BufRead, Read, Write},
            sync::{Arc, atomic::AtomicBool},
        };
        let home = home();
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let url = format!("http://{}/mcp", listener.local_addr().unwrap());
        config::save_json(
            &home.join(".config/comandos/extensions/credentials.json"),
            &json!({"demo":{"url":url,"access_token":"saved-token"}}),
        )
        .unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let stopping = stop.clone();
        let (deleted, deletion) = std::sync::mpsc::channel();
        let server = std::thread::spawn(move || {
            let mut handlers = vec![];
            while !stopping.load(Ordering::SeqCst) {
                match listener.accept() {
                    Ok((mut socket, _)) => {
                        let deleted = deleted.clone();
                        handlers.push(std::thread::spawn(move || {
                            socket.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
                            let mut input = std::io::BufReader::new(socket.try_clone().unwrap());
                            let mut start = String::new();
                            input.read_line(&mut start).unwrap();
                            let mut size = 0usize;
                            let mut authorized = false;
                            loop {
                                let mut header = String::new();
                                input.read_line(&mut header).unwrap();
                                if header == "\r\n" { break; }
                                let lower = header.to_ascii_lowercase();
                                if lower.starts_with("content-length:") {
                                    size = lower.split(':').nth(1).unwrap().trim().parse().unwrap();
                                }
                                if lower.trim() == "authorization: bearer saved-token" { authorized = true; }
                            }
                            let mut body = vec![0; size];
                            input.read_exact(&mut body).unwrap();
                            if start.starts_with("DELETE") {
                                deleted.send(authorized).unwrap();
                                socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").unwrap();
                                return;
                            }
                            let request: Value = serde_json::from_slice(&body).unwrap();
                            if request["method"] == "tools/list" {
                                std::thread::sleep(Duration::from_millis(500));
                                return;
                            }
                            let response = if request["method"] == "initialize" {
                                json!({"jsonrpc":"2.0","id":request["id"],"result":{"protocolVersion":"2025-03-26","capabilities":{"tools":{}},"serverInfo":{"name":"fixture","version":"1"}}}).to_string()
                            } else { String::new() };
                            let headers = format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nMcp-Session-Id: fixture\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", response.len(), response);
                            socket.write_all(headers.as_bytes()).unwrap();
                        }));
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(2))
                    }
                    Err(_) => break,
                }
            }
            for handler in handlers {
                handler.join().unwrap();
            }
        });
        let (_sender, cancelled) = watch::channel(false);
        let result = probe(
            &home,
            "demo",
            &json!({"url":url}),
            Duration::from_millis(200),
            cancelled,
        )
        .await;
        let closed = deletion.recv_timeout(Duration::from_millis(100));
        stop.store(true, Ordering::SeqCst);
        server.join().unwrap();
        std::fs::remove_dir_all(home).unwrap();
        assert_eq!(
            result,
            json!({"name":"demo","status":"failed","errors":["TimeoutError"]})
        );
        assert_eq!(
            closed,
            Ok(true),
            "cleanup must DELETE with saved auth without starting a fresh rotation"
        );
    }
    #[tokio::test(flavor = "current_thread")]
    async fn deadline_reaps_hung_and_saturated_children() {
        for saturated in [false, true] {
            let home = home();
            let marker = home.join("pid");
            let script = "import json,os,sys,time\nopen(sys.argv[1],'w').write(str(os.getpid()))\nif sys.argv[2]=='true':\n while True: print(json.dumps({'jsonrpc':'2.0','id':999,'method':'ping'}),flush=True)\ntime.sleep(60)";
            let spec = json!({"command":"/usr/bin/python3","args":["-c",script,marker,saturated.to_string()]});
            let (_sender, cancelled) = watch::channel(false);
            let at = std::time::Instant::now();
            let result = probe(&home, "demo", &spec, Duration::from_millis(300), cancelled).await;
            assert_eq!(
                result,
                json!({"name":"demo","status":"failed","errors":["TimeoutError"]})
            );
            assert!(at.elapsed() < Duration::from_secs(3));
            gone(&marker);
            std::fs::remove_dir_all(home).unwrap();
        }
    }
    #[tokio::test(flavor = "current_thread")]
    async fn cancellation_reaps_child() {
        let home = home();
        let marker = home.join("pid");
        let spec = json!({"command":"/usr/bin/python3","args":["-c","import os,sys,time;open(sys.argv[1],'w').write(str(os.getpid()));time.sleep(60)",marker]});
        let (sender, cancelled) = watch::channel(false);
        let h = home.clone();
        let task = tokio::spawn(async move {
            probe(&h, "demo", &spec, Duration::from_secs(40), cancelled).await
        });
        for _ in 0..100 {
            if marker.exists() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert!(marker.exists());
        sender.send_replace(true);
        let result = tokio::time::timeout(Duration::from_secs(3), task)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            result,
            json!({"name":"demo","status":"failed","errors":["CancelledError"]})
        );
        gone(&marker);
        std::fs::remove_dir_all(home).unwrap();
    }
}
