//! Window-independent application state. Callback tickets belong to an instance,
//! not a tab name, and become invalid at replacement or shutdown.
#![forbid(unsafe_code)]
use comandos_desktop::{BridgeMessage, Lang, VALID_THEMES};
use serde_json::Value;
use std::{
    collections::VecDeque,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunMode {
    Live,
    Sandbox {
        tmux_socket: PathBuf,
        hooks: PathBuf,
    },
}
#[derive(Debug, Clone)]
pub struct AppConfig {
    pub mode: RunMode,
    pub dash_url: String,
    pub dump_dom: Option<PathBuf>,
}
pub fn parse_args(args: &[String]) -> Result<AppConfig, String> {
    let mut cfg = AppConfig {
        mode: RunMode::Live,
        dash_url: "http://127.0.0.1:4777".into(),
        dump_dom: None,
    };
    let mut i = 0;
    while let Some(arg) = args.get(i) {
        let value = args
            .get(i + 1)
            .ok_or_else(|| format!("falta valor para {arg}"))?;
        match arg.as_str() {
            "--sandbox" => {
                let root = PathBuf::from(value);
                if !root.is_absolute() {
                    return Err("sandbox requiere ruta absoluta".into());
                }
                cfg.mode = RunMode::Sandbox {
                    tmux_socket: root.join("tmux-1000/default"),
                    hooks: root.join("hooks"),
                };
            }
            "--dash-url" => cfg.dash_url = comandos_desktop::dash_client::loopback_only(value)?.0,
            "--dump-dom" => {
                let path = PathBuf::from(value);
                if !path.is_absolute() {
                    return Err("dump-dom requiere ruta absoluta".into());
                }
                cfg.dump_dom = Some(path);
            }
            _ => return Err(format!("opción desconocida: {arg}")),
        }
        i += 2;
    }
    Ok(cfg)
}
#[derive(Debug, Clone)]
pub struct WindowSpec {
    pub size: (f64, f64),
    pub minimum: (f64, f64),
    pub left_fraction: f64,
    pub strip_height: f64,
    pub title: &'static str,
    pub dark_aqua: bool,
}
impl Default for WindowSpec {
    fn default() -> Self {
        Self {
            size: (1280., 800.),
            minimum: (760., 480.),
            left_fraction: 0.52,
            strip_height: 30.,
            title: "ComandOS",
            dark_aqua: true,
        }
    }
}
#[derive(Clone)]
pub struct Ticket {
    generation: u64,
    current: Arc<AtomicU64>,
    stopped: Option<Arc<std::sync::atomic::AtomicBool>>,
}
impl Ticket {
    pub(crate) fn with_stop(mut self, stop: Arc<std::sync::atomic::AtomicBool>) -> Self {
        self.stopped = Some(stop);
        self
    }
    pub fn current(&self) -> bool {
        self.generation != 0
            && self.current.load(Ordering::Acquire) == self.generation
            && !self
                .stopped
                .as_ref()
                .is_some_and(|s| s.load(Ordering::Acquire))
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    Open {
        session: String,
        win: String,
        label: Option<String>,
    },
    NewLocal,
}
#[derive(Debug, Clone)]
pub struct Tab {
    pub key: String,
    pub label: String,
    pub session: String,
    pub is_hub: bool,
    pub loaded: bool,
    pub instance: u64,
    pub dot_color: &'static str,
}
pub struct App {
    tabs: Vec<Tab>,
    active: Option<String>,
    theme: String,
    pub lang: Lang,
    pub term_visible: bool,
    token: String,
    ready: bool,
    pending: VecDeque<Action>,
    generation: Arc<AtomicU64>,
    closed: bool,
    next_tab: u64,
    theme_selected: bool,
    focus_request: Option<u64>,
}
impl App {
    pub fn new(theme: &str, lang: Lang) -> Self {
        Self {
            tabs: Vec::new(),
            active: None,
            theme: if VALID_THEMES.contains(&theme) {
                theme
            } else {
                "noche"
            }
            .into(),
            lang,
            term_visible: true,
            token: String::new(),
            ready: false,
            pending: VecDeque::new(),
            generation: Arc::new(AtomicU64::new(1)),
            closed: false,
            next_tab: 0,
            theme_selected: false,
            focus_request: None,
        }
    }
    pub fn ticket(&self) -> Ticket {
        Ticket {
            generation: self.generation.load(Ordering::Acquire),
            current: self.generation.clone(),
            stopped: None,
        }
    }
    pub fn restart_boot(&mut self) -> Ticket {
        if self.closed {
            return self.ticket();
        }
        if self
            .generation
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |value| {
                value.checked_add(1)
            })
            .is_err()
        {
            self.shutdown();
            return self.ticket();
        }
        self.ready = false;
        self.token.clear();
        self.ticket()
    }
    pub fn shutdown(&mut self) {
        if !self.closed {
            self.closed = true;
            self.generation.store(0, Ordering::Release);
            self.ready = false;
            self.token.clear();
            self.pending.clear();
            self.focus_request = None;
        }
    }
    pub fn ready(&self) -> bool {
        self.ready && !self.closed
    }
    pub fn finish_boot(&mut self, ticket: &Ticket, token: &str) -> Vec<Action> {
        if !Arc::ptr_eq(&self.generation, &ticket.current)
            || !ticket.current()
            || self.closed
            || token.is_empty()
        {
            return Vec::new();
        }
        self.token = token.into();
        self.ready = true;
        self.pending.drain(..).collect()
    }
    pub fn token(&self) -> &str {
        &self.token
    }
    pub fn tabs(&self) -> &[Tab] {
        &self.tabs
    }
    pub fn pending_count(&self) -> usize {
        self.pending.len()
    }
    pub fn theme(&self) -> &str {
        &self.theme
    }
    pub fn initial_theme(&mut self, name: &str) {
        if !self.theme_selected && VALID_THEMES.contains(&name) {
            self.theme = name.into();
        }
    }
    pub fn active_key(&self) -> Option<&str> {
        self.active.as_deref()
    }
    pub fn loaded_count(&self) -> usize {
        self.tabs.iter().filter(|t| t.loaded).count()
    }
    pub fn theme_script(&self) -> String {
        format!(
            "window.postMessage({{source:'comandos',type:'theme',theme:{}}},'*');",
            serde_json::to_string(&self.theme).unwrap_or_default()
        )
    }
    pub fn queue(&mut self, action: Action) -> Result<Vec<Action>, String> {
        if self.closed {
            return Err("ventana cerrada".into());
        }
        if self.ready() {
            return Ok(vec![action]);
        }
        if self.pending.len() >= 128 {
            return Err("cola de terminales llena".into());
        }
        self.pending.push_back(action);
        Ok(Vec::new())
    }
    pub fn receive_bridge(&mut self, message: BridgeMessage) -> Result<Vec<Action>, String> {
        if self.closed {
            return Err("ventana cerrada".into());
        }
        match message {
            BridgeMessage::Theme(value) => {
                if let Some(name) = value.as_str().filter(|n| VALID_THEMES.contains(n)) {
                    self.theme = name.into();
                    self.theme_selected = true;
                }
                Ok(Vec::new())
            }
            BridgeMessage::Rename { session, label } => {
                let session = text(&session, "session")?;
                if let Some(tab) = self.tabs.iter_mut().find(|t| t.key == session) {
                    tab.label = label;
                }
                Ok(Vec::new())
            }
            BridgeMessage::Open {
                session,
                win,
                label,
            } => {
                let session = text(&session, "session")?;
                if !comandos_desktop::validation::valid_session(session) {
                    return Err("sesión inválida".into());
                }
                if ["local", "hub", "control"].contains(&session) {
                    return Ok(Vec::new());
                }
                let win = if !comandos_core::json::truthy(&win) {
                    "claude"
                } else {
                    text(&win, "win")?
                };
                if !comandos_desktop::validation::valid_window(win) {
                    return Err("ventana inválida".into());
                }
                self.queue(Action::Open {
                    session: session.into(),
                    win: win.into(),
                    label,
                })
            }
        }
    }
    pub fn add_tab(
        &mut self,
        key: &str,
        label: &str,
        session: &str,
        is_hub: bool,
    ) -> Result<(), String> {
        if !self.ready() {
            return Err("terminal token unavailable".into());
        }
        if self.tabs.iter().any(|t| t.key == key) {
            return Ok(());
        }
        self.next_tab = self.next_tab.checked_add(1).ok_or("identidad agotada")?;
        self.tabs.push(Tab {
            key: key.into(),
            label: label.into(),
            session: session.into(),
            is_hub,
            loaded: false,
            instance: self.next_tab,
            dot_color: comandos_desktop::DOT_IDLE,
        });
        Ok(())
    }
    pub fn update_states(&mut self, value: &Value) {
        let Some(rows) = value.as_array() else {
            return;
        };
        for tab in &mut self.tabs {
            if tab.is_hub {
                continue;
            }
            let status = rows
                .iter()
                .rev()
                .find(|row| row.get("session").and_then(Value::as_str) == Some(&tab.key))
                .and_then(|row| row.get("status"))
                .and_then(Value::as_str)
                .unwrap_or("idle");
            tab.dot_color = comandos_desktop::DOT_COLORS
                .iter()
                .find_map(|(name, color)| (*name == status).then_some(*color))
                .unwrap_or(comandos_desktop::DOT_IDLE);
        }
    }
    pub fn take_focus_request(&mut self) -> Option<u64> {
        self.focus_request.take()
    }
    pub fn select_tab(&mut self, key: &str) -> bool {
        if self.closed {
            return false;
        }
        if let Some(tab) = self.tabs.iter_mut().find(|t| t.key == key) {
            tab.loaded = true;
            self.focus_request = Some(tab.instance);
            self.active = Some(key.into());
            return true;
        }
        false
    }
}
fn text<'a>(value: &'a Value, field: &str) -> Result<&'a str, String> {
    value
        .as_str()
        .ok_or_else(|| format!("{field} requiere texto"))
}
