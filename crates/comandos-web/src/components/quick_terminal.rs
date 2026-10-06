const DEFAULT_KEY: &str = "comandos.quickTerminal.pending";

pub struct Core<F>
where
    F: FnMut() -> String,
{
    pending: Option<String>,
    inflight: bool,
    make_id: F,
}

impl<F> Core<F>
where
    F: FnMut() -> String,
{
    pub fn new(stored: Option<String>, make_id: F) -> Self {
        Self {
            pending: stored,
            inflight: false,
            make_id,
        }
    }

    pub fn begin(&mut self) -> Option<String> {
        if self.inflight {
            return None;
        }
        if self.pending.is_none() {
            self.pending = Some((self.make_id)());
        }
        self.inflight = true;
        self.pending.clone()
    }

    pub fn fail(&mut self) {
        self.inflight = false;
    }

    pub fn succeed(&mut self, request_id: &str) {
        if self.pending.as_deref() == Some(request_id) {
            self.pending = None;
        }
        self.inflight = false;
    }

    pub fn pending_request_id(&self) -> Option<&str> {
        self.pending.as_deref()
    }

    pub fn busy(&self) -> bool {
        self.inflight
    }
}

pub fn storage_key(key: Option<&str>) -> &str {
    key.unwrap_or(DEFAULT_KEY)
}

pub fn request_body(request_id: &str, place: &str) -> serde_json::Value {
    if place.is_empty() {
        serde_json::json!({ "requestId": request_id })
    } else {
        serde_json::json!({ "requestId": request_id, "place": place })
    }
}

#[cfg(target_arch = "wasm32")]
pub fn mount() -> Result<(), wasm_bindgen::JsValue> {
    use comandos_web_dom::bridge::global_set;
    use js_sys::{Object, Reflect};
    let api = Object::new();
    Reflect::set(
        &api,
        &"storageKey".into(),
        &wasm_bindgen::JsValue::from(DEFAULT_KEY),
    )?;
    global_set("ComandosQuickTerminal", &api.into())
}

#[cfg(not(target_arch = "wasm32"))]
pub fn mount() -> Result<(), wasm_bindgen::JsValue> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{Core, request_body};

    #[test]
    fn same_request_id_until_success_then_new() {
        let mut c = Core::new(None, || "id-1".to_string());
        assert_eq!(c.begin().as_deref(), Some("id-1"));
        assert_eq!(
            c.begin(),
            None,
            "doble clic mientras vuela: no hay segunda petición"
        );
        c.fail();
        assert_eq!(
            c.begin().as_deref(),
            Some("id-1"),
            "tras un error se reutiliza el mismo id"
        );
        c.succeed("id-1");
        let mut c2 = Core::new(None, || "id-2".to_string());
        assert_eq!(c2.begin().as_deref(), Some("id-2"));
    }

    #[test]
    fn stored_pending_id_survives_reload() {
        let mut c = Core::new(Some("guardado".into()), || "nuevo".into());
        assert_eq!(c.begin().as_deref(), Some("guardado"));
    }

    #[test]
    fn request_body_keeps_sidebar_place_contract() {
        assert_eq!(request_body("r1", ""), serde_json::json!({"requestId": "r1"}));
        assert_eq!(
            request_body("r1", "sidebar"),
            serde_json::json!({"requestId": "r1", "place": "sidebar"})
        );
    }
}
