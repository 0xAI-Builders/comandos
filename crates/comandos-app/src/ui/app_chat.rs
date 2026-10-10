//! Chat view of the current session: its agent transcript as selectable bubbles, a
//! composer that types into the pane and quick keys. It replaces the terminal notebook
//! while on; the timer only fetches while it is visible, and an unchanged transcript
//! costs one tiny answer from the server.
use super::*;

#[derive(Default)]
pub(super) struct Owned {
    on: Cell<bool>,
    view: RefCell<Option<View>>,
    session: RefCell<String>,
    token: RefCell<String>,
    busy: Cell<bool>,
}
struct View {
    root: gtk::Box,
    notebook: gtk::Notebook,
    list: gtk::Box,
    scroll: gtk::ScrolledWindow,
    title: gtk::Label,
    input: gtk::TextView,
    toggle: gtk::Button,
}

fn bubble(role: &str, text: &str) -> gtk::Label {
    let label = gtk::Label::new(Some(text));
    label.set_line_wrap(true);
    label.set_line_wrap_mode(pango::WrapMode::WordChar);
    label.set_selectable(true);
    label.set_xalign(0.);
    label.set_max_width_chars(if role == "t" { 140 } else { 96 });
    label.set_halign(if role == "u" { gtk::Align::End } else { gtk::Align::Start });
    label.style_context().add_class(match role {
        "u" => "cv-me",
        "a" => "cv-ai",
        _ => "cv-tool",
    });
    label
}

