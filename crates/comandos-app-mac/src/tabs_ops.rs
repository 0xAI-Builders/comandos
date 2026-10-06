//! Restore, cancellation and persistence policy; no toolkit or I/O.
#![forbid(unsafe_code)]
use crate::app::{App, Tab, Ticket};
use comandos_desktop::mac_tabs::SavedTabs;
use comandos_desktop::{cancel_restore_snapshot, merge_tab_labels};
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    ops::{Deref, DerefMut},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};
#[derive(Default)]
pub struct RestoreState {
    pub saved: SavedTabs,
    pub active: bool,
    original_by_current: BTreeMap<String, String>,
    current_by_original: BTreeMap<String, String>,
    cancelled: BTreeSet<String>,
}
impl RestoreState {
    pub fn record_alias(&mut self, original: &str, current: &str) {
        if original.is_empty() || current.is_empty() || original == current {
            return;
        }
        self.original_by_current
            .insert(current.into(), original.into());
        self.current_by_original
            .insert(original.into(), current.into());
    }
    pub fn current_session(&self, session: &str) -> String {
        let original = self
            .original_by_current
            .get(session)
            .map_or(session, String::as_str);
        self.current_by_original
            .get(original)
            .map_or(session, String::as_str)
            .into()
    }
    pub fn cancel(&mut self, session: &str) -> bool {
        let original = self
            .original_by_current
            .get(session)
            .map_or(session, String::as_str)
            .to_string();
        let current = self
            .current_by_original
            .get(&original)
            .map_or(session, String::as_str)
            .to_string();
        let (remaining, removed) = cancel_restore_snapshot(&self.saved, &original);
        if !removed && original == session && current == session {
            return false;
        }
        self.saved = remaining;
        self.cancelled.extend([original, current, session.into()]);
        true
    }
    pub fn cancelled(&self, session: &str) -> bool {
        self.cancelled.contains(session)
    }
}
#[derive(Clone)]
pub struct Operation {
    root: Ticket,
    cancelled: Arc<AtomicBool>,
}
impl Operation {
    pub fn ticket_for(&self, root: &Ticket) -> Option<Ticket> {
        (self.current() && self.root.same_owner(root))
            .then(|| root.clone().with_stop(self.cancelled.clone()))
    }
    pub fn current(&self) -> bool {
        self.root.current() && !self.cancelled.load(Ordering::Acquire)
    }
}
pub struct TabsOps {
    pub app: App,
    pub restore: Arc<Mutex<RestoreState>>,
    operations: BTreeMap<String, Operation>,
}
impl Deref for TabsOps {
    type Target = App;
    fn deref(&self) -> &App {
        &self.app
    }
}
impl DerefMut for TabsOps {
    fn deref_mut(&mut self) -> &mut App {
        &mut self.app
    }
}
impl TabsOps {
    pub fn new(app: App) -> Self {
        Self {
            app,
            restore: Arc::new(Mutex::new(RestoreState::default())),
            operations: BTreeMap::new(),
        }
    }
    pub fn begin_restore(&mut self, saved: SavedTabs) {
        if let Ok(mut state) = self.restore.lock() {
            *state = RestoreState {
                saved,
                active: true,
                ..Default::default()
            };
        }
        self.app.begin_restore();
    }
    pub fn save_value(&self) -> Value {
        let saved = self
            .restore
            .lock()
            .map(|s| s.saved.clone())
            .unwrap_or_default();
        let current: Vec<_> = self
            .tabs()
            .iter()
            .filter(|t| !t.is_hub)
            .map(|t| (t.key.clone(), t.label.clone()))
            .collect();
        Value::Object(
            merge_tab_labels(&saved, &current)
                .into_iter()
                .map(|(key, label)| (key, Value::String(label)))
                .collect(),
        )
    }
    pub fn record_alias(&self, original: &str, current: &str) {
        if let Ok(mut state) = self.restore.lock() {
            state.record_alias(original, current);
        }
    }
    pub fn current_session(&self, session: &str) -> String {
        self.restore
            .lock()
            .map(|s| s.current_session(session))
            .unwrap_or_else(|_| session.into())
    }
    pub fn cancel_restore(&self, session: &str) -> bool {
        self.restore.lock().is_ok_and(|mut s| s.cancel(session))
    }
    pub fn restore_cancelled(&self, session: &str) -> bool {
        self.restore.lock().map_or(true, |s| s.cancelled(session))
    }
    pub fn finish_restore(&mut self, ticket: &Ticket) -> Vec<crate::app::Action> {
        if !self.app.owns_ticket(ticket) {
            return vec![];
        }
        if let Ok(mut state) = self.restore.lock() {
            *state = RestoreState::default();
        }
        self.app.finish_restore(ticket)
    }
    pub fn fail_restore(&mut self, ticket: &Ticket) -> Vec<crate::app::Action> {
        if !self.app.owns_ticket(ticket) {
            return vec![];
        }
        if let Ok(mut state) = self.restore.lock() {
            state.active = false;
        }
        self.app.finish_restore(ticket)
    }
    pub fn close_model(&mut self, key: &str, instance: u64) -> Option<Tab> {
        let tab = self.app.close_tab(key, instance)?;
        self.cancel_operation(key);
        self.cancel_restore(key);
        self.app.cancel_pending(key);
        Some(tab)
    }
    pub fn start_operation(&mut self, key: &str) -> Operation {
        self.cancel_operation(key);
        let operation = Operation {
            root: self.app.ticket(),
            cancelled: Arc::new(AtomicBool::new(false)),
        };
        self.operations.insert(key.into(), operation.clone());
        operation
    }
    pub fn cancel_operation(&mut self, key: &str) {
        if let Some(operation) = self.operations.remove(key) {
            operation.cancelled.store(true, Ordering::Release);
        }
    }
    pub fn finish_operation(&mut self, key: &str, operation: &Operation) {
        if self
            .operations
            .get(key)
            .is_some_and(|current| Arc::ptr_eq(&current.cancelled, &operation.cancelled))
        {
            self.operations.remove(key);
        }
    }
    pub fn shutdown(&mut self) {
        for operation in self.operations.values() {
            operation.cancelled.store(true, Ordering::Release);
        }
        self.operations.clear();
        if let Ok(mut state) = self.restore.lock() {
            *state = RestoreState::default();
        }
        self.app.shutdown();
    }
}
