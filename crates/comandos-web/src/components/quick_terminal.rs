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
    use comandos_web_dom::{bridge::global_set, port::*};
    use std::{cell::RefCell, rc::Rc};
    use wasm_bindgen::JsValue;
    #[derive(Default)]
    struct State {
        pending: JsValue,
        inflight: Option<js_sys::Promise>,
    }
    let api = object();
    method(&api, "createQuickTerminal", move |args| {
        let options = args.get(0);
        let storage = get(&options, "storage");
        let key = get(&options, "storageKey")
            .as_string()
            .unwrap_or(DEFAULT_KEY.into());
        let pending = call(&storage, "getItem", &[key.clone().into()]).unwrap_or(JsValue::NULL);
        let state = Rc::new(RefCell::new(State {
            pending,
            inflight: None,
        }));
        let out = object();
        let st = state.clone();
        let opts = options.clone();
        method(&out, "open", move |_| {
            let mut current = st
                .try_borrow_mut()
                .map_err(|_| js_sys::Error::new("terminal state busy"))?;
            if let Some(p) = &current.inflight {
                return Ok(p.clone().into());
            }
            if !truthy(&current.pending) {
                let make = get(&opts, "makeId");
                current.pending = if make.is_function() {
                    invoke(&make, &[])?
                } else {
                    let crypto = get(&js_sys::global(), "crypto");
                    call(&crypto, "randomUUID", &[]).unwrap_or_else(|_| {
                        format!("qt-{}-{}", js_sys::Date::now(), js_sys::Math::random()).into()
                    })
                };
                let _ = call(
                    &storage,
                    "setItem",
                    &[key.clone().into(), current.pending.clone()],
                );
            }
            let id = current.pending.clone();
            let body = object();
            set(&body, "requestId", &id)?;
            let place = get(&opts, "place");
            if truthy(&place) {
                set(&body, "place", &place)?;
            }
            // Invoke synchronously, before returning, as the original async IIFE does.
            let request = invoke(&get(&opts, "api"), &["/terminal/quick".into(), body]);
            let task_st = st.clone();
            let task_opts = opts.clone();
            let task_storage = storage.clone();
            let task_key = key.clone();
            let p = wasm_bindgen_futures::future_to_promise(async move {
                let result = async {
                    let r = wait(request).await?;
                    let mut c = task_st
                        .try_borrow_mut()
                        .map_err(|_| js_sys::Error::new("terminal state busy"))?;
                    if c.pending == id {
                        c.pending = JsValue::NULL;
                        let _ = call(&task_storage, "removeItem", &[task_key.into()]);
                    }
                    drop(c);
                    let tab = get(&r, "tabId");
                    let label = get(&r, "label");
                    invoke(
                        &get(&task_opts, "openTerm"),
                        &[tab.clone(), if truthy(&label) { label } else { tab }],
                    )?;
                    let opened = get(&task_opts, "onOpened");
                    if opened.is_function() {
                        invoke(&opened, std::slice::from_ref(&r))?;
                    }
                    Ok::<_, JsValue>(r)
                }
                .await;
                let mut callback_error = None;
                let answer = match result {
                    Ok(r) => r,
                    Err(e) => {
                        let message = get(&e, "message");
                        let toast = get(&task_opts, "toast");
                        if toast.is_function() {
                            let toasted = invoke(
                                &toast,
                                &[
                                    if truthy(&message) {
                                        message
                                    } else {
                                        "No se pudo abrir la terminal".into()
                                    },
                                    true.into(),
                                ],
                            );
                            if let Err(error) = toasted {
                                callback_error = Some(error);
                            }
                        }
                        JsValue::NULL
                    }
                };
                if let Ok(mut c) = task_st.try_borrow_mut() {
                    c.inflight = None;
                }
                if let Some(error) = callback_error {
                    Err(error)
                } else {
                    Ok(answer)
                }
            });
            current.inflight = Some(p.clone());
            Ok(p.into())
        })?;
        let st = state.clone();
        getter(&out, "pendingRequestId", move || {
            st.try_borrow()
                .map(|s| s.pending.clone())
                .unwrap_or(JsValue::NULL)
        })?;
        getter(&out, "busy", move || {
            state
                .try_borrow()
                .map(|s| s.inflight.is_some())
                .unwrap_or(false)
                .into()
        })?;
        Ok(out)
    })?;
    global_set("ComandosQuickTerminal", &api)
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
        assert_eq!(
            request_body("r1", ""),
            serde_json::json!({"requestId": "r1"})
        );
        assert_eq!(
            request_body("r1", "sidebar"),
            serde_json::json!({"requestId": "r1", "place": "sidebar"})
        );
    }
}
