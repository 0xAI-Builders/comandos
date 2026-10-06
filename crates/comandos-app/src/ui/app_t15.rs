//! Atajos y paneles T15: una ventana, selección del App y trabajos cancelables.
use super::*;
use crate::agent_stop::{self, Cancellation};
use ui::{
    keys::{KeyAction, KeyInput},
    switcher::{Candidate, Kind, Panel},
};
pub(super) struct Modal {
    generation: u64,
    kind: Kind,
    panel: Option<Rc<Panel>>,
    backdrop: gtk::EventBox,
    wrapper: gtk::EventBox,
}
impl App {
    pub(in crate::ui) fn install_keys(self: &Rc<Self>) {
        for (name, kind) in [
            ("T15.help", Kind::Help),
            ("T15.switcher", Kind::Switcher),
            ("T15.overview", Kind::Overview),
        ] {
            let weak = Rc::downgrade(self);
            self.install_handler(
                name,
                Rc::new(move |_| {
                    let app = weak.upgrade().filter(|a| a.writable()).ok_or_else(|| {
                        ui::app_commands::CommandError::Refused(
                            "restore not ready, shadow, or closed".into(),
                        )
                    })?;
                    if kind == Kind::Help {
                        app.open_help();
                    } else {
                        app.open_switcher(kind);
                    }
                    Ok(())
                }),
            );
        }
        let weak = Rc::downgrade(self);
        let signal = self.window.connect_key_press_event(move |window, event| {
            let Some(app) = weak.upgrade().filter(|a| !a.closed.load(Ordering::Acquire)) else {
                return glib::Propagation::Proceed;
            };
            // El DrawingArea propio despacha antes de escribir al PTY. El Window solo cubre los demás focos.
            if window.focused_widget().is_some_and(|focus| {
                app.terms
                    .borrow()
                    .values()
                    .any(|term| term.widget().clone().upcast::<gtk::Widget>() == focus)
            }) {
                return glib::Propagation::Proceed;
            }
            if app.handle_key(event, None) {
                glib::Propagation::Stop
            } else {
                glib::Propagation::Proceed
            }
        });
        self.protocol_signals
            .borrow_mut()
            .push((self.window.clone().upcast(), signal));
    }
    pub(super) fn attach_term_keys(self: &Rc<Self>, key: &str, term: &TermView) {
        let weak = Rc::downgrade(self);
        let key = key.to_string();
        let instance = term.cleanup_cancellation();
        term.on_app_key(Rc::new(move |event| {
            weak.upgrade().is_some_and(|app| {
                let current =
                    !instance.load(Ordering::Acquire)
                        && app.terms.borrow().get(&key).is_some_and(|term| {
                            Arc::ptr_eq(&instance, &term.cleanup_cancellation())
                        });
                current && app.handle_key(event, Some(&key))
            })
        }));
    }
    fn handle_key(self: &Rc<Self>, event: &gdk::EventKey, term_key: Option<&str>) -> bool {
        if self.closed.load(Ordering::Acquire) {
            return false;
        }
        let mut input = KeyInput::new(*event.keyval());
        input.control = event.state().contains(gdk::ModifierType::CONTROL_MASK);
        input.shift = event.state().contains(gdk::ModifierType::SHIFT_MASK);
        input.drag = self.drag_layer.active();
        input.terminal = term_key.is_some();
        input.has_term = self
            .current_session()
            .is_some_and(|key| self.terms.borrow().contains_key(&key));
        input.selection = term_key.is_some_and(|key| {
            self.terms
                .borrow()
                .get(key)
                .is_some_and(|term| term.selection_text().is_some())
        });
        if input.key == *gdk::keys::constants::Escape && input.drag {
            self.cancel_drag();
            return true;
        }
        if self.modal_key(input.key, input.control) {
            return true;
        }
        self.execute_key(input, term_key, glib::monotonic_time() as f64 / 1_000_000.)
    }
    fn execute_key(self: &Rc<Self>, input: KeyInput, term_key: Option<&str>, now: f64) -> bool {
        let action = ui::keys::action(input);
        if action == KeyAction::Pass {
            return false;
        }
        if !self.writable() {
            self.status.set_text("restore not ready, shadow, or closed");
            return true;
        }
        let command = match action {
            KeyAction::Pass => return false,
            KeyAction::CancelDrag => {
                self.cancel_drag();
                return true;
            }
            KeyAction::Help => ("help", json!({})),
            KeyAction::Paste => ("paste_clipboard", json!({})),
            KeyAction::Copy => ("copy_selection", json!({})),
            KeyAction::CtrlC => {
                let Some(key) = term_key else {
                    return false;
                };
                let Some(action) = self
                    .terms
                    .borrow()
                    .get(key)
                    .map(|term| term.ctrl_c_action(input.selection, false, now))
                else {
                    return false;
                };
                if action == agent_stop::CtrlCAction::Copy {
                    if let Err(error) = self.handlers.invoke("copy_selection", &json!({})) {
                        self.status.set_text(&error.to_string());
                    }
                    return true;
                }
                if action == agent_stop::CtrlCAction::Cleanup {
                    self.arm_agent_cleanup(key);
                }
                return false;
            }
            KeyAction::ToggleTerminals => (
                "terminals_visible",
                json!({"on":!self.active_notebook().is_visible()}),
            ),
            KeyAction::Reload => ("reload_dashboard", json!({})),
            KeyAction::StartAI => ("start_ai_here", json!({"session":self.current_session()})),
            KeyAction::MruOrNext => {
                self.remember_current_navigation_page();
                let has_mru = self.mru_pages.borrow().len() >= 2;
                let result = self
                    .handlers
                    .invoke(if has_mru { "mru_toggle" } else { "next_tab" }, &json!({}));
                if let Err(error) = result {
                    self.status.set_text(&error.to_string());
                }
                self.focus_current_term();
                return true;
            }
            KeyAction::Cycle(delta) => {
                if let Err(error) = self
                    .handlers
                    .invoke(if delta < 0 { "prev_tab" } else { "next_tab" }, &json!({}))
                {
                    self.status.set_text(&error.to_string());
                }
                self.focus_current_term();
                return true;
            }
            KeyAction::Mosaic => ("mosaic", json!({})),
            KeyAction::ZoomIn => ("font_scale", json!({"delta":0.1})),
            KeyAction::ZoomOut => ("font_scale", json!({"delta":-0.1})),
            KeyAction::ZoomReset => ("font_scale", json!({"delta":0})),
            KeyAction::Switcher => ("open_switcher", json!({})),
            KeyAction::Snippets => ("snippets", json!({})),
            KeyAction::NewTerminal => ("new_local_tab", json!({})),
            KeyAction::NewXterm => ("open_xterm_tab", json!({"session":"local"})),
            KeyAction::CloseTab => {
                if let Some(key) = self.current_session() {
                    self.close_tab(&key, true);
                }
                return true;
            }
            KeyAction::Quit => ("quit", json!({})),
        };
        if let Err(error) = self.handlers.invoke(command.0, &command.1) {
            self.status.set_text(&error.to_string());
        }
        if action == KeyAction::ToggleTerminals && self.active_notebook().is_visible() {
            self.focus_current_term();
        }
        true
    }
    fn focus_current_term(&self) {
        if let Some(key) = self.current_session()
            && let Some(term) = self.terms.borrow().get(&key)
        {
            term.widget().grab_focus();
        }
    }
    fn arm_agent_cleanup(self: &Rc<Self>, key: &str) {
        if !self.writable() {
            return;
        }
        let Some((tty, terminal)) = self.terms.borrow().get(key).and_then(|term| {
            term.child_tty()
                .map(|tty| (tty, term.cleanup_cancellation()))
        }) else {
            return;
        };
        let tmux = self.tmux.clone();
        let mode = self.cfg.mode();
        let cancelled = agent_stop::StopCancellation {
            app: self.closed.clone(),
            terminal,
        };
        let cleanup = self.cleanup.clone();
        let weak = Rc::downgrade(self);
        self.jobs.spawn(
            move || {
                if cancelled.is_cancelled() || mode == RunMode::Shadow {
                    return Ok(vec![]);
                }
                let clients = tmux.read(&[
                    "list-clients",
                    "-F",
                    "#{client_tty}|#{client_key_table}|#{client_prefix}|#{pane_in_mode}|#{pane_pid}",
                ]).map_err(|e| format!("{e:?}"))?;
                let state = agent_stop::parse_client_state(
                    if clients.ok() { &clients.stdout } else { "" }, &tty,
                );
                if cancelled.is_cancelled() || agent_stop::client_blocks_stop(&state) {
                    return Ok(vec![]);
                }
                let root = state.get("pane_pid").and_then(Value::as_u64)
                    .and_then(|pid| u32::try_from(pid).ok()).ok_or("Invalid pane process")?;
                cleanup.cleanup(root, &state, mode, &cancelled)
            },
            move |result| {
                if let Some(app) = weak.upgrade().filter(|a| !a.closed.load(Ordering::Acquire))
                    && let Err(error) = result {
                    app.status.set_text(&format!("Ctrl+C: {error}"));
                }
            },
        );
    }
    fn open_help(self: &Rc<Self>) {
        let frame = ui::help::panel(self.english);
        self.mount_modal(&frame, Kind::Help, None);
    }
    fn open_switcher(self: &Rc<Self>, kind: Kind) {
        let generation = self.modal_generation.get().wrapping_add(1);
        let weak = Rc::downgrade(self);
        let activate = Rc::new(move |row: &Candidate| {
            if let Some(app) = weak.upgrade().filter(|a| {
                a.writable()
                    && a.modal
                        .borrow()
                        .as_ref()
                        .is_some_and(|m| m.generation == generation)
            }) {
                if row.open && app.navigation_index(&row.key).is_none() {
                    return;
                }
                app.close_modal();
                if row.open {
                    app.select(&row.key);
                    app.focus_current_term();
                } else {
                    app.open_tab(&row.key, "claude", &row.label, true);
                }
            }
        });
        let weak = Rc::downgrade(self);
        let close = Rc::new(move |row: &Candidate| {
            if let Some(app) = weak.upgrade().filter(|a| {
                a.writable()
                    && a.modal
                        .borrow()
                        .as_ref()
                        .is_some_and(|m| m.generation == generation)
            }) && row.open
                && app.navigation_index(&row.key).is_some()
            {
                app.close_tab(&row.key, true);
                app.refill_switcher();
            }
        });
        let weak = Rc::downgrade(self);
        let query = Rc::new(move |_: &str| {
            if let Some(app) = weak.upgrade().filter(|a| {
                !a.closed.load(Ordering::Acquire)
                    && a.modal
                        .borrow()
                        .as_ref()
                        .is_some_and(|m| m.generation == generation)
            }) {
                app.refill_switcher();
            }
        });
        let panel = Rc::new(Panel::new(kind, self.english, activate, close, query));
        let frame = panel.frame.clone();
        self.mount_modal(&frame, kind, Some(panel));
        self.refill_switcher();
        if let Some(modal) = self.modal.borrow().as_ref()
            && let Some(panel) = &modal.panel
        {
            panel.focus();
        }
    }
    fn switcher_candidates(&self) -> Vec<Candidate> {
        let mut open = Vec::new();
        let states: BTreeMap<_, _> = self
            .state_items
            .borrow()
            .iter()
            .map(|(key, item)| {
                (
                    key.clone(),
                    item.get("status")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string(),
                )
            })
            .collect();
        let notebook = self.active_notebook();
        for index in 0..notebook.n_pages() {
            if let Some(page) = notebook.nth_page(Some(index)) {
                let key = if self.workspace_doc.borrow().is_null() {
                    self.strip.borrow().page_key(&page)
                } else {
                    self.workspace.page_key(&page)
                };
                if let Some(key) = key {
                    let label = self
                        .labels
                        .borrow()
                        .get(&key)
                        .map(|tab| tab.text.text().to_string())
                        .unwrap_or_else(|| key.clone());
                    open.push(Candidate {
                        state: states.get(&key).cloned().unwrap_or_default(),
                        label,
                        key,
                        open: true,
                    });
                }
            }
        }
        ui::switcher::candidates(&open, &self.state_items.borrow(), &states)
    }
    fn refill_switcher(&self) {
        let Some(modal) = self.modal.borrow().as_ref().map(|m| {
            (
                m.generation,
                m.kind,
                m.panel.as_ref().map(|panel| panel.query()),
            )
        }) else {
            return;
        };
        let mut rows = self.switcher_candidates();
        if modal.1 == Kind::Overview {
            rows.retain(|r| r.open);
        } else {
            rows = ui::switcher::search(modal.2.as_deref().unwrap_or(""), &rows);
        }
        if let Some(current) = self
            .modal
            .borrow()
            .as_ref()
            .filter(|m| m.generation == modal.0)
            && let Some(panel) = &current.panel
        {
            panel.refill(rows);
        }
    }
    fn mount_modal(self: &Rc<Self>, frame: &gtk::Frame, kind: Kind, panel: Option<Rc<Panel>>) {
        self.close_modal();
        let generation = self.modal_generation.get().wrapping_add(1);
        self.modal_generation.set(generation);
        frame.style_context().add_class("cc-modal-panel");
        let backdrop = gtk::EventBox::new();
        backdrop.set_visible_window(true);
        backdrop.set_halign(gtk::Align::Fill);
        backdrop.set_valign(gtk::Align::Fill);
        backdrop.set_hexpand(true);
        backdrop.set_vexpand(true);
        backdrop.style_context().add_class("cc-modal-backdrop");
        backdrop.add_events(gdk::EventMask::BUTTON_PRESS_MASK);
        let weak = Rc::downgrade(self);
        backdrop.connect_button_press_event(move |_, _| {
            if let Some(app) = weak
                .upgrade()
                .filter(|a| a.modal_generation.get() == generation)
            {
                app.close_modal();
            }
            glib::Propagation::Stop
        });
        let wrapper = gtk::EventBox::new();
        wrapper.set_visible_window(true);
        wrapper.set_halign(gtk::Align::Center);
        wrapper.set_valign(gtk::Align::Center);
        wrapper.add(frame);
        self.modal_overlay.add_overlay(&backdrop);
        self.modal_overlay.add_overlay(&wrapper);
        backdrop.show_all();
        wrapper.show_all();
        *self.modal.borrow_mut() = Some(Modal {
            generation,
            kind,
            panel,
            backdrop,
            wrapper,
        });
        self.restack_modal(generation);
        let weak = Rc::downgrade(self);
        *self.modal_idle.borrow_mut() = Some(glib::idle_add_local_once(move || {
            if let Some(app) = weak.upgrade() {
                app.modal_idle.borrow_mut().take();
                app.restack_modal(generation);
            }
        }));
    }
    fn restack_modal(&self, generation: u64) {
        if let Some(modal) = self
            .modal
            .borrow()
            .as_ref()
            .filter(|m| m.generation == generation)
        {
            if let Some(window) = modal.backdrop.window() {
                window.raise();
            }
            if let Some(window) = modal.wrapper.window() {
                window.raise();
            }
        }
    }
    fn modal_key(self: &Rc<Self>, key: u32, control: bool) -> bool {
        let kind = self.modal.borrow().as_ref().map(|m| m.kind);
        let Some(kind) = kind else {
            return false;
        };
        if key == *gdk::keys::constants::Escape
            || (kind == Kind::Help
                && (key == *gdk::keys::constants::F1 || key == *gdk::keys::constants::question))
        {
            self.close_modal();
            return true;
        }
        // El callback puede cerrar o rellenar el modal; no conservar el RefCell durante él.
        let panel = self.modal.borrow().as_ref().and_then(|m| m.panel.clone());
        panel.is_some_and(|panel| panel.key(key, control))
    }
    pub(super) fn close_modal(&self) {
        if let Some(source) = self.modal_idle.borrow_mut().take() {
            source.remove();
        }
        if let Some(modal) = self.modal.borrow_mut().take() {
            self.modal_overlay.remove(&modal.backdrop);
            self.modal_overlay.remove(&modal.wrapper);
        }
        if !self.closed.load(Ordering::Acquire) {
            self.focus_current_term();
        }
    }
}
