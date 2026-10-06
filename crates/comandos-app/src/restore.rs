use crate::config::RunMode;
use serde_json::Value;
use std::collections::BTreeSet;

#[derive(Debug, Clone, PartialEq)]
pub struct RestorePlan {
    pub actions: Vec<RestoreAction>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum RestoreAction {
    AttachExisting { key: String, label: String },
    CreatePlaceholder { key: String, label: String },
    RestoreLayout { key: String, snapshot: Value },
    ResumeExact { key: String, snapshot: Value },
    ShowAmbiguity { key: String, label: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RestoreError {
    Malformed,
}

impl RestorePlan {
    pub fn build(
        tabs: &Value,
        snapshots: &Value,
        present: &BTreeSet<String>,
        mode: RunMode,
    ) -> Result<Self, RestoreError> {
        let object = tabs.as_object().ok_or(RestoreError::Malformed)?;
        let mut actions = Vec::new();
        for (key, label) in object {
            let label = label.as_str().ok_or(RestoreError::Malformed)?.to_string();
            if present.contains(key) {
                actions.push(RestoreAction::AttachExisting {
                    key: key.clone(),
                    label,
                });
                continue;
            }
            if mode == RunMode::Shadow {
                actions.push(RestoreAction::ShowAmbiguity {
                    key: key.clone(),
                    label,
                });
                continue;
            }
            let snapshot = snapshots.get(key).cloned().unwrap_or(Value::Null);
            actions.push(RestoreAction::CreatePlaceholder {
                key: key.clone(),
                label,
            });
            if snapshot.get("windows").is_some() {
                actions.push(RestoreAction::RestoreLayout {
                    key: key.clone(),
                    snapshot: snapshot.clone(),
                });
            }
            if snapshot.get("resume_id").and_then(Value::as_str).is_some()
                || snapshot
                    .pointer("/acp/sessionId")
                    .and_then(Value::as_str)
                    .is_some()
            {
                actions.push(RestoreAction::ResumeExact {
                    key: key.clone(),
                    snapshot,
                });
            }
        }
        Ok(Self { actions })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RestoreResult {
    Complete,
    Partial,
    Failed,
}

#[derive(Default)]
pub struct RestoreCoordinator {
    restoring: bool,
    result: Option<RestoreResult>,
}

impl RestoreCoordinator {
    pub fn begin(&mut self) {
        self.restoring = true;
        self.result = None;
    }

    pub fn ready(&self) -> bool {
        !self.restoring
    }

    pub fn finish(&mut self, result: RestoreResult) {
        self.result = Some(result);
        self.restoring = false;
    }

    pub fn result(&self) -> Option<RestoreResult> {
        self.result
    }
}
