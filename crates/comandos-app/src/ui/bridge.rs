//! Ordered centro protocol. A selected invalid branch never falls through.
use super::app_commands::CommandError;
use serde_json::{Value, json};
use std::{collections::VecDeque, rc::Rc};
pub type BridgeError = CommandError;
#[derive(Debug, Clone, PartialEq)]
pub enum BridgeMsg {
    Extensions {
        session: String,
        pane: String,
        harness: String,
    },
    Reader {
        action: String,
        id: String,
        on: bool,
    },
    ButtonStyle(String),
    HeaderAction(String),
    SidebarTerm(Value),
    LeftPanel(String),
    ChainModal(Value),
    Theme(String),
    OpenUrl {
        url: String,
        label: String,
        modal: bool,
    },
    Rename {
        session: String,
        label: String,
    },
    OpenSession {
        session: String,
        window: String,
        label: Option<String>,
    },
}
pub const HEADER_ACTIONS: [&str; 6] = [
    "quickTerminal",
    "newSession",
    "sortMenu",
    "notices",
    "chains",
    "analytics",
];
fn string(value: &Value, key: &str, default: &str) -> Result<String, BridgeError> {
    super::app_commands::string_arg(value, key, default).map(str::to_string)
}
fn reader_id(id: &str) -> bool {
    // Python \d accepts Unicode decimal digits, including non-ASCII ids.
    regex::Regex::new(r"^\d{4}-\d{2}-\d{2}@\d{2}:\d{2}$").is_ok_and(|re| re.is_match(id))
}
fn limit(value: &str, count: usize) -> String {
    value.chars().take(count).collect()
}
fn scalar_string(value: &Value) -> String {
    comandos_core::pomodoro::python_str(value)
}
pub fn parse_bridge(raw: &str) -> Result<BridgeMsg, BridgeError> {
    let value: Value =
        serde_json::from_str(raw).map_err(|e| BridgeError::Invalid(e.to_string()))?;
    if !value.is_object() {
        return Err(BridgeError::Invalid("bridge must be an object".into()));
    }
    let d = &value;
    if d.get("type").and_then(Value::as_str) == Some("extensions") {
        return Ok(BridgeMsg::Extensions {
            session: string(d, "session", "")?,
            pane: string(d, "pane", "")?,
            harness: string(d, "harness", "")?,
        });
    }
    if d.get("type").and_then(Value::as_str) == Some("reader") {
        let action = string(d, "action", "")?;
        if !["open", "close", "terminal"].contains(&action.as_str()) {
            return Err(BridgeError::Invalid("unknown reader action".into()));
        }
        let id = if action == "open" {
            scalar_string(
                d.get("id")
                    .filter(|v| comandos_core::json::truthy(v))
                    .unwrap_or(&json!("")),
            )
        } else {
            String::new()
        };
        let on = action == "terminal" && d.get("on").is_some_and(comandos_core::json::truthy);
        return Ok(BridgeMsg::Reader {
            action,
            id: if reader_id(&id) { id } else { String::new() },
            on,
        });
    }
    if let Some(style) = d
        .get("buttonStyle")
        .and_then(Value::as_str)
        .filter(|s| ["sutil", "arcade", "tecla", "pixel", "consola"].contains(s))
    {
        return Ok(BridgeMsg::ButtonStyle(style.into()));
    }
    if d.get("headerAction")
        .is_some_and(|v| v.is_array() || v.is_object())
    {
        return Err(BridgeError::Invalid(
            "headerAction must be hashable text or scalar".into(),
        ));
    }
    if let Some(action) = d
        .get("headerAction")
        .and_then(Value::as_str)
        .filter(|s| HEADER_ACTIONS.contains(s))
    {
        return Ok(BridgeMsg::HeaderAction(action.into()));
    }
    if let Some(value) = d.get("sidebarTerm").filter(|v| v.is_object()) {
        return Ok(BridgeMsg::SidebarTerm(value.clone()));
    }
    if let Some(action) = d
        .get("leftPanel")
        .and_then(Value::as_str)
        .filter(|s| ["toggle", "hide", "show"].contains(s))
    {
        return Ok(BridgeMsg::LeftPanel(action.into()));
    }
    if d.get("chainModal")
        .and_then(Value::as_str)
        .is_some_and(|s| ["close", "saved", "run"].contains(&s))
    {
        return Ok(BridgeMsg::ChainModal(value));
    }
    if let Some(theme) = d.get("theme").filter(|v| comandos_core::json::truthy(v)) {
        return Ok(BridgeMsg::Theme(
            theme
                .as_str()
                .ok_or_else(|| BridgeError::Invalid("theme must be a string".into()))?
                .into(),
        ));
    }
    for (key, modal, max_label) in [("openUrlModal", true, 80), ("openUrl", false, 60)] {
        if let Some(url) = d.get(key).filter(|v| comandos_core::json::truthy(v)) {
            let url = limit(&scalar_string(url), 2000);
            if !(url.starts_with("http://") || url.starts_with("https://")) {
                return Err(BridgeError::Invalid("URL requires http or https".into()));
            }
            let label = limit(
                &scalar_string(
                    d.get("label")
                        .filter(|v| comandos_core::json::truthy(v))
                        .unwrap_or(&json!("")),
                ),
                max_label,
            );
            return Ok(BridgeMsg::OpenUrl { url, label, modal });
        }
    }
    let session = string(d, "session", "")?;
    if d.get("type").and_then(Value::as_str) == Some("rename") {
        let label = string(d, "label", "")?.trim().to_string();
        if label.is_empty() {
            return Err(BridgeError::Invalid("empty rename label".into()));
        }
        return Ok(BridgeMsg::Rename { session, label });
    }
    if !crate::tab_actions::valid_session(&session) {
        return Err(BridgeError::Invalid("invalid session".into()));
    }
    let window = match d.get("win") {
        None => "claude",
        Some(value) if !comandos_core::json::truthy(value) => "claude",
        Some(Value::String(value)) => value,
        _ => return Err(BridgeError::Invalid("nontextual window".into())),
    };
    let window = if crate::tab_actions::valid_window(window) {
        window.to_string()
    } else {
        "claude".into()
    };
    let label = d
        .get("label")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    Ok(BridgeMsg::OpenSession {
        session,
        window,
        label,
    })
}

