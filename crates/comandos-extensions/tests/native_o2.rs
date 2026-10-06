//! Puertos de contratos ejecutables: procesos/HTTP reales, HOME privado, sin intérpretes.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]
#[allow(dead_code)]
mod support;
use comandos_extensions::{config, python_json, tokenizer::ENCODING_FILE};
use nix::{
    sys::signal::{Signal, kill},
    unistd::Pid,
};
use serde_json::{Value, json};
use std::{
    fs,
    io::{BufRead, BufReader, Read, Write},
    path::{Path, PathBuf},
    process::{Child, Command, Output, Stdio},
    sync::{
        atomic::{AtomicU64, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};
const BIN: &str = env!("CARGO_BIN_EXE_comandos-extensions");
const TIMEOUT: Duration = Duration::from_secs(12);
struct Home(PathBuf);
impl Home {
    fn new() -> Self {
        use std::os::unix::fs::DirBuilderExt;
        static ID: AtomicU64 = AtomicU64::new(0);
        let p = std::env::temp_dir().join(format!(
            "extension-native-o2-{}-{}",
            std::process::id(),
            ID.fetch_add(1, Ordering::Relaxed)
        ));
        fs::DirBuilder::new().mode(0o700).create(&p).unwrap();
        Self(p)
    }
    fn warm(&self) {
        let p = self.0.join(".cache/comandos/tiktoken");
        fs::create_dir_all(&p).unwrap();
        fs::write(
            p.join(ENCODING_FILE),
            include_bytes!("fixtures/native-o2/cl100k_base.tiktoken"),
        )
        .unwrap();
    }
    fn catalog(&self, servers: Value) {
        config::save_json(
            &self.0.join(".config/comandos/extensions/catalog.json"),
            &json!({"version":1,"servers":servers}),
        )
        .unwrap();
    }
    fn size(&self) -> PathBuf {
        config::state_dir(&self.0).join("sizes").join(format!(
            "{}.json",
            python_json::digest(&json!("demo")).unwrap()
        ))
    }
}
impl Drop for Home {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn until(mut f: impl FnMut() -> bool) {
    let end = Instant::now() + TIMEOUT;
    while !f() {
        assert!(Instant::now() < end, "bounded fixture wait expired");
        thread::sleep(Duration::from_millis(10));
    }
}
struct Owned(Child);
impl Owned {
    fn wait(&mut self) -> std::process::ExitStatus {
        let mut result = None;
        until(|| {
            result = self.0.try_wait().unwrap();
            result.is_some()
        });
        result.unwrap()
    }
    fn output(mut self) -> Output {
        let out = self.0.stdout.take();
        let err = self.0.stderr.take();
        let read = |pipe: Option<Box<dyn Read + Send>>| {
            thread::spawn(move || {
                let mut bytes = Vec::new();
                if let Some(mut pipe) = pipe {
                    pipe.read_to_end(&mut bytes).unwrap();
                }
                bytes
            })
        };
        let stdout = read(out.map(|p| Box::new(p) as Box<dyn Read + Send>));
        let stderr = read(err.map(|p| Box::new(p) as Box<dyn Read + Send>));
        let status = self.wait();
        Output {
            status,
            stdout: stdout.join().unwrap(),
            stderr: stderr.join().unwrap(),
        }
    }
}
impl Drop for Owned {
    fn drop(&mut self) {
        if self.0.try_wait().ok().flatten().is_none() {
            let _ = kill(Pid::from_raw(self.0.id() as i32), Signal::SIGTERM);
            let end = Instant::now() + Duration::from_secs(2);
            while self.0.try_wait().ok().flatten().is_none() && Instant::now() < end {
                thread::sleep(Duration::from_millis(5));
            }
            if self.0.try_wait().ok().flatten().is_none() {
                let _ = self.0.kill();
            }
            let _ = self.0.wait();
        }
    }
}
fn command(home: &Home, args: &[&str]) -> Command {
    let mut c = Command::new(BIN);
    c.args(["--home", home.0.to_str().unwrap()])
        .args(args)
        .env("HOME", &home.0)
        .env_remove("COMANDOS_DB")
        .env_remove("TIKTOKEN_CACHE_DIR")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    support::isolate(&mut c);
    c
}
fn spec(mode: &str) -> Value {
    json!({"command":std::env::current_exe().unwrap(),"args":["--fixture",mode],"disabled_tools":["blocked"]})
}
fn count(home: &Home, texts: &[String]) -> Owned {
    let mut p = Owned(command(home, &["count"]).spawn().unwrap());
    let mut input = p.0.stdin.take().unwrap();
    input
        .write_all(&serde_json::to_vec(texts).unwrap())
        .unwrap();
    drop(input);
    p
}
struct Session {
    p: Owned,
    input: Option<std::process::ChildStdin>,
    lines: mpsc::Receiver<String>,
}
impl Session {
    fn new(home: &Home, spec: Value) -> Self {
        Self::with_env(home, spec, &[])
    }
    fn with_env(home: &Home, spec: Value, env: &[(&str, &str)]) -> Self {
        home.catalog(json!({"demo":spec}));
        let mut cmd = command(home, &["serve", "demo"]);
        cmd.envs(env.iter().copied());
        let mut p = Owned(cmd.spawn().unwrap());
        let input = p.0.stdin.take();
        let out = p.0.stdout.take().unwrap();
        let (tx, lines) = mpsc::channel();
        thread::spawn(move || {
            for line in BufReader::new(out).lines().map_while(Result::ok) {
                if tx.send(line).is_err() {
                    break;
                }
            }
        });
        Self { p, input, lines }
    }
    fn send(&mut self, method: &str, params: Value) {
        writeln!(
            self.input.as_mut().unwrap(),
            "{}",
            json!({"jsonrpc":"2.0","id":1,"method":method,"params":params})
        )
        .unwrap();
    }
    fn recv(&self) -> Value {
        let line = self.lines.recv_timeout(TIMEOUT).unwrap();
        comandos_core::json::parse_value(&line).unwrap()
    }
    fn request(&mut self, method: &str, params: Value) -> Value {
        self.send(method, params);
        self.recv()
    }
    fn initialize(&mut self) {
        assert!(self.request("initialize",json!({"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"test","version":"1"}}))["result"].is_object());
        writeln!(
            self.input.as_mut().unwrap(),
            "{}",
            json!({"jsonrpc":"2.0","method":"notifications/initialized"})
        )
        .unwrap();
    }
    fn close(mut self) {
        drop(self.input.take());
        let out = self.p.output();
        assert!(out.status.success(), "{out:?}");
        assert!(out.stderr.is_empty(), "{out:?}");
    }
}
fn stored(home: &Home) -> Value {
    until(|| home.size().exists());
    serde_json::from_slice(&fs::read(home.size()).unwrap()).unwrap()
}
fn slots(home: &Home) -> Vec<fs::File> {
    let dir = config::state_dir(&home.0).join("tokenizer-slots");
    fs::create_dir_all(&dir).unwrap();
    (0..2)
        .map(|i| {
            fs::OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(false)
                .open(dir.join(format!("{i}.lock")))
                .unwrap()
        })
        .collect()
}
fn rss(pid: u32) -> u64 {
    fs::read_to_string(format!("/proc/{pid}/status"))
        .ok()
        .and_then(|s| {
            s.lines()
                .find(|l| l.starts_with("VmRSS:"))
                .and_then(|l| l.split_whitespace().nth(1))
                .and_then(|n| n.parse().ok())
        })
        .unwrap_or(0)
}
fn counters(pid: u32) -> Vec<u32> {
    fs::read_to_string(format!("/proc/{pid}/task/{pid}/children"))
        .unwrap()
        .split_whitespace()
        .filter_map(|s| s.parse().ok())
        .filter(|p| {
            fs::read(format!("/proc/{p}/cmdline"))
                .ok()
                .is_some_and(|b| b.windows(7).any(|s| s == b"\0count\0"))
        })
        .collect()
}
fn metadata_complete_filtered_list() {
    let h = Home::new();
    h.warm();
    let s = spec("metadata");
    let mut p = Session::new(&h, s.clone());
    p.initialize();
    p.request("tools/list", json!({"cursor":"page2"}));
    thread::sleep(Duration::from_millis(80));
    assert!(!h.size().exists());
    let first = p.request("tools/list", json!({}))["result"].clone();
    assert_eq!(first["tools"][0]["futureField"], 3);
    thread::sleep(Duration::from_millis(80));
    assert!(!h.size().exists());
    p.request("tools/list", json!({"cursor":"wrong"}));
    thread::sleep(Duration::from_millis(80));
    assert!(!h.size().exists());
    p.request("tools/list", json!({}));
    p.request("tools/list", json!({"cursor":"page2"}));
    let data = stored(&h);
    let mut defs = Vec::new();
    for _ in 0..2 {
        for tool in first["tools"].as_array().unwrap() {
            let mut d = serde_json::Map::new();
            for key in ["name", "description", "inputSchema", "outputSchema"] {
                if let Some(value) = tool.get(key) {
                    d.insert(key.into(), value.clone());
                }
            }
            defs.push(Value::Object(d));
        }
    }
    let defs = Value::Array(defs);
    let text = python_json::dumps(&defs, false, true).unwrap();
    let out = count(&h, &[text]).output();
    assert!(out.status.success());
    let counts: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(data["tokens"], counts[0]);
    assert_eq!(data["content"], python_json::digest(&defs).unwrap());
    assert_eq!(data["configuration"], python_json::digest(&s).unwrap());
    let serial = data.to_string();
    assert!(!serial.contains("inputSchema") && !serial.contains("blocked"));
    let before = fs::metadata(h.size()).unwrap().modified().unwrap();
    p.request("tools/list", json!({}));
    p.request("tools/list", json!({"cursor":"page2"}));
    thread::sleep(Duration::from_millis(250));
    assert_eq!(fs::metadata(h.size()).unwrap().modified().unwrap(), before);
    p.close();
}
fn metadata_cursor_budget() {
    let h = Home::new();
    h.warm();
    let mut p = Session::new(&h, spec("large-cursor"));
    p.initialize();
    let mut cursor = Value::Null;
    for page in 0..5 {
        let params = if cursor.is_null() {
            json!({})
        } else {
            json!({"cursor":cursor})
        };
        let response = p.request("tools/list", params);
        let mut expected = json!({"tools":if page==0{vec![json!({"name":"partial","inputSchema":{}})]}else{vec![]},"futureResult":42});
        if page < 4 {
            expected["nextCursor"] = json!(format!("{page}{}", "x".repeat(20_000)));
        }
        assert_eq!(response, json!({"jsonrpc":"2.0","id":1,"result":expected}));
        cursor = expected.get("nextCursor").cloned().unwrap_or(Value::Null);
    }
    thread::sleep(Duration::from_millis(150));
    assert!(!h.size().exists());
    let fresh = json!({"tools":[{"name":"fresh","inputSchema":{}}],"futureResult":42});
    assert_eq!(
        p.request("tools/list", json!({"fresh":true})),
        json!({"jsonrpc":"2.0","id":1,"result":fresh})
    );
    assert_eq!(
        stored(&h)["content"],
        python_json::digest(&fresh["tools"]).unwrap()
    );
    p.close();
}
fn metadata_blocked_slots() {
    let h = Home::new();
    h.warm();
    let locks = slots(&h);
    for lock in &locks {
        lock.lock().unwrap();
    }
    let mut children: Vec<_> = (0..4).map(|_| count(&h, &["hello world".into()])).collect();
    thread::sleep(Duration::from_millis(150));
    assert!(
        children
            .iter_mut()
            .all(|p| p.0.try_wait().unwrap().is_none())
    );
    assert!(children.iter().map(|p| rss(p.0.id())).max().unwrap() < 20_000);
    children[0].0.kill().unwrap();
    children[0].wait();
    locks[0].unlock().unwrap();
    for p in children.into_iter().skip(1) {
        let out = p.output();
        assert!(out.status.success());
        assert_eq!(
            serde_json::from_slice::<Value>(&out.stdout).unwrap(),
            json!([2])
        );
    }
}
fn metadata_many_workers() {
    let h = Home::new();
    h.warm();
    let input = "hello world ".repeat(80_000);
    let mut children: Vec<_> = (0..6)
        .map(|_| count(&h, std::slice::from_ref(&input)))
        .collect();
    let mut peak = 0;
    until(|| {
        let active = children.iter().filter(|p| rss(p.0.id()) > 24_000).count();
        assert!(active <= 2, "more than two resident tokenizer processes");
        peak = peak.max(active);
        children
            .iter_mut()
            .all(|p| p.0.try_wait().unwrap().is_some())
    });
    assert_eq!(peak, 2);
    for p in children {
        let out = p.output();
        assert!(out.status.success() && out.stderr.is_empty());
        assert!(
            serde_json::from_slice::<Value>(&out.stdout).unwrap()[0]
                .as_u64()
                .unwrap()
                > 0
        );
    }
}
fn metadata_proxy_rotation() {
    let h = Home::new();
    h.warm();
    let locks = slots(&h);
    let mut previous = Value::Null;
    for mut s in [spec("metadata"), spec("metadata")] {
        if !previous.is_null() {
            s.as_object_mut().unwrap().remove("disabled_tools");
            s["enabled_tools"] = json!(["echo"]);
        }
        for lock in &locks {
            lock.lock().unwrap();
        }
        let mut proxies: Vec<_> = (0..3).map(|_| Session::new(&h, s.clone())).collect();
        for p in &mut proxies {
            p.initialize();
            p.request("tools/list", json!({}));
            p.request("tools/list", json!({"cursor":"page2"}));
        }
        thread::sleep(Duration::from_millis(150));
        assert_eq!(
            proxies
                .iter()
                .map(|p| counters(p.p.0.id()).len())
                .sum::<usize>(),
            1
        );
        for lock in &locks {
            lock.unlock().unwrap();
        }
        let expected = json!(python_json::digest(&s).unwrap());
        until(|| {
            h.size().exists()
                && serde_json::from_slice::<Value>(&fs::read(h.size()).unwrap()).unwrap()["configuration"]
                    == expected
        });
        assert_ne!(expected, previous);
        previous = expected;
        until(|| proxies.iter().all(|p| counters(p.p.0.id()).is_empty()));
        for p in proxies {
            p.close();
        }
    }
}
#[cfg(target_os = "linux")]
fn metadata_blocked_output() {
    let h = Home::new();
    h.warm();
    let locks = slots(&h);
    let texts = vec!["hello".into(); 10_000];
    let mut children = Vec::new();
    for _ in 0..3 {
        let p = count(&h, &texts);
        use std::os::fd::AsFd;
        nix::fcntl::fcntl(
            p.0.stdout.as_ref().unwrap().as_fd(),
            nix::fcntl::FcntlArg::F_SETPIPE_SZ(4096),
        )
        .unwrap();
        children.push(p);
    }
    thread::sleep(Duration::from_millis(500));
    assert!(
        children
            .iter_mut()
            .all(|p| p.0.try_wait().unwrap().is_none())
    );
    let held = locks
        .iter()
        .filter(|l| matches!(l.try_lock(), Err(fs::TryLockError::WouldBlock)))
        .count();
    assert_eq!(held, 2);
}
#[cfg(not(target_os = "linux"))]
fn metadata_blocked_output() {
    println!("Linux pipe sizing boundary: not run on this platform");
}
fn event(kind: &str) {
    if let Ok(path) = std::env::var("EVENTS") {
        let mut f = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .unwrap();
        f.write_all(
            format!(
                "{}\n",
                json!([kind, std::env::var("LABEL").unwrap_or_default()])
            )
            .as_bytes(),
        )
        .unwrap();
    }
}
fn fixture(mode: &str) {
    if mode == "exec" {
        println!(
            "{}",
            json!([
                std::process::id(),
                std::env::args().skip(3).collect::<Vec<_>>(),
                std::env::var("EXPANDED").ok(),
                std::env::current_dir().unwrap()
            ])
        );
        std::process::exit(17);
    }
    if mode == "ready" {
        println!("ready");
        return;
    }
    if mode == "nonfinite-env" {
        println!(
            "{}",
            json!([
                std::env::var("NAN").unwrap(),
                std::env::var("POS").unwrap(),
                std::env::var("NEG").unwrap()
            ])
        );
        return;
    }
    if mode == "crash" {
        std::process::exit(1);
    }
    let mut held = 0;
    if let Ok(path) = std::env::var("MARKER") {
        fs::write(path, std::process::id().to_string()).unwrap();
    }
    event("start");
    let mut out = std::io::stdout().lock();
    for line in std::io::stdin().lock().lines().map_while(Result::ok) {
        let d: Value = comandos_core::json::parse_value(&line).unwrap();
        if d["method"] == "notifications/cancelled"
            && let Ok(path) = std::env::var("CANCELLED")
        {
            fs::write(path, d["params"]["requestId"].to_string()).unwrap();
        }
        let Some(id) = d.get("id") else { continue };
        if mode == "cancel" && d["method"] == "tools/call" {
            continue;
        }
        if mode == "hold" && d["method"] == "tools/call" {
            held += 1;
            if let Ok(path) = std::env::var("CALL_COUNT") {
                fs::write(path, held.to_string()).unwrap();
            }
            if d["params"]["arguments"]["replacement"] != true {
                continue;
            }
        }
        if matches!(mode, "metadata" | "hold")
            && d["method"] == "tools/list"
            && d["params"]["cursor"] == "error-page"
        {
            writeln!(out,"{}",json!({"jsonrpc":"2.0","id":id,"error":{"code":-32099,"message":"Original list error","data":{"cursor":"error-page","extra":[1,2]}},"futureEnvelope":{"keep":true}})).unwrap();
            continue;
        }
        if matches!(mode, "metadata" | "hold")
            && d["method"] == "tools/call"
            && d["params"]["arguments"]["fail"] == true
        {
            writeln!(out,"{}",json!({"jsonrpc":"2.0","id":id,"error":{"code":-32099,"message":"Original error","data":{"extra":4}}})).unwrap();
            continue;
        }
        let method = d["method"].as_str().unwrap();
        if mode == "rpc" {
            writeln!(
                out,
                "{}",
                json!({"jsonrpc":"2.0","id":id,"error":{"code":-32000,"message":"SENTINEL"}})
            )
            .unwrap();
            continue;
        }
        let mut result = match method {
            "initialize" => {
                thread::sleep(Duration::from_secs_f64(
                    std::env::var("DELAY")
                        .ok()
                        .and_then(|s| s.parse().ok())
                        .unwrap_or(0.0),
                ));
                let mut r = json!({"protocolVersion":"2025-03-26","capabilities":{"tools":{}},"serverInfo":{"name":"fixture","version":"1"}});
                if matches!(mode, "metadata" | "hold" | "cancel") {
                    r["instructions"] = json!("Keep original instructions");
                    r["capabilities"] = json!({"tools":{"listChanged":true},"resources":{"subscribe":true},"prompts":{},"completions":{},"logging":{}});
                }
                match mode {
                    "invalid-init" => r = json!({}),
                    "no-tools" => r["capabilities"] = json!({}),
                    "unsupported-version" => r["protocolVersion"] = json!("2099-01-01"),
                    "server-info" => r["serverInfo"] = json!({}),
                    "tools-capability" => r["capabilities"]["tools"] = json!(false),
                    _ => {}
                }
                r
            }
            "tools/list" => {
                if mode == "large-cursor" {
                    if d["params"]["fresh"] == true {
                        json!({"tools":[{"name":"fresh","inputSchema":{}}],"futureResult":42})
                    } else {
                        let page = d["params"]["cursor"]
                            .as_str()
                            .map(|s| s.as_bytes()[0] - b'0' + 1)
                            .unwrap_or(0);
                        let mut r = json!({"tools":if page==0{vec![json!({"name":"partial","inputSchema":{}})]}else{vec![]},"futureResult":42});
                        if page < 4 {
                            r["nextCursor"] = json!(format!("{page}{}", "x".repeat(20_000)));
                        }
                        r
                    }
                } else {
                    let mut r = if mode == "metadata" {
                        json!({"tools":[{"name":"echo","description":"Original description","inputSchema":{"type":"object"},"futureField":3},{"name":"blocked","inputSchema":{"type":"object"}}],"futureResult":42})
                    } else {
                        json!({"tools":[{"name":"tool","inputSchema":{"type":"object"}},{"name":"tool","inputSchema":{"type":"object"}}]})
                    };
                    if mode == "cap" || d["params"]["cursor"].is_null() {
                        r["nextCursor"] = json!(if mode == "metadata" { "page2" } else { "next" });
                    } else {
                        event("end");
                    }
                    r
                }
            }
            "tools/call" => {
                if matches!(mode, "metadata" | "hold") {
                    thread::sleep(Duration::from_secs_f64(
                        d["params"]["arguments"]["delay"].as_f64().unwrap_or(0.0),
                    ));
                    json!({"content":[{"type":"text","text":"ok"}],"structuredContent":d["params"]["arguments"],"isError":false,"future":7})
                } else {
                    if let Ok(path) = std::env::var("CALLS") {
                        fs::write(path, d["params"].to_string()).unwrap();
                    }
                    let profile=std::env::var("PROFILE_JSON").unwrap_or_else(|_|json!({std::env::var("EMAIL_FIELD").unwrap_or_else(|_|"emailAddress".into()):std::env::var("EMAIL").unwrap_or_default()}).to_string());
                    let text = if mode == "bad-json" {
                        "SENTINEL invalid".into()
                    } else {
                        profile
                    };
                    let mut r = json!({"content":if mode=="no-text"{vec![]}else{vec![json!({"type":"text","text":text})]},"isError":mode=="denied"});
                    if mode == "invalid-content" {
                        r["content"] = json!([{"type":"text","text":[]}]);
                    }
                    r
                }
            }
            _ => {
                let mut r = json!({"echo":method,"params":d["params"],"extra":[1,2]});
                match method {
                    "resources/list" => {
                        if mode != "invalid-resources" {
                            r["resources"] = json!([]);
                        }
                    }
                    "resources/templates/list" => r["resourceTemplates"] = json!([]),
                    "resources/read" => r["contents"] = json!([]),
                    "prompts/list" => r["prompts"] = json!([]),
                    "prompts/get" => r["messages"] = json!([]),
                    "completion/complete" => r["completion"] = json!({"values":[]}),
                    _ => {}
                }
                r
            }
        };
        if mode == "protocol" {
            result = serde_json::from_str::<Value>(&std::env::var("PROTOCOL_REPLIES").unwrap())
                .unwrap()
                .get(method)
                .cloned()
                .unwrap_or(json!({}));
        }
        writeln!(out, "{}", json!({"jsonrpc":"2.0","id":id,"result":result})).unwrap();
        out.flush().unwrap();
    }
}
fn run_check(home: &Home, servers: Value, names: &[&str]) -> (Output, Vec<Value>, Option<Value>) {
    home.catalog(servers);
    let mut args = vec!["check"];
    args.extend_from_slice(names);
    let out = Owned(command(home, &args).spawn().unwrap()).output();
    let rows = String::from_utf8(out.stdout.clone())
        .unwrap()
        .lines()
        .map(|s| serde_json::from_str(s).unwrap())
        .collect();
    let saved = fs::read(config::state_dir(&home.0).join("last-check.json"))
        .ok()
        .map(|s| serde_json::from_slice(&s).unwrap());
    (out, rows, saved)
}
fn check_protocol_cases() {
    let cases: Value =
        serde_json::from_str(include_str!("fixtures/native-o2/check_protocol_cases.json")).unwrap();
    for case in cases.as_array().unwrap() {
        let h = Home::new();
        let mut s = spec("protocol");
        s["env"] = json!({"PROTOCOL_REPLIES":case[1].to_string()});
        let (out, rows, saved) = run_check(&h, json!({"google-drive":s}), &[]);
        assert_eq!(rows, vec![case[2].clone()], "{}: {out:?}", case[0]);
        assert_eq!(saved, Some(json!([case[2]])), "{}", case[0]);
        assert_eq!(
            out.status.code(),
            Some(i32::from(case[2]["status"] == "failed")),
            "{}",
            case[0]
        );
    }
    println!(
        "native protocol shapes: {}",
        cases.as_array().unwrap().len()
    );
}
fn check_process_contracts() {
    let h = Home::new();
    let marker = h.0.join("pid");
    let mut s = spec("normal");
    s["env"] = json!({"MARKER":marker});
    let (out, _, _) = run_check(&h, json!({"demo":s.clone()}), &[]);
    assert!(out.status.success());
    let pid: u32 = fs::read_to_string(&marker).unwrap().parse().unwrap();
    assert_eq!(
        kill(Pid::from_raw(pid as i32), None),
        Err(nix::errno::Errno::ESRCH)
    );
    fs::remove_file(&marker).unwrap();
    s["env"]["DELAY"] = json!("60");
    h.catalog(json!({"demo":s}));
    let p = Owned(command(&h, &["check"]).spawn().unwrap());
    until(|| marker.exists());
    let child: u32 = fs::read_to_string(&marker).unwrap().parse().unwrap();
    let cmdline = fs::read(format!("/proc/{child}/cmdline")).unwrap();
    assert!(cmdline.windows(9).any(|s| s == b"--fixture"));
    kill(Pid::from_raw(p.0.id() as i32), Signal::SIGTERM).unwrap();
    let out = p.output();
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(
        kill(Pid::from_raw(child as i32), None),
        Err(nix::errno::Errno::ESRCH)
    );
    assert!(!String::from_utf8_lossy(&out.stderr).contains("panicked"));
    let h = Home::new();
    let servers: serde_json::Map<String, Value> = (0..200)
        .map(|i| (format!("{i}{}", "x".repeat(2048)), json!({"enabled":false})))
        .collect();
    h.catalog(Value::Object(servers));
    let mut p = Owned(command(&h, &["check"]).spawn().unwrap());
    thread::sleep(Duration::from_millis(200));
    assert!(p.0.try_wait().unwrap().is_none());
    kill(Pid::from_raw(p.0.id() as i32), Signal::SIGTERM).unwrap();
    let out = p.output();
    assert_eq!(out.status.code(), Some(1));
    assert!(!String::from_utf8_lossy(&out.stderr).contains("panicked"));
}
fn check_native_contracts() {
    for (mode, n) in [("normal", 4), ("no-tools", 0), ("cap", 60)] {
        let h = Home::new();
        let mut s = spec(mode);
        s["enabled_tools"] = json!([]);
        s["disabled_tools"] = json!(["tool"]);
        let (out, rows, saved) = run_check(&h, json!({"demo":s}), &[]);
        assert!(out.status.success(), "{out:?}");
        assert_eq!(
            rows,
            vec![json!({"name":"demo","status":"connected","tools":n})]
        );
        assert_eq!(saved, Some(json!(rows)));
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(config::state_dir(&h.0).join("last-check.json"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        assert!(fs::read_dir(config::state_dir(&h.0)).unwrap().all(|e| {
            !e.unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".comandos-")
        }));
    }
    for (mode, error) in [
        ("rpc", "McpError"),
        ("invalid-init", "ValidationError"),
        ("bad-json", "JSONDecodeError"),
        ("no-text", "StopIteration"),
        ("server-info", "ValidationError"),
        ("tools-capability", "ValidationError"),
        ("invalid-content", "ValidationError"),
    ] {
        let h = Home::new();
        let (out, rows, _) = run_check(&h, json!({"gmail":spec(mode)}), &[]);
        assert_eq!(out.status.code(), Some(1));
        assert_eq!(
            rows,
            vec![json!({"name":"gmail","status":"failed","errors":[error]})]
        );
        assert!(
            !String::from_utf8_lossy(&out.stdout).contains("SENTINEL")
                && !String::from_utf8_lossy(&out.stderr).contains("SENTINEL")
        );
    }
    for enabled in [
        Value::Null,
        json!(false),
        json!(0),
        json!(""),
        json!([]),
        json!({}),
    ] {
        let h = Home::new();
        let marker = h.0.join("pid");
        let mut s = spec("normal");
        s["enabled"] = enabled;
        s["env"] = json!({"MARKER":marker});
        let (out, rows, _) = run_check(&h, json!({"demo":s}), &[]);
        assert!(out.status.success());
        assert_eq!(rows, vec![json!({"name":"demo","status":"disabled"})]);
        assert!(!marker.exists());
    }
    let mail = [
        ("gmail", "jesusbatallar@gmail.com"),
        ("gmail-signara", "jesus@signara.ai"),
        ("qcdr-mail", "jesus@qcdr.io"),
        ("proton-mail", "pdlgmcn@protonmail.com"),
        ("google-drive", ""),
        ("google-calendar", ""),
    ];
    for (name, email) in mail {
        for denied in [false, true] {
            let h = Home::new();
            let log = h.0.join("calls");
            let mut s = spec(if denied { "denied" } else { "normal" });
            s["env"] = json!({"EMAIL":email,"CALLS":log});
            let (out, rows, _) = run_check(&h, json!({name:s}), &[]);
            let expected = if denied {
                json!({"name":name,"status":"failed","phase":"read_access","tools":4})
            } else {
                json!({"name":name,"status":"connected","tools":4})
            };
            assert_eq!(rows, vec![expected]);
            assert_eq!(out.status.code(), Some(i32::from(denied)));
            let tool = if name == "google-drive" {
                "list_recent_files"
            } else if name == "google-calendar" {
                "list-calendars"
            } else {
                "get_profile"
            };
            assert_eq!(
                serde_json::from_slice::<Value>(&fs::read(log).unwrap()).unwrap(),
                json!({"name":tool,"arguments":if name=="google-drive"{json!({"pageSize":1})}else{json!({})}})
            );
        }
    }
    for field in ["emailAddress", "email"] {
        for (email, valid) in [
            ("other@example.test", false),
            ("jesusbatallar@gmail.com", true),
        ] {
            let h = Home::new();
            let mut s = spec("normal");
            s["env"] = json!({"EMAIL":email,"EMAIL_FIELD":field});
            let (out, rows, _) = run_check(&h, json!({"gmail":s}), &[]);
            assert_eq!(
                rows[0]["status"],
                if valid { "connected" } else { "failed" }
            );
            assert_eq!(rows[0]["tools"], 4);
            assert_eq!(out.status.code(), Some(i32::from(!valid)));
            if !valid {
                assert_eq!(rows[0]["phase"], "mailbox_identity");
            }
        }
    }
    for (profile, valid) in [
        (
            r#"{"emailAddress":"other", "emailAddress":"jesusbatallar@gmail.com"}"#,
            true,
        ),
        (r#"{"email":"jesusbatallar@gmail.com", "extra":NaN}"#, true),
        (
            r#"{"emailAddress":null,"email":"jesusbatallar@gmail.com", "extra":Infinity}"#,
            true,
        ),
        (
            r#"{"emailAddress":NaN,"email":"jesusbatallar@gmail.com"}"#,
            false,
        ),
    ] {
        let h = Home::new();
        let mut s = spec("normal");
        s["env"] = json!({"PROFILE_JSON":profile});
        let (out, rows, _) = run_check(&h, json!({"gmail":s}), &[]);
        assert_eq!(
            rows[0]["status"],
            if valid { "connected" } else { "failed" }
        );
        assert_eq!(rows[0]["tools"], 4);
        assert_eq!(out.status.code(), Some(i32::from(!valid)));
        if !valid {
            assert_eq!(rows[0]["phase"], "mailbox_identity");
        }
    }
}

use bytes::Bytes;
use http_body_util::{BodyExt, Full, StreamBody, combinators::UnsyncBoxBody};
use hyper::{
    Method, Request, Response,
    body::{Frame, Incoming},
    service::service_fn,
};
use hyper_util::rt::TokioIo;
use std::{
    convert::Infallible,
    sync::{Arc, Mutex},
};
type Body = UnsyncBoxBody<Bytes, Infallible>;
struct HttpRequest {
    method: Method,
    path: String,
    authorization: Option<String>,
    session: Option<String>,
    body: Vec<u8>,
}
impl HttpRequest {
    fn json(&self) -> Value {
        serde_json::from_slice(&self.body).unwrap()
    }
}
fn reply(status: u16, body: Vec<u8>, headers: &[(&str, String)]) -> Response<Body> {
    let mut r = Response::builder().status(status);
    for (k, v) in headers {
        r = r.header(*k, v);
    }
    r.body(Full::new(Bytes::from(body)).boxed_unsync()).unwrap()
}
fn json_reply(status: u16, data: Value) -> Response<Body> {
    reply(
        status,
        data.to_string().into_bytes(),
        &[("content-type", "application/json".into())],
    )
}
fn stream_reply(rx: tokio::sync::mpsc::UnboundedReceiver<Bytes>) -> Response<Body> {
    let frames = futures_util::stream::unfold(rx, |mut rx| async move {
        rx.recv()
            .await
            .map(|bytes| (Ok::<_, Infallible>(Frame::data(bytes)), rx))
    });
    Response::builder()
        .status(200)
        .header("content-type", "text/event-stream")
        .body(StreamBody::new(frames).boxed_unsync())
        .unwrap()
}
fn normal_result(request: &Value) -> Value {
    match request["method"].as_str().unwrap_or("") {
        "initialize" => {
            json!({"protocolVersion":"2025-03-26","capabilities":{"tools":{}},"serverInfo":{"name":"fixture","version":"1"}})
        }
        "tools/list" => {
            let mut r = json!({"tools":[{"name":"tool","inputSchema":{}},{"name":"tool","inputSchema":{}}]});
            if request["params"]["cursor"].is_null() {
                r["nextCursor"] = json!("page2");
            }
            r
        }
        "tools/call" => {
            json!({"content":[{"type":"text","text":"ok"}],"structuredContent":request["params"]["arguments"],"isError":false})
        }
        _ => json!({}),
    }
}
fn normal_http(request: &HttpRequest) -> Response<Body> {
    if request.method == Method::DELETE {
        return reply(200, vec![], &[]);
    }
    if request.method != Method::POST {
        return reply(405, vec![], &[]);
    }
    let message = request.json();
    if message.get("id").is_none() {
        return reply(202, vec![], &[]);
    }
    let payload = json!({"jsonrpc":"2.0","id":message["id"],"result":normal_result(&message)});
    reply(
        200,
        payload.to_string().into_bytes(),
        &[
            ("content-type", "application/json".into()),
            ("mcp-session-id", "fixture-session".into()),
        ],
    )
}
struct Server {
    url: String,
    shutdown: Option<tokio::sync::oneshot::Sender<()>>,
    worker: Option<thread::JoinHandle<()>>,
}
impl Server {
    fn new(
        tls: bool,
        handler: impl FnOnce(&str) -> Arc<dyn Fn(HttpRequest) -> Response<Body> + Send + Sync>,
    ) -> Self {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let port = listener.local_addr().unwrap().port();
        let url = format!("{}://127.0.0.1:{port}", if tls { "https" } else { "http" });
        let handler = handler(&url);
        let (tx, rx) = tokio::sync::oneshot::channel();
        let worker = thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            rt.block_on(async move{
   use rustls::pki_types::{CertificateDer,PrivateKeyDer,pem::PemObject};
   let acceptor=if tls{let cert=CertificateDer::from_pem_slice(include_bytes!("fixtures/native-o2/tls/server.pem")).unwrap();let key=PrivateKeyDer::from_pem_slice(include_bytes!("fixtures/native-o2/tls/server.key")).unwrap();let config=rustls::ServerConfig::builder_with_provider(Arc::new(rustls::crypto::aws_lc_rs::default_provider())).with_safe_default_protocol_versions().unwrap().with_no_client_auth().with_single_cert(vec![cert],key).unwrap();Some(tokio_rustls::TlsAcceptor::from(Arc::new(config)))}else{None};
   let listener=tokio::net::TcpListener::from_std(listener).unwrap();tokio::pin!(rx);loop{let stream=tokio::select!{_=&mut rx=>break,result=listener.accept()=>result.unwrap().0};let(handler,acceptor)=(handler.clone(),acceptor.clone());tokio::spawn(async move{let service=service_fn(move|request:Request<Incoming>|{let handler=handler.clone();async move{let method=request.method().clone();let path=request.uri().to_string();let authorization=request.headers().get("authorization").and_then(|v|v.to_str().ok()).map(str::to_owned);let session=request.headers().get("mcp-session-id").and_then(|v|v.to_str().ok()).map(str::to_owned);let body=request.into_body().collect().await.unwrap().to_bytes().to_vec();Ok::<_,Infallible>(handler(HttpRequest{method,path,authorization,session,body}))}});if let Some(tls)=acceptor{if let Ok(stream)=tls.accept(stream).await{let _=hyper::server::conn::http1::Builder::new().serve_connection(TokioIo::new(stream),service).await;}}else{let _=hyper::server::conn::http1::Builder::new().serve_connection(TokioIo::new(stream),service).await;}});}
  });
        });
        Self {
            url,
            shutdown: Some(tx),
            worker: Some(worker),
        }
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        if let Some(tx) = self.shutdown.take() {
            let _ = tx.send(());
        }
        if let Some(worker) = self.worker.take() {
            worker.join().unwrap();
        }
    }
}
fn http_check_status_and_sessions() {
    for status in [401, 403, 307, 500] {
        let server = Server::new(false, move |_| {
            Arc::new(move |_| {
                reply(
                    status,
                    b"SENTINEL_TOKEN private upstream exception 999".to_vec(),
                    &[],
                )
            })
        });
        let h = Home::new();
        let (out, rows, _) = run_check(
            &h,
            json!({"demo":{"url":format!("{}/mcp",server.url),"headers":{"Authorization":"Bearer SENTINEL_TOKEN"}}}),
            &[],
        );
        assert_eq!(out.status.code(), Some(1));
        assert_eq!(
            rows,
            vec![
                json!({"name":"demo","status":"failed","errors":["HTTPStatusError"],"http_status":[status]})
            ]
        );
        assert!(
            !String::from_utf8_lossy(&out.stdout).contains("SENTINEL")
                && !String::from_utf8_lossy(&out.stderr).contains("SENTINEL")
        );
    }
    for transport in ["http", "stream", "sse"] {
        let deletes = Arc::new(Mutex::new(Vec::new()));
        let capture = deletes.clone();
        let events = Arc::new(Mutex::new(
            None::<tokio::sync::mpsc::UnboundedSender<Bytes>>,
        ));
        let server = Server::new(false, move |_| {
            Arc::new(move |request| {
                if request.method == Method::DELETE {
                    capture.lock().unwrap().push(request.session.clone());
                    return normal_http(&request);
                }
                if request.method == Method::GET && request.path == "/sse" {
                    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
                    tx.send(Bytes::from_static(
                        b"event: endpoint\r\ndata: /messages?session=fixture\r\n\r\n",
                    ))
                    .unwrap();
                    *events.lock().unwrap() = Some(tx);
                    return stream_reply(rx);
                }
                if request.method == Method::POST {
                    let message = request.json();
                    if message.get("id").is_none() {
                        return reply(202, vec![], &[]);
                    }
                    let response = json!({"jsonrpc":"2.0","id":message["id"],"result":normal_result(&message)});
                    if request.path.starts_with("/messages") {
                        events
                            .lock()
                            .unwrap()
                            .as_ref()
                            .unwrap()
                            .send(Bytes::from(format!("event: message\ndata: {response}\n\n")))
                            .unwrap();
                        return reply(202, vec![], &[]);
                    }
                    if request.path == "/stream" {
                        return reply(
                            200,
                            format!("event: message\ndata: {response}\n\n").into_bytes(),
                            &[
                                ("content-type", "text/event-stream".into()),
                                ("mcp-session-id", "fixture-session".into()),
                            ],
                        );
                    }
                }
                normal_http(&request)
            })
        });
        let h = Home::new();
        let (out, rows, _) = run_check(
            &h,
            json!({"demo":{"url":format!("{}/{transport}",server.url),"transport":if transport=="sse"{"sse"}else{"http"},"enabled_tools":[]}}),
            &[],
        );
        assert!(out.status.success(), "{out:?}");
        assert_eq!(
            rows,
            vec![json!({"name":"demo","status":"connected","tools":4})]
        );
        if transport != "sse" {
            assert_eq!(
                *deletes.lock().unwrap(),
                vec![Some("fixture-session".into())]
            );
        }
    }
}
fn http_auth_precedence_and_rotation() {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let capture = seen.clone();
    let server = Server::new(false, move |_| {
        Arc::new(move |r| {
            if r.method == Method::POST {
                capture.lock().unwrap().push(r.authorization.clone());
            }
            normal_http(&r)
        })
    });
    let h = Home::new();
    let credentials = h.0.join(".config/comandos/extensions/credentials.json");
    config::save_json(
        &credentials,
        &json!({"demo":{"url":"https://foreign.invalid/SENTINEL","access_token":"SENTINEL"}}),
    )
    .unwrap();
    let url = format!("{}/mcp", server.url);
    let (out, rows, _) = run_check(&h, json!({"demo":{"url":url}}), &[]);
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(
        rows,
        vec![json!({"name":"demo","status":"failed","errors":["AuthError"]})]
    );
    assert!(seen.lock().unwrap().is_empty());
    assert!(
        !String::from_utf8_lossy(&out.stdout).contains("SENTINEL")
            && !String::from_utf8_lossy(&out.stderr).contains("SENTINEL")
    );
    let (out, rows, _) = run_check(
        &h,
        json!({"demo":{"url":url,"headers":{"authorization":"Bearer static"}}}),
        &[],
    );
    assert!(out.status.success());
    assert_eq!(rows[0]["status"], "connected");
    assert!(
        seen.lock()
            .unwrap()
            .iter()
            .all(|v| v.as_deref() == Some("Bearer static"))
    );
    let h = Home::new();
    let path = h.0.join(".config/comandos/extensions/credentials.json");
    let shared = path.clone();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let deleted = Arc::new(Mutex::new(Vec::new()));
    let (a, b) = (seen.clone(), deleted.clone());
    let server = Server::new(false, move |base| {
        let url = format!("{base}/mcp");
        Arc::new(move |r| {
            if r.method == Method::POST {
                a.lock().unwrap().push(r.authorization.clone());
                if r.authorization.as_deref() == Some("Bearer old") {
                    config::save_json(&shared, &json!({"demo":{"url":url,"access_token":"new"}}))
                        .unwrap();
                    return reply(401, vec![], &[]);
                }
            }
            if r.method == Method::DELETE {
                b.lock().unwrap().push(r.authorization.clone());
            }
            normal_http(&r)
        })
    });
    let url = format!("{}/mcp", server.url);
    config::save_json(&path, &json!({"demo":{"url":url,"access_token":"old"}})).unwrap();
    let (out, rows, _) = run_check(&h, json!({"demo":{"url":url}}), &[]);
    assert!(out.status.success(), "{out:?}");
    assert_eq!(rows[0]["status"], "connected");
    let seen = seen.lock().unwrap();
    assert_eq!(
        &seen[..2],
        &[Some("Bearer old".into()), Some("Bearer new".into())]
    );
    assert_eq!(
        seen.iter()
            .filter(|v| v.as_deref() == Some("Bearer old"))
            .count(),
        1
    );
    assert_eq!(*deleted.lock().unwrap(), vec![Some("Bearer new".into())]);
}
fn trusted_session(home: &Home, spec: Value) -> Session {
    home.catalog(json!({"demo":spec}));
    let mut cmd = command(home, &["serve", "demo"]);
    cmd.env(
        "SSL_CERT_FILE",
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/native-o2/tls/ca.pem"),
    );
    let mut p = Owned(cmd.spawn().unwrap());
    let input = p.0.stdin.take();
    let out = p.0.stdout.take().unwrap();
    let (tx, lines) = mpsc::channel();
    thread::spawn(move || {
        for line in BufReader::new(out).lines().map_while(Result::ok) {
            if tx.send(line).is_err() {
                break;
            }
        }
    });
    Session { p, input, lines }
}
fn tls_oauth_refresh_and_redirect() {
    use base64::{Engine, engine::general_purpose::STANDARD};
    for method in ["client_secret_basic", "client_secret_post"] {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let posts = Arc::new(Mutex::new(Vec::new()));
        let tokens = Arc::new(Mutex::new(Vec::new()));
        let (a, b, c) = (calls.clone(), posts.clone(), tokens.clone());
        let server = Server::new(true, move |base| {
            let token = format!("{base}/token");
            Arc::new(move |r| {
                if r.method == Method::GET {
                    a.lock().unwrap().push(r.path.clone());
                    return if r.path == "/issuer/.well-known/openid-configuration" {
                        json_reply(200, json!({"token_endpoint":token}))
                    } else {
                        reply(404, vec![], &[])
                    };
                }
                if r.method == Method::POST && r.path == "/token" {
                    let pairs: Vec<_> = url::form_urlencoded::parse(&r.body)
                        .map(|(k, v)| (k.into_owned(), v.into_owned()))
                        .collect();
                    b.lock().unwrap().push((pairs, r.authorization));
                    return json_reply(
                        200,
                        json!({"access_token":"new-test-access","expires_in":3600}),
                    );
                }
                if r.method == Method::POST && r.path == "/mcp" {
                    c.lock().unwrap().push(r.authorization.clone());
                    if r.authorization.as_deref() != Some("Bearer new-test-access") {
                        return reply(401, vec![], &[]);
                    }
                }
                normal_http(&r)
            })
        });
        let h = Home::new();
        let path = h.0.join(".config/comandos/extensions/credentials.json");
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        config::save_json(&path,&json!({"demo":{"url":format!("{}/mcp",server.url),"issuer":format!("{}/issuer",server.url),"access_token":"old-test-access","refresh_token":"old-test-refresh","expires_at":now+3600,"client_id":"test-client","client_secret":"test-secret","token_endpoint_auth_method":method},"untouched":{"marker":"keep"}})).unwrap();
        let mut p = trusted_session(&h, json!({"url":format!("{}/mcp",server.url)}));
        p.initialize();
        assert_eq!(
            p.request(
                "tools/call",
                json!({"name":"echo","arguments":{"value":"verified"}})
            )["result"]["structuredContent"],
            json!({"value":"verified"})
        );
        p.close();
        assert_eq!(
            *calls.lock().unwrap(),
            vec![
                "/.well-known/oauth-authorization-server/issuer",
                "/issuer/.well-known/openid-configuration"
            ]
        );
        let posts = posts.lock().unwrap();
        assert_eq!(posts.len(), 1);
        let (form, authorization) = &posts[0];
        let get = |key: &str| {
            form.iter()
                .filter(|(k, _)| k == key)
                .map(|(_, v)| v.as_str())
                .collect::<Vec<_>>()
        };
        assert_eq!(get("grant_type"), vec!["refresh_token"]);
        assert_eq!(get("refresh_token"), vec!["old-test-refresh"]);
        assert_eq!(get("client_id"), vec!["test-client"]);
        if method == "client_secret_basic" {
            assert_eq!(
                authorization.as_deref(),
                Some(format!("Basic {}", STANDARD.encode(b"test-client:test-secret")).as_str())
            );
            assert!(get("client_secret").is_empty());
        } else {
            assert!(authorization.is_none());
            assert_eq!(get("client_secret"), vec!["test-secret"]);
        }
        let data: Value = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
        assert_eq!(data["untouched"], json!({"marker":"keep"}));
        assert_eq!(data["demo"]["access_token"], "new-test-access");
        assert_eq!(data["demo"]["refresh_token"], "old-test-refresh");
        assert_eq!(
            data["demo"]["token_endpoint"],
            format!("{}/token", server.url)
        );
        let tokens = tokens.lock().unwrap();
        assert_eq!(tokens[0].as_deref(), Some("Bearer old-test-access"));
        assert!(
            tokens[1..]
                .iter()
                .all(|v| v.as_deref() == Some("Bearer new-test-access"))
        );
    }
    let unexpected = Arc::new(Mutex::new(Vec::new()));
    let seen = unexpected.clone();
    let server = Server::new(true, move |base| {
        let location = format!("{base}/must-not-follow");
        Arc::new(move |r| {
            if r.method == Method::POST && r.path == "/token" {
                return reply(302, vec![], &[("location", location.clone())]);
            }
            seen.lock().unwrap().push(r.path.clone());
            normal_http(&r)
        })
    });
    let h = Home::new();
    let path = h.0.join(".config/comandos/extensions/credentials.json");
    config::save_json(&path,&json!({"demo":{"url":format!("{}/mcp",server.url),"token_endpoint":format!("{}/token",server.url),"access_token":"old-test-access","refresh_token":"old-test-refresh","expires_at":1,"client_id":"test-client","client_secret":"test-secret"},"untouched":{"marker":"keep"}})).unwrap();
    let before = fs::read(&path).unwrap();
    let mut p = trusted_session(&h, json!({"url":format!("{}/mcp",server.url)}));
    p.send("initialize", json!({}));
    let out = p.p.output();
    assert!(!out.status.success());
    assert!(out.stdout.is_empty());
    assert_eq!(
        String::from_utf8(out.stderr).unwrap().trim(),
        "Refresh failed"
    );
    assert!(unexpected.lock().unwrap().is_empty());
    assert_eq!(fs::read(path).unwrap(), before);
}
fn main() {
    let args: Vec<_> = std::env::args().collect();
    if args.get(1).is_some_and(|s| s == "--fixture") {
        fixture(args.get(2).unwrap());
        return;
    }
    let cases: [(&str, fn()); 17] = [
        (
            "metadata complete filtered list",
            metadata_complete_filtered_list,
        ),
        ("metadata cursor budget", metadata_cursor_budget),
        ("metadata blocked slots", metadata_blocked_slots),
        ("metadata six workers", metadata_many_workers),
        ("metadata proxy rotation", metadata_proxy_rotation),
        ("metadata blocked output", metadata_blocked_output),
        ("check native contracts", check_native_contracts),
        ("check process signals", check_process_contracts),
        ("check 309 native protocol shapes", check_protocol_cases),
        (
            "http status and session transports",
            http_check_status_and_sessions,
        ),
        (
            "http auth precedence and rotation",
            http_auth_precedence_and_rotation,
        ),
        (
            "tls OAuth real refresh and redirect",
            tls_oauth_refresh_and_redirect,
        ),
        (
            "check scheduling and admission",
            check_scheduling_and_admission,
        ),
        ("serve subprocess and limits", serve_subprocess_and_limits),
        (
            "serve protocol frames and opaque JSON",
            serve_protocol_frames,
        ),
        (
            "http serve credentials redirects and events",
            http_serve_boundaries,
        ),
        (
            "serve startup configuration boundaries",
            serve_startup_boundaries,
        ),
    ];
    for (name, run) in cases {
        if args.get(1).is_some_and(|group| !name.starts_with(group)) {
            continue;
        }
        if !cfg!(target_os = "linux")
            && matches!(
                name,
                "metadata blocked slots"
                    | "metadata six workers"
                    | "metadata proxy rotation"
                    | "metadata blocked output"
                    | "check process signals"
            )
        {
            println!("SKIP {name}: Linux /proc or pipe sizing required");
            continue;
        }
        println!("RUN {name}");
        run();
        println!("PASS {name}");
    }
}
fn check_scheduling_and_admission() {
    let h = Home::new();
    let marker = h.0.join("spawned");
    let mut s = spec("normal");
    s["env"] = json!({"MARKER":marker});
    let mut servers = serde_json::Map::new();
    let mut disabled = s.clone();
    disabled["enabled"] = json!(false);
    servers.insert("z".into(), disabled);
    for name in [
        "chrome-bg",
        "claude-in-chrome",
        "playwright",
        "x-playwright",
        "lightpanda",
        "obscura",
        "screenwright",
        "teams",
    ] {
        servers.insert(name.into(), s.clone());
    }
    let (out, rows, saved) = run_check(&h, Value::Object(servers.clone()), &[]);
    assert!(out.status.success());
    let mut names: Vec<_> = servers.keys().collect();
    names.sort();
    let expected: Vec<Value> = names
        .into_iter()
        .map(|n| {
            if n == "z" {
                json!({"name":n,"status":"disabled"})
            } else {
                json!({"name":n,"status":"not_probed","reason":"interactive or browser runtime"})
            }
        })
        .collect();
    assert_eq!(rows, expected);
    assert_eq!(saved, Some(json!(expected)));
    assert!(!marker.exists());
    let (out, rows, _) = run_check(&h, json!({"spawn":s.clone()}), &["spawn", "missing"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(rows.is_empty() && !marker.exists());
    config::save_json(
        &h.0.join(".config/comandos/extensions/catalog.json"),
        &json!({"version":2,"servers":{"demo":s}}),
    )
    .unwrap();
    let out = Owned(command(&h, &["check"]).spawn().unwrap()).output();
    assert_eq!(out.status.code(), Some(1));
    assert!(out.stdout.is_empty() && !marker.exists());
    let h = Home::new();
    let events = h.0.join("events");
    let mut servers = serde_json::Map::new();
    for i in 0..6 {
        let mut s = spec("normal");
        s["env"] = json!({"DELAY":if i<4{".35"}else{".01"},"EVENTS":events,"LABEL":i.to_string()});
        servers.insert(i.to_string(), s);
    }
    let names = ["0", "1", "2", "3", "4", "5", "0"];
    let (out, rows, saved) = run_check(&h, Value::Object(servers), &names);
    assert!(out.status.success() && rows.len() == 7);
    assert_eq!(
        saved
            .unwrap()
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v["name"].as_str().unwrap())
            .collect::<Vec<_>>(),
        names
    );
    let mut active = 0;
    let mut peak = 0;
    for line in fs::read_to_string(events).unwrap().lines() {
        let row: Value = serde_json::from_str(line).unwrap();
        active += if row[0] == "start" { 1 } else { -1 };
        peak = peak.max(active);
    }
    assert_eq!((peak, active), (4, 0));
    let mut slow = spec("normal");
    slow["env"] = json!({"DELAY":".3"});
    let (out, rows, saved) = run_check(
        &h,
        json!({"slow":slow,"fast":spec("normal")}),
        &["slow", "fast", "fast"],
    );
    assert!(out.status.success());
    assert_eq!(
        rows.iter()
            .map(|v| v["name"].as_str().unwrap())
            .collect::<Vec<_>>(),
        vec!["fast", "fast", "slow"]
    );
    assert_eq!(
        saved
            .unwrap()
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v["name"].as_str().unwrap())
            .collect::<Vec<_>>(),
        vec!["slow", "fast", "fast"]
    );
    let servers: serde_json::Map<_, _> = (0..200)
        .map(|i| (i.to_string(), json!({"enabled":false})))
        .collect();
    let (out, rows, saved) = run_check(&h, Value::Object(servers), &[]);
    assert!(out.status.success());
    assert_eq!(rows.len(), 200);
    assert_eq!(saved.unwrap().as_array().unwrap().len(), 200);
}
fn send_id(p: &mut Session, method: &str, params: Value, id: Value) {
    writeln!(
        p.input.as_mut().unwrap(),
        "{}",
        json!({"jsonrpc":"2.0","id":id,"method":method,"params":params})
    )
    .unwrap();
}
fn serve_protocol_frames() {
    let invalid = Home::new();
    let mut rejected = Session::new(&invalid, spec("invalid-resources"));
    rejected.initialize();
    assert_eq!(
        rejected.request("resources/list", json!({"cursor":"opaque"})),
        json!({"jsonrpc":"2.0","id":1,"error":{"code":-32603,"message":"Upstream request failed for demo: ValidationError"}})
    );
    rejected.close();
    let h = Home::new();
    let mut p = Session::new(&h, spec("metadata"));
    let initial=p.request("initialize",json!({"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"test","version":"1"}}))["result"].clone();
    assert_eq!(initial["instructions"], "Keep original instructions");
    assert_eq!(
        initial["capabilities"],
        json!({"tools":{},"resources":{},"prompts":{},"completions":{}})
    );
    let first = p.request("tools/list", json!({}))["result"].clone();
    assert_eq!(
        first["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v["name"].as_str().unwrap())
            .collect::<Vec<_>>(),
        vec!["echo"]
    );
    assert_eq!(first["nextCursor"], "page2");
    assert_eq!(first["futureResult"], 42);
    assert_eq!(first["tools"][0]["futureField"], 3);
    assert!(
        p.request("tools/list", json!({"cursor":"page2"}))["result"]
            .get("nextCursor")
            .is_none()
    );
    assert_eq!(
        p.request("tools/call", json!({"name":"blocked"}))["error"]["code"],
        -32601
    );
    let r = p.request("tools/call", json!({"name":"echo","arguments":{"value":7}}));
    assert_eq!(r["result"]["structuredContent"], json!({"value":7}));
    assert_eq!(r["result"]["future"], 7);
    assert_eq!(
        p.request(
            "tools/call",
            json!({"name":"echo","arguments":{"fail":true}})
        )["error"],
        json!({"code":-32099,"message":"Original error","data":{"extra":4}})
    );
    for method in [
        "resources/list",
        "resources/templates/list",
        "resources/read",
        "prompts/list",
        "prompts/get",
        "completion/complete",
    ] {
        let response = p.request(method, json!({"cursor":"opaque"}));
        assert_eq!(
            response["result"]["params"],
            json!({"cursor":"opaque"}),
            "{method}: {response}"
        );
    }
    assert_eq!(
        p.request("resources/subscribe", json!({}))["error"]["code"],
        -32601
    );
    send_id(
        &mut p,
        "tools/list",
        json!({"cursor":"error-page"}),
        json!("list-error"),
    );
    assert_eq!(
        p.recv(),
        json!({"jsonrpc":"2.0","id":"list-error","error":{"code":-32099,"message":"Original list error","data":{"cursor":"error-page","extra":[1,2]}},"futureEnvelope":{"keep":true}})
    );
    send_id(
        &mut p,
        "tools/call",
        json!({"name":"echo","arguments":{"value":"slow","delay":0.2}}),
        json!("slow"),
    );
    send_id(
        &mut p,
        "tools/call",
        json!({"name":"echo","arguments":{"value":"fast"}}),
        json!(99),
    );
    let rows = [p.recv(), p.recv()];
    assert!(
        rows.iter()
            .any(|v| v["id"] == "slow" && v["result"]["structuredContent"]["value"] == "slow")
    );
    assert!(
        rows.iter()
            .any(|v| v["id"] == 99 && v["result"]["structuredContent"]["value"] == "fast")
    );
    let payload=comandos_core::json::parse_value(r#"{"integer":115792089237316195423570985008687907853269984665640564039457584007913129639943,"negative":-680564733841876926926749214863536422915,"opaque":{"$serde_json::private::Number":"literal-number-key","$serde_json::private::RawValue":"[1,2]"},"fraction":-0.0}"#).unwrap();
    let actual=p.request("tools/call",json!({"name":"echo","arguments":payload}))["result"]["structuredContent"].clone();
    assert_eq!(actual, payload);
    assert_eq!(
        actual["integer"].as_number().unwrap().as_str(),
        payload["integer"].as_number().unwrap().as_str()
    );
    assert!(actual["fraction"].as_f64().unwrap().is_sign_negative());
    p.close();
    let mut s = spec("metadata");
    s["enabled_tools"] = json!([]);
    let mut p = Session::new(&h, s);
    p.initialize();
    assert_eq!(
        p.request("tools/list", json!({}))["result"]["tools"],
        json!([])
    );
    assert_eq!(
        p.request("tools/call", json!({"name":"echo"}))["error"]["code"],
        -32601
    );
    p.close();
}
fn serve_subprocess_and_limits() {
    let h = Home::new();
    let s = json!({"command":std::env::current_exe().unwrap(),"args":["--fixture","exec","$(echo leaked)","a b","$SOURCE"],"env":{"EXPANDED":"${SOURCE}/$MISSING"},"cwd":h.0});
    h.catalog(json!({"demo":s}));
    let mut cmd = command(&h, &["serve", "demo"]);
    cmd.env("SOURCE", "value");
    let p = Owned(cmd.spawn().unwrap());
    let pid = p.0.id();
    let out = p.output();
    assert_eq!(out.status.code(), Some(17));
    assert_eq!(
        serde_json::from_slice::<Value>(&out.stdout).unwrap(),
        json!([
            pid,
            ["$(echo leaked)", "a b", "$SOURCE"],
            "value/$MISSING",
            h.0
        ])
    );
    assert!(out.stderr.is_empty());
    for s in [
        json!({"enabled":false,"url":"http://secret.invalid/token"}),
        json!({"command":"/missing/private-token"}),
    ] {
        h.catalog(json!({"demo":s}));
        let out = Owned(command(&h, &["serve", "demo"]).spawn().unwrap()).output();
        assert!(!out.status.success() && out.stdout.is_empty());
        assert!(!String::from_utf8_lossy(&out.stderr).contains("secret"));
    }
    for signal in [false, true] {
        let marker = h.0.join(format!("child-{signal}"));
        let mut s = spec("metadata");
        s["enabled_tools"] = json!([]);
        s["env"] = json!({"MARKER":marker});
        let mut p = Session::new(&h, s);
        p.initialize();
        let child: i32 = fs::read_to_string(marker).unwrap().parse().unwrap();
        if signal {
            kill(Pid::from_raw(p.p.0.id() as i32), Signal::SIGTERM).unwrap();
        } else {
            drop(p.input.take());
        }
        p.p.wait();
        assert_eq!(
            kill(Pid::from_raw(child), None),
            Err(nix::errno::Errno::ESRCH)
        );
    }
    let mut s = spec("crash");
    s["enabled_tools"] = json!([]);
    let p = Session::new(&h, s);
    let out = p.p.output();
    assert!(!out.status.success() && out.stdout.is_empty());
    let mut s = spec("metadata");
    s["enabled_tools"] = json!([]);
    let mut p = Session::new(&h, s);
    p.initialize();
    let _ = p
        .input
        .as_mut()
        .unwrap()
        .write_all(&vec![b'x'; 8 * 1024 * 1024 + 1]);
    p.p.wait();
    let marker = h.0.join("cancelled");
    let mut s = spec("cancel");
    s["env"] = json!({"CANCELLED":marker});
    let mut p = Session::new(&h, s);
    p.initialize();
    send_id(
        &mut p,
        "tools/call",
        json!({"name":"echo"}),
        json!("cancel-me"),
    );
    thread::sleep(Duration::from_millis(100));
    writeln!(p.input.as_mut().unwrap(),"{}",json!({"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":"cancel-me"}})).unwrap();
    until(|| marker.exists());
    assert!(
        fs::read_to_string(marker)
            .unwrap()
            .chars()
            .all(|c| c.is_ascii_digit())
    );
    p.close();
    let marker = h.0.join("saturated");
    let mut s = spec("hold");
    s["env"] = json!({"CALL_COUNT":marker});
    let mut p = Session::new(&h, s);
    p.initialize();
    for i in 0..65 {
        send_id(&mut p, "tools/call", json!({"name":"echo"}), json!(i + 10));
    }
    let r = p.recv();
    assert_eq!(r["error"]["code"], -32000);
    assert_eq!(r["id"], 74);
    until(|| fs::read_to_string(&marker).ok().as_deref() == Some("64"));
    writeln!(
        p.input.as_mut().unwrap(),
        "{}",
        json!({"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":10}})
    )
    .unwrap();
    send_id(
        &mut p,
        "tools/call",
        json!({"name":"echo","arguments":{"replacement":true}}),
        json!(75),
    );
    let r = p.recv();
    assert_eq!(r["id"], 75);
    assert_eq!(
        r["result"]["structuredContent"],
        json!({"replacement":true})
    );
    assert_eq!(fs::read_to_string(marker).unwrap(), "65");
    p.close();
}
fn serve_startup_boundaries() {
    let h = Home::new();
    for raw in [
        b"".as_slice(),
        b"{bad private-secret",
        br#"{"version":1,"version":1,"servers":{}}"#,
        br#"{"version":2}"#,
    ] {
        let path = h.0.join("catalog.json");
        fs::write(&path, raw).unwrap();
        let out = Owned(
            command(&h, &["--catalog", path.to_str().unwrap(), "serve", "demo"])
                .spawn()
                .unwrap(),
        )
        .output();
        assert!(!out.status.success() && out.stdout.is_empty());
        assert!(!String::from_utf8_lossy(&out.stderr).contains("private-secret"));
    }
    let out = Owned(command(&h, &["unknown"]).spawn().unwrap()).output();
    assert!(!out.status.success() && out.stdout.is_empty());
    let exe = serde_json::to_string(&std::env::current_exe().unwrap()).unwrap();
    let path = h.0.join(".config/comandos/extensions/catalog.json");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path,format!(r#"{{"version":1,"servers":{{"demo":{{"command":{exe},"args":["--fixture","ready"]}},"unrelated":{{"enabled":false,"env":{{"value":NaN}}}}}}}}"#)).unwrap();
    let out = Owned(command(&h, &["serve", "demo"]).spawn().unwrap()).output();
    assert!(out.status.success() && out.stderr.is_empty());
    assert_eq!(out.stdout, b"ready\n");
    fs::write(&path,format!(r#"{{"version":1,"servers":{{"demo":{{"command":{exe},"args":["--fixture","nonfinite-env"],"env":{{"NAN":NaN,"POS":Infinity,"NEG":-Infinity}}}}}}}}"#)).unwrap();
    let out = Owned(command(&h, &["serve", "demo"]).spawn().unwrap()).output();
    assert!(out.status.success() && out.stderr.is_empty());
    assert_eq!(
        serde_json::from_slice::<Value>(&out.stdout).unwrap(),
        json!(["nan", "inf", "-inf"])
    );
}
fn http_serve_boundaries() {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let capture = seen.clone();
    let server = Server::new(false, move |_| {
        Arc::new(move |r| {
            if r.method == Method::POST {
                capture.lock().unwrap().push(r.authorization.clone());
            }
            normal_http(&r)
        })
    });
    let h = Home::new();
    let credentials = h.0.join(".config/comandos/extensions/credentials.json");
    config::save_json(
        &credentials,
        &json!({"demo":{"url":"https://different.invalid/secret","access_token":"never-send"}}),
    )
    .unwrap();
    let mut p = Session::with_env(
        &h,
        json!({"url":format!("{}/mcp",server.url),"headers":{"authorization":"Bearer ${STATIC}"},"bearer_token_env_var":"BEARER","env_http_headers":{"Authorization":"FINAL"}}),
        &[
            ("STATIC", "one"),
            ("BEARER", "two"),
            ("FINAL", "Bearer three"),
        ],
    );
    p.initialize();
    p.close();
    let seen = seen.lock().unwrap();
    assert!(!seen.is_empty() && seen.iter().all(|v| v.as_deref() == Some("Bearer three")));
    drop(seen);
    let h = Home::new();
    let path = h.0.join(".config/comandos/extensions/credentials.json");
    let shared = path.clone();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let capture = seen.clone();
    let server = Server::new(false, move |base| {
        let url = format!("{base}/mcp");
        Arc::new(move |r| {
            if r.method == Method::POST {
                capture.lock().unwrap().push(r.authorization.clone());
                if r.authorization.as_deref() == Some("Bearer old") {
                    config::save_json(&shared, &json!({"demo":{"url":url,"access_token":"new"}}))
                        .unwrap();
                    return reply(401, vec![], &[]);
                }
            }
            normal_http(&r)
        })
    });
    config::save_json(
        &path,
        &json!({"demo":{"url":format!("{}/mcp",server.url),"access_token":"old"}}),
    )
    .unwrap();
    let mut p = Session::new(&h, json!({"url":format!("{}/mcp",server.url)}));
    p.initialize();
    p.close();
    let seen = seen.lock().unwrap();
    assert_eq!(
        &seen[..2],
        &[Some("Bearer old".into()), Some("Bearer new".into())]
    );
    assert_eq!(
        seen.iter()
            .filter(|v| v.as_deref() == Some("Bearer old"))
            .count(),
        1
    );
    drop(seen);
    let leaked = Arc::new(Mutex::new(Vec::new()));
    let capture = leaked.clone();
    let target = Server::new(false, move |_| {
        Arc::new(move |r| {
            capture.lock().unwrap().push(r.authorization.clone());
            normal_http(&r)
        })
    });
    let dest = format!("{}/mcp", target.url);
    let redirect = Server::new(false, move |_| {
        Arc::new(move |_| reply(307, vec![], &[("location", dest.clone())]))
    });
    let h = Home::new();
    let p = Session::new(
        &h,
        json!({"url":format!("{}/mcp",redirect.url),"headers":{"Authorization":"Bearer private-test"}}),
    );
    let out = p.p.output();
    assert!(!out.status.success() && out.stdout.is_empty());
    assert!(leaked.lock().unwrap().is_empty());
    assert!(!String::from_utf8_lossy(&out.stderr).contains("private-test"));
    let dest = format!("{}/messages", target.url);
    let endpoint = Server::new(false, move |_| {
        Arc::new(move |_| {
            reply(
                200,
                format!("event: endpoint\ndata: {dest}\n\n").into_bytes(),
                &[("content-type", "text/event-stream".into())],
            )
        })
    });
    let h = Home::new();
    let url = format!("{}/sse", endpoint.url);
    config::save_json(
        &h.0.join(".config/comandos/extensions/credentials.json"),
        &json!({"demo":{"url":url,"access_token":"secret"}}),
    )
    .unwrap();
    let p = Session::new(&h, json!({"url":url,"transport":"sse"}));
    let out = p.p.output();
    assert!(!out.status.success());
    assert!(leaked.lock().unwrap().is_empty());
    assert!(!String::from_utf8_lossy(&out.stderr).contains("secret"));
    let server = Server::new(false, move |_| {
        Arc::new(move |r| {
            if r.method == Method::POST {
                let m = r.json();
                if m.get("id").is_some() && m["method"] == "tools/list" {
                    let mut result = normal_result(&m);
                    result["secret-body"] = json!("x".repeat(8 * 1024 * 1024));
                    return json_reply(200, json!({"jsonrpc":"2.0","id":m["id"],"result":result}));
                }
            }
            normal_http(&r)
        })
    });
    let h = Home::new();
    let mut p = Session::new(&h, json!({"url":format!("{}/mcp",server.url)}));
    p.initialize();
    let response = p.request("tools/list", json!({}));
    assert!(response.get("error").is_none());
    assert_eq!(
        response["result"]["secret-body"].as_str().unwrap().len(),
        8 * 1024 * 1024
    );
    p.close();
    let server = Server::new(false, move |_| {
        Arc::new(move |r| {
            if r.method == Method::POST {
                let m = r.json();
                if m.get("id").is_none() {
                    return reply(202, vec![], &[]);
                }
                let value = json!({"jsonrpc":"2.0","id":m["id"],"result":normal_result(&m)});
                let data = serde_json::to_string_pretty(&value).unwrap();
                let lines = data
                    .lines()
                    .map(|line| format!("data: {line}\r"))
                    .collect::<String>();
                return reply(
                    200,
                    format!("\u{feff}: comment\r{lines}\r").into_bytes(),
                    &[("content-type", "text/event-stream".into())],
                );
            }
            normal_http(&r)
        })
    });
    let h = Home::new();
    let mut p = Session::new(&h, json!({"url":format!("{}/mcp",server.url)}));
    p.initialize();
    p.close();
    for transport in ["stream", "sse"] {
        let replies = Arc::new(Mutex::new(Vec::new()));
        let capture = replies.clone();
        let events = Arc::new(Mutex::new(
            None::<tokio::sync::mpsc::UnboundedSender<Bytes>>,
        ));
        let server = Server::new(false, move |_| {
            Arc::new(move |r| {
                if r.method == Method::GET && r.path == "/sse" {
                    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
                    tx.send(Bytes::from_static(
                        b"event: endpoint\r\ndata: /messages?session=fixture\r\n\r\n",
                    ))
                    .unwrap();
                    *events.lock().unwrap() = Some(tx);
                    return stream_reply(rx);
                }
                if r.method == Method::POST {
                    let m = r.json();
                    if m.get("method").is_none() {
                        capture.lock().unwrap().push(m);
                        return reply(202, vec![], &[]);
                    }
                    if m.get("id").is_none() {
                        return reply(202, vec![], &[]);
                    }
                    let mut values = vec![];
                    if m["method"] == "tools/list" {
                        values.push(json!({"jsonrpc":"2.0","id":"up-ping","method":"ping"}));
                        values.push(json!({"jsonrpc":"2.0","id":"up-sampling","method":"sampling/createMessage","params":{}}));
                    }
                    values.push(json!({"jsonrpc":"2.0","id":m["id"],"result":normal_result(&m)}));
                    if r.path.starts_with("/messages") {
                        for value in values {
                            events
                                .lock()
                                .unwrap()
                                .as_ref()
                                .unwrap()
                                .send(Bytes::from(format!("event: message\ndata: {value}\n\n")))
                                .unwrap();
                        }
                        return reply(202, vec![], &[]);
                    }
                    let body = values
                        .into_iter()
                        .map(|v| format!("event: message\ndata: {v}\n\n"))
                        .collect::<String>();
                    return reply(
                        200,
                        body.into_bytes(),
                        &[("content-type", "text/event-stream".into())],
                    );
                }
                normal_http(&r)
            })
        });
        let h = Home::new();
        let mut p = Session::new(
            &h,
            json!({"url":format!("{}/{}",server.url,if transport=="sse"{"sse"}else{"mcp"}),"transport":if transport=="sse"{"sse"}else{"http"}}),
        );
        p.initialize();
        assert!(
            !p.request("tools/list", json!({}))["result"]["tools"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        until(|| replies.lock().unwrap().len() == 2);
        let replies = replies.lock().unwrap();
        assert!(
            replies
                .iter()
                .any(|v| v["id"] == "up-ping" && v["result"] == json!({}))
        );
        assert!(replies.iter().any(|v| v["id"] == "up-sampling"
            && v["error"] == json!({"code":-32601,"message":"Unsupported upstream request"})));
        drop(replies);
        p.close();
    }
}
