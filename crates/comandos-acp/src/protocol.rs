//! Bounded JSON-lines transport, owned child cleanup, and JSON-RPC request identity.
use super::Result;
use nix::{
    sys::signal::{Signal, killpg},
    unistd::Pid,
};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, VecDeque},
    fs,
    io::{BufRead, BufReader, Read, Write},
    os::{
        fd::AsFd,
        unix::{fs::OpenOptionsExt, process::CommandExt},
    },
    path::{Path, PathBuf},
    process::{Child, ChildStdin, Command, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver},
    },
    thread,
    time::{Duration, Instant},
};
pub const MAX_LINE: usize = 4 * 1024 * 1024;
const QUEUE: usize = 8;
#[derive(Clone)]
pub struct Timeouts {
    pub initialize: Duration,
    pub new_session: Duration,
    pub load: Duration,
    pub config: Duration,
    pub prompt: Duration,
    pub write: Duration,
    pub cancellation: Duration,
}
impl Default for Timeouts {
    fn default() -> Self {
        Self {
            initialize: Duration::from_secs(60),
            new_session: Duration::from_secs(90),
            load: Duration::from_secs(120),
            config: Duration::from_secs(30),
            prompt: Duration::from_secs(3600),
            write: Duration::from_secs(5),
            cancellation: Duration::from_secs(5),
        }
    }
}
pub trait Events {
    fn event(&mut self, event: Value) -> Result<()>;
    fn permission(&mut self, request: &Value) -> Result<Permission>;
}
pub enum Permission {
    Default,
    Selected(String),
    Cancelled,
}
struct MetadataEvents<'a, E>(&'a mut E);
impl<E: Events> Events for MetadataEvents<'_, E> {
    fn event(&mut self, _: Value) -> Result<()> {
        Ok(())
    }
    fn permission(&mut self, request: &Value) -> Result<Permission> {
        self.0.permission(request)
    }
}
pub struct Quiet;
impl Events for Quiet {
    fn event(&mut self, _: Value) -> Result<()> {
        Ok(())
    }
    fn permission(&mut self, _: &Value) -> Result<Permission> {
        Ok(Permission::Cancelled)
    }
}
enum Line {
    Json(Value),
    Eof,
    Error(String),
}
pub(crate) struct OwnedChild(pub(crate) Option<Child>);
impl OwnedChild {
    pub(crate) fn alive(&self) -> bool {
        let Some(child) = self.0.as_ref() else {
            return false;
        };
        // Observe without reaping: the leader PID keeps this private group
        // reserved until close has signalled its remaining descendants.
        matches!(
            comandos_runtime::procs::child_exited_unreaped(child),
            Ok(false)
        )
    }
    fn close(&mut self) {
        let Some(mut child) = self.0.take() else {
            return;
        };
        drop(child.stdin.take());
        if let Ok(pid) = i32::try_from(child.id())
            && pid > 1
        {
            let group = Pid::from_raw(pid);
            let _ = killpg(group, Signal::SIGTERM);
            let deadline = Instant::now() + Duration::from_millis(100);
            while Instant::now() < deadline {
                if !matches!(
                    comandos_runtime::procs::child_exited_unreaped(&child),
                    Ok(false)
                ) {
                    break;
                }
                thread::sleep(Duration::from_millis(2));
            }
            // Never reap before this signal: even an exited leader still
            // protects the group identity while TERM-resistant children close.
            let _ = killpg(group, Signal::SIGKILL);
        } else {
            let _ = child.kill();
        }
        let _ = child.wait();
    }
}
impl Drop for OwnedChild {
    fn drop(&mut self) {
        self.close();
    }
}
struct Process {
    child: OwnedChild,
    stdin: Option<ChildStdin>,
    lines: Receiver<Line>,
    logs: Arc<Mutex<VecDeque<String>>>,
    write_timeout: Duration,
}
fn push_log(logs: &Mutex<VecDeque<String>>, line: &str) {
    if let Ok(mut logs) = logs.lock() {
        logs.push_back(line.chars().take(8192).collect());
        while logs.len() > 200 {
            logs.pop_front();
        }
    }
}
impl Process {
    fn open(
        argv: &[String],
        env: &BTreeMap<String, String>,
        cwd: &Path,
        write_timeout: Duration,
    ) -> Result<Self> {
        let mut child = OwnedChild(Some(
            Command::new(&argv[0])
                .args(&argv[1..])
                .envs(env)
                .current_dir(cwd)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .process_group(0)
                .spawn()
                .map_err(|e| e.to_string())?,
        ));
        let handle = child.0.as_mut().expect("new owned child");
        let stdin = handle.stdin.take().ok_or("agente sin stdin")?;
        let flags = nix::fcntl::fcntl(stdin.as_fd(), nix::fcntl::FcntlArg::F_GETFL)
            .map_err(|e| e.to_string())?;
        nix::fcntl::fcntl(
            stdin.as_fd(),
            nix::fcntl::FcntlArg::F_SETFL(
                nix::fcntl::OFlag::from_bits_truncate(flags) | nix::fcntl::OFlag::O_NONBLOCK,
            ),
        )
        .map_err(|e| e.to_string())?;
        let stdout = handle.stdout.take().ok_or("agente sin stdout")?;
        let stderr = handle.stderr.take().ok_or("agente sin stderr")?;
        let (tx, lines) = mpsc::sync_channel(QUEUE);
        let logs = Arc::new(Mutex::new(VecDeque::new()));
        let log = logs.clone();
        thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            loop {
                let mut line = vec![];
                let result = reader
                    .by_ref()
                    .take((MAX_LINE + 1) as u64)
                    .read_until(b'\n', &mut line);
                let value = match result {
                    Ok(0) => Line::Eof,
                    Ok(_) if line.len() > MAX_LINE => Line::Error("línea ACP excede 4 MiB".into()),
                    Err(e) => Line::Error(e.to_string()),
                    Ok(_) => {
                        let Ok(text) = std::str::from_utf8(&line) else {
                            let _ = tx.send(Line::Error("salida ACP no es UTF-8".into()));
                            break;
                        };
                        let text = text.trim();
                        if text.is_empty() {
                            continue;
                        }
                        match serde_json::from_str(text) {
                            Ok(v) => Line::Json(v),
                            Err(_) => {
                                push_log(&log, text);
                                continue;
                            }
                        }
                    }
                };
                let terminal = matches!(value, Line::Eof | Line::Error(_));
                if tx.send(value).is_err() || terminal {
                    break;
                }
            }
        });
        let log = logs.clone();
        thread::spawn(move || {
            let mut reader = BufReader::new(stderr);
            loop {
                let mut line = vec![];
                match reader.by_ref().take(8193).read_until(b'\n', &mut line) {
                    Ok(0) | Err(_) => break,
                    Ok(_) => push_log(&log, String::from_utf8_lossy(&line).trim_end()),
                }
            }
        });
        Ok(Self {
            child,
            stdin: Some(stdin),
            lines,
            logs,
            write_timeout,
        })
    }
    fn detail(&self) -> String {
        self.logs
            .lock()
            .map(|l| {
                l.iter()
                    .rev()
                    .take(3)
                    .collect::<Vec<_>>()
                    .into_iter()
                    .rev()
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(" | ")
            })
            .unwrap_or_default()
    }
    fn write(&mut self, value: &Value) -> Result<()> {
        let mut body = serde_json::to_vec(value).map_err(|e| e.to_string())?;
        if body.len() > MAX_LINE {
            return Err("mensaje ACP excede 4 MiB".into());
        }
        body.push(b'\n');
        let deadline = Instant::now() + self.write_timeout;
        let mut at = 0;
        while at < body.len() {
            if Instant::now() >= deadline {
                self.close();
                return Err("timeout escribiendo al agente".into());
            }
            let stdin = self.stdin.as_mut().ok_or("el agente cerró stdin")?;
            match stdin.write(&body[at..]) {
                Ok(0) => return Err(format!("el agente cerró la conexión: {}", self.detail())),
                Ok(n) => at += n,
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(2))
                }
                Err(_) => return Err(format!("el agente cerró la conexión: {}", self.detail())),
            }
        }
        Ok(())
    }
    fn receive(&mut self, timeout: Duration) -> Result<Option<Value>> {
        match self.lines.recv_timeout(timeout) {
            Ok(Line::Json(v)) => Ok(Some(v)),
            Ok(Line::Eof) | Err(mpsc::RecvTimeoutError::Disconnected) => {
                self.close();
                Err(format!("el agente cerró stdout: {}", self.detail()))
            }
            Ok(Line::Error(e)) => {
                self.close();
                Err(e)
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if !self.alive() {
                    Err(format!("el agente murió: {}", self.detail()))
                } else {
                    Ok(None)
                }
            }
        }
    }
    fn alive(&mut self) -> bool {
        self.child.alive()
    }
    fn close(&mut self) {
        drop(self.stdin.take());
        self.child.close();
    }
}
impl Drop for Process {
    fn drop(&mut self) {
        self.close();
    }
}

