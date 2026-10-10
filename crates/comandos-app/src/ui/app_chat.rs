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
    /// Terminal containers hidden by the chat, restored when it closes.
    hidden: RefCell<Vec<gtk::Widget>>,
    /// Messages currently drawn, one list child each; a refresh only redraws the tail
    /// that changed.
    shown: RefCell<Vec<(String, String)>>,
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

/// Inline markdown (`code`, **bold**, *em*, [link](url)) as Pango markup; everything
/// else is escaped.
fn inline(line: &str) -> String {
    let mut out = String::with_capacity(line.len() + 16);
    let mut rest = line;
    while let Some(i) = rest.find(['`', '*', '[']) {
        out.push_str(&glib::markup_escape_text(&rest[..i]));
        rest = &rest[i..];
        let (open, close, tag) = if rest.starts_with("**") {
            ("**", "**", "b")
        } else if rest.starts_with('`') {
            ("`", "`", "tt")
        } else if rest.starts_with('*') {
            ("*", "*", "i")
        } else {
            ("[", "](", "a")
        };
        let body = &rest[open.len()..];
        match body.find(close).filter(|&n| n > 0) {
            Some(n) if tag == "a" => {
                let after = &body[n + 2..];
                if let Some(end) = after.find(')') {
                    let url = glib::markup_escape_text(&after[..end]);
                    out.push_str(&format!("<a href=\"{url}\">{}</a>", glib::markup_escape_text(&body[..n])));
                    rest = &after[end + 1..];
                } else {
                    out.push('[');
                    rest = body;
                }
            }
            Some(n) if tag != "a" => {
                let text = glib::markup_escape_text(&body[..n]);
                out.push_str(&format!("<{tag}>{text}</{tag}>"));
                rest = &body[n + close.len()..];
            }
            _ => {
                out.push_str(&glib::markup_escape_text(open));
                rest = body;
            }
        }
    }
    out.push_str(&glib::markup_escape_text(rest));
    out
}

/// Splits a message into prose (as Pango markup) and fenced code blocks (verbatim).
fn segments(text: &str) -> Vec<(bool, String)> {
    let mut out: Vec<(bool, String)> = Vec::new();
    let mut prose: Vec<String> = Vec::new();
    let mut code: Option<Vec<&str>> = None;
    for line in text.lines() {
        if line.trim_start().starts_with("```") {
            match code.take() {
                Some(block) => out.push((true, block.join("\n"))),
                None => {
                    if !prose.is_empty() {
                        out.push((false, prose.join("\n")));
                        prose.clear();
                    }
                    code = Some(Vec::new());
                }
            }
            continue;
        }
        if let Some(block) = code.as_mut() {
            block.push(line);
            continue;
        }
        let trimmed = line.trim_start();
        let indent = &line[..line.len() - trimmed.len()];
        let markup = if let Some(h) = trimmed.strip_prefix("### ").or(trimmed.strip_prefix("## ")).or(trimmed.strip_prefix("# ")) {
            format!("<b><big>{}</big></b>", inline(h))
        } else if let Some(item) = trimmed.strip_prefix("- ").or(trimmed.strip_prefix("* ")) {
            format!("{indent}  •  {}", inline(item))
        } else if trimmed.starts_with("---") && trimmed.chars().all(|c| c == '-') {
            "<span alpha=\"40%\">────────────</span>".into()
        } else {
            inline(line)
        };
        prose.push(markup);
    }
    if let Some(block) = code {
        out.push((true, block.join("\n")));
    }
    if !prose.is_empty() {
        out.push((false, prose.join("\n")));
    }
    out
}

fn text_label(markup: Option<&str>, plain: &str, class: &str) -> gtk::Label {
    let label = gtk::Label::new(None);
    match markup {
        Some(m) => label.set_markup(m),
        None => label.set_text(plain),
    }
    label.set_line_wrap(true);
    label.set_line_wrap_mode(pango::WrapMode::WordChar);
    label.set_selectable(true);
    label.set_can_focus(false);
    label.set_xalign(0.);
    label.style_context().add_class(class);
    label
}

