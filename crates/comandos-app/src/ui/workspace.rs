pub use crate::workspace_view::WorkspaceView;
use gtk::prelude::*;
use serde_json::Value;
use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    rc::Rc,
};
type Resize = Rc<dyn Fn(&str, &[usize], f64)>;
pub struct GtkWorkspace {
    root: gtk::Notebook,
    pages: RefCell<HashMap<String, gtk::Widget>>,
    focus: Rc<RefCell<Option<String>>>,
    document: RefCell<Value>,
    nodes: RefCell<HashMap<String, gtk::Widget>>,
    resize: Rc<RefCell<Option<Resize>>>,
    applying: Rc<Cell<bool>>,
}
impl GtkWorkspace {
    pub fn new() -> Self {
        let root = gtk::Notebook::new();
        root.set_show_tabs(false);
        root.set_show_border(false);
        root.set_hexpand(true);
        root.set_vexpand(true);
        root.style_context().add_class("workspace-root");
        Self {
            root,
            pages: RefCell::new(HashMap::new()),
            focus: Rc::new(RefCell::new(None)),
            document: RefCell::new(Value::Null),
            nodes: RefCell::new(HashMap::new()),
            resize: Rc::new(RefCell::new(None)),
            applying: Rc::new(Cell::new(false)),
        }
    }
    pub fn widget(&self) -> &gtk::Notebook {
        &self.root
    }
    pub fn on_resize(&self, callback: Resize) {
        *self.resize.borrow_mut() = Some(callback);
    }
    pub fn register_tab(&self, key: impl Into<String>, widget: &gtk::Widget) {
        widget.set_hexpand(true);
        widget.set_vexpand(true);
        let key = key.into();
        let focus = self.focus.clone();
        let focus_key = key.clone();
        widget.connect_focus_in_event(move |_, _| {
            *focus.borrow_mut() = Some(focus_key.clone());
            glib::Propagation::Proceed
        });
        self.pages.borrow_mut().insert(key, widget.clone());
        self.nodes.borrow_mut().clear();
    }
    pub fn refresh(&self) {
        let doc = self.document.borrow().clone();
        *self.document.borrow_mut() = Value::Null;
        self.apply(&doc);
    }
    pub fn remove_tab(&self, key: &str) {
        if let Some(widget) = self.pages.borrow_mut().remove(key) {
            detach(&widget);
        }
        self.nodes.borrow_mut().clear();
        let mut doc = self.document.borrow().clone();
        let present = self.pages.borrow().keys().cloned().collect();
        if let Some(groups) = doc.get_mut("groups").and_then(Value::as_array_mut) {
            for group in groups.iter_mut() {
                if let Some(object) = group.as_object_mut() {
                    let tree = object
                        .get("tree")
                        .map(|tree| crate::workspace_view::prune(tree, &present))
                        .unwrap_or(Value::Null);
                    object.insert("tree".into(), tree);
                }
            }
            groups.retain(|group| !group["tree"].is_null());
        }
        *self.document.borrow_mut() = Value::Null;
        self.apply(&doc);
    }
    pub fn apply(&self, doc: &Value) {
        if self.document.borrow().eq(doc) {
            return;
        }
        if comandos_core::workspace::validate_document(doc).is_err() {
            return;
        }
        self.applying.set(true);
        let mut used = std::collections::HashSet::new();
        if let Some(groups) = doc["groups"].as_array() {
            for group in groups {
                collect_keys(
                    group["id"].as_str().unwrap_or("group"),
                    &group["tree"],
                    Vec::new(),
                    &mut used,
                );
            }
        }
        self.nodes.borrow_mut().retain(|key, _| used.contains(key));
        let group_shapes = |doc: &Value| {
            doc.get("groups").and_then(Value::as_array).map(|groups| {
                groups
                    .iter()
                    .map(|group| (group["id"].clone(), signature(&group["tree"])))
                    .collect::<Vec<_>>()
            })
        };
        let same_shapes = group_shapes(doc) == group_shapes(&self.document.borrow());
        if !same_shapes {
            for child in self.root.children() {
                self.root.remove(&child);
            }
        }
        if let Some(groups) = doc["groups"].as_array() {
            for group in groups {
                let group_id = group["id"].as_str().unwrap_or("group");
                let node = self.build_node(group_id, &group["tree"], Vec::new());
                if !same_shapes {
                    detach(&node);
                    self.root
                        .append_page(&node, Some(&gtk::Label::new(Some(group_id))));
                }
            }
        }
        *self.document.borrow_mut() = doc.clone();
        self.root.show_all();
        self.applying.set(false);
        let previous = self.focused();
        if previous.as_deref().is_none_or(|key| !self.select(key)) {
            *self.focus.borrow_mut() = None;
            let first = doc
                .get("groups")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .flat_map(|group| {
                    comandos_core::workspace::tab_ids(&group["tree"]).unwrap_or_default()
                })
                .find(|key| self.pages.borrow().contains_key(key));
            if let Some(first) = first {
                self.select(&first);
            }
        }
    }
    pub fn select(&self, key: &str) -> bool {
        let pages = self.pages.borrow();
        let Some(widget) = pages.get(key) else {
            return false;
        };
        if !widget.is_ancestor(&self.root) {
            return false;
        }
        let mut child = widget.clone();
        while let Some(parent) = child.parent() {
            if parent == self.root.clone().upcast::<gtk::Widget>() {
                break;
            }
            child = parent;
        }
        if let Some(index) = self.root.page_num(&child) {
            self.root.set_current_page(Some(index));
        }
        widget.grab_focus();
        *self.focus.borrow_mut() = Some(key.into());
        true
    }
    pub fn commit(&self, doc: &Value, focus: Option<&str>) {
        self.apply(doc);
        if let Some(key) = focus {
            self.select(key);
        }
    }
    pub fn focused(&self) -> Option<String> {
        self.focus.borrow().clone()
    }
    fn build_node(&self, group: &str, node: &Value, path: Vec<usize>) -> gtk::Widget {
        let cache_key = format!("{group}:{path:?}:{}", signature(node));
        if let Some(widget) = self.nodes.borrow().get(&cache_key).cloned() {
            if let Some(paned) = widget.downcast_ref::<gtk::Paned>() {
                let size = if paned.orientation() == gtk::Orientation::Horizontal {
                    paned.allocated_width()
                } else {
                    paned.allocated_height()
                };
                if size > 1 {
                    paned.set_position(
                        (f64::from(size) * node.get("ratio").and_then(Value::as_f64).unwrap_or(0.5))
                            .round() as i32,
                    );
                }
                // Recurse to apply ratios of cached descendants without reparenting their leaves.
                let mut first_path = path.clone();
                first_path.push(0);
                let mut second_path = path.clone();
                second_path.push(1);
                self.build_node(group, &node["first"], first_path);
                self.build_node(group, &node["second"], second_path);
            }
            return widget;
        }
        let widget = match node["type"].as_str() {
            Some("tab") => {
                let key = node["tabId"].as_str().unwrap_or("");
                let widget = self
                    .pages
                    .borrow()
                    .get(key)
                    .cloned()
                    .unwrap_or_else(|| gtk::Label::new(Some(key)).upcast());
                detach(&widget);
                widget
            }
            Some("split") => {
                let orientation = if node["axis"] == "y" {
                    gtk::Orientation::Vertical
                } else {
                    gtk::Orientation::Horizontal
                };
                let paned = gtk::Paned::new(orientation);
                paned.set_wide_handle(true);
                paned.set_hexpand(true);
                paned.set_vexpand(true);
                let mut first_path = path.clone();
                first_path.push(0);
                let mut second_path = path.clone();
                second_path.push(1);
                let first = self.build_node(group, &node["first"], first_path);
                let second = self.build_node(group, &node["second"], second_path);
                detach(&first);
                detach(&second);
                paned.pack1(&first, true, false);
                paned.pack2(&second, true, false);
                let ratio = node["ratio"].as_f64().unwrap_or(0.5).clamp(0.1, 0.9);
                let allocated = Rc::new(Cell::new(false));
                let allocated_for_size = allocated.clone();
                paned.connect_size_allocate(move |paned, allocation| {
                    if !allocated_for_size.replace(true) {
                        let size = if orientation == gtk::Orientation::Horizontal {
                            allocation.width()
                        } else {
                            allocation.height()
                        };
                        paned.set_position((f64::from(size) * ratio).round() as i32);
                    }
                });
                let callback = self.resize.clone();
                let applying = self.applying.clone();
                let group = group.to_string();
                paned.connect_button_release_event(move |paned, _| {
                    if !applying.get() && allocated.get() {
                        let size = if orientation == gtk::Orientation::Horizontal {
                            paned.allocated_width()
                        } else {
                            paned.allocated_height()
                        };
                        if size > 0
                            && let Some(callback) = callback.borrow().as_ref()
                        {
                            callback(
                                &group,
                                &path,
                                (f64::from(paned.position()) / f64::from(size)).clamp(0.1, 0.9),
                            );
                        }
                    }
                    glib::Propagation::Proceed
                });
                paned.upcast()
            }
            _ => gtk::Label::new(None).upcast(),
        };
        self.nodes.borrow_mut().insert(cache_key, widget.clone());
        widget
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
        .and_then(|p| p.downcast::<gtk::Container>().ok())
    {
        parent.remove(widget);
    }
}

fn signature(node: &Value) -> Value {
    crate::workspace_view::shape(node)
}
fn collect_keys(
    group: &str,
    node: &Value,
    path: Vec<usize>,
    keys: &mut std::collections::HashSet<String>,
) {
    keys.insert(format!("{group}:{path:?}:{}", signature(node)));
    if node["type"] == "split" {
        let mut first = path.clone();
        first.push(0);
        let mut second = path;
        second.push(1);
        collect_keys(group, &node["first"], first, keys);
        collect_keys(group, &node["second"], second, keys);
    }
}
