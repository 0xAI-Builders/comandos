use super::*;
use javascriptcore::ValueExt;
use ui::{
    app_commands::{self as commands, CommandError},
    bridge::BridgeMsg,
};
use webkit2gtk::{UserContentManagerExt, WebViewExt};

impl App {
    pub(super) fn dispatch_message(self: &Rc<Self>, message: &Value) -> Result<(), CommandError> {
        if !self.writable() {
            return Err(CommandError::Refused(
                "restore not ready, shadow, or closed".into(),
            ));
        }
        if message.get("command").is_some() {
            // Compatibility foundation actions are not added to the original command catalog.
            let args = message.get("args").unwrap_or(message);
            match message.get("command").and_then(Value::as_str) {
                Some("quick_terminal" | "quickTerminal") => {
                    self.quick_terminal();
                    return Ok(());
                }
                Some("sortMenu") => {
                    self.sort_menu(None);
                    return Ok(());
                }
                Some("sort_tabs") => {
                    self.sort_tabs(args.get("by").and_then(Value::as_str));
                    return Ok(());
                }
                Some("close_split") => {
                    let session = commands::session_arg(args, "")?;
                    let pane = commands::string_arg(args, "pane", "")?;
                    if !commands::valid_pane(pane) {
                        return Err(CommandError::Invalid("invalid pane".into()));
                    }
                    self.close_split(session, pane);
                    return Ok(());
                }
                Some("open_tab") => {
                    let session = commands::session_arg(args, "")?;
                    let window = commands::string_arg(
                        args,
                        "window",
                        commands::string_arg(args, "win", "claude")?,
                    )?;
                    let label = commands::string_arg(args, "label", session)?;
                    self.open_tab(session, window, label, true);
                    return Ok(());
                }
                _ => {}
            }
            return self.handlers.dispatch(&commands::parse_command(message)?);
        }
        self.dispatch_bridge(ui::bridge::parse_bridge(&message.to_string())?)
    }
    fn dispatch_bridge(self: &Rc<Self>, message: BridgeMsg) -> Result<(), CommandError> {
        if !self.writable() {
            return Err(CommandError::Refused(
                "restore not ready, shadow, or closed".into(),
            ));
        }
        match message {
            BridgeMsg::ButtonStyle(value) => self.bridge_preference("button_style", &value),
            BridgeMsg::Theme(value) => self.bridge_preference("theme", &value),
            BridgeMsg::HeaderAction(action) => match action.as_str() {
                "quickTerminal" => self.quick_terminal(),
                "sortMenu" => self.sort_menu(None),
                "newSession" => return self.handlers.invoke("T14.wizard", &json!({})),
                "notices" => return self.handlers.invoke("T17.notices", &json!({})),
                "chains" => return self.handlers.invoke("T18.chains", &json!({})),
                "analytics" => return self.handlers.invoke("T18.analytics", &json!({})),
                _ => return Err(CommandError::Unknown(action)),
            },
            BridgeMsg::OpenSession {
                session,
                window,
                label,
            } => self.open_tab(
                &session,
                &window,
                label.as_deref().unwrap_or(&session),
                true,
            ),
            BridgeMsg::Rename { session, label } => {
                if let Some(tab) = self.labels.borrow().get(&session) {
                    tab.set_text(&label);
                    self.registry.borrow_mut().rename(&session, &label);
                    self.persist();
                }
            }
            BridgeMsg::Extensions {
                session,
                pane,
                harness,
            } => {
                return self.handlers.invoke(
                    "T17.extensions",
                    &json!({"session":session,"pane":pane,"harness":harness}),
                );
            }
            BridgeMsg::Reader { action, id, on } => {
                return self
                    .handlers
                    .invoke("T18.reader", &json!({"action":action,"id":id,"on":on}));
            }
            BridgeMsg::SidebarTerm(value) => {
                return self.handlers.invoke("T18.sidebar_term", &value);
            }
            BridgeMsg::LeftPanel(action) => {
                return self
                    .handlers
                    .invoke("T18.left_panel", &json!({"action":action}));
            }
            BridgeMsg::ChainModal(value) => return self.handlers.invoke("T18.chain_modal", &value),
            BridgeMsg::OpenUrl { url, label, modal } => {
                return self.handlers.invoke(
                    if modal {
                        "T18.web_modal"
                    } else {
                        "T18.web_tab"
                    },
                    &json!({"url":url,"label":label}),
                );
            }
        }
        Ok(())
    }
    fn bridge_preference(self: &Rc<Self>, key: &str, value: &str) {
        let mut prefs = self.preferences.borrow().clone();
        if let Some(object) = prefs.as_object_mut() {
            object.insert(key.into(), json!(value));
        }
        if key == "theme"
            && let Some(settings) = gtk::Settings::default()
        {
            settings.set_gtk_application_prefer_dark_theme(value != "dia");
        }
        let received = self.prefs_received.get();
        self.poll(PollUpdate::Prefs {
            value: prefs,
            favorite_generation: self.generation.load(Ordering::Acquire),
        });
        // This is a local paint request, not evidence of a completed GET /prefs.
        self.prefs_received.set(received);
    }
    pub(in crate::ui) fn install_app_commands(self: &Rc<Self>) {
        for name in commands::COMMAND_NAMES {
            let weak = Rc::downgrade(self);
            self.install_handler(
                name,
                Rc::new(move |args| {
                    weak.upgrade()
                        .ok_or_else(|| CommandError::Refused("app destroyed".into()))?
                        .run_app_command(name, args)
                }),
            );
        }
        let weak = Rc::downgrade(self);
        let signal = self.notebook.connect_switch_page(move |_, page, _| {
            if let Some(app) = weak.upgrade().filter(|a| !a.closed.load(Ordering::Acquire)) {
                let mut pages = app.mru_pages.borrow_mut();
                pages.retain(|old| old != page);
                pages.push(page.clone());
                if pages.len() > 2 {
                    pages.remove(0);
                }
            }
        });
        self.protocol_signals
            .borrow_mut()
            .push((self.notebook.clone().upcast(), signal));
    }
    fn run_app_command(self: &Rc<Self>, name: &str, args: &Value) -> Result<(), CommandError> {
        if !self.writable() {
            return Err(CommandError::Refused(
                "restore not ready, shadow, or closed".into(),
            ));
        }
        if let Some(consumer) = commands::deferred_consumer(name) {
            return self
                .handlers
                .invoke(consumer, &commands::deferred_arguments(name, args)?);
        }
        match name {
            "split" | "split_ssh" => {
                let side = commands::string_arg(args, "side", "right")?.to_string();
                commands::split_flags(&side)?;
                let session = self
                    .current_session()
                    .ok_or_else(|| CommandError::Invalid("no current terminal".into()))?;
                let tmux = self.tmux.clone();
                let closed = self.closed.clone();
                let ssh = name == "split_ssh";
                let weak = Rc::downgrade(self);
                self.jobs.spawn(
                    move || {
                        commands::execute_split(&tmux, &session, &side, ssh, || {
                            closed.load(Ordering::Acquire)
                        })
                    },
                    move |result| {
                        if let Some(app) =
                            weak.upgrade().filter(|a| !a.closed.load(Ordering::Acquire))
                            && let Err(error) = result
                        {
                            app.status.set_text(&error.to_string());
                        }
                    },
                );
            }
            "select_pane" => {
                let pane = commands::string_arg(args, "pane", "")?.to_string();
                if !commands::valid_pane(&pane) {
                    return Err(CommandError::Invalid("invalid pane id".into()));
                }
                let tmux = self.tmux.clone();
                let closed = self.closed.clone();
                let weak = Rc::downgrade(self);
                self.jobs.spawn(
                    move || {
                        commands::execute_select_pane(&tmux, &pane, closed.load(Ordering::Acquire))
                    },
                    move |result| {
                        if let Some(app) =
                            weak.upgrade().filter(|a| !a.closed.load(Ordering::Acquire))
                            && let Err(error) = result
                        {
                            app.status.set_text(&error.to_string());
                        }
                    },
                );
            }
            "kill_pane" => {
                let current = self.current_session().unwrap_or_else(|| "local".into());
                let session = commands::session_arg(args, &current)?.to_string();
                let tmux = self.tmux.clone();
                let closed = self.closed.clone();
                let weak = Rc::downgrade(self);
                self.jobs.spawn(
                    move || {
                        if closed.load(Ordering::Acquire) {
                            return Err(CommandError::Refused("closed".into()));
                        }
                        commands::active_pane(&tmux, &session).map(|pane| (session, pane))
                    },
                    move |result| {
                        if let Some(app) = weak.upgrade().filter(|a| a.writable()) {
                            match result {
                                Ok((session, Some(pane))) => app.close_split(&session, &pane),
                                Ok((_, None)) => app.status.set_text("No active pane"),
                                Err(error) => app.status.set_text(&error.to_string()),
                            }
                        }
                    },
                );
            }
            "toggle_window" => {
                self.with_current_term(|term| {
                    term.feed(b"\x02l")
                        .map_err(|e| CommandError::Failed(format!("{e:?}")))
                })?;
            }
            "next_tab" | "prev_tab" => {
                let n = self.notebook.n_pages();
                if n > 0 {
                    let current = self.notebook.current_page().unwrap_or(0) as i64;
                    self.notebook.set_current_page(Some(
                        (current + if name == "next_tab" { 1 } else { -1 }).rem_euclid(i64::from(n))
                            as u32,
                    ));
                }
            }
            "mru_toggle" => {
                let previous = self
                    .mru_pages
                    .borrow()
                    .first()
                    .cloned()
                    .filter(|_| self.mru_pages.borrow().len() >= 2);
                if let Some(page) = previous
                    && let Some(index) = self.notebook.page_num(&page)
                {
                    self.notebook.set_current_page(Some(index));
                }
            }
            "focus_page" => {
                let session = commands::string_arg(args, "session", "")?;
                if !session.is_empty() {
                    self.select(session);
                }
            }
            "tab_reorder" => {
                let session = commands::string_arg(args, "session", "")?;
                let index = commands::python_int(args.get("index").unwrap_or(&json!(0)))?;
                self.reorder_tab(session, index.max(0) as usize);
            }
            "terminals_visible" => self
                .notebook
                .set_visible(args.get("on").is_none_or(comandos_core::json::truthy)),
            "reload_dashboard" => {
                if let Some(uri) =
                    ui::webview::dashboard_uri(self.cfg.dash_url(), env!("CARGO_PKG_VERSION"))
                {
                    self.webview.load_uri(&uri);
                } else {
                    return Err(CommandError::Failed("dashboard disconnected".into()));
                }
            }
            "font_scale" => {
                self.with_current_term(|term| {
                    term.set_font_scale(commands::font_scale(
                        term.font_scale(),
                        args.get("delta").unwrap_or(&Value::Null),
                    )?);
                    Ok(())
                })?;
            }
            "new_local_tab" => self.new_terminal(),
            "open_xterm_tab" => {
                let session = commands::session_arg(args, "local")?;
                let key = format!("xterm-{session}");
                self.add_tab(&key, &format!("xterm · {session}"), false, None);
                self.select(&key);
                self.persist();
            }
            "window" => match commands::string_arg(args, "action", "raise")? {
                "minimize" => self.window.iconify(),
                "maximize" => self.window.maximize(),
                "restore" => self.window.unmaximize(),
                "raise" => self.window.present(),
                _ => return Err(CommandError::Invalid("invalid window action".into())),
            },
            "paned_position" => self
                .paned
                .set_position(commands::python_int(args.get("px").unwrap_or(&json!(700)))?),
            "quit" => {
                if let Some(application) = self.window.application() {
                    application.quit();
                } else {
                    return Err(CommandError::Failed("window has no application".into()));
                }
            }
            _ => return Err(CommandError::Unknown(name.into())),
        }
        Ok(())
    }
    fn with_current_term(
        &self,
        f: impl FnOnce(&TermView) -> Result<(), CommandError>,
    ) -> Result<(), CommandError> {
        let Some(key) = self.current_session() else {
            return Ok(());
        };
        let terms = self.terms.borrow();
        if let Some(term) = terms.get(&key) {
            f(term)
        } else {
            Ok(())
        }
    }
    pub(in crate::ui) fn install_bridge(self: &Rc<Self>) -> glib::SourceId {
        if let Some(content) = self.webview.user_content_manager() {
            let weak = Rc::downgrade(self);
            let signal =
                content.connect_script_message_received(Some("centro"), move |_, result| {
                    let Some(app) = weak.upgrade().filter(|a| {
                        !a.closed.load(Ordering::Acquire) && a.cfg.mode() != RunMode::Shadow
                    }) else {
                        return;
                    };
                    let parsed = result
                        .js_value()
                        .ok_or_else(|| CommandError::Invalid("missing JavaScript value".into()))
                        .and_then(|js| ui::bridge::parse_bridge(js.to_str().as_str()));
                    if let Err(error) =
                        parsed.and_then(|message| app.bridge_queue.borrow_mut().push(message))
                    {
                        app.status.set_text(&error.to_string());
                    }
                });
            self.protocol_signals
                .borrow_mut()
                .push((content.upcast(), signal));
        } else {
            self.status.set_text("Missing centro UserContentManager");
        }
        let weak = Rc::downgrade(self);
        glib::timeout_add_local(Duration::from_millis(10), move || {
            let Some(app) = weak.upgrade().filter(|a| !a.closed.load(Ordering::Acquire)) else {
                return glib::ControlFlow::Break;
            };
            let messages = app.bridge_queue.borrow_mut().drain_ready(app.writable());
            for message in messages {
                if let Err(error) = app.dispatch_bridge(message) {
                    app.status.set_text(&error.to_string());
                }
            }
            glib::ControlFlow::Continue
        })
    }
    /// Known Rust→JS entry point for consumers installed in subsequent slices.
    pub fn call_dashboard(
        self: &Rc<Self>,
        function: ui::bridge::JsFunction,
        args: &[Value],
    ) -> Result<(), CommandError> {
        if !self.writable() {
            return Err(CommandError::Refused(
                "restore not ready, shadow, or closed".into(),
            ));
        }
        let script = ui::bridge::js_call(function, args)?;
        let weak = Rc::downgrade(self);
        self.webview.evaluate_javascript(
            &script,
            None,
            None,
            Some(&self.protocol_cancellable),
            move |result| {
                if let Some(app) = weak.upgrade().filter(|a| !a.closed.load(Ordering::Acquire))
                    && let Err(error) = result
                {
                    app.status
                        .set_text(&format!("Dashboard JavaScript: {error}"));
                }
            },
        );
        Ok(())
    }
    fn report_presence(self: &Rc<Self>, interaction: bool) {
        if self.closed.load(Ordering::Acquire) || self.cfg.mode() == RunMode::Shadow {
            return;
        }

        let now_ms = (glib::monotonic_time().max(0) as u64) / 1000;
        let Some(body) = self.presence.borrow_mut().payload(
            now_ms,
            interaction,
            self.presence_visible.get(),
            &self.device_id(),
            self.cfg.mode(),
        ) else {
            return;
        };
        if interaction {
            self.interacted.set(true);
        }
        let dash = self.dash.clone();
        let closed = self.closed.clone();
        self.jobs.spawn(
            move || {
                if closed.load(Ordering::Acquire) {
                    Err(crate::dash_client::DashError::Cancelled)
                } else {
                    dash.post("/presence", &body, Duration::from_secs(3))
                }
            },
            // The original presence completion deliberately ignores network errors.
            |_| {},
        );
    }

