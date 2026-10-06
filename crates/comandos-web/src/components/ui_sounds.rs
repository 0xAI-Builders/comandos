pub const DEFAULT_CATALOG: &[(&str, &str)] = comandos_web_dom::audio::CATALOG;
pub const LOOP_CUES: &[&str] = &[
    "loading",
    "processing",
    "recording",
    "connecting",
    "scanning",
    "streaming",
];
pub const MAX_VOLUME: f32 = 0.3;

#[derive(Debug, Clone)]
pub struct Controller {
    enabled: bool,
    armed: bool,
    visible: bool,
    volume: f32,
    played: std::collections::BTreeSet<String>,
}

impl Default for Controller {
    fn default() -> Self {
        Self {
            enabled: false,
            armed: false,
            visible: true,
            volume: 0.6,
            played: std::collections::BTreeSet::new(),
        }
    }
}

impl Controller {
    pub fn set_enabled(&mut self, enabled: bool) -> bool {
        self.enabled = enabled;
        enabled
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    pub fn set_volume(&mut self, volume: f32) -> f32 {
        if volume.is_finite() {
            self.volume = volume.clamp(0.0, 1.0);
        }
        self.volume
    }

    pub fn get_volume(&self) -> f32 {
        self.volume
    }

    pub fn unlock(&mut self, trusted: bool) -> bool {
        if trusted && self.visible {
            self.armed = true;
        }
        self.armed
    }

    pub fn play(&mut self, cue: &str, event_id: Option<&str>) -> bool {
        if !self.enabled || !self.armed || !self.visible || resolve(cue).is_none() {
            return false;
        }
        if let Some(id) = event_id {
            if self.played.contains(id) {
                return false;
            }
            self.played.insert(id.to_string());
        }
        true
    }
}

pub fn resolve(cue: &str) -> Option<String> {
    let name = DEFAULT_CATALOG
        .iter()
        .find_map(|(public, engine)| (*public == cue).then_some(*engine))
        .unwrap_or(cue);
    (!LOOP_CUES.contains(&name)
        && DEFAULT_CATALOG
            .iter()
            .any(|(_, engine)| *engine == name || *engine == cue))
    .then(|| name.to_string())
}

#[cfg(target_arch = "wasm32")]
pub fn mount() -> Result<(), wasm_bindgen::JsValue> {
    use comandos_web_dom::bridge::global_set;
    use js_sys::{Array, Object, Reflect};
    use wasm_bindgen::JsValue;
    let api = Object::new();
    let cues = Array::new();
    for (cue, _) in DEFAULT_CATALOG {
        cues.push(&JsValue::from(*cue));
    }
    Reflect::set(&api, &"DEFAULT_CATALOG".into(), &cues)?;
    global_set("ComandosUISounds", &api.clone().into())?;
    global_set("uiSounds", &api.into())
}

#[cfg(not(target_arch = "wasm32"))]
pub fn mount() -> Result<(), wasm_bindgen::JsValue> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{Controller, resolve};

    #[test]
    fn opt_in_unlock_and_event_id_gate_playback() {
        let mut c = Controller::default();
        assert!(!c.play("complete", Some("e1")));
        c.set_enabled(true);
        assert!(!c.play("complete", Some("e1")));
        c.unlock(true);
        assert!(c.play("complete", Some("e1")));
        assert!(!c.play("complete", Some("e1")));
        assert!(c.play("complete", Some("e2")));
    }

    #[test]
    fn loops_are_not_registered_for_playback() {
        assert_eq!(resolve("loading"), None);
        assert_eq!(resolve("focus-start").as_deref(), Some("open"));
    }
}
