//! Disposable remote MCP smoke. TCP is available only with explicit empty host.
use base64::Engine;
use serde_json::{Value, json};
use std::{
    io::{BufRead, BufReader, Read, Write},
    net::{Shutdown, TcpStream},
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::mpsc::{self, Receiver},
    thread::{self, JoinHandle},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

const MAX_REPLY: u64 = 16 * 1024 * 1024;
const RPC_TIMEOUT: Duration = Duration::from_secs(150);
const USAGE: &str = "browser-e2e [--port N] [--host H] [--catalog-only] [--status-path ABSOLUTE_PATH]; --host '' selects private local TCP and requires --status-path";

struct Options {
    host: String,
    port: u16,
    catalog_only: bool,
    status_path: Option<PathBuf>,
}
fn parse(args: &[String]) -> Result<Options, String> {
    let mut opts = Options {
        host: "macmini".into(),
        port: 19442,
        catalog_only: false,
        status_path: None,
    };
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--catalog-only" => opts.catalog_only = true,
            "--port" => {
                opts.port = iter
                    .next()
                    .and_then(|s| s.parse::<u16>().ok())
                    .filter(|n| *n > 0)
                    .ok_or("invalid port")?
            }
            "--host" => opts.host = iter.next().ok_or("missing host")?.clone(),
            "--status-path" => {
                opts.status_path = Some(PathBuf::from(iter.next().ok_or("missing status path")?))
            }
            _ => return Err(format!("unknown option {arg}")),
        }
    }
    if opts.host.starts_with('-')
        || !opts
            .host
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-@".contains(&b))
    {
        return Err("invalid SSH host".into());
    }
    if opts.status_path.as_ref().is_some_and(|p| !p.is_absolute()) {
        return Err("status path must be absolute".into());
    }
    if opts.host.is_empty() && opts.status_path.is_none() {
        return Err("local TCP needs an explicit private status path".into());
    }
    Ok(opts)
}