impl App {
    pub(super) fn install_chat(self: &Rc<Self>, terminals: &gtk::Box, notebook: &gtk::Notebook) {
        let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
        root.style_context().add_class("cv-root");
        root.set_no_show_all(true);
        let title = gtk::Label::new(None);
        title.style_context().add_class("cv-title");
        title.set_xalign(0.);
        let list = gtk::Box::new(gtk::Orientation::Vertical, 10);
        list.style_context().add_class("cv-list");
        let scroll = gtk::ScrolledWindow::new(gtk::Adjustment::NONE, gtk::Adjustment::NONE);
        scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
        scroll.add(&list);
        let keys = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        keys.style_context().add_class("cv-keys");
        for (label, key, class) in [
            ("Sí", "Enter", "cv-yes"),
            ("No", "Escape", "cv-no"),
            ("Esc", "Escape", ""),
            ("↑", "Up", ""),
            ("↓", "Down", ""),
            ("1", "1", ""),
            ("2", "2", ""),
            ("Tab", "Tab", ""),
        ] {
            let button = gtk::Button::with_label(label);
            button.style_context().add_class("cv-key");
            if !class.is_empty() {
                button.style_context().add_class(class);
            }
            let weak = Rc::downgrade(self);
            button.connect_clicked(move |_| {
                if let Some(app) = weak.upgrade() {
                    app.chat_post("/key", json!({"key": key}));
                }
            });
            keys.pack_start(&button, false, false, 0);
        }
        let input = gtk::TextView::new();
        input.set_wrap_mode(gtk::WrapMode::WordChar);
        input.style_context().add_class("cv-input");
        input.set_accepts_tab(false);
        let send = gtk::Button::with_label("Enviar ↵");
        send.style_context().add_class("cv-send");
        let compose = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        compose.style_context().add_class("cv-compose");
        compose.pack_start(&input, true, true, 0);
        compose.pack_start(&send, false, false, 0);
        root.pack_start(&title, false, false, 0);
        root.pack_start(&scroll, true, true, 0);
        root.pack_start(&keys, false, false, 0);
        root.pack_start(&compose, false, false, 0);
        terminals.pack_start(&root, true, true, 0);
        let weak = Rc::downgrade(self);
        send.connect_clicked(move |_| {
            if let Some(app) = weak.upgrade() {
                app.chat_send();
            }
        });
        let weak = Rc::downgrade(self);
        input.connect_key_press_event(move |_, event| {
            let enter = matches!(event.keyval(), gdk::keys::constants::Return | gdk::keys::constants::KP_Enter);
            if enter && !event.state().contains(gdk::ModifierType::SHIFT_MASK) {
                if let Some(app) = weak.upgrade() {
                    app.chat_send();
                }
                return glib::Propagation::Stop;
            }
            glib::Propagation::Proceed
        });
        let toggle = ui::icons::button("chat", 18, "Ver como chat");
        toggle.set_widget_name("chat");
        toggle.style_context().add_class("cc-key");
        let weak = Rc::downgrade(self);
        toggle.connect_clicked(move |_| {
            if let Some(app) = weak.upgrade() {
                app.chat_show(!app.chat.on.get());
            }
        });
        self.tab_layout.actions().pack_start(&toggle, false, false, 0);
        toggle.show();
        *self.chat.view.borrow_mut() = Some(View {
            root,
            notebook: notebook.clone(),
            list,
            scroll,
            title,
            input,
            toggle,
        });
        let weak = Rc::downgrade(self);
        glib::timeout_add_local(Duration::from_millis(1500), move || {
            let Some(app) = weak.upgrade().filter(|a| !a.closed.load(Ordering::Acquire)) else {
                return glib::ControlFlow::Break;
            };
            app.chat_tick(false);
            glib::ControlFlow::Continue
        });
    }
    fn chat_show(self: &Rc<Self>, on: bool) {
        self.chat.on.set(on);
        if let Some(view) = &*self.chat.view.borrow() {
            view.notebook.set_visible(!on);
            view.root.set_visible(on);
            if on {
                view.root.show_all();
                view.input.grab_focus();
            }
            view.toggle.set_tooltip_text(Some(if on { "Ver terminal" } else { "Ver como chat" }));
            let ctx = view.toggle.style_context();
            if on { ctx.add_class("cv-on") } else { ctx.remove_class("cv-on") }
        }
        self.chat.session.replace(String::new());
        self.chat_tick(true);
    }
    fn chat_target(&self) -> String {
        self.current_session()
            .and_then(|key| key.split(':').next().map(str::to_owned))
            .unwrap_or_default()
    }
    fn chat_tick(self: &Rc<Self>, force: bool) {
        if !self.chat.on.get() || !self.window.is_active() && !force {
            return;
        }
        let session = self.chat_target();
        if session.is_empty() {
            return;
        }
        if *self.chat.session.borrow() != session {
            self.chat.session.replace(session.clone());
            self.chat.token.replace(String::new());
            if let Some(view) = &*self.chat.view.borrow() {
                view.title.set_text(&session);
                for child in view.list.children() {
                    view.list.remove(&child);
                }
                view.list.pack_start(&bubble("t", "Cargando conversación…"), false, false, 0);
                view.list.show_all();
            }
        }
        if self.chat.busy.replace(true) {
            return;
        }
        let since = self.chat.token.borrow().clone();
        let dash = self.dash.clone();
        let body = json!({"session": session, "since": since});
        let weak = Rc::downgrade(self);
        self.jobs.spawn(
            move || dash.post("/chat/transcript", &body, Duration::from_secs(5)),
            move |result| {
                let Some(app) = weak.upgrade() else { return };
                app.chat.busy.set(false);
                let Ok((_, v)) = result else { return };
                if *app.chat.session.borrow() != session || v["unchanged"] == true {
                    return;
                }
                app.chat.token.replace(v["token"].as_str().unwrap_or("").into());
                let Some(view) = &*app.chat.view.borrow() else { return };
                let adj = view.scroll.vadjustment();
                let pinned = since.is_empty() || adj.upper() - adj.value() - adj.page_size() < 120.;
                for child in view.list.children() {
                    view.list.remove(&child);
                }
                let empty = Vec::new();
                let messages = v["messages"].as_array().unwrap_or(&empty);
                if v["agent"].is_null() || messages.is_empty() {
                    view.list.pack_start(
                        &bubble("t", "Sin conversación de Claude o Codex en esta sesión. Usa la terminal."),
                        false,
                        false,
                        0,
                    );
                }
                for m in messages {
                    view.list.pack_start(
                        &bubble(m["r"].as_str().unwrap_or("t"), m["t"].as_str().unwrap_or("")),
                        false,
                        false,
                        0,
                    );
                }
                view.list.show_all();
                if pinned {
                    let scroll = view.scroll.clone();
                    glib::idle_add_local_once(move || {
                        let adj = scroll.vadjustment();
                        adj.set_value(adj.upper() - adj.page_size());
                    });
                }
            },
        );
    }
    fn chat_send(self: &Rc<Self>) {
        let text = {
            let Some(view) = &*self.chat.view.borrow() else { return };
            let buffer = view.input.buffer().expect("text view buffer");
            let (start, end) = buffer.bounds();
            let text = buffer.text(&start, &end, false).map(|t| t.to_string()).unwrap_or_default();
            if text.trim().is_empty() {
                return;
            }
            buffer.set_text("");
            view.list.pack_start(&bubble("u", &text), false, false, 0);
            view.list.show_all();
            text
        };
        self.chat_post("/send", json!({"text": text}));
    }
    fn chat_post(self: &Rc<Self>, path: &'static str, mut body: Value) {
        let session = self.chat_target();
        if session.is_empty() || !self.writable() {
            return;
        }
        body["session"] = session.into();
        let dash = self.dash.clone();
        let weak = Rc::downgrade(self);
        self.jobs.spawn(
            move || dash.post(path, &body, Duration::from_secs(5)),
            move |result| {
                let Some(app) = weak.upgrade() else { return };
                if let Err(error) = result {
                    app.status.set_text(&format!("Chat: {error:?}"));
                }
                app.chat_tick(true);
            },
        );
    }
}
