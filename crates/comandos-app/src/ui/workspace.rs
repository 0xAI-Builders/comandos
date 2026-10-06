use gtk::prelude::*;
use serde_json::Value;
use std::{cell::RefCell, collections::HashMap};

pub use crate::workspace_view::WorkspaceView;

#[derive(Clone)]
pub struct GtkWorkspace {
    root: gtk::Box,
    pages: RefCell<HashMap<String, gtk::Widget>>,
    focus: RefCell<Option<String>>,
}

impl GtkWorkspace {
    pub fn new() -> Self {
        let root = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        root.set_hexpand(true);
        root.set_vexpand(true);
        root.style_context().add_class("workspace-root");
        Self {
            root,
            pages: RefCell::new(HashMap::new()),
            focus: RefCell::new(None),
        }
    }

    pub fn widget(&self) -> &gtk::Box {
        &self.root
    }

    pub fn register_tab(&self, key: impl Into<String>, widget: &gtk::Widget) {
        widget.set_hexpand(true);
        widget.set_vexpand(true);
        self.pages.borrow_mut().insert(key.into(), widget.clone());
    }

    pub fn apply(&self, doc: &Value) {
        for child in self.root.children() {
            self.root.remove(&child);
        }
        let Some(groups) = doc.get("groups").and_then(Value::as_array) else {
            return;
        };
        for group in groups {
            let group_id = group.get("id").and_then(Value::as_str).unwrap_or("group");
            let page = self.build_group(group_id, &group["tree"]);
            self.root.pack_start(&page, true, true, 0);
        }
        self.root.show_all();
    }

    pub fn select(&self, key: &str) -> bool {
        if self.pages.borrow().contains_key(key) {
            *self.focus.borrow_mut() = Some(key.to_string());
            true
        } else {
            false
        }
    }

    pub fn commit(&self, doc: &Value, focus: Option<&str>) {
        self.apply(doc);
        if let Some(focus) = focus {
            let _ = self.select(focus);
        }
    }

    pub fn focused(&self) -> Option<String> {
        self.focus.borrow().clone()
    }

    fn build_group(&self, group_id: &str, tree: &Value) -> gtk::Widget {
        let notebook = gtk::Notebook::new();
        notebook.set_show_tabs(false);
        notebook.set_show_border(false);
        notebook.set_hexpand(true);
        notebook.set_vexpand(true);
        notebook.set_widget_name(&format!("workspace-group-{group_id}"));
        let child = self.build_node(tree);
        let label = gtk::Label::new(Some(group_id));
        notebook.append_page(&child, Some(&label));
        notebook.upcast()
    }

    fn build_node(&self, node: &Value) -> gtk::Widget {
        match node.get("type").and_then(Value::as_str) {
            Some("tab") => {
                let key = node.get("tabId").and_then(Value::as_str).unwrap_or("");
                self.tab_widget(key)
            }
            Some("split") => {
                let orientation = if node.get("axis").and_then(Value::as_str) == Some("y") {
                    gtk::Orientation::Vertical
                } else {
                    gtk::Orientation::Horizontal
                };
                let paned = gtk::Paned::new(orientation);
                paned.set_wide_handle(true);
                paned.set_hexpand(true);
                paned.set_vexpand(true);
                let first = self.build_node(&node["first"]);
                let second = self.build_node(&node["second"]);
                paned.pack1(&first, true, false);
                paned.pack2(&second, true, false);
                let ratio = node
                    .get("ratio")
                    .and_then(Value::as_f64)
                    .unwrap_or(0.5)
                    .clamp(0.1, 0.9);
                paned.set_position((ratio * 1000.0).round() as i32);
                paned.upcast()
            }
            _ => placeholder("empty-workspace"),
        }
    }

    fn tab_widget(&self, key: &str) -> gtk::Widget {
        if let Some(widget) = self.pages.borrow().get(key).cloned() {
            detach(&widget);
            widget
        } else {
            placeholder(key)
        }
    }
}

impl Default for GtkWorkspace {
    fn default() -> Self {
        Self::new()
    }
}

fn detach(widget: &gtk::Widget) {
    if let Some(parent) = widget
        .parent()
        .and_then(|parent| parent.downcast::<gtk::Container>().ok())
    {
        parent.remove(widget);
    }
}

fn placeholder(key: &str) -> gtk::Widget {
    let label = gtk::Label::new(Some(key));
    label.set_hexpand(true);
    label.set_vexpand(true);
    label.style_context().add_class("workspace-placeholder");
    label.upcast()
}
