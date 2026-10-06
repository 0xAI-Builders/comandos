use serde_json::{Map, Value, json};

pub fn line_at(text: &str, index: usize) -> String {
    // JS indices and slice limits are UTF-16 code units, including emoji.
    let chars = text.encode_utf16().collect::<Vec<_>>();
    let boundary = index.saturating_sub(1).min(chars.len().saturating_sub(1));
    let start = chars
        .iter()
        .enumerate()
        .take(boundary + 1)
        .filter(|(_, c)| **c == 10)
        .map(|(i, _)| i + 1)
        .next_back()
        .unwrap_or(0);
    let end = chars
        .iter()
        .enumerate()
        .skip(start)
        .find(|(_, c)| **c == 10)
        .map(|(i, _)| i)
        .unwrap_or(chars.len());
    String::from_utf16_lossy(chars.get(start..end).unwrap_or_default())
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

    pub fn remember_json(
        &mut self,
        text: &str,
        top_index: usize,
        ratio: f64,
        now: u64,
    ) -> Option<String> {
        let line = line_at(text, top_index)
            .trim()
            .chars()
            .take(300)
            .collect::<String>();
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
    use crate::port::*;
    use std::{cell::RefCell, rc::Rc};
    use wasm_bindgen::JsValue;
    #[derive(Default)]
    struct State {
        timer: JsValue,
        last_saved: Option<String>,
        pending: Option<(String, JsValue, JsValue)>,
    }
    #[derive(Default)]
    struct AnchorState {
        timer: JsValue,
        saved: JsValue,
    }
    // Keep browser strings in UTF-16: a JavaScript slice may end in a lone
    // surrogate, which cannot round-trip through Rust's UTF-8 String.
    fn js_line_at(text: JsValue, index: JsValue) -> js_sys::JsString {
        let text = js_sys::JsString::from(text);
        let position = (number(&index) - 1.0).max(0.0) as i32;
        let start = text.last_index_of("\n", position) + 1;
        let end = text.index_of("\n", start);
        text.slice(
            start as u32,
            if end < 0 { text.length() } else { end as u32 },
        )
    }
    fn timer_options(opts: &JsValue, delay: f64) -> (JsValue, JsValue, JsValue, JsValue) {
        let global = js_sys::global();
        let schedule = get(opts, "schedule");
        let cancel = get(opts, "cancel");
        let now = get(opts, "now");
        let ms = get(opts, "delayMs");
        (
            if schedule.is_function() {
                schedule
            } else {
                get(&global, "setTimeout")
            },
            if cancel.is_function() {
                cancel
            } else {
                get(&global, "clearTimeout")
            },
            if now.is_function() {
                now
            } else {
                get(&get(&global, "Date"), "now")
            },
            if ms.is_undefined() { delay.into() } else { ms },
        )
    }
    fn integer(v: &JsValue) -> bool {
        v.as_f64()
            .is_some_and(|n| n.is_finite() && n.fract() == 0.0)
    }
    pub fn export() -> Result<(), JsValue> {
        let api = object();
        method(&api, "lineAt", |args| {
            Ok(js_line_at(args.get(0), args.get(1)).into())
        })?;
        method(&api, "createDrafts", |args| {
            let opts = args.get(0);
            let key = string(&get(&opts, "key"));
            let (schedule, cancel, now, delay) = timer_options(&opts, 600.0);
            let state = Rc::new(RefCell::new(State {
                timer: 0.into(),
                ..State::default()
            }));
            let out = object();
            let st = state.clone();
            let options = opts.clone();
            let k = key.clone();
            method(&out, "restore", move |_| {
                let st = st.clone();
                let opts = options.clone();
                let key = k.clone();
                // The pre-load local check and load call occur in the calling stack.
                let local = truthy(&invoke(&get(&opts, "read"), &[])?);
                let loaded = if local {
                    Ok(JsValue::NULL)
                } else {
                    invoke(&get(&opts, "load"), &[])
                };
                Ok(wasm_bindgen_futures::future_to_promise(async move {
                    if local {
                        return Ok("local".into());
                    }
                    let loaded = match wait(loaded).await {
                        Ok(v) => v,
                        Err(_) => return Ok("unavailable".into()),
                    };
                    let draft = get(&get(&loaded, "drafts"), &key);
                    let text = get(&draft, "text");
                    if text.as_string().is_none_or(|s| s.is_empty())
                        || truthy(&invoke(&get(&opts, "read"), &[])?)
                    {
                        return Ok("none".into());
                    }
                    invoke(&get(&opts, "write"), std::slice::from_ref(&text))?;
                    let start = get(&draft, "selStart");
                    let end = get(&draft, "selEnd");
                    let select = get(&opts, "select");
                    if select.is_function() && integer(&start) {
                        invoke(
                            &select,
                            &[start.clone(), if integer(&end) { end } else { start }],
                        )?;
                    }
                    if let Ok(mut s) = st.try_borrow_mut() {
                        s.last_saved = text.as_string();
                    }
                    Ok("restored".into())
                })
                .into())
            })?;
            let st = state.clone();
            let opts_flush = opts.clone();
            let cancel_flush = cancel.clone();
            let key_flush = key.clone();
            let flush = function(move |_| {
                let mut state = st
                    .try_borrow_mut()
                    .map_err(|_| js_sys::Error::new("draft state busy"))?;
                invoke(&cancel_flush, std::slice::from_ref(&state.timer))?;
                state.timer = 0.into();
                let Some((text, start, end)) = state.pending.take() else {
                    return Ok(js_sys::Promise::resolve(&JsValue::FALSE).into());
                };
                if state.last_saved.as_deref() == Some(&text) {
                    return Ok(js_sys::Promise::resolve(&JsValue::FALSE).into());
                }
                state.last_saved = Some(text.clone());
                drop(state);
                let entry = if text.is_empty() {
                    JsValue::NULL
                } else {
                    let o = object();
                    set(&o, "text", &text.into())?;
                    set(&o, "selStart", &start)?;
                    set(&o, "selEnd", &end)?;
                    set(&o, "updatedAt", &invoke(&now, &[])?)?;
                    o
                };
                let patch = object();
                set(&patch, &key_flush, &entry)?;
                let payload = object();
                set(&payload, "draftsPatch", &patch)?;
                let saved = invoke(&get(&opts_flush, "save"), &[payload]);
                let st = st.clone();
                Ok(wasm_bindgen_futures::future_to_promise(async move {
                    if wait(saved).await.is_ok() {
                        Ok(true.into())
                    } else {
                        if let Ok(mut s) = st.try_borrow_mut() {
                            s.last_saved = None;
                        }
                        Ok(false.into())
                    }
                })
                .into())
            });
            set(&out, "flush", &flush)?;
            method(&out, "changed", move |args| {
                let raw = args.get(0);
                let text = if truthy(&raw) {
                    string(&raw)
                } else {
                    String::new()
                };
                let mut st = state
                    .try_borrow_mut()
                    .map_err(|_| js_sys::Error::new("draft state busy"))?;
                st.pending = Some((text, args.get(1), args.get(2)));
                invoke(&cancel, std::slice::from_ref(&st.timer))?;
                st.timer = invoke(&schedule, &[flush.clone(), delay.clone()])?;
                Ok(JsValue::UNDEFINED)
            })?;
            Ok(out)
        })?;
        method(&api, "createAnchor", |args| {
            let opts = args.get(0);
            let key = string(&get(&opts, "key"));
            let (schedule, cancel, now, delay) = timer_options(&opts, 800.0);
            let state = Rc::new(RefCell::new(AnchorState {
                timer: 0.into(),
                ..AnchorState::default()
            }));
            let out = object();
            let options = opts.clone();
            let k = key.clone();
            method(&out, "find", move |args| {
                let text = args.get(0);
                let load = invoke(&get(&options, "load"), &[]);
                let key = k.clone();
                Ok(wasm_bindgen_futures::future_to_promise(async move {
                    let loaded = match wait(load).await {
                        Ok(v) => v,
                        Err(_) => return from_json(&serde_json::json!({"state":"unavailable"})),
                    };
                    let anchor = get(&get(&loaded, "readingAnchors"), &key);
                    let line = get(&anchor, "text");
                    if !truthy(&line) {
                        return from_json(&serde_json::json!({"state":"none"}));
                    }
                    let prototype = get(&get(&js_sys::global(), "String"), "prototype");
                    let index = js_sys::Function::from(get(&prototype, "lastIndexOf"))
                        .call1(&text, &line)?
                        .as_f64()
                        .unwrap_or(-1.0) as i32;
                    if index < 0 {
                        let o = object();
                        set(&o, "state", &"missing".into())?;
                        set(&o, "anchor", &anchor)?;
                        Ok(o)
                    } else {
                        from_json(&serde_json::json!({"state":"found","index":index}))
                    }
                })
                .into())
            })?;
            method(&out, "remember", move |args| {
                let line: JsValue = js_line_at(args.get(0), args.get(1))
                    .trim()
                    .slice(0, 300)
                    .into();
                let mut st = state
                    .try_borrow_mut()
                    .map_err(|_| js_sys::Error::new("anchor state busy"))?;
                if !truthy(&line) || st.saved == line {
                    return Ok(JsValue::UNDEFINED);
                }
                invoke(&cancel, std::slice::from_ref(&st.timer))?;
                let ratio = args.get(2);
                let key = key.clone();
                let now = now.clone();
                let opts = opts.clone();
                let memory = state.clone();
                let callback = function(move |_| {
                    if let Ok(mut s) = memory.try_borrow_mut() {
                        s.saved = line.clone();
                    }
                    let entry = object();
                    set(&entry, "text", &line)?;
                    set(&entry, "ratio", &ratio)?;
                    set(&entry, "updatedAt", &invoke(&now, &[])?)?;
                    let patch = object();
                    set(&patch, &key, &entry)?;
                    let payload = object();
                    set(&payload, "anchorsPatch", &patch)?;
                    let saved = invoke(&get(&opts, "save"), &[payload]);
                    let memory = memory.clone();
                    wasm_bindgen_futures::spawn_local(async move {
                        if wait(saved).await.is_err()
                            && let Ok(mut s) = memory.try_borrow_mut()
                        {
                            s.saved = JsValue::NULL;
                        }
                    });
                    Ok(JsValue::UNDEFINED)
                });
                st.timer = invoke(&schedule, &[callback, delay.clone()])?;
                Ok(JsValue::UNDEFINED)
            })?;
            Ok(out)
        })?;
        crate::bridge::global_set("ComandosDeviceDrafts", &api)
    }
}