fn ssh() -> Command {
    let mut cmd = Command::new("ssh");
    cmd.args([
        "-T",
        "-o",
        "BatchMode=yes",
        "-o",
        "ConnectTimeout=10",
        "-o",
        "ServerAliveInterval=15",
        "-o",
        "ServerAliveCountMax=2",
    ]);
    cmd
}
fn reader(input: impl Read + Send + 'static) -> (Receiver<Result<Value, String>>, JoinHandle<()>) {
    let (tx, rx) = mpsc::channel();
    let task = thread::spawn(move || {
        let mut input = BufReader::new(input);
        loop {
            let mut line = Vec::new();
            let count = match input
                .by_ref()
                .take(MAX_REPLY + 1)
                .read_until(b'\n', &mut line)
            {
                Ok(0) => break,
                Ok(n) => n,
                Err(e) => {
                    let _ = tx.send(Err(e.to_string()));
                    break;
                }
            };
            if count as u64 > MAX_REPLY {
                let _ = tx.send(Err("MCP reply exceeds 16 MiB".into()));
                break;
            }
            let value = serde_json::from_slice(&line).map_err(|e| format!("invalid MCP JSON: {e}"));
            if tx.send(value).is_err() {
                break;
            }
        }
    });
    (rx, task)
}
struct Client {
    writer: Option<Box<dyn Write>>,
    socket: Option<TcpStream>,
    child: Option<Child>,
    reader: Option<JoinHandle<()>>,
    replies: Receiver<Result<Value, String>>,
    id: u64,
}
impl Client {
    fn open(opts: &Options, name: &str) -> Result<Self, String> {
        let mut client = if opts.host.is_empty() {
            let stream = TcpStream::connect_timeout(
                &format!("127.0.0.1:{}", opts.port)
                    .parse()
                    .map_err(|e| format!("address: {e}"))?,
                Duration::from_secs(10),
            )
            .map_err(|e| e.to_string())?;
            stream
                .set_write_timeout(Some(Duration::from_secs(10)))
                .map_err(|e| e.to_string())?;
            let (replies, task) = reader(stream.try_clone().map_err(|e| e.to_string())?);
            let writer = stream.try_clone().map_err(|e| e.to_string())?;
            Self {
                writer: Some(Box::new(writer)),
                socket: Some(stream),
                child: None,
                reader: Some(task),
                replies,
                id: 0,
            }
        } else {
            let mut command = ssh();
            // Options precede the destination; no intermediary shell or config migration.
            command.args(["-W", &format!("127.0.0.1:{}", opts.port), &opts.host]);
            let mut child = command
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()
                .map_err(|e| e.to_string())?;
            let writer = child.stdin.take().ok_or("SSH stdin unavailable")?;
            let output = child.stdout.take().ok_or("SSH stdout unavailable")?;
            let (replies, task) = reader(output);
            Self {
                writer: Some(Box::new(writer)),
                socket: None,
                child: Some(child),
                reader: Some(task),
                replies,
                id: 0,
            }
        };
        let result=client.call("initialize",json!({"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":name,"version":"1"}}))?;
        if result["serverInfo"]["name"] != "comandos-browser-macmini" {
            return Err("unexpected MCP server identity".into());
        }
        Ok(client)
    }
    fn call(&mut self, method: &str, params: Value) -> Result<Value, String> {
        self.id = self.id.checked_add(1).ok_or("request id exhausted")?;
        let id = self.id;
        let writer = self.writer.as_mut().ok_or("closed MCP client")?;
        serde_json::to_writer(
            &mut **writer,
            &json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}),
        )
        .map_err(|e| e.to_string())?;
        writer
            .write_all(b"\n")
            .and_then(|()| writer.flush())
            .map_err(|e| e.to_string())?;
        let deadline = Instant::now() + RPC_TIMEOUT;
        loop {
            let remaining = deadline
                .checked_duration_since(Instant::now())
                .ok_or("MCP response timed out")?;
            let reply = self
                .replies
                .recv_timeout(remaining)
                .map_err(|e| format!("MCP connection: {e}"))??;
            if reply.get("id").and_then(Value::as_u64) != Some(id) {
                continue;
            }
            if let Some(error) = reply.get("error") {
                return Err(format!("MCP error: {error}"));
            }
            return reply
                .get("result")
                .cloned()
                .ok_or_else(|| "MCP reply lacks result".into());
        }
    }
    fn tool(&mut self, name: &str, args: Value) -> Result<Value, String> {
        self.call("tools/call", json!({"name":name,"arguments":args}))
    }
}
impl Drop for Client {
    fn drop(&mut self) {
        self.writer.take();
        if let Some(socket) = self.socket.take() {
            let _ = socket.shutdown(Shutdown::Both);
        }
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        if let Some(task) = self.reader.take() {
            let _ = task.join();
        }
    }
}
fn content_text(result: &Value) -> String {
    result["content"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|v| v["type"] == "text")
        .filter_map(|v| v["text"].as_str())
        .collect::<Vec<_>>()
        .join("\n")
}
fn valid_png(data: &str) -> bool {
    let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(data) else {
        return false;
    };
    const FRAME_LIMIT: usize = 64 * 1024 * 1024;
    let mut decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    decoder.set_limits(png::Limits { bytes: FRAME_LIMIT });
    let Ok(mut image) = decoder.read_info() else {
        return false;
    };
    let Some(size) = image
        .output_buffer_size()
        .filter(|size| *size <= FRAME_LIMIT)
    else {
        return false;
    };
    let mut pixels = vec![0; size];
    image.next_frame(&mut pixels).is_ok() && image.finish().is_ok()
}
fn successful(result: &Value) -> Result<(), String> {
    if result["isError"] == true {
        Err(format!("tool failed: {}", content_text(result)))
    } else {
        Ok(())
    }
}
fn busy(result: &Value) -> bool {
    result["isError"] == true && content_text(result).contains("browser is busy")
}
fn page_id(result: &Value, marker: &str) -> Result<u64, String> {
    successful(result)?;
    content_text(result)
        .lines()
        .filter(|line| line.contains(marker))
        .filter_map(|line| line.split_once(':')?.0.trim().parse().ok())
        .next_back()
        .ok_or_else(|| format!("new_page omitted marker {marker}"))
}
fn status(opts: &Options) -> Result<Value, String> {
    let bytes = if opts.host.is_empty() {
        let path = opts
            .status_path
            .as_ref()
            .ok_or("missing private status path")?;
        let mut bytes = Vec::new();
        std::fs::File::open(path)
            .map_err(|e| e.to_string())?
            .take(MAX_REPLY + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        bytes
    } else {
        let command = match &opts.status_path {
            Some(path) => format!("cat -- '{}'", path.to_string_lossy().replace('\'', "'\\''")),
            None => "cat -- \"$HOME/.local/share/comandos-browser/state-rs/status.json\"".into(),
        };
        let mut child = ssh()
            .arg(&opts.host)
            .arg(command)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| e.to_string())?;
        let output = child.stdout.take().ok_or("SSH status stdout unavailable")?;
        let task = thread::spawn(move || {
            let mut bytes = Vec::new();
            output
                .take(MAX_REPLY + 1)
                .read_to_end(&mut bytes)
                .map(|_| bytes)
        });
        let deadline = Instant::now() + Duration::from_secs(12);
        let exit = loop {
            match child.try_wait() {
                Ok(Some(exit)) => break Some(exit),
                Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(25)),
                _ => {
                    let _ = child.kill();
                    let _ = child.wait();
                    break None;
                }
            }
        };
        let bytes = task
            .join()
            .map_err(|_| "SSH status reader panicked")?
            .map_err(|e| e.to_string())?;
        if !exit.is_some_and(|e| e.success()) {
            return Err("SSH status read failed or timed out".into());
        }
        bytes
    };
    if bytes.len() as u64 > MAX_REPLY {
        return Err("status exceeds 16 MiB".into());
    }
    serde_json::from_slice(&bytes).map_err(|e| e.to_string())
}
fn run(opts: &Options) -> Result<(), String> {
    let count = if opts.catalog_only { 8 } else { 3 };
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_nanos();
    let names = (0..count)
        .map(|i| format!("comandos-rust-e2e-{}-{stamp}-{i}", std::process::id()))
        .collect::<Vec<_>>();
    let mut clients = names
        .iter()
        .map(|name| Client::open(opts, name).map(Some))
        .collect::<Result<Vec<_>, _>>()?;
    for client in clients.iter_mut().flatten() {
        let catalog = client.call("tools/list", json!({}))?;
        if catalog["tools"].as_array().map(Vec::len) != Some(29) {
            return Err("catalog must contain 29 tools".into());
        }
    }
    println!("PASS: {count} clients initialized; 29 tools each");
    if opts.catalog_only {
        let deadline = Instant::now() + Duration::from_secs(12);
        loop {
            if let Ok(s) = status(opts) {
                let sessions = s["sessions"].as_array();
                let present = names.iter().all(|name| {
                    sessions.is_some_and(|items| items.iter().any(|v| v["client"] == *name))
                });
                if present {
                    if s["workers"].as_u64() != Some(0) {
                        return Err(
                            "catalog-only created workers or broker already has workers".into()
                        );
                    }
                    println!("PASS: fresh broker status identifies all 8 clients; workers = 0");
                    return Ok(());
                }
            }
            if Instant::now() >= deadline {
                return Err(
                    "status never identified all catalog clients; cannot verify workers = 0".into(),
                );
            }
            thread::sleep(Duration::from_millis(200));
        }
    }
    let markers = ["COMANDOS_ISOLATION_ALPHA", "COMANDOS_ISOLATION_BETA"];
    let mut pages = Vec::new();
    for (client, marker) in clients.iter_mut().flatten().take(2).zip(markers) {
        let reply = client.tool(
            "new_page",
            json!({"url":format!("data:text/html,<title>{marker}</title><h1>{marker}</h1>")}),
        )?;
        pages.push(page_id(&reply, marker)?);
    }
    for (i, client) in clients.iter_mut().flatten().take(2).enumerate() {
        let inventory = client.tool("list_pages", json!({}))?;
        successful(&inventory)?;
        let text = content_text(&inventory);
        if !text.contains(markers[i]) || text.contains(markers[1 - i]) {
            return Err("page inventory crosses client ownership".into());
        }
        let snapshot = client.tool("take_snapshot", json!({"pageId":pages[i]}))?;
        successful(&snapshot)?;
        if !content_text(&snapshot).contains(markers[i]) {
            return Err("snapshot omitted owned page marker".into());
        }
    }
    println!("PASS: isolated pages and owned snapshots");
    let first = clients
        .first_mut()
        .and_then(Option::as_mut)
        .ok_or("first client missing")?;
    let screenshot = first.tool(
        "take_screenshot",
        json!({"pageId":pages.first().ok_or("first page missing")?,"format":"png"}),
    )?;
    successful(&screenshot)?;
    let png = screenshot["content"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|v| v["type"] == "image" && v["mimeType"] == "image/png")
        .filter_map(|v| v["data"].as_str())
        .any(valid_png);
    if !png {
        return Err("screenshot lacks a PNG image".into());
    }
    println!("PASS: PNG screenshot");
    let third = clients
        .get_mut(2)
        .and_then(Option::as_mut)
        .ok_or("third client missing")?;
    let args = json!({"url":"data:text/html,<title>COMANDOS_ISOLATION_THIRD</title><h1>COMANDOS_ISOLATION_THIRD</h1>"});
    let reply = third.tool("new_page", args.clone())?;
    if !busy(&reply) {
        return Err("third client did not receive ERR_BUSY at two workers".into());
    }
    println!("PASS: ERR_BUSY enforces two-worker capacity");
    if let Some(first) = clients.first_mut() {
        first.take();
    }
    let third = clients
        .get_mut(2)
        .and_then(Option::as_mut)
        .ok_or("third client missing")?;
    let deadline = Instant::now() + Duration::from_secs(12);
    loop {
        let reply = third.tool("new_page", args.clone())?;
        if !busy(&reply) {
            page_id(&reply, "COMANDOS_ISOLATION_THIRD")?;
            println!("PASS: released capacity after disconnect");
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err("disconnect did not release worker capacity".into());
        }
        thread::sleep(Duration::from_millis(100));
    }
}
pub fn main(args: &[String]) -> i32 {
    if args.iter().any(|s| matches!(s.as_str(), "--help" | "-h")) {
        println!("{USAGE}");
        return 0;
    }
    let opts = match parse(args) {
        Ok(opts) => opts,
        Err(e) => {
            eprintln!("{USAGE}\nerror: {e}");
            return 2;
        }
    };
    match run(&opts) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("browser-e2e: {e}");
            1
        }
    }
}
