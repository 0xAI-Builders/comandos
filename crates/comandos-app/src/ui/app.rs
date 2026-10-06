use crate::{
    config::{self, AppConfig, RunMode},
    dash_client::DashClient,
    guard::WriteGuard,
    ipc::{self, IpcConsumer, IpcKind, IpcRequest},
    jobs::Jobs,
    poll::{PollUpdate, Poller},
    restore::{self, RestoreCoordinator, RestoreResult},
    snapshot,
    state_files::StateFiles,
    tabs::{TabKind, TabRecord, TabRegistry},
    term::{
        engine::Palette,
        view::{TermOptions, TermView},
    },
    tmux::TmuxCtl,
    ui,
};
use gtk::prelude::*;
use nix::fcntl::Flock;
use serde_json::{Value, json};
use std::{
    cell::{Cell, RefCell},
    collections::{BTreeMap, BTreeSet},
    process::ExitCode,
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Duration,
};

type Handler = Rc<dyn Fn(&Value) -> bool>;
pub struct App {
    pub cfg: AppConfig,
    pub tmux: TmuxCtl,
    pub guard: WriteGuard,
    pub jobs: Jobs,
    pub dash: DashClient,
    pub window: gtk::ApplicationWindow,
    pub paned: gtk::Paned,
    pub notebook: gtk::Notebook,
    pub webview: webkit2gtk::WebView,
    handlers: RefCell<BTreeMap<&'static str, Handler>>,
    state: StateFiles,
    registry: RefCell<TabRegistry>,
    strip: RefCell<ui::tabstrip::TabStripNotebook>,
    terms: RefCell<BTreeMap<String, TermView>>,
    labels: RefCell<BTreeMap<String, gtk::Box>>,
    toolbar: gtk::Box,
    workspace: ui::workspace::GtkWorkspace,
    restore: RefCell<RestoreCoordinator>,
    startup_valid: Cell<bool>,
    workspace_doc: RefCell<Value>,
    pending_workspace: RefCell<Option<Value>>,
    revision: Cell<u64>,
    posting: Cell<bool>,
    dragging: Cell<bool>,
    preferences: RefCell<Value>,
    generation: Arc<AtomicU64>,
    snapshot_busy: Arc<AtomicBool>,
    state_epoch: Arc<AtomicU64>,
    closed: Arc<AtomicBool>,
    ipc_pending: RefCell<BTreeSet<std::path::PathBuf>>,
    ipc_reading: Cell<bool>,
    ipc_seen: RefCell<Vec<IpcRequest>>,
    status: gtk::Label,
}
struct AppRuntime {
    app: Rc<App>,
    poller: Poller,
    sources: Vec<glib::SourceId>,
    monitors: Vec<gio::FileMonitor>,
    _lock: Flock<std::fs::File>,
}
impl AppRuntime {
    fn shutdown(self) {
        self.app.closed.store(true, Ordering::Release);
        for source in self.sources {
            source.remove();
        }
        for monitor in self.monitors {
            monitor.cancel();
        }
        self.poller.stop();
        self.app.dash.cancel_pending();
        self.app.jobs.shutdown(Duration::from_secs(6));
        for term in self.app.terms.borrow().values() {
            term.shutdown();
        }
    }
}
impl App {
    fn install_foundation_handlers(self: &Rc<Self>) {
        use javascriptcore::ValueExt;
        use webkit2gtk::{UserContentManagerExt, WebViewExt};
        let weak = Rc::downgrade(self);
        self.install_handler(
            "tabs",
            Rc::new(move |message| {
                let Some(app) = weak.upgrade() else {
                    return false;
                };
                let args = message.get("args").unwrap_or(message);
                let key = args.get("session").and_then(Value::as_str).unwrap_or("");
                match message.get("command").and_then(Value::as_str) {
                    Some("focus_page") if !key.is_empty() => {
                        if app.cfg.mode() == RunMode::Shadow {
                            return false;
                        }
                        app.select(key);
                        true
                    }
                    Some("tab_reorder") if !key.is_empty() => {
                        let index = args.get("index").and_then(Value::as_u64).unwrap_or(0) as usize;
                        app.reorder_tab(key, index);
                        true
                    }
                    Some("open_xterm_tab") => {
                        if !app.writable() {
                            return false;
                        }
                        let session = if key.is_empty() { "local" } else { key };
                        let web_key = format!("xterm-{session}");
                        app.add_tab(&web_key, &format!("xterm · {session}"), false, None);
                        app.select(&web_key);
                        app.persist();
                        true
                    }
                    Some("new_local_tab") => {
                        app.new_terminal();
                        true
                    }
                    _ => false,
                }
            }),
        );
        if let Some(content) = self.webview.user_content_manager() {
            let weak = Rc::downgrade(self);
            content.connect_script_message_received(Some("centro"), move |_, result| {
                let Some(app) = weak.upgrade() else {
                    return;
                };
                if app.cfg.mode() == RunMode::Shadow {
                    return;
                }
                if let Some(js) = result.js_value()
                    && let Ok(message) = serde_json::from_str::<Value>(js.to_str().as_str())
                    && !app.dispatch(&message)
                {
                    app.status.set_text("Unrecognized dashboard message");
                }
            });
        }
        let plus = gtk::Button::with_label("+");
        let weak = Rc::downgrade(self);
        plus.connect_clicked(move |_| {
            if let Some(app) = weak.upgrade() {
                app.new_terminal();
            }
        });
        self.toolbar.pack_end(&plus, false, false, 0);
    }

    fn new_terminal(self: &Rc<Self>) {
        if !self.writable() {
            return;
        }
        let mut bytes = [0u8; 8];
        if getrandom::fill(&mut bytes).is_err() {
            return;
        }
        let key = format!(
            "term-{}",
            bytes.iter().map(|b| format!("{b:02x}")).collect::<String>()
        );
        let backend = restore::CancelableTmux {
            tmux: self.tmux.clone(),
            cancelled: self.closed.clone(),
        };
        let home = self.private_home();
        let selected = self.current_session();
        let weak = Rc::downgrade(self);
        self.jobs.spawn(
            move || {
                let cwd = selected
                    .as_deref()
                    .and_then(|session| {
                        backend
                            .tmux
                            .read(&[
                                "display-message",
                                "-p",
                                "-t",
                                &format!("={session}:"),
                                "#{pane_current_path}",
                            ])
                            .ok()
                    })
                    .filter(|out| out.ok())
                    .map(|out| out.stdout.trim().to_string())
                    .unwrap_or_else(|| home.display().to_string());
                let plan = restore::RestorePlan::build(
                    &json!({key.clone():"Terminal"}),
                    &json!({key:{"cwd":cwd,"agent":""}}),
                    &BTreeSet::new(),
                    backend.tmux.mode(),
                )
                .map_err(|e| format!("{e:?}"))?;
                Ok::<_, String>(restore::execute(&plan, &backend, &SandboxScopes, &home))
            },
            move |result| {
                if let Some(app) = weak.upgrade().filter(|a| a.writable()) {
                    match result {
                        Ok(result) => {
                            for tab in result.tabs {
                                app.add_tab(
                                    &tab.key,
                                    &tab.label,
                                    tab.attached,
                                    tab.error.as_deref(),
                                );
                                app.select(&tab.key);
                            }
                            app.persist();
                        }
                        Err(e) => app.status.set_text(&e),
                    }
                }
            },
        );
    }

