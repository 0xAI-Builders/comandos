//! Heartbeat and real interaction have separate semantics and a monotonic clock.
use crate::config::RunMode;
use serde_json::{Value, json};
use std::rc::Rc;
#[derive(Default, Debug)]
pub struct Presence {
    last: u64,
}
impl Presence {
    pub fn last_interaction_ms(&self) -> u64 {
        self.last
    }
    pub fn payload(
        &mut self,
        now_ms: u64,
        interaction: bool,
        visible: bool,
        device_id: &str,
        mode: RunMode,
    ) -> Option<Value> {
        if mode == RunMode::Shadow {
            return None;
        }
        if interaction && now_ms.saturating_sub(self.last) < 5000 {
            return None;
        }
        if interaction {
            self.last = now_ms;
        }
        Some(
            json!({"deviceId":device_id,"kind":"desktop","visible":visible,"canPlayAudio":true,"interaction":interaction}),
        )
    }
    pub fn focus_payload(
        device_id: &str,
        key: &str,
        focus_ready: bool,
        mode: RunMode,
    ) -> Option<Value> {
        (mode != RunMode::Shadow && focus_ready && !key.is_empty())
            .then(|| json!({"deviceId":device_id,"activeTabId":key}))
    }
}
pub fn device_id(host: &str) -> String {
    let host = if host.is_empty() { "local" } else { host };
    format!(
        "desktop-{}",
        host.chars()
            .map(
                |c| if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') {
                    c
                } else {
                    '-'
                }
            )
            .take(60)
            .collect::<String>()
    )
}
pub fn install(app: &Rc<super::app::App>) -> glib::SourceId {
    app.install_presence()
}
