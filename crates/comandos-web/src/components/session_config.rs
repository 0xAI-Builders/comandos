use serde_json::{Value, json};

const CONFIG_FIELDS: &[&str] = &[
    "toHarness",
    "motor",
    "model",
    "effort",
    "harnessAccount",
    "motorAccount",
];

pub fn draft(item: &Value) -> Value {
    let harness = item.get("agent").and_then(Value::as_str).unwrap_or("shell");
    json!({
        "toHarness": harness,
        "motor": item.get("motor").and_then(Value::as_str).unwrap_or(harness),
        "model": item.get("model").and_then(Value::as_str).unwrap_or(""),
        "effort": item.get("effort").and_then(Value::as_str).unwrap_or(""),
        "harnessAccount": item.get("harnessAccount").or_else(|| item.get("account")).and_then(Value::as_str).unwrap_or("main"),
        "motorAccount": item.get("motorAccount").or_else(|| item.get("account")).and_then(Value::as_str).unwrap_or("main"),
        "interrupt": false,
        "expectedIdentity": item.pointer("/observedConfig/identity").cloned().unwrap_or(Value::Null),
        "expectedConversationId": item.pointer("/observedConfig/conversationId").cloned().unwrap_or(Value::Null),
    })
}

pub fn configuration(base: &Value, values: &Value) -> Value {
    let mut next = base.clone();
    if let Some(o) = next.as_object_mut() {
        o.insert("interrupt".into(), Value::Bool(false));
        for key in CONFIG_FIELDS {
            if let Some(s) = values.get(*key).and_then(Value::as_str) {
                o.insert((*key).into(), Value::String(s.to_string()));
            }
        }
    }
    next
}

pub fn changed(before: &Value, after: &Value) -> bool {
    CONFIG_FIELDS.iter().any(|k| {
        before.get(*k).and_then(Value::as_str).unwrap_or("")
            != after.get(*k).and_then(Value::as_str).unwrap_or("")
    })
}

pub fn requires_confirmation(before: &Value, after: &Value) -> bool {
    before.get("toHarness").and_then(Value::as_str)
        != after.get("toHarness").and_then(Value::as_str)
        || before.get("motor").and_then(Value::as_str) != after.get("motor").and_then(Value::as_str)
        || after.get("interrupt").and_then(Value::as_bool).unwrap_or(false)
}

#[cfg(target_arch = "wasm32")]
pub fn mount() -> Result<(), wasm_bindgen::JsValue> {
    use comandos_web_dom::bridge::global_set;
    use js_sys::Object;
    global_set("SessionConfig", &Object::new().into())
}

#[cfg(not(target_arch = "wasm32"))]
pub fn mount() -> Result<(), wasm_bindgen::JsValue> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{changed, configuration, draft, requires_confirmation};
    use serde_json::json;

    #[test]
    fn draft_preserves_accounts_and_observed_identity() {
        let got = draft(&json!({"agent":"codex","model":"gpt-5","account":"work","observedConfig":{"identity":"i","conversationId":"c"}}));
        assert_eq!(got["toHarness"], "codex");
        assert_eq!(got["motor"], "codex");
        assert_eq!(got["harnessAccount"], "work");
        assert_eq!(got["motorAccount"], "work");
        assert_eq!(got["expectedIdentity"], "i");
    }

    #[test]
    fn configuration_copies_only_string_config_fields_and_resets_interrupt() {
        let base = json!({"toHarness":"claude","motor":"claude","model":"old","interrupt":true});
        let next = configuration(&base, &json!({"model":"new","interrupt":"ignored"}));
        assert_eq!(next["model"], "new");
        assert_eq!(next["interrupt"], false);
        assert!(changed(&base, &next));
        assert!(!requires_confirmation(&base, &next));
    }
}