/// One message as a widget: a bubble whose prose renders the agent's markdown and
/// whose code blocks keep their own monospace panel; tool calls are a slim line.
fn bubble(role: &str, text: &str) -> gtk::Widget {
    if role == "t" {
        let label = text_label(None, text, "cv-tool");
        label.set_max_width_chars(140);
        return label.upcast();
    }
    let me = role == "u";
    let column = gtk::Box::new(gtk::Orientation::Vertical, 6);
    column.style_context().add_class(if me { "cv-me" } else { "cv-ai" });
    column.set_halign(if me { gtk::Align::End } else { gtk::Align::Fill });
    for (code, body) in segments(text) {
        let label = if code {
            text_label(None, &body, "cv-code")
        } else {
            text_label(Some(&body), "", "cv-text")
        };
        label.set_max_width_chars(if me { 72 } else { 110 });
        if code {
            label.set_line_wrap_mode(pango::WrapMode::Char);
        }
        column.pack_start(&label, false, false, 0);
    }
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    row.style_context().add_class(if me { "cv-row-me" } else { "cv-row-ai" });
    if me {
        row.pack_end(&column, false, false, 0);
    } else {
        row.pack_start(&column, true, true, 0);
    }
    row.upcast()
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
            if on {
                self.chat_cover(view);
                // `show_all` is a no-op on a widget marked `no_show_all`.
                view.root.set_no_show_all(false);
                view.root.show_all();
                view.input.grab_focus();
            } else {
                view.root.hide();
                view.root.set_no_show_all(true);
                for widget in self.chat.hidden.borrow_mut().drain(..) {
                    widget.show();
                }
            }
            view.toggle.set_tooltip_text(Some(if on { "Ver terminal" } else { "Ver como chat" }));
            let ctx = view.toggle.style_context();
            if on { ctx.add_class("cv-on") } else { ctx.remove_class("cv-on") }
        }
        self.chat.session.replace(String::new());
        self.chat_tick(true);
    }
    /// Hides whichever terminal container is showing (tabs or the pane mosaic).
    fn chat_cover(&self, view: &View) {
        let workspace: gtk::Widget = self.workspace.widget().clone().upcast();
        for widget in [view.notebook.clone().upcast::<gtk::Widget>(), workspace] {
            if widget.is_visible() {
                widget.hide();
                self.chat.hidden.borrow_mut().push(widget);
            }
        }
    }
    fn chat_title(&self, session: &str) -> String {
        let key = self.current_session().unwrap_or_default();
        self.labels
            .borrow()
            .get(&key)
            .map(|tab| tab.text.text().to_string())
            .filter(|t| !t.trim().is_empty())
            .unwrap_or_else(|| session.to_owned())
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
        if let Some(view) = &*self.chat.view.borrow() {
            self.chat_cover(view);
        }
        let session = self.chat_target();
        if session.is_empty() {
            return;
        }
        if *self.chat.session.borrow() != session {
            self.chat.session.replace(session.clone());
            self.chat.token.replace(String::new());
            if let Some(view) = &*self.chat.view.borrow() {
                view.title.set_text(&self.chat_title(&session));
                for child in view.list.children() {
                    view.list.remove(&child);
                }
                self.chat.shown.borrow_mut().clear();
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
                let empty = Vec::new();
                let messages: Vec<(String, String)> = v["messages"]
                    .as_array()
                    .unwrap_or(&empty)
                    .iter()
                    .map(|m| (m["r"].as_str().unwrap_or("t").to_owned(), m["t"].as_str().unwrap_or("").to_owned()))
                    .collect();
                let children = view.list.children();
                let mut shown = app.chat.shown.borrow_mut();
                // Keep the drawn prefix that still matches; redraw only from there.
                let keep = if children.len() == shown.len() {
                    shown.iter().zip(&messages).take_while(|(a, b)| a == b).count()
                } else {
                    0
                };
                for child in children.iter().skip(keep) {
                    view.list.remove(child);
                }
                if v["agent"].is_null() || messages.is_empty() {
                    view.list.pack_start(
                        &bubble("t", "Sin conversación de Claude o Codex en esta sesión. Usa la terminal."),
                        false,
                        false,
                        0,
                    );
                    shown.clear();
                } else {
                    for (role, text) in &messages[keep..] {
                        view.list.pack_start(&bubble(role, text), false, false, 0);
                    }
                    *shown = messages;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markdown_becomes_escaped_pango() {
        assert_eq!(inline("a **b** `c<d>` *e*"), "a <b>b</b> <tt>c&lt;d&gt;</tt> <i>e</i>");
        assert_eq!(inline("[doc](http://x/?a=1&b=2)"), "<a href=\"http://x/?a=1&amp;b=2\">doc</a>");
        assert_eq!(inline("2 * 3 < 4"), "2 * 3 &lt; 4");
        assert_eq!(inline("unclosed `tick"), "unclosed `tick");
    }

    #[test]
    fn code_fences_split_into_their_own_segment() {
        let parts = segments("# Plan\n- uno\n```rust\nlet x = 1 < 2;\n```\nfin");
        assert_eq!(parts.len(), 3);
        assert_eq!(parts[0], (false, "<b><big>Plan</big></b>\n  •  uno".into()));
        assert_eq!(parts[1], (true, "let x = 1 < 2;".into()));
        assert_eq!(parts[2], (false, "fin".into()));
    }
}
