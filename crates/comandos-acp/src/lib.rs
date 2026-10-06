//! A pane client: registry argv, private stdio protocol, and store-owned UI state.
mod agents;
mod prompt;
pub mod protocol;
use comandos_core::json::{workspace_dumps, workspace_loads};
use comandos_store::domains::{DocHandle, DomainStore};
use protocol::{Session, Timeouts};
use serde_json::{Value, json};
use std::{
    env,
    ffi::OsString,
    fs,
    io::{self, IsTerminal, Read, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{Arc, atomic::AtomicBool},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
pub type Result<T> = std::result::Result<T, String>;

#[derive(Clone)]
pub struct Program {
    pub path: PathBuf,
    pub prefix: Vec<OsString>,
}
impl Program {
    fn call(&self, args: &[&str], payload: Option<&Value>) -> Result<()> {
        let mut child = Command::new(&self.path)
            .args(&self.prefix)
            .args(args)
            .env("HARNESS", "acp")
            .stdin(if payload.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| e.to_string())?;
        if let Some(payload) = payload {
            let bytes = serde_json::to_vec(payload).map_err(|e| e.to_string())?;
            let result = child
                .stdin
                .take()
                .ok_or("hook sin stdin")?
                .write_all(&bytes);
            if result.is_err() {
                let _ = child.kill();
                let _ = child.wait();
                return Err("hook cerró stdin".into());
            }
        }
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if child.try_wait().map_err(|e| e.to_string())?.is_some() {
                return Ok(());
            }
            if Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                return Err("timeout de helper ACP".into());
            }
            thread::sleep(Duration::from_millis(5));
        }
    }
}
pub struct Config {
    pub home: PathBuf,
    pub cwd: PathBuf,
    pub registry: Value,
    pub path: Option<OsString>,
    pub pane: String,
    pub tmux: Option<Program>,
    pub notify: Option<Program>,
    pub colors: bool,
    pub timeouts: Timeouts,
    pub cancel: Arc<AtomicBool>,
}
impl Config {
    pub fn from_env() -> Result<Self> {
        let home = env::var_os("HOME")
            .map(PathBuf::from)
            .ok_or("HOME ausente")?;
        let cwd = env::current_dir().map_err(|e| e.to_string())?;
        let registry = agents::registry()?;
        let path = env::var_os("PATH");
        let tmux = agents::which("tmux", &home, path.as_deref()).map(|path| Program {
            path,
            prefix: vec![],
        });
        let notify = if home.join(".claude/hooks/cc-notify.sh").exists() {
            Some(Program {
                path: env::current_exe().map_err(|e| e.to_string())?,
                prefix: vec!["hook".into(), "claude".into()],
            })
        } else {
            None
        };
        Ok(Self {
            home,
            cwd,
            registry,
            path,
            pane: env::var("TMUX_PANE").unwrap_or_default(),
            tmux,
            notify,
            colors: io::stdout().is_terminal(),
            timeouts: Timeouts::default(),
            cancel: Arc::new(AtomicBool::new(false)),
        })
    }
    fn doc(&self) -> DocHandle<'_> {
        DomainStore { home: &self.home }.document(
            "hooks/acp-panes.json",
            "ui-docs",
            self.home.join(".claude/hooks/acp-panes.json"),
        )
    }
    fn notify(&self, event: &str, cwd: &Path, last: Option<&str>) {
        if let Some(program) = &self.notify {
            let mut value = json!({"hook_event_name":event,"cwd":cwd,"session_id":env::var("CC_ACP_SESSION").unwrap_or_default(),"harness":"acp"});
            if let Some(last) = last {
                value["last_assistant_message"] = json!(last);
            }
            let _ = program.call(&[], Some(&value));
        }
    }
}
#[derive(Clone, Default)]
struct Selection {
    agent: String,
    model: String,
    effort: String,
    account: String,
    danger: bool,
    resume: String,
}
struct Args {
    selection: Selection,
    cwd: String,
    once: String,
}
const USAGE: &str = "usage: cc-acp [-h] [--agent AGENT] [--model MODEL] [--effort EFFORT]\n              [--account ACCOUNT] [--cwd CWD] [--resume RESUME] [--danger]\n              [--once ONCE]\n";
const HELP: &str = "\npane cliente ACP de ComandOS\n\noptions:\n  -h, --help         show this help message and exit\n  --agent AGENT\n  --model MODEL\n  --effort EFFORT\n  --account ACCOUNT\n  --cwd CWD\n  --resume RESUME\n  --danger           auto-aprobar permisos\n  --once ONCE        manda un solo prompt y sale (pruebas E2E)\n";
enum Parse {
    Run(Args),
    Help,
    Error(String),
}
fn parse(args: &[String]) -> Parse {
    const OPTIONS: &[&str] = &[
        "--help",
        "--agent",
        "--model",
        "--effort",
        "--account",
        "--cwd",
        "--resume",
        "--danger",
        "--once",
    ];
    let mut out = Args {
        selection: Selection {
            agent: "claude".into(),
            account: "main".into(),
            ..Selection::default()
        },
        cwd: String::new(),
        once: String::new(),
    };
    let mut at = 0;
    let mut unknown = vec![];
    let negative = regex::Regex::new(r"^-([0-9]+(\.[0-9]*)?|\.[0-9]+)$").expect("fixed regex");
    while let Some(raw) = args.get(at) {
        at += 1;
        if raw == "--" {
            unknown.push(raw.clone());
            unknown.extend(args[at..].iter().cloned());
            break;
        }
        let (flag, inline) = raw
            .split_once('=')
            .map_or((raw.as_str(), None), |(a, b)| (a, Some(b)));
        let flag = if flag == "-h" {
            "--help"
        } else if OPTIONS.contains(&flag) {
            flag
        } else if flag.starts_with("--") {
            let matches: Vec<_> = OPTIONS
                .iter()
                .copied()
                .filter(|v| v.starts_with(flag))
                .collect();
            match matches.len() {
                1 => matches[0],
                n if n > 1 => {
                    return Parse::Error(format!(
                        "ambiguous option: {flag} could match {}",
                        matches.join(", ")
                    ));
                }
                _ => {
                    unknown.push(raw.clone());
                    continue;
                }
            }
        } else {
            unknown.push(raw.clone());
            continue;
        };
        if matches!(flag, "--help" | "--danger") {
            if let Some(value) = inline {
                return Parse::Error(format!(
                    "argument {flag}: ignored explicit argument '{}'",
                    value
                ));
            }
            if flag == "--help" {
                return Parse::Help;
            }
            out.selection.danger = true;
            continue;
        }
        let value = if let Some(v) = inline {
            v.to_owned()
        } else {
            let Some(v) = args.get(at) else {
                return Parse::Error(format!("argument {flag}: expected one argument"));
            };
            if v.starts_with('-') && v != "-" && !negative.is_match(v) {
                return Parse::Error(format!("argument {flag}: expected one argument"));
            }
            at += 1;
            v.clone()
        };
        match flag {
            "--agent" => out.selection.agent = value,
            "--model" => out.selection.model = value,
            "--effort" => out.selection.effort = value,
            "--account" => {
                out.selection.account = if value.is_empty() {
                    "main".into()
                } else {
                    value
                }
            }
            "--resume" => out.selection.resume = value,
            "--cwd" => out.cwd = value,
            "--once" => out.once = value,
            _ => unreachable!(),
        }
    }
    if unknown.is_empty() {
        Parse::Run(out)
    } else {
        Parse::Error(format!("unrecognized arguments: {}", unknown.join(" ")))
    }
}
fn object(bytes: Option<&[u8]>) -> comandos_store::Result<Value> {
    match bytes {
        None => Ok(json!({})),
        Some(bytes) => {
            let text = std::str::from_utf8(bytes).map_err(|_| {
                comandos_store::Error::Validation("documento ACP no es UTF-8".into())
            })?;
            let value = workspace_loads(text)
                .map_err(|e| comandos_store::Error::Validation(e.to_string()))?;
            if !value.is_object() {
                return Err(comandos_store::Error::Validation(
                    "documento ACP debe ser objeto".into(),
                ));
            }
            Ok(value)
        }
    }
}
fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or_default()
}
struct Pane<'a> {
    config: &'a Config,
    cwd: PathBuf,
    selection: Selection,
    session: Option<Session>,
    safe_mode: String,
    launch_danger: bool,
    published_session: Option<String>,
    history: bool,
}
impl<'a> Pane<'a> {
    fn spec(&self, agent: &str) -> Result<Value> {
        self.config.registry["acpAgents"][agent]
            .as_object()
            .map(|v| Value::Object(v.clone()))
            .ok_or_else(|| {
                format!(
                    "agente ACP desconocido: {agent} (hay: {})",
                    self.names().join(", ")
                )
            })
    }
    fn names(&self) -> Vec<&str> {
        self.config.registry["acpAgents"]
            .as_object()
            .into_iter()
            .flat_map(|v| v.keys().map(String::as_str))
            .collect()
    }
    fn default_model(&self, agent: &str) -> Value {
        self.config.registry["motors"][agent]["models"]
            .as_array()
            .and_then(|v| v.first())
            .cloned()
            .unwrap_or(json!({}))
    }
    fn connect(
        &self,
        selection: &Selection,
        ui: &mut prompt::Ui<'_>,
    ) -> Result<(Session, String, String)> {
        let spec = self.spec(&selection.agent)?;
        if !agents::available(&spec, &self.config.home, self.config.path.as_deref()) {
            return Err(format!(
                "{} no está instalado",
                spec["label"].as_str().unwrap_or(&selection.agent)
            ));
        }
        let requested = if selection.model.is_empty() && spec["transport"] == "agy-stream" {
            protocol::string(&self.default_model(&selection.agent), "id")
        } else {
            selection.model.clone()
        };
        let argvmodel = if spec["modelVia"] == "acp" {
            ""
        } else {
            &requested
        };
        let env = agents::account_env(
            &self.config.registry,
            &spec,
            &selection.account,
            &self.config.home,
            &self.cwd,
        )?;
        let (argv, env) = agents::command(
            &spec,
            &self.config.home,
            self.config.path.as_deref(),
            argvmodel,
            &selection.effort,
            selection.danger,
            &env,
        )?;
        let mut session = Session::open(
            &argv,
            &env,
            &self.cwd,
            spec["transport"] == "agy-stream",
            self.config.timeouts.clone(),
            self.config.cancel.clone(),
        )?;
        ui.danger = selection.danger;
        session.initialize(ui)?;
        session.start(&selection.resume, ui)?;
        if spec["modelVia"] == "acp" && !requested.is_empty() {
            let ids: Vec<_> = session
                .models
                .iter()
                .filter_map(|v| v["modelId"].as_str())
                .collect();
            let target = if ids.contains(&requested.as_str()) {
                requested.clone()
            } else {
                let wanted = comandos_runtime::providers::model_spec(
                    &self.config.registry,
                    &selection.agent,
                    &requested,
                    "motors",
                )
                .map_err(|_| "modelo inválido")?;
                let matches: Vec<_> = ids
                    .iter()
                    .filter(|id| {
                        let live = comandos_runtime::providers::model_spec(
                            &self.config.registry,
                            &selection.agent,
                            id,
                            "motors",
                        )
                        .ok()
                        .flatten();
                        wanted
                            .as_ref()
                            .zip(live.as_ref())
                            .is_some_and(|(a, b)| a["id"] == b["id"])
                    })
                    .collect();
                if matches.len() == 1 {
                    (*matches[0]).to_string()
                } else {
                    return Err("el agente no ofrece el modelo solicitado".into());
                }
            };
            if target != session.current_model {
                session.set_model(&target, ui)?;
            }
            if session.current_model != target {
                return Err("el agente no confirmó el modelo solicitado".into());
            }
        }
        if !selection.effort.is_empty() && session.option("thought_level").is_some() {
            session.set_effort(&selection.effort, ui)?;
        }
        let safe = session.current_mode.clone();
        if selection.danger
            && let Some(mode) = spec["dangerMode"].as_str()
        {
            if !session.modes.iter().any(|v| v["id"] == mode) {
                return Err("el agente no ofrece el modo de permisos solicitado".into());
            }
            session.set_mode(mode, ui)?;
            if session.current_mode != mode {
                return Err("el agente no confirmó el modo de permisos".into());
            }
        }
        let model = if session.current_model.is_empty() {
            requested
        } else {
            session.current_model.clone()
        };
        Ok((
            session,
            model,
            if spec["dangerMode"] == safe {
                "".into()
            } else {
                safe
            },
        ))
    }
    fn publish(&mut self) -> Result<()> {
        let session = self.session.as_ref().ok_or("no hay sesión ACP")?;
        let s = &self.selection;
        let pid = std::process::id();
        let now = now_ms();
        let key = if self.config.pane.is_empty() {
            format!("pid-{pid}")
        } else {
            self.config.pane.clone()
        };
        let body = json!({"agent":s.agent,"model":s.model,"effort":s.effort,"account":s.account,"cwd":self.cwd,"sessionId":session.session_id,"pid":pid,"ts":now/1000,"observationVersion":1,"observedModel":session.current_model,"observedEffort":session.current_effort,"effortSource":if session.current_effort.is_empty(){"unconfirmed"}else{"acp-config-options"},"requestedModel":s.model,"requestedEffort":s.effort});
        let expected = self.published_session.as_deref();
        self.config
            .doc()
            .update_owned(now, |old| {
                let mut value = object(old)?;
                if let Some(expected) = expected
                    && (value[&key]["pid"] != pid || value[&key]["sessionId"] != expected)
                {
                    return Err(comandos_store::Error::Validation(
                        "la generación del pane ACP cambió; se conserva el reemplazo".into(),
                    ));
                }
                value[&key] = body;
                let bytes = workspace_dumps(&value)
                    .map_err(comandos_store::Error::Validation)?
                    .into_bytes();
                Ok(Some(bytes))
            })
            .map_err(|e| e.to_string())?;
        self.published_session = Some(session.session_id.clone());
        if !self.config.pane.is_empty()
            && let Some(tmux) = &self.config.tmux
        {
            let label = format!(
                "acp:{} · {}{}",
                s.agent,
                if s.model.is_empty() { "?" } else { &s.model },
                if s.effort.is_empty() {
                    String::new()
                } else {
                    format!(" · {}", s.effort)
                }
            );
            let _ = tmux.call(
                &[
                    "set-option",
                    "-p",
                    "-t",
                    &self.config.pane,
                    "@ccmodel",
                    &label,
                ],
                None,
            );
        }
        Ok(())
    }
    fn cleanup(&mut self) -> Result<()> {
        self.session.take();
        let Some(session) = self.published_session.take() else {
            return Ok(());
        };
        let pid = std::process::id();
        let key = if self.config.pane.is_empty() {
            format!("pid-{pid}")
        } else {
            self.config.pane.clone()
        };
        self.config
            .doc()
            .update_owned(now_ms(), |old| {
                let mut value = object(old)?;
                if value[&key]["pid"] != pid || value[&key]["sessionId"] != session {
                    return Ok(None);
                }
                value
                    .as_object_mut()
                    .expect("validated object")
                    .remove(&key);
                Ok(Some(
                    workspace_dumps(&value)
                        .map_err(comandos_store::Error::Validation)?
                        .into_bytes(),
                ))
            })
            .map_err(|e| e.to_string())?;
        Ok(())
    }
    fn reconnect(&mut self, target: Selection, ui: &mut prompt::Ui<'_>) -> Result<()> {
        let old = self.session.as_ref().ok_or("no hay sesión ACP")?;
        let old_spec = self.spec(&self.selection.agent)?;
        let spec = self.spec(&target.agent)?;
        if !agents::available(&spec, &self.config.home, self.config.path.as_deref()) {
            return Err("el agente destino no está instalado".into());
        }
        if old.session_id.is_empty() || !old.supports_load() || old_spec["resume"] != true {
            return Err("el origen no ofrece reanudación exacta; crea una sesión nueva".into());
        }
        if spec["resume"] != true {
            return Err("el destino no ofrece reanudación exacta; crea una sesión nueva".into());
        }
        if target.agent == self.selection.agent && target.account != self.selection.account {
            return Err(
                "cambiar cuenta ACP requiere exportar la conversación; se conserva el origen"
                    .into(),
            );
        }
        if spec["accountsProvider"].as_str().is_some() {
            if !agents::accounts(&self.config.registry, &spec, &self.config.home, &self.cwd)?
                .iter()
                .any(|v| v["alias"] == target.account && v["selectable"] == true)
            {
                return Err("la cuenta destino no tiene login".into());
            }
        } else if target.account != "main" {
            return Err("el destino no admite cuentas múltiples".into());
        }
        if !target.model.is_empty() {
            let model = comandos_runtime::providers::model_spec(
                &self.config.registry,
                &target.agent,
                &target.model,
                "motors",
            )
            .map_err(|_| "modelo inválido")?
            .ok_or("modelo o esfuerzo no admitido por el destino")?;
            if !target.effort.is_empty()
                && !model["efforts"]
                    .as_array()
                    .is_some_and(|v| v.iter().any(|v| v == &target.effort))
            {
                return Err("modelo o esfuerzo no admitido por el destino".into());
            }
        }
        let mut target = target;
        target.resume = if target.agent == self.selection.agent {
            old.session_id.clone()
        } else {
            String::new()
        };
        let saved_danger = ui.danger;
        let (session, model, safe) = match self.connect(&target, ui) {
            Ok(v) => v,
            Err(e) => {
                ui.danger = saved_danger;
                return Err(e);
            }
        };
        let previous = self.selection.clone();
        let old = self.session.replace(session);
        target.model = model;
        self.selection = target;
        let previous_safe = std::mem::replace(&mut self.safe_mode, safe);
        let previous_launch = self.launch_danger;
        self.launch_danger = self.selection.danger;
        if let Err(e) = self.publish() {
            self.selection = previous;
            self.session = old;
            self.safe_mode = previous_safe;
            self.launch_danger = previous_launch;
            ui.danger = saved_danger;
            return Err(e);
        }
        drop(old);
        if self.selection.resume.is_empty() && self.history {
            ui.say("La conversación anterior está guardada; /status muestra la nueva sesión. Contexto disponible en memoria.","dim")?;
        }
        Ok(())
    }
    fn banner(&self, ui: &mut prompt::Ui<'_>) -> Result<()> {
        let spec = self.spec(&self.selection.agent)?;
        let s = &self.selection;
        let text = format!(
            "{}ComandOS ACP{} · {} · {}{}{}{} · cuenta {}{}",
            ui.color("bold"),
            ui.color("off"),
            spec["label"].as_str().unwrap_or(&s.agent),
            ui.color("cyan"),
            if s.model.is_empty() {
                "modelo del agente"
            } else {
                &s.model
            },
            ui.color("off"),
            if s.effort.is_empty() {
                String::new()
            } else {
                format!(" · {}", s.effort)
            },
            s.account,
            if s.danger {
                format!(" · {}auto-aprobar{}", ui.color("yel"), ui.color("off"))
            } else {
                String::new()
            }
        );
        ui.say(&text, "")?;
        ui.say(
            &format!(
                "{}  ·  /help para comandos  ·  Ctrl-C cancela el turno",
                self.cwd.display()
            ),
            "dim",
        )
    }
    fn run_prompt(&mut self, text: &str, ui: &mut prompt::Ui<'_>) -> Result<bool> {
        ui.reply.clear();
        ui.danger = self.selection.danger;
        self.config.notify("UserPromptSubmit", &self.cwd, None);
        let start = Instant::now();
        let result = self
            .session
            .as_mut()
            .ok_or("no hay sesión ACP")?
            .prompt(text, ui);
        let success = result.is_ok();
        let stop = match result {
            Ok(s) => s,
            Err(e) => {
                ui.say(&format!("\n✗ {e}"), "red")?;
                "error".into()
            }
        };
        self.history = true;
        ui.say("", "")?;
        ui.say(
            &format!(
                "— {stop} · {:.1}s · {} · {}",
                start.elapsed().as_secs_f64(),
                self.selection.agent,
                self.selection.model
            ),
            "dim",
        )?;
        self.config.notify("Stop", &self.cwd, Some(&ui.reply));
        let session = self.session.as_ref().expect("active session");
        if !session.current_model.is_empty() {
            self.selection.model = session.current_model.clone();
        }
        if self.session.as_mut().is_some_and(Session::alive) {
            self.publish()?;
        } else {
            self.cleanup()?;
        }
        Ok(success)
    }
}
pub fn run(
    config: &Config,
    args: &[String],
    input: impl Read + Send + 'static,
    output: &mut dyn Write,
    error: &mut dyn Write,
) -> i32 {
    let args = match parse(args) {
        Parse::Help => {
            let _ = write!(output, "{USAGE}{HELP}");
            return 0;
        }
        Parse::Error(e) => {
            let _ = writeln!(error, "{USAGE}cc-acp: error: {e}");
            return 2;
        }
        Parse::Run(a) => a,
    };
    let agent = args.selection.agent.clone();
    let cwd = if args.cwd.is_empty() {
        config.cwd.clone()
    } else {
        PathBuf::from(&args.cwd)
    };
    let cwd = if cwd.is_absolute() {
        cwd
    } else {
        config.cwd.join(cwd)
    };
    let cwd = match fs::canonicalize(cwd) {
        Ok(p) => p,
        Err(e) => {
            let _ = writeln!(output, "✗ no pude arrancar {agent}: {e}");
            return 2;
        }
    };
    if cwd.to_str().is_none() {
        let _ = writeln!(output, "✗ no pude arrancar {agent}: cwd no es UTF-8");
        return 2;
    }
    let mut pane = Pane {
        config,
        cwd,
        selection: args.selection,
        session: None,
        safe_mode: String::new(),
        launch_danger: false,
        published_session: None,
        history: false,
    };
    let mut ui = prompt::Ui::new(input, output, config.colors, config.cancel.clone());
    let initial = (|| {
        let bytes = config.doc().read_readonly().map_err(|e| e.to_string())?;
        object(bytes.as_deref()).map_err(|e| e.to_string())?;
        let (session, model, safe) = pane.connect(&pane.selection, &mut ui)?;
        pane.session = Some(session);
        pane.selection.model = model;
        pane.safe_mode = safe;
        pane.launch_danger = pane.selection.danger;
        pane.publish()?;
        pane.banner(&mut ui)
    })();
    if let Err(e) = initial {
        let _ = ui.say(&format!("✗ no pude arrancar {agent}: {e}"), "red");
        let _ = pane.cleanup();
        return 2;
    }
    let result = if !args.once.is_empty() {
        pane.run_prompt(&args.once, &mut ui)
            .map(|success| if success { 0 } else { 1 })
    } else {
        prompt::loop_input(&mut pane, &mut ui)
    };
    if args.once.is_empty() {
        config.notify("SessionEnd", &pane.cwd, None);
    }
    let cleanup = pane.cleanup();
    match (result, cleanup) {
        (Ok(code), Ok(())) => code,
        (Err(e), _) | (_, Err(e)) => {
            let _ = writeln!(error, "cc-acp: {e}");
            2
        }
    }
}
pub fn main(args: &[String]) -> i32 {
    // Help and argument errors do not read configuration or create controls.
    match parse(args) {
        Parse::Help => {
            print!("{USAGE}{HELP}");
            return 0;
        }
        Parse::Error(e) => {
            eprintln!("{USAGE}cc-acp: error: {e}");
            return 2;
        }
        Parse::Run(_) => {}
    }
    let config = match Config::from_env() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("cc-acp: {e}");
            return 2;
        }
    };
    let signal =
        match signal_hook::flag::register(signal_hook::consts::SIGINT, config.cancel.clone()) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("cc-acp: {e}");
                return 2;
            }
        };
    let code = run(
        &config,
        args,
        io::stdin(),
        &mut io::stdout(),
        &mut io::stderr(),
    );
    signal_hook::low_level::unregister(signal);
    code
}