pub struct Session {
    proc: Process,
    pub cwd: PathBuf,
    pub session_id: String,
    pub capabilities: Value,
    pub models: Vec<Value>,
    pub current_model: String,
    pub current_effort: String,
    pub config_options: Vec<Value>,
    pub modes: Vec<Value>,
    pub current_mode: String,
    pub commands: Vec<Value>,
    pub agy: bool,
    id: u64,
    timeouts: Timeouts,
    pub cancel: Arc<AtomicBool>,
}
pub fn string(v: &Value, key: &str) -> String {
    v[key].as_str().unwrap_or("").into()
}
fn array(v: &Value, key: &str) -> Vec<Value> {
    v[key].as_array().cloned().unwrap_or_default()
}
impl Session {
    pub fn open(
        argv: &[String],
        env: &BTreeMap<String, String>,
        cwd: &Path,
        agy: bool,
        timeouts: Timeouts,
        cancel: Arc<AtomicBool>,
    ) -> Result<Self> {
        if cwd.to_str().is_none() {
            return Err("cwd no es UTF-8".into());
        }
        let proc = Process::open(argv, env, cwd, timeouts.write)?;
        Ok(Self {
            proc,
            cwd: cwd.into(),
            session_id: String::new(),
            capabilities: json!({}),
            models: vec![],
            current_model: String::new(),
            current_effort: String::new(),
            config_options: vec![],
            modes: vec![],
            current_mode: String::new(),
            commands: vec![],
            agy,
            id: 0,
            timeouts,
            cancel,
        })
    }
    fn request(
        &mut self,
        method: &str,
        params: Value,
        timeout: Duration,
        events: &mut impl Events,
    ) -> Result<Value> {
        let visible = method == "session/prompt";
        self.id = self.id.checked_add(1).ok_or("ids ACP agotados")?;
        let id = self.id;
        self.proc
            .write(&json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}))?;
        let mut deadline = Instant::now() + timeout;
        let mut cancelling = false;
        loop {
            if self.cancel.swap(false, Ordering::Relaxed) {
                if self.session_id.is_empty() {
                    return Err("cancelado".into());
                }
                self.cancel_turn()?;
                events.event(json!({"type":"cancel"}))?;
                cancelling = true;
                deadline = deadline.min(Instant::now() + self.timeouts.cancellation);
            }
            let now = Instant::now();
            if now >= deadline {
                self.proc.close();
                return Err(if cancelling {
                    "timeout cancelando turno".into()
                } else {
                    format!(
                        "timeout esperando respuesta ({:.0}s)",
                        timeout.as_secs_f64()
                    )
                });
            }
            let Some(msg) = self
                .proc
                .receive((deadline - now).min(Duration::from_millis(25)))?
            else {
                continue;
            };
            if !msg.is_object() || msg["jsonrpc"] != "2.0" {
                self.proc.close();
                return Err("mensaje JSON-RPC ACP inválido".into());
            }
            if let Some(method) = msg["method"].as_str() {
                let method = method.to_owned();
                let incoming = if visible {
                    self.incoming(&method, &msg, events)
                } else {
                    self.incoming(&method, &msg, &mut MetadataEvents(events))
                };
                if let Err(e) = incoming {
                    self.proc.close();
                    return Err(e);
                }
                continue;
            }
            if msg["id"] != json!(id) {
                continue;
            }
            if let Some(error) = msg.get("error") {
                let mut text = error["message"]
                    .as_str()
                    .unwrap_or("error del agente")
                    .to_owned();
                if let Some(detail) = error["data"]["message"].as_str() {
                    text.push_str(": ");
                    text.extend(detail.chars().take(400));
                }
                return Err(text);
            }
            let result = msg
                .get("result")
                .filter(|v| v.is_object())
                .cloned()
                .ok_or_else(|| "respuesta JSON-RPC sin resultado objeto".into());
            if result.is_err() {
                self.proc.close();
            }
            return result;
        }
    }
    fn response(&mut self, id: &Value, result: Result<Value>) -> Result<()> {
        let value = match result {
            Ok(v) => json!({"jsonrpc":"2.0","id":id,"result":v}),
            Err(e) => json!({"jsonrpc":"2.0","id":id,"error":{"code":-32603,"message":e}}),
        };
        self.proc.write(&value)
    }
    fn incoming(&mut self, method: &str, msg: &Value, events: &mut impl Events) -> Result<()> {
        let params = &msg["params"];
        let rid = msg.get("id");
        if !params.is_object() {
            return Err("parámetros JSON-RPC ACP inválidos".into());
        }
        let wrong_session = !self.session_id.is_empty()
            && params["sessionId"]
                .as_str()
                .is_some_and(|id| id != self.session_id);
        if method == "session/update" {
            if !wrong_session {
                self.update(&params["update"], events)?;
            }
            return Ok(());
        }
        let Some(rid) = rid else { return Ok(()) };
        if wrong_session {
            return self.response(rid, Err("solicitud de otra conversación".into()));
        }
        match method{
   "session/request_permission"=>{let options=array(params,"options");let choice=events.permission(params)?;let decision=match choice{Permission::Selected(s)=>options.iter().find(|v|v["optionId"]==s).map(|_|s),Permission::Cancelled=>None,Permission::Default=>options.iter().find(|v|v["kind"].as_str().is_some_and(|s|s.contains("allow"))).or_else(||options.first()).and_then(|v|v["optionId"].as_str()).map(str::to_owned)};let outcome=decision.as_ref().map_or_else(||json!({"outcome":"cancelled"}),|id|json!({"outcome":"selected","optionId":id}));self.response(rid,Ok(json!({"outcome":outcome})))?;events.event(json!({"type":"permission","decision":decision}))?;}
   "fs/read_text_file"=>{let result=self.fs_read(params);self.response(rid,result)?;}
   "fs/write_text_file"=>{let result=self.fs_write(params);self.response(rid,result)?;}
   _=>self.proc.write(&json!({"jsonrpc":"2.0","id":rid,"error":{"code":-32601,"message":format!("{method} no soportado por cc-acp")}}))?,
  }
        Ok(())
    }
    fn file_path(&self, params: &Value) -> Result<PathBuf> {
        let path = Path::new(params["path"].as_str().ok_or("ruta de archivo ausente")?);
        if !path.is_absolute() {
            return Err("ACP requiere una ruta de archivo absoluta".into());
        }
        Ok(path.into())
    }
    fn fs_read(&self, params: &Value) -> Result<Value> {
        let path = self.file_path(params)?;
        let mut bytes = vec![];
        regular_file(&path, false)?
            .take((MAX_LINE + 1) as u64)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        if bytes.len() > MAX_LINE {
            return Err("archivo ACP excede 4 MiB".into());
        }
        // Python text-mode universal newlines, then splitlines(keepends=True).
        let text = String::from_utf8_lossy(&bytes)
            .replace("\r\n", "\n")
            .replace('\r', "\n");
        let lines: Vec<_> = text
            .split_inclusive(|c| {
                matches!(
                    c,
                    '\n' | '\u{b}'
                        | '\u{c}'
                        | '\u{1c}'
                        | '\u{1d}'
                        | '\u{1e}'
                        | '\u{85}'
                        | '\u{2028}'
                        | '\u{2029}'
                )
            })
            .collect();
        let start = params["line"]
            .as_i64()
            .unwrap_or(1)
            .saturating_sub(1)
            .max(0) as usize;
        let end = match params["limit"].as_i64() {
            Some(n) if n != 0 => {
                let end = i128::try_from(start).unwrap_or(i128::MAX) + i128::from(n);
                if end < 0 {
                    (lines.len() as i128 + end).max(0) as usize
                } else {
                    end as usize
                }
            }
            _ => lines.len(),
        };
        Ok(json!({"content":lines.get(start..end.min(lines.len())).unwrap_or(&[]).join("")}))
    }
    fn fs_write(&self, params: &Value) -> Result<Value> {
        let path = self.file_path(params)?;
        let content = params["content"].as_str().unwrap_or("");
        if content.len() > MAX_LINE {
            return Err("archivo ACP excede 4 MiB".into());
        }
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let mut file = regular_file(&path, true)?;
        // Check the opened descriptor before truncating, including replacements
        // between pathname metadata and open. Regular symlinks remain supported.
        file.set_len(0).map_err(|e| e.to_string())?;
        file.write_all(content.as_bytes())
            .map_err(|e| e.to_string())?;
        Ok(json!({}))
    }
    fn update(&mut self, update: &Value, events: &mut impl Events) -> Result<()> {
        let kind = update["sessionUpdate"].as_str().unwrap_or("");
        match kind{
  "config_option_update"=>self.absorb_options(array(update,"configOptions")),
  "current_mode_update"=>{if let Some(mode)=update["currentModeId"].as_str(){self.current_mode=mode.into();}},
  "available_commands_update"=>self.commands=array(update,"availableCommands"),
  "agent_message_chunk"|"agent_thought_chunk"=>if update["content"]["type"]=="text"{events.event(json!({"type":if kind=="agent_message_chunk"{"text"}else{"thought"},"text":update["content"]["text"].as_str().unwrap_or("")}))?},
  "tool_call"|"tool_call_update"=>events.event(json!({"type":"tool","title":string(update,"title"),"status":string(update,"status"),"kind":string(update,"kind")}))?,
  "plan"=>events.event(json!({"type":"plan","entries":array(update,"entries")}))?,_=>{}
 }
        Ok(())
    }
    pub fn option(&self, category: &str) -> Option<&Value> {
        self.config_options
            .iter()
            .find(|v| v["category"] == category && v["type"] == "select")
    }
    pub fn option_values(option: &Value) -> Vec<Value> {
        array(option, "options")
            .into_iter()
            .flat_map(|item| {
                item["options"]
                    .as_array()
                    .cloned()
                    .unwrap_or_else(|| vec![item])
            })
            .collect()
    }
    fn absorb_options(&mut self, options: Vec<Value>) {
        self.config_options = options.into_iter().filter(Value::is_object).collect();
        self.current_effort.clear();
        for category in ["model", "mode", "thought_level"] {
            if let Some(option) = self.option(category).cloned() {
                let current = string(&option, "currentValue");
                let values = Self::option_values(&option);
                match category {
                    "model" => {
                        self.current_model = current;
                        self.models=values.iter().map(|v|json!({"modelId":v["value"],"name":v["name"].as_str().unwrap_or("")})).collect();
                    }
                    "mode" => {
                        self.current_mode = current;
                        self.modes=values.iter().map(|v|json!({"id":v["value"],"name":v["name"].as_str().unwrap_or("")})).collect();
                    }
                    _ => self.current_effort = current,
                }
            }
        }
    }
    fn absorb(&mut self, result: &Value) {
        self.models = array(&result["models"], "availableModels");
        if let Some(v) = result["models"]["currentModelId"].as_str() {
            self.current_model = v.into();
        }
        self.modes = array(&result["modes"], "availableModes");
        if let Some(v) = result["modes"]["currentModeId"].as_str() {
            self.current_mode = v.into();
        }
        if result.get("configOptions").is_some() {
            self.absorb_options(array(result, "configOptions"));
        }
    }
    pub fn initialize(&mut self, events: &mut impl Events) -> Result<()> {
        if self.agy {
            self.capabilities = json!({"loadSession":false});
            return Ok(());
        }
        let r=self.request("initialize",json!({"protocolVersion":1,"clientCapabilities":{"fs":{"readTextFile":true,"writeTextFile":true},"terminal":false},"clientInfo":{"name":"comandos","title":"ComandOS","version":"1.0"}}),self.timeouts.initialize,events)?;
        if r["protocolVersion"] != 1 {
            return Err("versión ACP no admitida (se requiere 1)".into());
        }
        self.capabilities = r["agentCapabilities"].clone();
        Ok(())
    }
    pub fn supports_load(&self) -> bool {
        self.capabilities["loadSession"].as_bool() == Some(true)
    }
    pub fn start(&mut self, resume: &str, events: &mut impl Events) -> Result<()> {
        if !resume.is_empty() && !self.supports_load() {
            return Err(
                "el agente no admite reanudación exacta; no se crea otra conversación".into(),
            );
        }
        if self.agy {
            self.session_id = "agy-pending".into();
            return Ok(());
        }
        let mut params = json!({"cwd":self.cwd,"mcpServers":[]});
        let (method, timeout) = if resume.is_empty() {
            ("session/new", self.timeouts.new_session)
        } else {
            params["sessionId"] = json!(resume);
            ("session/load", self.timeouts.load)
        };
        let r = self.request(method, params, timeout, events)?;
        self.session_id = if resume.is_empty() {
            string(&r, "sessionId")
        } else {
            if r["sessionId"].as_str().is_some_and(|v| v != resume) {
                return Err("el agente no confirmó la conversación solicitada".into());
            }
            resume.into()
        };
        if self.session_id.is_empty() {
            return Err("session/new sin sessionId".into());
        }
        self.absorb(&r);
        Ok(())
    }
    fn set_option(&mut self, id: &str, value: &str, events: &mut impl Events) -> Result<()> {
        let option = self
            .config_options
            .iter()
            .find(|v| v["id"] == id)
            .ok_or("valor de configuración no ofrecido por el agente")?;
        if option["type"] != "select"
            || !Self::option_values(option)
                .iter()
                .any(|v| v["value"] == value)
        {
            return Err("valor de configuración no ofrecido por el agente".into());
        }
        let r = self.request(
            "session/set_config_option",
            json!({"sessionId":self.session_id,"configId":id,"value":value}),
            self.timeouts.config,
            events,
        )?;
        let options = r["configOptions"]
            .as_array()
            .ok_or("el agente no confirmó las opciones de configuración")?
            .clone();
        self.absorb_options(options);
        if !self
            .config_options
            .iter()
            .any(|v| v["id"] == id && v["currentValue"] == value)
        {
            return Err("el agente confirmó un valor distinto del solicitado".into());
        }
        Ok(())
    }
    pub fn set_effort(&mut self, effort: &str, events: &mut impl Events) -> Result<()> {
        let id = string(
            self.option("thought_level")
                .ok_or("el agente no publica una opción de esfuerzo")?,
            "id",
        );
        self.set_option(&id, effort, events)
    }
    pub fn set_model(&mut self, model: &str, events: &mut impl Events) -> Result<()> {
        if self.agy {
            return Err(
                "agy cambia de modelo relanzando (usa /model y se reinicia el proceso)".into(),
            );
        }
        if let Some(option) = self.option("model") {
            return self.set_option(&string(option, "id"), model, events);
        }
        self.request(
            "session/set_model",
            json!({"sessionId":self.session_id,"modelId":model}),
            self.timeouts.config,
            events,
        )?;
        self.current_model = model.into();
        Ok(())
    }
    pub fn set_mode(&mut self, mode: &str, events: &mut impl Events) -> Result<()> {
        if self.agy {
            return Err("agy no expone modos por stream-json".into());
        }
        if let Some(option) = self.option("mode") {
            return self.set_option(&string(option, "id"), mode, events);
        }
        self.request(
            "session/set_mode",
            json!({"sessionId":self.session_id,"modeId":mode}),
            self.timeouts.config,
            events,
        )?;
        self.current_mode = mode.into();
        Ok(())
    }
    pub fn cancel_turn(&mut self) -> Result<()> {
        if self.agy {
            self.proc.close();
            return Err(
                "agy no admite cancelación por stream-json; se cierra el proceso propio".into(),
            );
        }
        self.proc.write(&json!({"jsonrpc":"2.0","method":"session/cancel","params":{"sessionId":self.session_id}}))
    }
    pub fn alive(&mut self) -> bool {
        self.proc.alive()
    }
    pub fn prompt(&mut self, text: &str, events: &mut impl Events) -> Result<String> {
        if !self.agy {
            let r = self.request(
                "session/prompt",
                json!({"sessionId":self.session_id,"prompt":[{"type":"text","text":text}]}),
                self.timeouts.prompt,
                events,
            )?;
            return Ok(r["stopReason"].as_str().unwrap_or("end_turn").into());
        }
        self.proc
            .write(&json!({"event":"user","message":{"role":"user","content":text}}))?;
        let deadline = Instant::now() + self.timeouts.prompt;
        loop {
            if self.cancel.swap(false, Ordering::Relaxed) {
                return self.cancel_turn().map(|_| "cancelled".into());
            }
            let now = Instant::now();
            if now >= deadline {
                return Err(format!(
                    "timeout esperando a agy ({:.0}s)",
                    self.timeouts.prompt.as_secs_f64()
                ));
            }
            let Some(msg) = self
                .proc
                .receive((deadline - now).min(Duration::from_millis(25)))?
            else {
                continue;
            };
            if !msg.is_object() {
                return Err("mensaje stream-json inválido".into());
            }
            match msg["event"].as_str() {
                Some("init") => {
                    if let Some(v) = msg["init"]["model"].as_str() {
                        self.current_model = v.into();
                    }
                    if let Some(v) = msg["conversation_id"].as_str() {
                        self.session_id = v.into();
                    }
                }
                Some("step_update") => {
                    let step = &msg["step_update"];
                    if let Some(text) = step["text_delta"].as_str().filter(|s| !s.is_empty()) {
                        events.event(json!({"type":"text","text":text}))?;
                    } else if !matches!(
                        step["step_type"].as_str(),
                        Some("user_input" | "agent_response")
                    ) {
                        events.event(json!({"type":"tool","title":string(step,"step_type"),"status":string(step,"state").to_lowercase()}))?;
                    }
                }
                Some("result") => {
                    let r = &msg["result"];
                    if let Some(id) = r["conversation_id"].as_str() {
                        self.session_id = id.into();
                    }
                    if r["status"] != "SUCCESS" {
                        return Err(r["error"]
                            .as_str()
                            .or_else(|| r["status"].as_str())
                            .unwrap_or("agy error")
                            .into());
                    }
                    return Ok("end_turn".into());
                }
                _ => {}
            }
        }
    }
}

fn regular_file(path: &Path, write: bool) -> Result<fs::File> {
    match fs::metadata(path) {
        Ok(metadata) if !metadata.is_file() => {
            return Err("ACP requiere un archivo regular".into());
        }
        Ok(_) => {}
        Err(e) if write && e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e.to_string()),
    }
    // A FIFO substituted after the metadata check must not block open. Never
    // request truncation before validating the actual opened descriptor.
    let file = fs::OpenOptions::new()
        .read(!write)
        .write(write)
        .create(write)
        .custom_flags(nix::libc::O_NONBLOCK | nix::libc::O_NOCTTY)
        .open(path)
        .map_err(|e| e.to_string())?;
    if !file.metadata().map_err(|e| e.to_string())?.is_file() {
        return Err("ACP requiere un archivo regular".into());
    }
    Ok(file)
}
