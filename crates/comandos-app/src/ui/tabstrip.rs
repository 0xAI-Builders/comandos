//! Thin key-based adapter for the GTK notebook implementation owned by T7.
use gtk::prelude::*;
use std::collections::HashMap;

pub struct TabStripNotebook {
    notebook: gtk::Notebook,
    pages: HashMap<String, gtk::Widget>,
}

impl TabStripNotebook {
    pub fn new(notebook: gtk::Notebook) -> Self {
        notebook.set_show_tabs(false);
        notebook.set_show_border(false);
        Self {
            notebook,
            pages: HashMap::new(),
        }
    }

    pub fn widget(&self) -> &gtk::Notebook {
        &self.notebook
    }

    pub fn insert(&mut self, key: String, child: &gtk::Widget, label: &gtk::Widget) -> u32 {
        let index = self.notebook.append_page(child, Some(label));
        self.notebook.set_tab_reorderable(child, true);
        self.pages.insert(key, child.clone());
        index
    }

    pub fn focus(&self, key: &str) -> bool {
        let Some(child) = self.pages.get(key) else {
            return false;
        };
        if let Some(page) = self.notebook.page_num(child) {
            self.notebook.set_current_page(Some(page));
            true
        } else {
            false
        }
    }

    pub fn reorder(&self, key: &str, position: u32) -> bool {
        let Some(child) = self.pages.get(key) else {
            return false;
        };
        self.notebook.reorder_child(child, Some(position));
        true
    }

    pub fn remove(&mut self, key: &str) -> bool {
        let Some(child) = self.pages.remove(key) else {
            return false;
        };
        if let Some(page) = self.notebook.page_num(&child) {
            self.notebook.remove_page(Some(page));
            true
        } else {
            false
        }
    }

    pub fn overview(&self) -> Vec<String> {
        let mut pages: Vec<_> = self
            .pages
            .iter()
            .filter_map(|(key, child)| {
                self.notebook
                    .page_num(child)
                    .map(|index| (index, key.clone()))
            })
            .collect();
        pages.sort_by_key(|(index, _)| *index);
        pages.into_iter().map(|(_, key)| key).collect()
    }
}
