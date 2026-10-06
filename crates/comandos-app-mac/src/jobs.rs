//! Bounded background work; neither HTTP nor tmux waits on the AppKit thread.
#![forbid(unsafe_code)]
use crate::{
    app::{Action, AppConfig, RunMode, Ticket},
    tmux::{TmuxRunner, runner},
};
use comandos_desktop::{Lang, dash_client::DashClient, proc::ProcOutput};
use serde_json::{Value, json};
use std::{
    io::Read,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, SyncSender},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};
pub trait Backend: Send + Sync {
    fn get(&self, path: &str) -> Result<Value, String>;
    fn post(&self, path: &str, body: &Value) -> Result<Value, String>;
    fn tmux(&self, args: &[&str], allowed: &dyn Fn() -> bool) -> Result<ProcOutput, String>;
    fn cancel(&self);
}
pub struct SystemBackend {
    client: DashClient,
    tmux: Box<dyn TmuxRunner>,
}
impl SystemBackend {
    pub fn new(cfg: &AppConfig) -> Result<Self, String> {
        let mode = if matches!(cfg.mode, RunMode::Live) {
            comandos_desktop::mode::RunMode::Live
        } else {
            comandos_desktop::mode::RunMode::Sandbox
        };
        Ok(Self {
            client: DashClient::new(Some(&cfg.dash_url), mode).map_err(dash_error)?,
            tmux: runner(&cfg.mode),
        })
    }
}
impl Backend for SystemBackend {
    fn get(&self, path: &str) -> Result<Value, String> {
        self.client
            .get(path, Duration::from_secs(3))
            .map_err(dash_error)
    }
    fn post(&self, path: &str, body: &Value) -> Result<Value, String> {
        let (status, value) = self
            .client
            .post(path, body, Duration::from_secs(3))
            .map_err(dash_error)?;
        if !(200..300).contains(&status) {
            return Err(format!("HTTP {status}"));
        }
        Ok(value)
    }
    fn tmux(&self, args: &[&str], allowed: &dyn Fn() -> bool) -> Result<ProcOutput, String> {
        self.tmux.run_when(args, allowed)
    }
    fn cancel(&self) {
        self.client.cancel_pending();
    }
}
#[derive(Debug, Clone)]
pub struct Opened {
    pub session: String,
    pub label: String,
    pub is_hub: bool,
    pub raise: bool,
}
pub fn execute_open(
    backend: &dyn Backend,
    action: &Action,
    existing: bool,
    ticket: &Ticket,
) -> Result<Opened, String> {
    let Action::Open {
        session,
        win,
        label,
    } = action
    else {
        return Err("acción no abrible".into());
    };
    check(ticket)?;
    if !existing {
        backend.post("/ensure", &json!({"session":session,"win":win}))?;
        check(ticket)?;
    }
    select_window(backend, session, win, ticket)?;
    check(ticket)?;
    Ok(Opened {
        session: session.clone(),
        label: label.clone().unwrap_or_else(|| session.clone()),
        is_hub: false,
        raise: true,
    })
}
fn select_window(
    backend: &dyn Backend,
    session: &str,
    win: &str,
    ticket: &Ticket,
) -> Result<(), String> {
    check(ticket)?;
    let target = format!("={session}:{win}");
    let first = backend.tmux(&["select-window", "-t", &target], &|| ticket.current())?;
    check(ticket)?;
    if first.code == Some(0) {
        return Ok(());
    }
    if win != "claude" {
        return Ok(());
    }
    let target = format!("={session}");
    let output = backend.tmux(
        &[
            "list-panes",
            "-s",
            "-t",
            &target,
            "-F",
            "#{window_index}|#{pane_current_command}",
        ],
        &|| ticket.current(),
    )?;
    check(ticket)?;
    let output = String::from_utf8_lossy(&output.stdout);
    let index = output
        .lines()
        .find_map(|line| {
            line.split_once('|')
                .filter(|(_, command)| *command == "claude")
                .map(|(idx, _)| idx)
        })
        .unwrap_or("^");
    let target = format!("={session}:{index}");
    backend.tmux(&["select-window", "-t", &target], &|| ticket.current())?;
    check(ticket)
}
fn check(ticket: &Ticket) -> Result<(), String> {
    if ticket.current() {
        Ok(())
    } else {
        Err("operación cancelada".into())
    }
}
pub enum Task {
    Preferences {
        env_lang: String,
    },
    Boot {
        hooks: std::path::PathBuf,
        home: std::path::PathBuf,
    },
    Open {
        action: Action,
        existing: bool,
    },
    NewLocal {
        home: std::path::PathBuf,
        active: Option<String>,
        epoch: u64,
        pid: u32,
    },
    Dump {
        path: std::path::PathBuf,
        html: String,
    },
}
pub enum ResultData {
    Preferences { theme: String, lang: Lang },
    Boot { token: String, hub: Opened },
    Opened(Opened),
    Dumped,
    States(Value),
}
pub struct Delivery {
    pub ticket: Ticket,
    pub result: Result<ResultData, String>,
}
pub struct Jobs {
    sender: Option<SyncSender<(Ticket, Task)>>,
    receiver: Receiver<Delivery>,
    backend: Arc<dyn Backend>,
    stop: Arc<AtomicBool>,
    notified: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}
