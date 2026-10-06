//! Shared tab registry model compatible with `app-tabs.json`.
use serde_json::Value;
use std::collections::{BTreeSet, VecDeque};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TabRecord {
    pub key: String,
    pub label: String,
    pub favorite: bool,
    pub kind: TabKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TabKind {
    Session,
    Local,
    Web,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TabsError {
    Malformed,
    EmptyKey,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TabRegistry {
    records: VecDeque<TabRecord>,
    favorite_generation: u64,
    archived: Vec<(String, String)>,
}

impl TabRegistry {
    pub fn records(&self) -> impl Iterator<Item = &TabRecord> {
        self.records.iter()
    }
    pub fn from_json(value: &Value) -> Result<Self, TabsError> {
        let object = value.as_object().ok_or(TabsError::Malformed)?;
        let mut records = VecDeque::new();
        for (key, raw) in object {
            if key.is_empty() {
                return Err(TabsError::EmptyKey);
            }
            let label = raw.as_str().ok_or(TabsError::Malformed)?;
            if label.is_empty() {
                continue;
            }
            records.push_back(TabRecord {
                key: key.clone(),
                label: label.to_string(),
                favorite: false,
                kind: kind_for(key),
            });
        }
        Ok(Self {
            records,
            favorite_generation: 0,
            archived: Vec::new(),
        })
    }

    pub fn ordered_keys(&self) -> Vec<String> {
        let mut records: Vec<_> = self.records.iter().collect();
        records.sort_by_key(|record| {
            if record.key == "local" {
                0
            } else if record.favorite {
                1
            } else {
                2
            }
        });
        records.into_iter().map(|r| r.key.clone()).collect()
    }

    pub fn insert(&mut self, record: TabRecord) {
        if let Some(existing) = self.records.iter_mut().find(|r| r.key == record.key) {
            *existing = record;
        } else {
            self.records.push_back(record);
        }
    }

    pub fn rename(&mut self, key: &str, label: &str) {
        if label.is_empty() {
            return;
        }
        if let Some(record) = self.records.iter_mut().find(|r| r.key == key) {
            record.label = label.to_string();
        }
    }

    pub fn reorder(&mut self, key: &str, position: usize) {
        if let Some(index) = self.records.iter().position(|r| r.key == key)
            && let Some(record) = self.records.remove(index)
        {
            self.records
                .insert(position.min(self.records.len()), record);
        }
    }

    pub fn favorite_keys(&self) -> Vec<String> {
        self.records
            .iter()
            .filter(|r| r.favorite)
            .map(|r| r.key.clone())
            .collect()
    }

    pub fn archive(&mut self, key: &str, reason: &str) {
        if key.is_empty() {
            return;
        }
        self.records.retain(|r| r.key != key);
        self.archived.push((key.to_string(), reason.to_string()));
    }

    pub fn apply_favorites(&mut self, value: &Value, generation: u64) {
        if generation < self.favorite_generation {
            return;
        }
        self.favorite_generation = generation;
        let favorites: BTreeSet<&str> = value
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .collect();
        for record in &mut self.records {
            record.favorite = favorites.contains(record.key.as_str());
        }
    }

    pub fn to_json(&self) -> Value {
        let mut out = serde_json::Map::new();
        for record in &self.records {
            out.insert(record.key.clone(), Value::String(record.label.clone()));
        }
        Value::Object(out)
    }
}

fn kind_for(key: &str) -> TabKind {
    if key == "local" {
        TabKind::Local
    } else if key.starts_with("web:") || key.starts_with("xterm-") {
        TabKind::Web
    } else {
        TabKind::Session
    }
}
