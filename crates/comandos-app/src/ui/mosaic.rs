//! Mosaic owns at most twelve client views and a separately cancelled zoom view.
use serde_json::Value;
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Session {
    pub session: String,
    pub label: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MosaicState {
    pub on: bool,
    pub sessions: Vec<Session>,
    pub zoomed: Option<String>,
}
impl MosaicState {
    pub fn open(&mut self, sessions: &[Value]) {
        if self.on {
            return;
        }
        self.sessions = sessions
            .iter()
            .filter_map(|value| {
                let name = value.get("session").and_then(Value::as_str)?;
                if !crate::tab_actions::valid_session(name) {
                    return None;
                }
                Some(Session {
                    session: name.into(),
                    label: value
                        .get("label")
                        .filter(|v| comandos_core::json::truthy(v))
                        .map(comandos_core::pomodoro::python_str)
                        .unwrap_or_else(|| name.into()),
                })
            })
            .collect();
        self.sessions
            .sort_by_key(|s| (s.session != "local", s.label.to_lowercase()));
        self.sessions.truncate(12);
        self.on = !self.sessions.is_empty();
    }
    pub fn close(&mut self) {
        self.on = false;
        self.sessions.clear();
        self.zoomed = None;
    }
    pub fn zoom(&mut self, session: &str) -> Result<(), String> {
        if !self.on || !self.sessions.iter().any(|s| s.session == session) {
            return Err("unknown mosaic session".into());
        }
        self.zoomed = Some(session.into());
        Ok(())
    }
    pub fn unzoom(&mut self) {
        self.zoomed = None;
    }
}
pub fn slots(count: usize) -> Vec<(i32, i32, i32)> {
    let n = count.min(12);
    if n == 0 {
        return vec![];
    }
    let cols = if n <= 4 {
        2
    } else if n <= 9 {
        3
    } else {
        4
    };
    let rows = n.div_ceil(cols);
    let (base, extra) = (n / rows, n % rows);
    let mut result = vec![];
    for row in 0..rows {
        let size = base + usize::from(row < extra);
        let span = 12 / size.max(1);
        let mut used = 0;
        for j in 0..size {
            let width = if j < size - 1 { span } else { 12 - used };
            result.push((row as i32, used as i32, width as i32));
            used += width;
        }
    }
    result
}
pub trait ManagedView {
    fn shutdown(&self);
}
pub struct OwnedViews<T: ManagedView> {
    views: std::collections::BTreeMap<String, T>,
    limit: usize,
    scope: super::snippets::Scope,
}
impl<T: ManagedView> OwnedViews<T> {
    pub fn new(limit: usize) -> Self {
        Self {
            views: std::collections::BTreeMap::new(),
            limit,
            scope: super::snippets::Scope::default(),
        }
    }
    pub fn ticket(&self) -> super::snippets::Ticket {
        self.scope.ticket()
    }
    pub fn insert(&mut self, key: String, view: T) -> Result<(), String> {
        if self.views.len() >= self.limit && !self.views.contains_key(&key) {
            view.shutdown();
            return Err("client view limit".into());
        }
        if let Some(old) = self.views.insert(key, view) {
            old.shutdown();
        }
        Ok(())
    }
    pub fn remove(&mut self, key: &str) {
        if let Some(old) = self.views.remove(key) {
            old.shutdown();
        }
    }
    pub fn clear(&mut self) {
        self.scope.advance();
        for (_, view) in std::mem::take(&mut self.views) {
            view.shutdown();
        }
    }
    pub fn get(&self, key: &str) -> Option<&T> {
        self.views.get(key)
    }
    pub fn values(&self) -> impl Iterator<Item = &T> {
        self.views.values()
    }
    pub fn keys(&self) -> impl Iterator<Item = &String> {
        self.views.keys()
    }
}
impl<T: ManagedView> Drop for OwnedViews<T> {
    fn drop(&mut self) {
        self.clear();
        self.scope.close();
    }
}