    pub(in crate::ui) fn install_presence(self: &Rc<Self>) -> glib::SourceId {
        let key = gtk::EventControllerKey::new(&self.window);
        key.set_propagation_phase(gtk::PropagationPhase::Capture);
        let weak = Rc::downgrade(self);
        let signal = key.connect_key_pressed(move |_, _, _, _| {
            if let Some(app) = weak.upgrade() {
                app.report_presence(true);
            }
            false
        });
        self.protocol_signals
            .borrow_mut()
            .push((key.clone().upcast(), signal));
        self.interaction_controllers.borrow_mut().push(key.upcast());
        let clicks = gtk::GestureMultiPress::new(&self.window);
        clicks.set_button(0);
        clicks.set_propagation_phase(gtk::PropagationPhase::Capture);
        let weak = Rc::downgrade(self);
        let signal = clicks.connect_pressed(move |_, _, _, _| {
            if let Some(app) = weak.upgrade() {
                app.report_presence(true);
            }
        });
        self.protocol_signals
            .borrow_mut()
            .push((clicks.clone().upcast(), signal));
        self.interaction_controllers
            .borrow_mut()
            .push(clicks.upcast());
        let weak = Rc::downgrade(self);
        let signal = self.window.connect_window_state_event(move |_, event| {
            if let Some(app) = weak.upgrade() {
                let visible = !event
                    .new_window_state()
                    .contains(gdk::WindowState::ICONIFIED);
                if app.presence_visible.replace(visible) != visible {
                    app.report_presence(false);
                }
            }
            glib::Propagation::Proceed
        });
        self.protocol_signals
            .borrow_mut()
            .push((self.window.clone().upcast(), signal));
        let weak = Rc::downgrade(self);
        glib::timeout_add_local(Duration::from_secs(30), move || {
            if let Some(app) = weak.upgrade().filter(|a| !a.closed.load(Ordering::Acquire)) {
                app.report_presence(false);
                glib::ControlFlow::Continue
            } else {
                glib::ControlFlow::Break
            }
        })
    }
    pub(super) fn shutdown_protocol(&self) {
        self.bridge_queue.borrow_mut().close();
        self.protocol_cancellable.cancel();
        self.handlers.clear();
        for (object, signal) in self.protocol_signals.borrow_mut().drain(..) {
            object.disconnect(signal);
        }
        for controller in self.interaction_controllers.borrow_mut().drain(..) {
            controller.reset();
            controller.set_propagation_phase(gtk::PropagationPhase::None);
        }
        self.mru_pages.borrow_mut().clear();
    }
}