    fn reorder_tab(self: &Rc<Self>, key: &str, index: usize) {
        if !self.writable() {
            return;
        }
        if !self.workspace_doc.borrow().is_null() {
            let changed = comandos_core::workspace::layout::move_tab_group(
                &self.workspace_doc.borrow(),
                key,
                &json!(index),
            );
            if let Ok(Some(doc)) = changed {
                self.commit_workspace(doc, Some(key.into()));
            }
        } else {
            self.registry.borrow_mut().reorder(key, index);
            self.strip.borrow().reorder(key, index as u32);
            if let Some(label) = self.labels.borrow().get(key) {
                self.toolbar.reorder_child(label, index as i32);
            }
            self.persist();
        }
    }
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        cfg: AppConfig,
        tmux: TmuxCtl,
        guard: WriteGuard,
        jobs: Jobs,
        dash: DashClient,
        window: gtk::ApplicationWindow,
        paned: gtk::Paned,
        notebook: gtk::Notebook,
        webview: webkit2gtk::WebView,
    ) -> Rc<Self> {
        let toolbar = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        let status = gtk::Label::new(None);
        let workspace = ui::workspace::GtkWorkspace::new();
        let state = StateFiles::new(cfg.clone(), guard.clone());
        let strip = RefCell::new(ui::tabstrip::TabStripNotebook::new(notebook.clone()));
        Rc::new(Self {
            cfg,
            tmux,
            guard,
            jobs,
            dash,
            window,
            paned,
            notebook,
            webview,
            toolbar,
            status,
            workspace,
            state,
            strip,
            registry: RefCell::new(
                TabRegistry::from_json(&json!({})).unwrap_or_else(|_| unreachable!()),
            ),
            handlers: RefCell::new(BTreeMap::new()),
            terms: RefCell::new(BTreeMap::new()),
            labels: RefCell::new(BTreeMap::new()),
            restore: RefCell::new(RestoreCoordinator::default()),
            startup_valid: Cell::new(false),
            workspace_doc: RefCell::new(Value::Null),
            pending_workspace: RefCell::new(None),
            revision: Cell::new(0),
            posting: Cell::new(false),
            dragging: Cell::new(false),
            preferences: RefCell::new(json!({})),
            generation: Arc::new(AtomicU64::new(0)),
            snapshot_busy: Arc::new(AtomicBool::new(false)),
            state_epoch: Arc::new(AtomicU64::new(0)),
            closed: Arc::new(AtomicBool::new(false)),
            ipc_pending: RefCell::new(BTreeSet::new()),
            ipc_reading: Cell::new(false),
            ipc_seen: RefCell::new(Vec::new()),
        })
    }
    pub fn install_handler(&self, name: &'static str, callback: Handler) {
        self.handlers.borrow_mut().insert(name, callback);
    }
    pub fn dispatch(&self, message: &Value) -> bool {
        if self.closed.load(Ordering::Acquire) || !self.restore.borrow().ready() {
            return false;
        }
        self.handlers.borrow().values().any(|h| h(message))
    }
    fn writable(&self) -> bool {
        self.cfg.writes_allowed()
            && self.startup_valid.get()
            && self.restore.borrow().ready()
            && !self.closed.load(Ordering::Acquire)
    }
    fn persist(&self) {
        if !self.writable() {
            return;
        }
        let epoch = self.state_epoch.fetch_add(1, Ordering::AcqRel) + 1;
        let current = self.state_epoch.clone();
        let state = self.state.clone();
        let value = self.registry.borrow().to_json();
        let closed = self.closed.clone();
        self.jobs.spawn(
            move || {
                if !closed.load(Ordering::Acquire) {
                    state
                        .write_tabs_when(&value, || {
                            !closed.load(Ordering::Acquire)
                                && current.load(Ordering::Acquire) == epoch
                        })
                        .map_err(|e| format!("{e:?}"))
                } else {
                    Ok(())
                }
            },
            |result| {
                if let Err(e) = result {
                    eprintln!("save tabs: {e}");
                }
            },
        );
    }
    fn startup(self: &Rc<Self>) {
        self.restore.borrow_mut().begin();
        let state = self.state.clone();
        let tmux = self.tmux.clone();
        let home = self.private_home();
        let mode = self.cfg.mode();
        let cancel = self.closed.clone();
        let weak = Rc::downgrade(self);
        self.jobs.spawn(
            move || {
                let raw = state.read("app-tabs.json").map_err(|e| format!("{e:?}"))?;
                let mut tabs = if raw.is_null() {
                    json!({})
                } else {
                    TabRegistry::from_json(&raw)
                        .map_err(|e| format!("{e:?}"))?
                        .to_json()
                };
                if tabs.as_object().is_none_or(|o| o.is_empty()) {
                    tabs = json!({"local":"Local"});
                }
                let layouts = state.read_session_snapshot();
                let legacy = state
                    .read("app-tabs-snapshot.json")
                    .ok()
                    .filter(Value::is_object)
                    .unwrap_or_else(|| json!({}));
                let mut snapshots = legacy;
                if let (Some(dest), Some(src)) = (
                    snapshots.as_object_mut(),
                    layouts.get("sessions").unwrap_or(&Value::Null).as_object(),
                ) {
                    dest.extend(src.clone());
                }
                let present = tmux
                    .read(&["list-sessions", "-F", "#{session_name}"])
                    .ok()
                    .filter(|o| o.ok())
                    .map(|o| o.stdout.lines().map(ToString::to_string).collect())
                    .unwrap_or_default();
                let plan = restore::RestorePlan::build(&tabs, &snapshots, &present, mode)
                    .map_err(|e| format!("{e:?}"))?;
                let backend = restore::CancelableTmux {
                    tmux,
                    cancelled: cancel,
                };
                // Sandbox never launches agents, even if a copied fixture includes saved identities.
                let launcher: &dyn restore::ScopeLauncher = if mode == RunMode::Sandbox {
                    &SandboxScopes
                } else {
                    &restore::PaneScopes
                };
                Ok::<_, String>(restore::execute(&plan, &backend, launcher, &home))
            },
            move |result| {
                let Some(app) = weak.upgrade().filter(|a| !a.closed.load(Ordering::Acquire)) else {
                    return;
                };
                match result {
                    Ok(result) => {
                        app.startup_valid.set(true);
                        for tab in result.tabs {
                            app.add_tab(
                                &tab.key,
                                &tab.label,
                                tab.attached && app.cfg.mode() != RunMode::Shadow,
                                tab.error
                                    .as_deref()
                                    .or_else(|| tab.ambiguity.first().map(String::as_str)),
                            );
                        }
                        app.restore.borrow_mut().finish(result.result);
                    }
                    Err(error) => {
                        app.status.set_text(&error);
                        app.restore.borrow_mut().finish(RestoreResult::Failed);
                    }
                }
                if let Some(doc) = app.pending_workspace.borrow_mut().take() {
                    app.apply_workspace(&doc);
                }
                app.persist();
                app.drain_ipc();
            },
        );
    }
    fn private_home(&self) -> std::path::PathBuf {
        self.cfg
            .sandbox_root()
            .map(|r| r.join("home"))
            .unwrap_or_else(|| self.cfg.home().to_path_buf())
    }
    fn mount_web_terminal(self: &Rc<Self>, key: &str, slot: &gtk::Box) {
        if self.cfg.mode() != RunMode::Live {
            slot.add(&gtk::Label::new(Some(
                "Web terminal requires an explicitly configured live backend",
            )));
            return;
        }
        let Some(root) = self.cfg.repo_root().map(std::path::Path::to_path_buf) else {
            slot.add(&gtk::Label::new(Some("Web terminal assets unavailable")));
            return;
        };
        let session = key.strip_prefix("xterm-").unwrap_or("local").to_string();
        if session.is_empty()
            || session.len() > 32
            || !session
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'_' | b'-'))
        {
            slot.add(&gtk::Label::new(Some("Invalid web terminal session")));
            return;
        }
        let cancelled = self.closed.clone();
        let dash = self.dash.clone();
        let weak_slot = slot.downgrade();
        let weak = Rc::downgrade(self);
        self.jobs.spawn(
            move || {
                if cancelled.load(Ordering::Acquire) {
                    return Err("webterm request cancelled".into());
                }
                let backend = DashClient::new(Some("http://127.0.0.1:4779"), RunMode::Live)
                    .map_err(|e| format!("{e:?}"))?;
                if backend.get("/token", Duration::from_millis(400)).is_err() {
                    crate::proc::spawn_detached(
                        root.join("bin/cc-webterm")
                            .to_str()
                            .ok_or("invalid webterm path")?,
                        &[],
                    )
                    .map_err(|e| format!("{e:?}"))?;
                }
                let token = dash
                    .get("/webterm-token", Duration::from_secs(2))
                    .map_err(|e| format!("{e:?}"))?;
                let token = token.get("token").and_then(Value::as_str).unwrap_or("");
                Ok::<_, String>(ui::webview::terminal_uri(
                    &root.join("dash/term.html"),
                    &session,
                    token,
                    "noche",
                ))
            },
            move |result| {
                let (Some(app), Some(slot)) = (weak.upgrade(), weak_slot.upgrade()) else {
                    return;
                };
                if app.closed.load(Ordering::Acquire) {
                    return;
                }
                match result {
                    Ok(uri) => match ui::webview::terminal_view(&app.cfg, &uri) {
                        Ok(view) => {
                            slot.add(&view);
                            slot.show_all();
                        }
                        Err(e) => app.status.set_text(&format!("{e:?}")),
                    },
                    Err(e) => {
                        slot.add(&gtk::Label::new(Some(&e)));
                        slot.show_all();
                    }
                }
            },
        );
    }
    fn current_session(&self) -> Option<String> {
        self.workspace.focused().or_else(|| {
            let current = self.notebook.current_page()?;
            self.registry
                .borrow()
                .ordered_keys()
                .into_iter()
                .find(|key| {
                    self.terms
                        .borrow()
                        .get(key)
                        .is_some_and(|term| self.notebook.page_num(term.widget()) == Some(current))
                })
        })
    }
    fn terminal(&self, key: &str) -> Result<TermView, String> {
        let home = self.private_home();
        let sandbox = self.cfg.mode() == RunMode::Sandbox;
        TermView::new(TermOptions {
            argv: self.tmux.attach_argv(key),
            cwd: home.clone(),
            session: Some(key.into()),
            palette: crate::theme::desktop_theme(
                self.preferences
                    .borrow()
                    .get("theme")
                    .and_then(Value::as_str)
                    .unwrap_or("bruno"),
                &crate::theme::themes_from_file(Some(include_bytes!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/../../config/themes.json"
                )))),
            )
            .map(|theme| theme_palette(&theme))
            .unwrap_or_else(|| {
                Palette::xterm_default(
                    [204, 204, 204],
                    [26, 26, 26],
                    [204, 204, 204],
                    [26, 26, 26],
                    [81, 82, 87],
                )
            }),
            scrollback: 10000,
            preferences: self.preferences.borrow().clone(),
            mode: self.cfg.mode(),
            clear_env: sandbox,
            environment: if sandbox {
                vec![
                    ("HOME".into(), home.display().to_string()),
                    ("PATH".into(), "/usr/bin:/bin".into()),
                    ("SHELL".into(), "/bin/sh".into()),
                    ("TERM".into(), "xterm-256color".into()),
                ]
            } else {
                vec![]
            },
            tmux_size: None,
            before_spawn: None,
            on_title: None,
            on_exit: None,
            on_bell: None,
            on_link: None,
            on_ssh_scroll: None,
        })
        .map_err(|e| format!("{e:?}"))
    }
    fn add_tab(self: &Rc<Self>, key: &str, label: &str, attach: bool, error: Option<&str>) {
        if self.labels.borrow().contains_key(key) {
            return;
        }
        self.registry.borrow_mut().insert(TabRecord {
            key: key.into(),
            label: label.into(),
            favorite: false,
            kind: if key == "local" {
                TabKind::Local
            } else if key.starts_with("xterm-") {
                TabKind::Web
            } else {
                TabKind::Session
            },
        });
        let child: gtk::Widget = if key.starts_with("xterm-") {
            let slot = gtk::Box::new(gtk::Orientation::Vertical, 0);
            self.mount_web_terminal(key, &slot);
            slot.upcast()
        } else if attach {
            match self.terminal(key) {
                Ok(term) => {
                    let widget = term.widget().clone().upcast();
                    self.terms.borrow_mut().insert(key.into(), term);
                    widget
                }
                Err(e) => {
                    self.status.set_text(&e);
                    gtk::Label::new(Some(&e)).upcast()
                }
            }
        } else {
            let button = gtk::Button::with_label(
                error.unwrap_or("Select to attach existing session read-only"),
            );
            let weak = Rc::downgrade(self);
            let k = key.to_string();
            button.connect_clicked(move |_| {
                if let Some(app) = weak.upgrade() {
                    app.attach_selected(&k);
                }
            });
            button.upcast()
        };
        child.set_widget_name(&format!("tab-{key}"));
        self.workspace.register_tab(key, &child);
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 2);
        row.style_context().add_class("strip-tab-label");
        let favorite = gtk::Button::with_label("☆");
        let name = gtk::Button::with_label(label);
        let close = gtk::Button::with_label("×");
        row.pack_start(&favorite, false, false, 0);
        row.pack_start(&name, true, true, 0);
        row.pack_start(&close, false, false, 0);
        let weak = Rc::downgrade(self);
        let k = key.to_string();
        name.connect_clicked(move |_| {
            if let Some(a) = weak.upgrade() {
                a.select(&k);
            }
        });
        let weak = Rc::downgrade(self);
        let k = key.to_string();
        favorite.connect_clicked(move |_| {
            if let Some(a) = weak.upgrade() {
                a.toggle_favorite(&k);
            }
        });
        let weak = Rc::downgrade(self);
        let k = key.to_string();
        close.connect_clicked(move |_| {
            if let Some(a) = weak.upgrade() {
                a.close_tab(&k, true);
            }
        });
        name.add_events(gdk::EventMask::BUTTON_PRESS_MASK);
        let weak = Rc::downgrade(self);
        let k = key.to_string();
        name.connect_button_press_event(move |_, event| {
            if event.event_type() == gdk::EventType::DoubleButtonPress {
                if let Some(a) = weak.upgrade() {
                    a.rename_tab(&k);
                }
                glib::Propagation::Stop
            } else {
                glib::Propagation::Proceed
            }
        });
        self.install_drag(key, &name, &child);
        self.strip
            .borrow_mut()
            .insert(key.into(), &child, &gtk::Label::new(Some(label)).upcast());
        self.toolbar.pack_start(&row, false, false, 0);
        self.labels.borrow_mut().insert(key.into(), row);
        self.toolbar.show_all();
        self.notebook.show_all();
        if !self.workspace_doc.borrow().is_null() {
            self.workspace.refresh();
            self.notebook.hide();
        }
    }
    fn attach_selected(self: &Rc<Self>, key: &str) {
        if self.cfg.mode() != RunMode::Shadow || self.terms.borrow().contains_key(key) {
            return;
        }
        let tmux = self.tmux.clone();
        let key = key.to_string();
        let weak = Rc::downgrade(self);
        self.jobs.spawn(
            move || {
                let exists = tmux
                    .read(&["has-session", "-t", &format!("={key}")])
                    .is_ok_and(|o| o.ok());
                (key, exists)
            },
            move |(key, exists)| {
                let Some(app) = weak.upgrade().filter(|a| !a.closed.load(Ordering::Acquire)) else {
                    return;
                };
                if !exists {
                    app.status.set_text("Session unavailable");
                    return;
                }
                match app.terminal(&key) {
                    Ok(term) => {
                        let widget: gtk::Widget = term.widget().clone().upcast();
                        app.strip.borrow_mut().remove(&key);
                        app.workspace.register_tab(&key, &widget);
                        let label = app
                            .registry
                            .borrow()
                            .to_json()
                            .get(&key)
                            .unwrap_or(&Value::Null)
                            .as_str()
                            .unwrap_or(&key)
                            .to_string();
                        app.strip.borrow_mut().insert(
                            key.clone(),
                            &widget,
                            &gtk::Label::new(Some(&label)).upcast(),
                        );
                        app.install_drop_target(&key, &widget);
                        app.terms.borrow_mut().insert(key.clone(), term);
                        app.workspace.refresh();
                        app.notebook.show_all();
                        app.select(&key);
                    }
                    Err(e) => app.status.set_text(&e),
                }
            },
        );
    }
    fn select(&self, key: &str) {
        if self.strip.borrow().focus(key) {
            if let Some(term) = self.terms.borrow().get(key) {
                term.widget().grab_focus();
            }
        } else {
            self.workspace.select(key);
        }
    }
    fn rename_tab(self: &Rc<Self>, key: &str) {
        if !self.writable() {
            return;
        }
        let dialog = gtk::Dialog::with_buttons(
            Some("Rename"),
            Some(&self.window),
            gtk::DialogFlags::MODAL,
            &[
                ("Cancel", gtk::ResponseType::Cancel),
                ("Save", gtk::ResponseType::Accept),
            ],
        );
        let entry = gtk::Entry::new();
        entry.set_text(
            self.registry
                .borrow()
                .to_json()
                .get(key)
                .unwrap_or(&Value::Null)
                .as_str()
                .unwrap_or(key),
        );
        dialog.content_area().add(&entry);
        dialog.show_all();
        if dialog.run() == gtk::ResponseType::Accept && !entry.text().is_empty() {
            self.registry
                .borrow_mut()
                .rename(key, entry.text().as_str());
            if let Some(row) = self.labels.borrow().get(key)
                && let Some(name) = row
                    .children()
                    .get(1)
                    .and_then(|w| w.clone().downcast::<gtk::Button>().ok())
            {
                name.set_label(entry.text().as_str());
            }
            self.persist();
        }
        dialog.close();
    }
    fn toggle_favorite(self: &Rc<Self>, key: &str) {
        if !self.writable() {
            return;
        }
        let mut favorites = self.registry.borrow().favorite_keys();
        if favorites.iter().any(|k| k == key) {
            favorites.retain(|k| k != key);
        } else {
            favorites.push(key.into());
        }
        let generation = self.generation.fetch_add(1, Ordering::AcqRel) + 1;
        self.registry
            .borrow_mut()
            .apply_favorites(&json!(favorites), generation);
        self.paint_favorites();
        let dash = self.dash.clone();
        let weak = Rc::downgrade(self);
        self.jobs.spawn(
            move || {
                dash.post(
                    "/prefs",
                    &json!({"favorites":favorites}),
                    Duration::from_secs(3),
                )
            },
            move |result| {
                if let (Some(a), Err(e)) = (weak.upgrade(), result) {
                    a.status.set_text(&format!("Favorite: {e:?}"));
                }
            },
        );
    }
    fn paint_favorites(&self) {
        let favorites = self.registry.borrow().favorite_keys();
        for (key, row) in self.labels.borrow().iter() {
            if let Some(button) = row
                .children()
                .first()
                .and_then(|w| w.clone().downcast::<gtk::Button>().ok())
            {
                button.set_label(if favorites.contains(key) {
                    "★"
                } else {
                    "☆"
                });
            }
        }
    }
    fn close_tab(self: &Rc<Self>, key: &str, confirm: bool) {
        if !self.writable() {
            return;
        }
        if confirm
            && ui::confirm::confirm_must_answer(
                Some(self.window.upcast_ref()),
                "Close tab?",
                "The session remains running.",
                "Close",
                "Keep",
            ) != ui::confirm::ConfirmAnswer::Yes
        {
            return;
        }
        let label = self
            .registry
            .borrow()
            .to_json()
            .get(key)
            .and_then(Value::as_str)
            .unwrap_or(key)
            .to_string();
        let key_for_archive = key.to_string();
        let state = self.state.clone();
        let tmux = self.tmux.clone();
        let cancelled = self.closed.clone();
        self.jobs.spawn(move||{
            if cancelled.load(Ordering::Acquire){return Ok(());}
            let target=format!("={key_for_archive}:");
            let cwd=tmux.read(&["display-message","-p","-t",&target,"#{pane_current_path}"]).ok().filter(|o|o.ok()).map(|o|o.stdout.trim().to_string()).unwrap_or_default();
            let command=tmux.read(&["display-message","-p","-t",&target,"#{pane_current_command}"]).ok().filter(|o|o.ok()).map(|o|o.stdout.trim().to_string()).unwrap_or_default();
            let agent=if command=="node"{"codex"}else if command=="cc-acp"{"acp"}else if matches!(command.as_str(),"claude"|"grok"|"opencode"|"gemini"|"agy"|"aider"|"acp"){command.as_str()}else{"claude"};
            state.archive_tab(&json!({"session":key_for_archive,"label":label,"cwd":cwd,"agent":agent,"reason":"closed","ts":std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0,|d|d.as_secs())})).map_err(|e|format!("{e:?}"))?;
            if !cancelled.load(Ordering::Acquire)&& let Ok(Some(owned))=tmux.idle_scratch(&key_for_archive){tmux.kill_owned_session(owned).map_err(|e|format!("{e:?}"))?;}
            Ok::<_,String>(())
        },|result|{if let Err(e)=result{eprintln!("close tab: {e}");}});
        if let Some(term) = self.terms.borrow_mut().remove(key) {
            term.shutdown();
        }
        self.strip.borrow_mut().remove(key);
        self.workspace.remove_tab(key);
        if let Some(row) = self.labels.borrow_mut().remove(key) {
            self.toolbar.remove(&row);
        }
        self.registry.borrow_mut().archive(key, "closed");
        self.persist();
    }
    fn poll(self: &Rc<Self>, update: PollUpdate) {
        match update {
            PollUpdate::Prefs {
                value,
                favorite_generation,
            } => {
                if crate::poll::live_pref_snapshot(&self.preferences.borrow())
                    != crate::poll::live_pref_snapshot(&value)
                {
                    *self.preferences.borrow_mut() = value.clone();
                    let themes = crate::theme::themes_from_file(Some(include_bytes!(concat!(
                        env!("CARGO_MANIFEST_DIR"),
                        "/../../config/themes.json"
                    ))));
                    let theme = crate::theme::desktop_theme(
                        value
                            .get("theme")
                            .and_then(Value::as_str)
                            .unwrap_or("bruno"),
                        &themes,
                    );
                    if let Some(theme) = theme.as_ref() {
                        let provider = gtk::CssProvider::new();
                        if provider
                            .load_from_data(crate::theme::theme_css(theme).as_bytes())
                            .is_ok()
                            && let Some(screen) = gdk::Screen::default()
                        {
                            gtk::StyleContext::add_provider_for_screen(
                                &screen,
                                &provider,
                                gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
                            );
                        }
                    }
                    for term in self.terms.borrow().values() {
                        term.set_preferences(&value);
                        if let Some(theme) = theme.as_ref() {
                            term.set_palette(theme_palette(theme));
                        }
                    }
                }
                if let Some(f) = value.get("favorites") {
                    self.registry
                        .borrow_mut()
                        .apply_favorites(f, favorite_generation);
                    self.paint_favorites();
                }
            }
            PollUpdate::Workspace(value) => self.apply_workspace(&value),
            PollUpdate::Notices { badge, .. } => self.status.set_text(&format!("Notices: {badge}")),
            PollUpdate::State(value) => {
                for item in value
                    .as_array()
                    .or_else(|| {
                        value
                            .get("sessions")
                            .or_else(|| value.get("items"))
                            .and_then(Value::as_array)
                    })
                    .into_iter()
                    .flatten()
                {
                    if let Some(key) = item.get("session").unwrap_or(&Value::Null).as_str()
                        && let Some(row) = self.labels.borrow().get(key)
                    {
                        row.set_tooltip_text(item.get("status").unwrap_or(&Value::Null).as_str());
                    }
                }
            }
            PollUpdate::Marks(_) => {}
        }
    }
    fn apply_workspace(&self, value: &Value) {
        if !self.restore.borrow().ready() || self.posting.get() || self.dragging.get() {
            *self.pending_workspace.borrow_mut() = Some(value.clone());
            return;
        }
        let doc = value.get("document").unwrap_or(value);
        if comandos_core::workspace::validate_document(doc).is_err() {
            return;
        }
        self.revision.set(
            value
                .get("revision")
                .unwrap_or(&Value::Null)
                .as_u64()
                .unwrap_or(0),
        );
        self.workspace.apply(doc);
        *self.workspace_doc.borrow_mut() = doc.clone();
        self.notebook.hide();
        self.workspace.widget().show_all();
    }
    fn commit_workspace(self: &Rc<Self>, doc: Value, focus: Option<String>) {
        if self.posting.get() || !self.restore.borrow().ready() {
            return;
        }
        if self.cfg.mode() == RunMode::Shadow {
            self.workspace.commit(&doc, focus.as_deref());
            return;
        }
        if !self.writable() {
            return;
        }
        let mut bytes = [0u8; 12];
        if getrandom::fill(&mut bytes).is_err() {
            self.status.set_text("No request identity available");
            return;
        }
        let request_id = bytes.iter().map(|b| format!("{b:02x}")).collect::<String>();
        self.posting.set(true);
        let dash = self.dash.clone();
        let revision = self.revision.get();
        let weak = Rc::downgrade(self);
        self.jobs.spawn(
            move || {
                dash.post(
                    "/workspace",
                    &json!({"requestId":request_id,"expectedRevision":revision,"document":doc}),
                    Duration::from_secs(5),
                )
            },
            move |response| {
                let Some(app) = weak.upgrade().filter(|a| !a.closed.load(Ordering::Acquire)) else {
                    return;
                };
                app.posting.set(false);
                match response {
                    Ok((status, value)) => {
                        let current = if status == 409 {
                            value.get("current")
                        } else if (200..300).contains(&status) {
                            Some(&value)
                        } else {
                            None
                        };
                        if let Some(current) = current {
                            app.apply_workspace(current);
                            if let Some(focus) = focus.as_deref() {
                                app.workspace.select(focus);
                            }
                        }
                        if status == 409 {
                            app.status.set_text(
                                "Another device changed the layout; showing current layout",
                            );
                        } else if !(200..300).contains(&status) {
                            app.status.set_text(&format!("Workspace HTTP {status}"));
                        }
                    }
                    Err(e) => app.status.set_text(&format!("Workspace: {e:?}")),
                }
                // A queued poll may be older than the authoritative POST response.
                if let Some(pending) = app.pending_workspace.borrow_mut().take()
                    && pending
                        .get("revision")
                        .unwrap_or(&Value::Null)
                        .as_u64()
                        .is_some_and(|r| r > app.revision.get())
                {
                    app.apply_workspace(&pending);
                }
            },
        );
    }
    fn install_drag(self: &Rc<Self>, key: &str, source: &gtk::Button, target: &gtk::Widget) {
        let targets = [gtk::TargetEntry::new(
            "application/x-comandos-tab",
            gtk::TargetFlags::SAME_APP,
            0,
        )];
        source.drag_source_set(
            gdk::ModifierType::BUTTON1_MASK,
            &targets,
            gdk::DragAction::MOVE,
        );
        let k = key.to_string();
        source.connect_drag_data_get(move |_, _, data, _, _| {
            data.set_text(&k);
        });
        let weak = Rc::downgrade(self);
        source.connect_drag_begin(move |source, _| {
            if let Some(app) = weak.upgrade() {
                app.dragging.set(true);
                source.style_context().add_class("lifted");
            }
        });
        let weak = Rc::downgrade(self);
        source.connect_drag_end(move |source, _| {
            source.style_context().remove_class("lifted");
            if let Some(app) = weak.upgrade() {
                app.finish_drag();
            }
        });
        let weak = Rc::downgrade(self);
        source.connect_drag_failed(move |source, _, _| {
            source.style_context().remove_class("lifted");
            if let Some(app) = weak.upgrade() {
                app.finish_drag();
            }
            glib::Propagation::Proceed
        });
        self.install_drop_target(key, target);
        self.install_drop_target(key, source.upcast_ref());
    }
    fn finish_drag(&self) {
        self.dragging.set(false);
        if !self.posting.get() {
            let pending = self.pending_workspace.borrow_mut().take();
            if let Some(pending) = pending {
                self.apply_workspace(&pending);
            }
        }
    }
    fn install_drop_target(self: &Rc<Self>, key: &str, target: &gtk::Widget) {
        let targets = [gtk::TargetEntry::new(
            "application/x-comandos-tab",
            gtk::TargetFlags::SAME_APP,
            0,
        )];
        target.drag_dest_set(gtk::DestDefaults::ALL, &targets, gdk::DragAction::MOVE);
        let weak = Rc::downgrade(self);
        let k = key.to_string();
        target.connect_drag_data_received(move |widget, context, x, y, data, _, time| {
            let success = if let (Some(app), Some(moved)) = (weak.upgrade(), data.text()) {
                app.dragging.set(false);
                let width = widget.allocated_width().max(1) as f64;
                let height = widget.allocated_height().max(1) as f64;
                let target = crate::workspace_view::dock_target(
                    &json!({"groups":[{"id":"hit","tree":{"type":"tab","tabId":k}}]}),
                    f64::from(x) / width,
                    f64::from(y) / height,
                    &BTreeSet::from([moved.to_string()]),
                );
                if let Some(target) = target {
                    let edge = match target.edge {
                        crate::workspace_view::DockEdge::Left => "left",
                        crate::workspace_view::DockEdge::Right => "right",
                        crate::workspace_view::DockEdge::Top => "top",
                        crate::workspace_view::DockEdge::Bottom => "bottom",
                        crate::workspace_view::DockEdge::Center => "right",
                    };
                    let index = app
                        .registry
                        .borrow()
                        .ordered_keys()
                        .iter()
                        .position(|key| key == &k);
                    if app.workspace_doc.borrow().is_null()
                        && let Some(index) = index
                    {
                        app.reorder_tab(moved.as_str(), index);
                        context.drag_finish(true, false, time);
                        return;
                    }
                    let result = comandos_core::workspace::layout::move_tab(
                        &app.workspace_doc.borrow(),
                        moved.as_str(),
                        &k,
                        edge,
                    );
                    if let Ok(doc) = result {
                        app.commit_workspace(doc, Some(moved.to_string()));
                        true
                    } else {
                        false
                    }
                } else {
                    false
                }
            } else {
                false
            };
            context.drag_finish(success, false, time);
        });
    }
    fn snapshot(self: &Rc<Self>, tabs_only: bool) {
        if !self.writable() || self.snapshot_busy.swap(true, Ordering::AcqRel) {
            return;
        }
        let epoch = self.state_epoch.load(Ordering::Acquire);
        let current = self.state_epoch.clone();
        let keys = self.registry.borrow().ordered_keys();
        let state = self.state.clone();
        let tmux = self.tmux.clone();
        let home = self.private_home();
        let closed = self.closed.clone();
        let busy = self.snapshot_busy.clone();
        let weak = Rc::downgrade(self);
        self.jobs.spawn(
            move || {
                let result = (|| {
                    let old=state.read_session_snapshot();
                    let inspector = comandos_runtime::pane_snapshot::PaneInspector::new(
                        &home,
                        std::path::Path::new("/proc"),
                    )
                    .map_err(|_| "process metadata unavailable".to_string())?;
                    let mut sessions = serde_json::Map::new();
                    let mut tabs = serde_json::Map::new();
                    for key in keys
                        .iter()
                        .filter(|k| k.as_str() == "local" || k.starts_with("term-"))
                    {
                        if closed.load(Ordering::Acquire) {
                            return Ok(());
                        }
                        let captured = snapshot::capture_session(&tmux, key, &inspector)
                            .ok()
                            .map(|v| {
                                snapshot::carry_pane_keys(
                                    snapshot::carry_resume_ids(
                                        v,
                                        old.get("sessions")
                                            .and_then(|s| s.get(key))
                                            .unwrap_or(&Value::Null),
                                    ),
                                    old.get("sessions")
                                        .and_then(|s| s.get(key))
                                        .unwrap_or(&Value::Null),
                                )
                            })
                            .or_else(|| {
                                old.get("sessions")
                                    .unwrap_or(&Value::Null)
                                    .get(key)
                                    .cloned()
                            });
                        if let Some(captured) = captured {
                            if let Some(pane) = captured
                                .get("windows")
                                .unwrap_or(&Value::Null)
                                .as_array()
                                .and_then(|w| w.iter().find(|w| w["active"] == true))
                                .and_then(|w| w["panes"].as_array())
                                .and_then(|p| p.iter().find(|p| p["active"] == true))
                            {
                                tabs.insert(key.clone(), pane.clone());
                            }
                            sessions.insert(key.clone(), captured);
                        }
                    }
                    if closed.load(Ordering::Acquire) {
                        return Ok(());
                    }
                    if tabs_only {
                        state
                            .write_snapshot_when(
                                "app-tabs-snapshot.json",
                                &Value::Object(tabs),
                                || {
                                    !closed.load(Ordering::Acquire)
                                        && current.load(Ordering::Acquire) == epoch
                                },
                            )
                            .map_err(|e| format!("{e:?}"))?;
                    } else {
                        let snapshot = json!({"version":2,"sessions":sessions,"saved_at":std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0,|d|d.as_secs())});
                        if comandos_core::workspace::snapshot::check_snapshot(&snapshot)
                            != comandos_core::workspace::snapshot::Snapshot::Valid
                        {
                            return Err("refusing incomplete snapshot".into());
                        }
                        state
                            .write_snapshot_when("app-sessions-v2.json", &snapshot, || {
                                !closed.load(Ordering::Acquire)
                                    && current.load(Ordering::Acquire) == epoch
                            })
                            .map_err(|e| format!("{e:?}"))?;
                    }
                    Ok(())
                })();
                busy.store(false, Ordering::Release);
                result
            },
            move |result: Result<(), String>| {
                if let (Some(a), Err(e)) = (weak.upgrade(), result) {
                    a.status.set_text(&format!("Snapshot: {e}"));
                }
            },
        );
    }
    fn drain_ipc(self: &Rc<Self>) {
        if !self.writable() || self.ipc_reading.replace(true) {
            return;
        }
        let paths = std::mem::take(&mut *self.ipc_pending.borrow_mut());
        if paths.is_empty() {
            self.ipc_reading.set(false);
            return;
        }
        let weak = Rc::downgrade(self);
        self.jobs.spawn(
            move || {
                paths
                    .into_iter()
                    .filter_map(|p| ipc::read_request(&p).ok())
                    .collect::<Vec<_>>()
            },
            move |requests| {
                let Some(app) = weak.upgrade() else {
                    return;
                };
                app.ipc_reading.set(false);
                for request in requests {
                    if app.ipc_seen.borrow().contains(&request) {
                        continue;
                    }
                    if app.handle_ipc(&request) {
                        let consumer = IpcConsumer::new(app.cfg.mode(), app.guard.clone());
                        let request_for_consume = request.clone();
                        app.jobs.spawn(
                            move || consumer.consume(&request_for_consume),
                            |result| {
                                if let Err(e) = result {
                                    eprintln!("IPC consume: {e:?}");
                                }
                            },
                        );
                        let mut seen = app.ipc_seen.borrow_mut();
                        seen.push(request);
                        if seen.len() > 64 {
                            seen.remove(0);
                        }
                    }
                }
            },
        );
    }
    fn handle_ipc(self: &Rc<Self>, request: &IpcRequest) -> bool {
        if !self.writable() {
            return false;
        }
        let key = request
            .payload
            .get("session")
            .unwrap_or(&Value::Null)
            .as_str()
            .unwrap_or("");
        match request.kind {
            IpcKind::TabClose => {
                let keys = request
                    .payload
                    .get("sessions")
                    .unwrap_or(&Value::Null)
                    .as_array()
                    .map(|v| v.iter().filter_map(Value::as_str).collect::<Vec<_>>())
                    .unwrap_or_else(|| vec![key]);
                for key in keys {
                    self.close_tab(key, false);
                }
                true
            }
            IpcKind::Focus | IpcKind::TabOpen if !key.is_empty() => {
                if self.labels.borrow().contains_key(key) {
                    if request.kind == IpcKind::Focus {
                        self.select(key);
                        self.window.present();
                    }
                } else {
                    self.open_existing(
                        key,
                        request
                            .payload
                            .get("label")
                            .unwrap_or(&Value::Null)
                            .as_str()
                            .unwrap_or(key),
                        request.kind == IpcKind::Focus,
                    );
                }
                true
            }
            IpcKind::Command => self.dispatch(&request.payload),
            _ => false,
        }
    }
    fn open_existing(self: &Rc<Self>, key: &str, label: &str, focus: bool) {
        let tmux = self.tmux.clone();
        let key = key.to_string();
        let label = label.to_string();
        let weak = Rc::downgrade(self);
        self.jobs.spawn(
            move || {
                let exists = tmux
                    .read(&["has-session", "-t", &format!("={key}")])
                    .is_ok_and(|o| o.ok());
                (key, label, exists)
            },
            move |(key, label, exists)| {
                if let Some(app) = weak.upgrade().filter(|a| a.writable()) {
                    app.add_tab(
                        &key,
                        &label,
                        exists,
                        (!exists).then_some("Session unavailable"),
                    );
                    if focus {
                        app.select(&key);
                    }
                    app.persist();
                }
            },
        );
    }
}
// Copied sandbox state may contain real agent identities. Keep those panels as shells.
struct SandboxScopes;
impl restore::ScopeLauncher for SandboxScopes {
    fn resume(&self, _: &Value, _: &std::path::Path) -> Option<String> {
        None
    }
    fn argv(&self, _: &str, _: &str) -> Result<Vec<String>, String> {
        Ok(vec!["/bin/true".into()])
    }
}
pub fn run(args: &[String], default_live: bool) -> ExitCode {
    let cfg = match config::parse_args(args, default_live, &|k| std::env::var(k).ok()) {
        Ok(cfg) => cfg,
        Err(e) => {
            eprintln!("comandos-app: {e}");
            return ExitCode::from(2);
        }
    };
    let display = std::env::var("DISPLAY").unwrap_or_default();
    let guard = WriteGuard::from_config(&cfg, &display);
    let lock = match ui::window::instance_lock(&cfg, &guard, &display) {
        Ok(lock) => lock,
        Err(ui::window::InstanceError::Contended) => {
            ui::window::activate_existing(&cfg, &crate::proc::run);
            return ExitCode::SUCCESS;
        }
        Err(error) => {
            eprintln!("comandos-app: instance lock: {error:?}");
            return ExitCode::from(2);
        }
    };
    gdk::set_allowed_backends("x11");
    glib::set_prgname(Some(cfg.wm_class()));
    if let Err(e) = gtk::init() {
        eprintln!("comandos-app: GTK: {e}");
        return ExitCode::from(2);
    }
    let application = gtk::Application::new(None, gio::ApplicationFlags::NON_UNIQUE);
    let lock = Rc::new(RefCell::new(Some(lock)));
    application.connect_activate(move |application| {
        let Some(lock) = lock.borrow_mut().take() else {
            return;
        };
        let Ok(tmux) = TmuxCtl::from_config(&cfg, &|k| std::env::var(k).ok()) else {
            application.quit();
            return;
        };
        if let Err(e) = tmux.prepare_socket_dir(&guard) {
            eprintln!("comandos-app: {e:?}");
            application.quit();
            return;
        }
        let Ok(dash) = DashClient::new(cfg.dash_url(), cfg.mode()) else {
            application.quit();
            return;
        };
        let window = ui::window::create(application, &cfg);
        let paned = gtk::Paned::new(gtk::Orientation::Horizontal);
        paned.set_wide_handle(true);
        let notebook = gtk::Notebook::new();
        notebook.set_hexpand(true);
        notebook.set_vexpand(true);
        let webview = match ui::webview::create(&cfg) {
            Ok(w) => w,
            Err(e) => {
                eprintln!("comandos-app: WebKit: {e:?}");
                application.quit();
                return;
            }
        };
        let app = App::new(
            cfg.clone(),
            tmux,
            guard.clone(),
            Jobs::new(2, "app"),
            dash.clone(),
            window.clone(),
            paned.clone(),
            notebook.clone(),
            webview.clone(),
        );
        let terminals = gtk::Box::new(gtk::Orientation::Vertical, 0);
        terminals.pack_start(&app.toolbar, false, false, 0);
        terminals.pack_start(&notebook, true, true, 0);
        terminals.pack_start(app.workspace.widget(), true, true, 0);
        terminals.pack_start(&app.status, false, false, 0);
        paned.pack1(&webview, true, false);
        paned.pack2(&terminals, true, false);
        let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
        content.pack_start(&ui::window::header(&window), false, false, 0);
        content.pack_start(&paned, true, true, 0);
        window.add(&content);
        app.install_foundation_handlers();
        let weak = Rc::downgrade(&app);
        app.workspace.on_resize(Rc::new(move |group, path, ratio| {
            if let Some(app) = weak.upgrade() {
                let result = comandos_core::workspace::layout::resize_split(
                    &app.workspace_doc.borrow(),
                    group,
                    &json!(path),
                    &json!(ratio),
                );
                if let Ok(doc) = result {
                    app.commit_workspace(doc, None);
                }
            }
        }));
        // Registro T13: bridge actions beyond the connected foundation.
        // Registro T14: advanced quick/local commands.
        // Registro T15: extensions.
        // Registro T16: visual snapshots.
        // Registro T17: operator actions.
        // Registro T18: native notifications.
        let (poller, rx) = Poller::start(dash, app.generation.clone());
        let weak = Rc::downgrade(&app);
        let poll = glib::timeout_add_local(Duration::from_millis(100), move || {
            if let Some(app) = weak.upgrade() {
                let mut batch = crate::poll::PollBatch::default();
                for update in rx.try_iter().take(256) {
                    batch.push(update);
                }
                for update in batch.drain() {
                    app.poll(update);
                }
                app.drain_ipc();
                glib::ControlFlow::Continue
            } else {
                glib::ControlFlow::Break
            }
        });
        let mut sources = vec![poll];
        for (seconds, tabs_only) in [(5, false), (30, true)] {
            let weak = Rc::downgrade(&app);
            sources.push(glib::timeout_add_local(
                Duration::from_secs(seconds),
                move || {
                    if let Some(app) = weak.upgrade() {
                        app.snapshot(tabs_only);
                        glib::ControlFlow::Continue
                    } else {
                        glib::ControlFlow::Break
                    }
                },
            ));
        }
        let mut monitors = Vec::new();
        let file = gio::File::for_path(cfg.hooks_dir());
        if let Ok(monitor) =
            file.monitor_directory(gio::FileMonitorFlags::NONE, gio::Cancellable::NONE)
        {
            let weak = Rc::downgrade(&app);
            monitor.connect_changed(move |_, file, _, event| {
                if matches!(
                    event,
                    gio::FileMonitorEvent::Created
                        | gio::FileMonitorEvent::ChangesDoneHint
                        | gio::FileMonitorEvent::MovedIn
                ) && let (Some(app), Some(path)) = (weak.upgrade(), file.path())
                    && matches!(
                        path.file_name().and_then(|s| s.to_str()),
                        Some(
                            "app-focus.json"
                                | "app-tab-open.json"
                                | "app-tab-close.json"
                                | "app-command.json"
                        )
                    )
                {
                    app.ipc_pending.borrow_mut().insert(path);
                    app.drain_ipc();
                }
            });
            monitors.push(monitor);
        }
        let runtime = Rc::new(RefCell::new(Some(AppRuntime {
            app: app.clone(),
            poller,
            sources,
            monitors,
            _lock: lock,
        })));
        let close_runtime = runtime.clone();
        window.connect_delete_event(move |_, _| {
            if let Some(runtime) = close_runtime.borrow_mut().take() {
                runtime.shutdown();
            }
            glib::Propagation::Proceed
        });
        let destroy_runtime = runtime.clone();
        window.connect_destroy(move |_| {
            if let Some(runtime) = destroy_runtime.borrow_mut().take() {
                runtime.shutdown();
            }
        });
        window.show_all();
        app.workspace.widget().hide();
        app.startup();
    });
    application.run();
    ExitCode::SUCCESS
}

fn theme_palette(theme: &crate::theme::ThemeTokens) -> Palette {
    let color = |name: &str, fallback: [u8; 3]| {
        theme
            .values
            .get(name)
            .and_then(Value::as_str)
            .and_then(parse_color)
            .unwrap_or(fallback)
    };
    let bg = color("bg", [26, 26, 26]);
    let mut palette = Palette::xterm_default(
        color("fg", [204, 204, 204]),
        bg,
        color("text", [204, 204, 204]),
        bg,
        color("brand", [81, 82, 87]),
    );
    for (slot, hex) in palette.ansi.iter_mut().zip(theme.ansi.iter()) {
        if let Some(rgb) = parse_color(hex) {
            *slot = rgb;
        }
    }
    palette
}
fn parse_color(hex: &str) -> Option<[u8; 3]> {
    let hex = hex.strip_prefix('#')?;
    if hex.len() != 6 {
        return None;
    }
    Some([
        u8::from_str_radix(hex.get(..2)?, 16).ok()?,
        u8::from_str_radix(hex.get(2..4)?, 16).ok()?,
        u8::from_str_radix(hex.get(4..6)?, 16).ok()?,
    ])
}
