use super::{
    Pane, Result,
    protocol::{Events, Permission, Session, string},
};
use comandos_runtime::extension_launch::shlex_split;
use serde_json::Value;
use std::{
    io::{BufRead, BufReader, Read, Write},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver},
    },
    thread,
    time::Duration,
};
enum Input {
    Line(String),
    Eof,
    Error(String),
}
pub(super) struct Ui<'a> {
    input: Receiver<Input>,
    output: &'a mut dyn Write,
    colors: bool,
    cancel: Arc<AtomicBool>,
    pub danger: bool,
    pub reply: String,
}
impl<'a> Ui<'a> {
    pub fn new(
        input: impl Read + Send + 'static,
        output: &'a mut dyn Write,
        colors: bool,
        cancel: Arc<AtomicBool>,
    ) -> Self {
        let (tx, rx) = mpsc::sync_channel(1);
        thread::spawn(move || {
            let mut input = BufReader::new(input);
            loop {
                let mut line = vec![];
                let item = match input
                    .by_ref()
                    .take((super::protocol::MAX_LINE + 1) as u64)
                    .read_until(b'\n', &mut line)
                {
                    Ok(0) => Input::Eof,
                    Ok(_) if line.len() > super::protocol::MAX_LINE => {
                        Input::Error("entrada excede 4 MiB".into())
                    }
                    Ok(_) => match String::from_utf8(line) {
                        Ok(s) => Input::Line(s),
                        Err(_) => Input::Error("entrada no es UTF-8".into()),
                    },
                    Err(e) => Input::Error(e.to_string()),
                };
                let terminal = !matches!(item, Input::Line(_));
                if tx.send(item).is_err() || terminal {
                    break;
                }
            }
        });
        Self {
            input: rx,
            output,
            colors,
            cancel,
            danger: false,
            reply: String::new(),
        }
    }
    pub fn color(&self, color: &str) -> &'static str {
        if !self.colors {
            return "";
        }
        match color {
            "dim" => "\x1b[2m",
            "bold" => "\x1b[1m",
            "off" => "\x1b[0m",
            "cyan" => "\x1b[36m",
            "yel" => "\x1b[33m",
            "grn" => "\x1b[32m",
            "red" => "\x1b[31m",
            "mag" => "\x1b[35m",
            _ => "",
        }
    }
    pub fn say(&mut self, text: &str, color: &str) -> Result<()> {
        let prefix = self.color(color);
        let suffix = if color.is_empty() {
            ""
        } else {
            self.color("off")
        };
        writeln!(self.output, "{prefix}{text}{suffix}")
            .and_then(|_| self.output.flush())
            .map_err(|e| e.to_string())
    }
    fn raw(&mut self, text: &str) -> Result<()> {
        self.output
            .write_all(text.as_bytes())
            .and_then(|_| self.output.flush())
            .map_err(|e| e.to_string())
    }
    fn line(&mut self, permission: bool) -> Result<Option<String>> {
        loop {
            if self.cancel.load(Ordering::Relaxed) {
                if !permission {
                    self.cancel.store(false, Ordering::Relaxed);
                    self.say("\n(/exit para salir)", "dim")?;
                }
                return if permission {
                    Ok(None)
                } else {
                    Ok(Some(String::new()))
                };
            }
            match self.input.recv_timeout(Duration::from_millis(25)) {
                Ok(Input::Line(s)) => return Ok(Some(s.trim().into())),
                Ok(Input::Eof) | Err(mpsc::RecvTimeoutError::Disconnected) => return Ok(None),
                Ok(Input::Error(e)) => return Err(e),
                Err(mpsc::RecvTimeoutError::Timeout) => {}
            }
        }
    }
}
impl Events for Ui<'_> {
    fn event(&mut self, event: Value) -> Result<()> {
        match event["type"].as_str() {
            Some("text") => {
                let text = event["text"].as_str().unwrap_or("");
                self.raw(text)?;
                self.reply.push_str(text);
                let mut count = self.reply.chars().count();
                if count > 400 {
                    let cut = self
                        .reply
                        .char_indices()
                        .find_map(|(i, _)| {
                            if count == 400 {
                                Some(i)
                            } else {
                                count -= 1;
                                None
                            }
                        })
                        .unwrap_or(self.reply.len());
                    self.reply.drain(..cut);
                }
            }
            Some("thought") => {
                let text = format!(
                    "{}{}{}",
                    self.color("dim"),
                    string(&event, "text"),
                    self.color("off")
                );
                self.raw(&text)?;
            }
            Some("tool") => {
                let mark = match event["status"].as_str() {
                    Some("completed") => "✓",
                    Some("failed") => "✗",
                    Some("in_progress") => "…",
                    _ => "·",
                };
                let title = event["title"]
                    .as_str()
                    .filter(|s| !s.is_empty())
                    .or_else(|| event["kind"].as_str().filter(|s| !s.is_empty()))
                    .unwrap_or("tool");
                self.say(&format!("\n{mark} {title}"), "mag")?
            }
            Some("plan") => {
                let entries = event["entries"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(|v| format!("  □ {}", v["content"].as_str().unwrap_or("")))
                    .collect::<Vec<_>>()
                    .join("\n");
                self.say(&format!("\n{entries}"), "dim")?
            }
            Some("permission") => self.say(
                &format!(
                    "  → {}",
                    event["decision"]
                        .as_str()
                        .filter(|s| !s.is_empty())
                        .unwrap_or("cancelado")
                ),
                "dim",
            )?,
            Some("cancel") => self.say("\n■ cancelando turno…", "yel")?,
            _ => {}
        }
        Ok(())
    }
    fn permission(&mut self, request: &Value) -> Result<Permission> {
        if self.danger {
            return Ok(Permission::Default);
        }
        let options = request["options"].as_array().cloned().unwrap_or_default();
        let call = &request["toolCall"];
        let title = call["title"]
            .as_str()
            .filter(|s| !s.is_empty())
            .or_else(|| call["kind"].as_str().filter(|s| !s.is_empty()))
            .unwrap_or("acción");
        self.say("", "")?;
        self.say(&format!("⚠ permiso: {title}"), "yel")?;
        for (i, option) in options.iter().enumerate() {
            self.say(
                &format!(
                    "  {}. {}  [{}]",
                    i + 1,
                    option["name"]
                        .as_str()
                        .or_else(|| option["optionId"].as_str())
                        .unwrap_or(""),
                    option["kind"].as_str().unwrap_or("")
                ),
                "dim",
            )?;
        }
        self.raw(&format!(
            "{}elige (Enter = permitir):{} ",
            self.color("yel"),
            self.color("off")
        ))?;
        let Some(choice) = self.line(true)? else {
            return Ok(Permission::Cancelled);
        };
        if let Ok(n) = choice.parse::<usize>()
            && let Some(option) = n.checked_sub(1).and_then(|n| options.get(n))
        {
            return Ok(Permission::Selected(string(option, "optionId")));
        }
        Ok(if choice.is_empty() {
            Permission::Default
        } else {
            Permission::Cancelled
        })
    }
}
const PROMPT_HELP: &str = "\n  /model <id>     /effort <nivel>    /agent <claude|codex|grok|opencode|agy>\n  /account <alias>  /models  /mode <id>  /danger  /status  /exit\n";
fn command(pane: &mut Pane<'_>, ui: &mut Ui<'_>, line: &str) -> Result<bool> {
    let parts = shlex_split(line).map_err(|e| format!("comando inválido: {e}"))?;
    let Some(cmd) = parts.first().map(String::as_str) else {
        return Ok(true);
    };
    let arg = parts.get(1).map_or("", String::as_str);
    match cmd {
        "/exit" | "/quit" | "/q" => return Ok(false),
        "/help" => ui.say(PROMPT_HELP, "dim")?,
        "/status" => {
            let session = pane.session.as_ref().ok_or("no hay sesión ACP")?;
            let s = &pane.selection;
            let effort = if session.current_effort.is_empty() {
                format!(
                    "{} (solicitado; sin confirmación)",
                    if s.effort.is_empty() { "-" } else { &s.effort }
                )
            } else {
                session.current_effort.clone()
            };
            ui.say(
                &format!(
                    "agente={} modelo={} esfuerzo={} cuenta={} danger={} sesión={} cwd={}",
                    s.agent,
                    if s.model.is_empty() { "?" } else { &s.model },
                    effort,
                    s.account,
                    if s.danger { "True" } else { "False" },
                    session.session_id.chars().take(12).collect::<String>(),
                    pane.cwd.display()
                ),
                "cyan",
            )?
        }
        "/models" => {
            let live: Vec<_> = pane
                .session
                .as_ref()
                .ok_or("no hay sesión ACP")?
                .models
                .iter()
                .filter_map(|v| v["modelId"].as_str())
                .map(str::to_owned)
                .collect();
            let models = if live.is_empty() {
                pane.config.registry["motors"][&pane.selection.agent]["models"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|v| v["id"].as_str())
                    .map(str::to_owned)
                    .collect()
            } else {
                live
            };
            ui.say(
                &format!(
                    "modelos: {}",
                    if models.is_empty() {
                        "(el agente no publica lista; usa el id del vendor)".into()
                    } else {
                        models.join(", ")
                    }
                ),
                "cyan",
            )?
        }
        "/model" => {
            if arg.is_empty() {
                ui.say("uso: /model <id>", "yel")?;
                return Ok(true);
            }
            let spec = pane.spec(&pane.selection.agent)?;
            let session = pane.session.as_mut().ok_or("no hay sesión ACP")?;
            if spec["modelVia"] == "acp" && !session.models.is_empty() {
                if !session.models.iter().any(|v| v["modelId"] == arg) {
                    return Err("el agente no ofrece ese modelo".into());
                }
                session.set_model(arg, ui)?;
                if session.current_model != arg {
                    return Err("el agente no confirmó el modelo".into());
                }
                pane.selection.model = session.current_model.clone();
                pane.publish()?;
                ui.say(&format!("✓ modelo → {arg}"), "grn")?;
            } else if arg != pane.selection.model {
                let mut target = pane.selection.clone();
                target.model = arg.into();
                pane.reconnect(target, ui)?;
            }
        }
        "/effort" => {
            let option = pane
                .session
                .as_ref()
                .ok_or("no hay sesión ACP")?
                .option("thought_level")
                .cloned();
            let available: Vec<_> = option
                .as_ref()
                .map(Session::option_values)
                .map(|v| {
                    v.iter()
                        .filter_map(|v| v["value"].as_str())
                        .map(str::to_owned)
                        .collect()
                })
                .unwrap_or_else(|| {
                    ["low", "medium", "high", "xhigh", "max"]
                        .map(str::to_owned)
                        .to_vec()
                });
            if !available.iter().any(|v| v == arg) {
                ui.say(&format!("uso: /effort <{}>", available.join("|")), "yel")?;
                return Ok(true);
            }
            if option.is_some() {
                let session = pane.session.as_mut().expect("session");
                let result = session.set_effort(arg, ui);
                pane.selection.effort = session.current_effort.clone();
                pane.publish()?;
                result?;
                ui.say(
                    &format!("✓ esfuerzo confirmado → {}", pane.selection.effort),
                    "grn",
                )?;
            } else {
                if arg != pane.selection.effort {
                    let mut target = pane.selection.clone();
                    target.effort = arg.into();
                    pane.reconnect(target, ui)?;
                }
                ui.say(
                    "Esfuerzo solicitado al iniciar; este agente no publica su valor activo.",
                    "yel",
                )?;
            }
        }
        "/agent" => {
            if !pane.names().contains(&arg) {
                ui.say(&format!("uso: /agent <{}>", pane.names().join("|")), "yel")?;
                return Ok(true);
            }
            if arg != pane.selection.agent {
                let model = pane.default_model(arg);
                let mut target = pane.selection.clone();
                target.agent = arg.into();
                target.model = string(&model, "id");
                target.effort = string(&model, "defaultEffort");
                target.account = "main".into();
                pane.reconnect(target, ui)?;
            }
        }
        "/account" => {
            let spec = pane.spec(&pane.selection.agent)?;
            if spec["accountsProvider"].as_str().is_none() {
                ui.say(
                    &format!("{} no maneja cuentas múltiples", pane.selection.agent),
                    "yel",
                )?;
                return Ok(true);
            }
            let live: Vec<_> = super::agents::accounts(
                &pane.config.registry,
                &spec,
                &pane.config.home,
                &pane.cwd,
            )?
            .into_iter()
            .filter(|v| v["selectable"] == true)
            .collect();
            if arg.is_empty() {
                ui.say(
                    &format!(
                        "cuentas: {}",
                        live.iter()
                            .map(|v| format!(
                                "{} ({})",
                                string(v, "alias"),
                                v["identity"]
                                    .as_str()
                                    .filter(|s| !s.is_empty())
                                    .unwrap_or("sin login")
                            ))
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                    "cyan",
                )?;
            } else if !live.iter().any(|v| v["alias"] == arg) {
                ui.say(&format!("cuenta sin login: {arg}"), "red")?;
            } else if arg != pane.selection.account {
                let mut target = pane.selection.clone();
                target.account = arg.into();
                pane.reconnect(target, ui)?;
            }
        }
        "/mode" => {
            let spec = pane.spec(&pane.selection.agent)?;
            let session = pane.session.as_mut().ok_or("no hay sesión ACP")?;
            session.set_mode(arg, ui)?;
            if session.current_mode != arg {
                return Err("el agente no confirmó el modo".into());
            }
            pane.selection.danger = spec["dangerMode"] == arg;
            if !pane.selection.danger {
                pane.safe_mode = arg.into();
            }
            ui.danger = pane.selection.danger;
            pane.publish()?;
            ui.say(&format!("✓ modo → {arg}"), "grn")?;
        }
        "/danger" => {
            let desired = !pane.selection.danger;
            let spec = pane.spec(&pane.selection.agent)?;
            if let Some(mode) = spec["dangerMode"].as_str() {
                let session = pane.session.as_mut().ok_or("no hay sesión ACP")?;
                let safe = if pane.safe_mode.is_empty() && !pane.selection.danger {
                    session.current_mode.clone()
                } else {
                    pane.safe_mode.clone()
                };
                let target = if desired { mode } else { &safe };
                if target.is_empty() || !session.modes.iter().any(|v| v["id"] == target) {
                    return Err(
                        "no se puede restaurar un modo seguro conocido; crea una sesión nueva"
                            .into(),
                    );
                }
                session.set_mode(target, ui)?;
                if session.current_mode != target {
                    return Err("el agente no confirmó el modo de permisos".into());
                }
                pane.safe_mode = safe;
            } else if pane.launch_danger {
                return Err(
                    "el permiso del proceso requiere una sesión nueva para desactivarlo".into(),
                );
            }
            pane.selection.danger = desired;
            ui.danger = desired;
            pane.publish()?;
            ui.say(
                &format!("auto-aprobar: {}", if desired { "ON" } else { "OFF" }),
                "yel",
            )?;
        }
        _ => ui.say(&format!("comando desconocido: {cmd} (/help)"), "yel")?,
    }
    Ok(true)
}
pub(super) fn loop_input(pane: &mut Pane<'_>, ui: &mut Ui<'_>) -> Result<i32> {
    loop {
        ui.raw(&format!("{}❯{} ", ui.color("grn"), ui.color("off")))?;
        let Some(line) = ui.line(false)? else { break };
        if line.is_empty() {
            continue;
        }
        if line.starts_with('/') {
            match command(pane, ui, &line) {
                Ok(false) => break,
                Ok(true) => {}
                Err(e) => ui.say(&format!("✗ {e}"), "red")?,
            }
        } else {
            pane.run_prompt(&line, ui)?;
            if pane.session.is_none() {
                return Ok(1);
            }
        }
    }
    Ok(0)
}