/// Only fixed functions may be called; arguments are serialized, never interpolated.
#[derive(Debug, Clone, Copy)]
pub enum JsFunction {
    RefreshChains,
    StartChain,
    NewSessionForPane,
}
pub fn js_call(function: JsFunction, args: &[Value]) -> Result<String, BridgeError> {
    let name = match function {
        JsFunction::RefreshChains => "refresh",
        JsFunction::StartChain => "startChain",
        JsFunction::NewSessionForPane => "nsOpenForPane",
    };
    let args = args
        .iter()
        .map(serde_json::to_string)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| BridgeError::Invalid(e.to_string()))?
        .join(",");
    if matches!(function, JsFunction::NewSessionForPane) {
        return Ok(format!(
            "if(typeof window.nsOpenForPane!=='function'){{throw new Error('Missing nsOpenForPane');}}window.nsOpenForPane({args});"
        ));
    }
    Ok(format!(
        "if(!window.commandSidebar || typeof window.commandSidebar.{name}!=='function'){{throw new Error('Missing commandSidebar.{name}');}}window.commandSidebar.{name}({args});"
    ))
}
/// Bounded idle delivery owned by App, rather than untracked per-message sources.
#[derive(Default)]
pub struct BridgeQueue {
    pending: VecDeque<BridgeMsg>,
    closed: bool,
}
impl BridgeQueue {
    pub fn push(&mut self, message: BridgeMsg) -> Result<(), BridgeError> {
        if self.closed {
            return Err(BridgeError::Refused("bridge closed".into()));
        }
        if self.pending.len() >= 128 {
            return Err(BridgeError::Refused("bridge queue full".into()));
        }
        self.pending.push_back(message);
        Ok(())
    }
    pub fn drain_ready(&mut self, ready: bool) -> Vec<BridgeMsg> {
        if self.closed || !ready {
            Vec::new()
        } else {
            self.pending.drain(..).collect()
        }
    }
    pub fn close(&mut self) {
        self.closed = true;
        self.pending.clear();
    }
}
pub fn install(app: &Rc<super::app::App>) -> glib::SourceId {
    app.install_bridge()
}
