use serde_json::{Map, Value, json};

pub fn line_at(text: &str, index: usize) -> String {
    let safe_index = index.min(text.len());
    let start = text
        .get(..safe_index)
        .and_then(|s| s.rfind('\n').map(|i| i + 1))
        .unwrap_or(0);
    let rest = text.get(start..).unwrap_or_default();
    let end = rest.find('\n').unwrap_or(rest.len());
    rest.get(..end).unwrap_or_default().to_string()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestoreResult {
    pub state: &'static str,
    pub text: Option<String>,
    pub selection: Option<(usize, usize)>,
}

pub struct DraftMemory {
    key: String,
    last_saved: Option<String>,
    pending: Option<(String, Option<usize>, Option<usize>)>,
}

impl DraftMemory {
    pub fn new(key: &str) -> Self {
        Self {
            key: key.to_string(),
            last_saved: None,
            pending: None,
        }
    }

    pub fn restore(&mut self, state_json: &str, current_text: &str) -> RestoreResult {
        if !current_text.is_empty() {
            return RestoreResult {
                state: "local",
                text: None,
                selection: None,
            };
        }
        let parsed: Value = serde_json::from_str(state_json).unwrap_or(Value::Null);
        let draft = parsed
            .get("drafts")
            .and_then(|d| d.get(&self.key))
            .unwrap_or(&Value::Null);
        let text = draft
            .get("text")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if text.is_empty() {
            return RestoreResult {
                state: "none",
                text: None,
                selection: None,
            };
        }
        self.last_saved = Some(text.to_string());
        let start = draft
            .get("selStart")
            .and_then(Value::as_u64)
            .and_then(|n| usize::try_from(n).ok());
        let end = draft
            .get("selEnd")
            .and_then(Value::as_u64)
            .and_then(|n| usize::try_from(n).ok())
            .or(start);
        RestoreResult {
            state: "restored",
            text: Some(text.to_string()),
            selection: start.zip(end),
        }
    }

    pub fn changed(&mut self, text: &str, sel_start: Option<usize>, sel_end: Option<usize>) {
        self.pending = Some((text.to_string(), sel_start, sel_end));
    }

    pub fn flush_json(&mut self, now: u64) -> Option<String> {
        let (text, sel_start, sel_end) = self.pending.take()?;
        if self.last_saved.as_deref() == Some(text.as_str()) {
            return None;
        }
        self.last_saved = Some(text.clone());
        let value = if text.is_empty() {
            json!({"draftsPatch": {self.key.clone(): Value::Null}})
        } else {
            let mut entry = Map::new();
            entry.insert("text".into(), json!(text));
            if let Some(v) = sel_start {
                entry.insert("selStart".into(), json!(v));
            }
            if let Some(v) = sel_end {
                entry.insert("selEnd".into(), json!(v));
            }
            entry.insert("updatedAt".into(), json!(now));
            json!({"draftsPatch": {self.key.clone(): Value::Object(entry)}})
        };
        serde_json::to_string(&value).ok()
    }

    pub fn mark_save_failed(&mut self) {
        self.last_saved = None;
    }

    pub fn can_send_to_terminal(&self) -> bool {
        false
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnchorFind {
    pub state: &'static str,
    pub index: Option<usize>,
}

pub struct AnchorMemory {
    key: String,
    saved: Option<String>,
}

impl AnchorMemory {
    pub fn new(key: &str) -> Self {
        Self {
            key: key.to_string(),
            saved: None,
        }
    }

    pub fn find_json(&self, state_json: &str, text: &str) -> AnchorFind {
        let parsed: Value = serde_json::from_str(state_json).unwrap_or(Value::Null);
        let anchor = parsed
            .get("readingAnchors")
            .and_then(|d| d.get(&self.key))
            .unwrap_or(&Value::Null);
        let Some(line) = anchor.get("text").and_then(Value::as_str) else {
            return AnchorFind {
                state: "none",
                index: None,
            };
        };
        if line.is_empty() {
            return AnchorFind {
                state: "none",
                index: None,
            };
        }
        match text.rfind(line) {
            Some(index) => AnchorFind {
                state: "found",
                index: Some(index),
            },
            None => AnchorFind {
                state: "missing",
                index: None,
            },
        }
    }

    pub fn remember_json(&mut self, text: &str, top_index: usize, ratio: f64, now: u64) -> Option<String> {
        let line = line_at(text, top_index).trim().chars().take(300).collect::<String>();
        if line.is_empty() || self.saved.as_deref() == Some(line.as_str()) {
            return None;
        }
        self.saved = Some(line.clone());
        serde_json::to_string(&json!({
            "anchorsPatch": {
                self.key.clone(): {"text": line, "ratio": ratio, "updatedAt": now}
            }
        }))
        .ok()
    }
}

#[cfg(target_arch = "wasm32")]
pub mod web {
    use super::line_at;
    use js_sys::{Object, Reflect};
    use wasm_bindgen::JsValue;

    pub fn export() -> Result<(), JsValue> {
        let api = Object::new();
        let line = wasm_bindgen::closure::Closure::<dyn Fn(JsValue, JsValue) -> JsValue>::new(
            |text: JsValue, index: JsValue| {
                JsValue::from(line_at(
                    &text.as_string().unwrap_or_default(),
                    index.as_f64().unwrap_or(0.0).max(0.0) as usize,
                ))
            },
        );
        Reflect::set(&api, &"lineAt".into(), line.as_ref())?;
        line.forget();
        Reflect::set(&js_sys::global(), &"ComandosDeviceDrafts".into(), &api).map(|_| ())
    }
}
