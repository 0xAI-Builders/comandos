//! Búsqueda de subsecuencia; puntuaciones originales expresadas en centésimas.
use serde_json::Value;
use std::collections::BTreeSet;
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    pub label: String,
    pub key: String,
    pub state: String,
    pub open: bool,
}
/// -1 es no-match. Cada salto cuesta 100; cada carácter del candidato cuesta 1.
pub fn fuzzy_score(query: &str, candidate: &str) -> i32 {
    let query = query.to_lowercase();
    let candidate: Vec<_> = candidate.to_lowercase().chars().collect();
    let mut pos = 0;
    let mut score = 0usize;
    for ch in query.chars() {
        let Some(offset) = candidate
            .get(pos..)
            .and_then(|rest| rest.iter().position(|c| *c == ch))
        else {
            return -1;
        };
        score = score.saturating_add(offset.saturating_mul(100));
        pos = pos.saturating_add(offset + 1);
    }
    i32::try_from(score.saturating_add(candidate.len())).unwrap_or(i32::MAX)
}
pub fn candidates(
    open: &[Candidate],
    items: &std::collections::BTreeMap<String, Value>,
    states: &std::collections::BTreeMap<String, String>,
) -> Vec<Candidate> {
    let mut result = open.to_vec();
    let mut seen: BTreeSet<_> = open.iter().map(|r| r.key.clone()).collect();
    for (key, item) in items {
        if seen.contains(key)
            || key == "local"
            || item.get("alive") == Some(&Value::Bool(false))
            || item.get("zombie").is_some_and(comandos_core::json::truthy)
        {
            continue;
        }
        if key.starts_with("term-")
            && !item.get("tabbed").is_some_and(comandos_core::json::truthy)
            && !item.get("agent").is_some_and(comandos_core::json::truthy)
            && item.get("status").and_then(Value::as_str).unwrap_or("idle") == "idle"
        {
            continue;
        }
        seen.insert(key.clone());
        result.push(Candidate {
            key: key.clone(),
            label: item
                .get("project")
                .filter(|v| comandos_core::json::truthy(v))
                .and_then(Value::as_str)
                .unwrap_or(key)
                .into(),
            state: states.get(key).cloned().unwrap_or_default(),
            open: false,
        });
    }
    result
}
pub fn search(query: &str, candidates: &[Candidate]) -> Vec<Candidate> {
    let q = query.trim();
    let mut rows: Vec<_> = candidates
        .iter()
        .filter_map(|row| {
            let score = if q.is_empty() {
                0
            } else {
                fuzzy_score(q, &format!("{} {}", row.label, row.key))
            };
            (score >= 0).then_some((priority(&row.state), score, row.label.to_lowercase(), row))
        })
        .collect();
    rows.sort_by(|a, b| {
        (a.0, a.1, &a.2, &a.3.label, &a.3.key, &a.3.state, a.3.open)
            .cmp(&(b.0, b.1, &b.2, &b.3.label, &b.3.key, &b.3.state, b.3.open))
    });
    rows.into_iter()
        .take(12)
        .map(|(_, _, _, r)| r.clone())
        .collect()
}
fn priority(state: &str) -> i32 {
    match state {
        "waiting" => 0,
        "done" => 1,
        "working" => 2,
        _ => 3,
    }
}
pub fn hint(state: &str, open: bool, english: bool) -> String {
    let text = match (state, english) {
        ("waiting", false) => "espera TU respuesta",
        ("waiting", true) => "waiting for YOU",
        ("working", false) => "trabajando",
        ("working", true) => "working",
        ("done", false) => "listo",
        ("done", true) => "done",
        _ => "",
    };
    format!(
        "{text}{}",
        if open {
            ""
        } else if english {
            "   [open]"
        } else {
            "   [abrir]"
        }
    )
    .trim()
    .into()
}
use gtk::prelude::*;
use std::{cell::RefCell, rc::Rc};
pub type RowCallback = Rc<dyn Fn(&Candidate)>;
pub type QueryCallback = Rc<dyn Fn(&str)>;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Switcher,
    Overview,
    Help,
}
pub struct Panel {
    pub frame: gtk::Frame,
    pub entry: Option<gtk::Entry>,
    list: gtk::ListBox,
    rows: Rc<RefCell<Vec<Candidate>>>,
    english: bool,
    kind: Kind,
    activate: RowCallback,
    close: RowCallback,
}
impl Panel {
    pub fn new(
        kind: Kind,
        english: bool,
        activate: RowCallback,
        close: RowCallback,
        query: QueryCallback,
    ) -> Self {
        let frame = gtk::Frame::new(None);
        frame.set_size_request(
            if kind == Kind::Switcher { 560 } else { 500 },
            if kind == Kind::Switcher { -1 } else { 480 },
        );
        frame.set_halign(gtk::Align::Center);
        frame.set_valign(gtk::Align::Center);
        frame.style_context().add_class("switcher");
        let outer = gtk::Box::new(gtk::Orientation::Vertical, 8);
        for side in [true, false] {
            if side {
                outer.set_margin_start(12);
                outer.set_margin_top(12)
            } else {
                outer.set_margin_end(12);
                outer.set_margin_bottom(12)
            }
        }
        let entry = if kind == Kind::Switcher {
            let entry = gtk::Entry::new();
            entry.set_placeholder_text(Some(if english {
                "Jump to...  (type part of a name, Enter opens the first)"
            } else {
                "Salta a...  (parte del nombre, Enter va a la primera)"
            }));
            entry.connect_changed(move |e| query(e.text().as_str()));
            outer.pack_start(&entry, false, false, 0);
            Some(entry)
        } else {
            let title = gtk::Label::new(Some(if english {
                "Open tabs"
            } else {
                "Tabs abiertas"
            }));
            title.set_xalign(0.);
            title.style_context().add_class("help-sec");
            outer.pack_start(&title, false, false, 0);
            None
        };
        let list = gtk::ListBox::new();
        list.set_activate_on_single_click(true);
        if kind == Kind::Overview {
            let scroll = gtk::ScrolledWindow::new(gtk::Adjustment::NONE, gtk::Adjustment::NONE);
            scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
            scroll.set_min_content_height(320);
            scroll.add(&list);
            outer.pack_start(&scroll, true, true, 0);
        } else {
            outer.pack_start(&list, true, true, 0);
        }
        frame.add(&outer);
        let rows: Rc<RefCell<Vec<Candidate>>> = Rc::new(RefCell::new(vec![]));
        let weak = Rc::downgrade(&rows);
        let go = activate.clone();
        list.connect_row_activated(move |_, row| {
            let candidate = weak.upgrade().and_then(|rows| {
                usize::try_from(row.index())
                    .ok()
                    .and_then(|index| rows.borrow().get(index).cloned())
            });
            if let Some(candidate) = candidate {
                go(&candidate);
            }
        });
        Self {
            frame,
            entry,
            list,
            rows,
            english,
            kind,
            activate,
            close,
        }
    }
    pub fn refill(&self, rows: Vec<Candidate>) {
        for child in self.list.children() {
            self.list.remove(&child);
        }
        *self.rows.borrow_mut() = rows.clone();
        for candidate in rows {
            let row = gtk::ListBoxRow::new();
            let h = gtk::Box::new(gtk::Orientation::Horizontal, 9);
            let margin = if self.kind == Kind::Overview { 8 } else { 7 };
            h.set_margin_start(margin);
            h.set_margin_end(margin);
            h.set_margin_top(margin);
            h.set_margin_bottom(margin);
            let dot = gtk::Label::new(None);
            dot.set_markup(&format!(
                "<span size=\"{}\" foreground=\"{}\">●</span>",
                if self.kind == Kind::Overview {
                    9500
                } else {
                    9000
                },
                dot_color(&candidate.state)
            ));
            h.pack_start(&dot, false, false, 0);
            let label = gtk::Label::new(Some(&candidate.label));
            label.set_xalign(0.);
            if self.kind == Kind::Overview {
                let texts = gtk::Box::new(gtk::Orientation::Vertical, 1);
                texts.pack_start(&label, false, false, 0);
                let meta = gtk::Label::new(Some(&candidate.key));
                meta.set_xalign(0.);
                meta.style_context().add_class("sw-meta");
                texts.pack_start(&meta, false, false, 0);
                h.pack_start(&texts, true, true, 0);
                let button = gtk::Button::with_label("×");
                button.set_relief(gtk::ReliefStyle::None);
                button.style_context().add_class("tabplus");
                let close = self.close.clone();
                button.connect_clicked(move |_| close(&candidate));
                h.pack_end(&button, false, false, 0);
            } else {
                h.pack_start(&label, true, true, 0);
                let meta =
                    gtk::Label::new(Some(&hint(&candidate.state, candidate.open, self.english)));
                meta.set_xalign(1.);
                meta.style_context().add_class("sw-meta");
                h.pack_end(&meta, false, false, 0);
            }
            row.add(&h);
            self.list.add(&row);
        }
        self.list.show_all();
        if self.kind == Kind::Switcher
            && let Some(row) = self.list.row_at_index(0)
        {
            self.list.select_row(Some(&row));
        }
    }
    pub fn query(&self) -> String {
        self.entry
            .as_ref()
            .map(|e| e.text().to_string())
            .unwrap_or_default()
    }
    pub fn key(&self, key: u32, control: bool) -> bool {
        if self.kind != Kind::Switcher {
            return false;
        }
        use gdk::keys::constants as k;
        let selected = self.list.selected_row().map(|r| r.index());
        if key == *k::Return || key == *k::KP_Enter {
            let row =
                selected.and_then(|index| self.rows.borrow().get(index.max(0) as usize).cloned());
            if let Some(row) = row {
                (self.activate)(&row);
            }
            return true;
        }
        if key == *k::Down || key == *k::Up {
            let count = self.rows.borrow().len();
            if count > 0 {
                let next = (selected.unwrap_or(0) + if key == *k::Down { 1 } else { -1 })
                    .clamp(0, count as i32 - 1);
                self.list.select_row(self.list.row_at_index(next).as_ref());
            }
            return true;
        }
        if ((key == *k::Delete || key == *k::BackSpace) && self.query().is_empty())
            || (control && (key == *k::w || key == *k::W))
        {
            let row = selected.and_then(|index| {
                self.rows
                    .borrow()
                    .get(index.max(0) as usize)
                    .filter(|r| r.open)
                    .cloned()
            });
            if let Some(row) = row {
                (self.close)(&row);
            }
            return true;
        }
        false
    }
    pub fn focus(&self) {
        if let Some(entry) = &self.entry {
            entry.grab_focus();
        }
    }
}
pub fn dot_color(state: &str) -> &'static str {
    match state {
        "waiting" => "#D08770",
        "working" => "#81A1C1",
        "done" => "#A3BE8C",
        "dead" => "#BF616A",
        _ => "#4B5568",
    }
}