impl Jobs {
    pub fn new(
        backend: Arc<dyn Backend>,
        notify: Arc<dyn Fn() + Send + Sync>,
    ) -> Result<Self, String> {
        let (tx, rx) = mpsc::sync_channel::<(Ticket, Task)>(8);
        let (out, input) = mpsc::sync_channel(8);
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        let notified = Arc::new(AtomicBool::new(false));
        let pending = notified.clone();
        let service = backend.clone();
        let worker = thread::Builder::new()
            .name("comandos-mac-jobs".into())
            .spawn(move || {
                let mut poll: Option<Ticket> = None;
                let mut next_poll = Instant::now() + Duration::from_secs(3);
                let publish = |delivery: Delivery| {
                    if out.try_send(delivery).is_err() {
                        eprintln!("cola de resultados de ComandOS llena");
                        return;
                    }
                    if !pending.swap(true, Ordering::AcqRel) {
                        notify();
                    }
                };
                while !flag.load(Ordering::Acquire) {
                    if poll.as_ref().is_some_and(Ticket::current) && Instant::now() >= next_poll {
                        if let Some(ticket) = &poll
                            && let Ok(value) = service.get("/state")
                            && ticket.current()
                            && !flag.load(Ordering::Acquire)
                        {
                            publish(Delivery {
                                ticket: ticket.clone(),
                                result: Ok(ResultData::States(value)),
                            });
                        }
                        next_poll = Instant::now() + Duration::from_secs(3);
                    }
                    let wait = if poll.as_ref().is_some_and(Ticket::current) {
                        next_poll.saturating_duration_since(Instant::now())
                    } else {
                        Duration::from_secs(10)
                    };
                    let (ticket, task) = match rx.recv_timeout(wait) {
                        Ok(work) => work,
                        Err(mpsc::RecvTimeoutError::Timeout) => continue,
                        Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    };
                    if !ticket.current() {
                        continue;
                    }
                    let ticket = ticket.with_stop(flag.clone());
                    poll = Some(ticket.clone());
                    let result = execute_task(service.as_ref(), task, &ticket);
                    if ticket.current() && !flag.load(Ordering::Acquire) {
                        publish(Delivery { ticket, result });
                    }
                }
            })
            .map_err(|e| e.to_string())?;
        Ok(Self {
            sender: Some(tx),
            receiver: input,
            backend,
            stop,
            notified,
            worker: Some(worker),
        })
    }
    pub fn submit(&self, ticket: Ticket, task: Task) -> Result<(), String> {
        self.sender
            .as_ref()
            .ok_or("worker cerrado")?
            .try_send((ticket, task))
            .map_err(|_| "cola de operaciones llena".into())
    }
    pub fn drain(&self) -> Vec<Delivery> {
        self.notified.store(false, Ordering::Release);
        self.receiver.try_iter().collect()
    }
    pub fn close(&mut self) {
        self.stop.store(true, Ordering::Release);
        self.backend.cancel();
        self.sender.take();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
impl Drop for Jobs {
    fn drop(&mut self) {
        self.close();
    }
}
fn execute_task(backend: &dyn Backend, task: Task, ticket: &Ticket) -> Result<ResultData, String> {
    match task {
        Task::Dump { path, html } => {
            write_dom(&path, &html, ticket)?;
            Ok(ResultData::Dumped)
        }
        Task::Open { action, existing } => {
            execute_open(backend, &action, existing, ticket).map(ResultData::Opened)
        }
        Task::Preferences { env_lang } => {
            check(ticket)?;
            let prefs = backend.get("/prefs").unwrap_or(Value::Null);
            check(ticket)?;
            let conf = backend.get("/conf").unwrap_or(Value::Null);
            check(ticket)?;
            Ok(ResultData::Preferences {
                theme: comandos_desktop::lang::initial_theme(&prefs).into(),
                lang: comandos_desktop::ui_lang(
                    conf.get("_lang").and_then(Value::as_str),
                    &env_lang,
                ),
            })
        }
        Task::Boot { hooks, home } => {
            check(ticket)?;
            let response = backend.get("/webterm-token").unwrap_or(Value::Null);
            check(ticket)?;
            let token = response
                .get("token")
                .filter(|v| comandos_core::json::truthy(v))
                .map(comandos_core::pomodoro::python_str)
                .unwrap_or_else(|| read_token(&hooks.join("dash-token")));
            if token.is_empty() {
                return Err("terminal token unavailable; tab opens remain queued".into());
            }
            let exists = backend.tmux(&["has-session", "-t", "=local"], &|| ticket.current())?;
            check(ticket)?;
            if exists.code != Some(0) {
                let output = backend.tmux(
                    &[
                        "new-session",
                        "-d",
                        "-s",
                        "local",
                        "-c",
                        &home.to_string_lossy(),
                    ],
                    &|| ticket.current(),
                )?;
                check(ticket)?;
                if output.code != Some(0) {
                    return Err("no pude crear hub local".into());
                }
            }
            Ok(ResultData::Boot {
                token,
                hub: Opened {
                    session: "local".into(),
                    label: "local".into(),
                    is_hub: true,
                    raise: false,
                },
            })
        }
        Task::NewLocal {
            home,
            active,
            epoch,
            pid,
        } => {
            check(ticket)?;
            let mut cwd = home.to_string_lossy().into_owned();
            if let Some(key) = active {
                let target = format!("={key}:");
                let out = backend.tmux(
                    &[
                        "display-message",
                        "-p",
                        "-t",
                        &target,
                        "#{pane_current_path}",
                    ],
                    &|| ticket.current(),
                )?;
                check(ticket)?;
                if out.code == Some(0) {
                    let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
                    if !text.is_empty() {
                        cwd = text;
                    }
                }
            }
            let session = format!("term-{}-{}", pid % 10000, epoch % 100000);
            let out = backend.tmux(&["new-session", "-d", "-s", &session, "-c", &cwd], &|| {
                ticket.current()
            })?;
            check(ticket)?;
            if out.code != Some(0) {
                return Err("tmux fallo".into());
            }
            let label = std::path::Path::new(&cwd)
                .file_name()
                .map(|s| s.to_string_lossy().into_owned())
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| "term".into());
            Ok(ResultData::Opened(Opened {
                session,
                label,
                is_hub: false,
                raise: false,
            }))
        }
    }
}
/// Publish only a complete private dump; existing files/FIFOs/symlinks are never
/// opened or replaced. The temporary file is owned by this invocation.
pub fn write_dom(path: &std::path::Path, html: &str, ticket: &Ticket) -> Result<(), String> {
    use std::{
        io::Write,
        os::unix::fs::{MetadataExt, OpenOptionsExt},
        sync::atomic::AtomicU64,
    };
    static NEXT: AtomicU64 = AtomicU64::new(1);
    check(ticket)?;
    if html.len() > 8 << 20 {
        return Err("DOM demasiado grande".into());
    }
    let parent = path.parent().ok_or("dump sin directorio")?;
    let metadata = std::fs::symlink_metadata(parent).map_err(|e| e.to_string())?;
    if !metadata.is_dir()
        || metadata.uid() != nix::unistd::geteuid().as_raw()
        || metadata.mode() & 0o022 != 0
    {
        return Err("directorio dump inseguro".into());
    }
    let temp = parent.join(format!(
        ".comandos-dom-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    struct Cleanup(std::path::PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&temp)
        .map_err(|e| e.to_string())?;
    let _cleanup = Cleanup(temp.clone());
    for chunk in html.as_bytes().chunks(65536) {
        check(ticket)?;
        file.write_all(chunk).map_err(|e| e.to_string())?;
    }
    file.sync_all().map_err(|e| e.to_string())?;
    check(ticket)?;
    std::fs::hard_link(&temp, path).map_err(|e| e.to_string())?;
    Ok(())
}
fn read_token(path: &std::path::Path) -> String {
    use std::os::unix::fs::OpenOptionsExt;
    let Ok(file) = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(
            (nix::fcntl::OFlag::O_NONBLOCK
                | nix::fcntl::OFlag::O_NOFOLLOW
                | nix::fcntl::OFlag::O_NOCTTY)
                .bits(),
        )
        .open(path)
    else {
        return String::new();
    };
    if !file
        .metadata()
        .is_ok_and(|m| m.is_file() && m.len() <= 65536)
    {
        return String::new();
    }
    let mut text = String::new();
    if file.take(65537).read_to_string(&mut text).is_err() || text.len() > 65536 {
        return String::new();
    }
    comandos_core::text::strip(&text).into()
}

fn dash_error(error: comandos_desktop::dash_client::DashError) -> String {
    use comandos_desktop::dash_client::DashError;
    match error {
        DashError::HttpStatus(status, _) => format!("HTTP {status}"),
        DashError::Cancelled => "operación cancelada".into(),
        DashError::Disconnected => "tablero desconectado".into(),
        _ => "fallo de comunicación con el tablero".into(),
    }
}
