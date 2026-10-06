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
    let harness = item
        .get("agent")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .unwrap_or("shell");
    json!({
        "toHarness": harness,
        "motor": item.get("motor").and_then(Value::as_str).filter(|s|!s.is_empty()).unwrap_or(harness),
        "model": item.get("model").and_then(Value::as_str).unwrap_or(""),
        "effort": item.get("effort").and_then(Value::as_str).unwrap_or(""),
        "harnessAccount": item.get("harnessAccount").and_then(Value::as_str).filter(|s|!s.is_empty()).or_else(||item.get("account").and_then(Value::as_str).filter(|s|!s.is_empty())).unwrap_or("main"),
        "motorAccount": item.get("motorAccount").and_then(Value::as_str).filter(|s|!s.is_empty()).or_else(||item.get("account").and_then(Value::as_str).filter(|s|!s.is_empty())).unwrap_or("main"),
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
        || after
            .get("interrupt")
            .and_then(Value::as_bool)
            .unwrap_or(false)
}

fn at<'a>(v: &'a Value, k: &str) -> &'a Value {
    v.get(k).unwrap_or(&Value::Null)
}
fn put(v: &mut Value, k: &str, next: Value) {
    if let Some(o) = v.as_object_mut() {
        o.insert(k.to_string(), next);
    }
}
fn text<'a>(v: &'a Value, k: &str) -> &'a str {
    v.get(k).and_then(Value::as_str).unwrap_or("")
}
fn array(v: &Value) -> Vec<Value> {
    v.as_array().cloned().unwrap_or_default()
}
fn yes(v: &Value, k: &str) -> bool {
    v.get(k).and_then(Value::as_bool).unwrap_or(false)
}
pub fn route(registry: &Value, state: &Value) -> Value {
    array(at(registry, "matrix"))
        .into_iter()
        .find(|r| {
            r.get("harness") == state.get("toHarness") && r.get("motor") == state.get("motor")
        })
        .unwrap_or(Value::Null)
}
pub fn choices(registry: &Value, state: &Value) -> Value {
    let models = array(at(
        at(at(registry, "motors"), text(state, "motor")),
        "models",
    ));
    let model = models.iter().find(|m| m.get("id") == state.get("model"));
    let provider = if text(state, "toHarness") == "acp" {
        text(state, "motor")
    } else {
        text(state, "toHarness")
    };
    json!({"route":route(registry,state),"models":models,"efforts":model.map(|m|array(at(m, "efforts"))).unwrap_or_default(),"accounts":array(at(at(at(registry, "harnesses"), provider), "accounts")),"motorAccounts":array(at(at(at(registry, "harnesses"), text(state,"motor")), "accounts"))})
}
pub fn update(registry: &Value, state: &Value, field: &str, value: &str) -> Value {
    let mut next = state.clone();
    put(&mut next, field, json!(value));
    if field == "toHarness" && !yes(&route(registry, &next), "selectable") {
        let rows = array(at(registry, "matrix"));
        let r = rows
            .iter()
            .find(|r| {
                text(r, "harness") == value && text(r, "motor") == value && yes(r, "selectable")
            })
            .or_else(|| {
                rows.iter()
                    .find(|r| text(r, "harness") == value && yes(r, "selectable"))
            });
        put(
            &mut next,
            "motor",
            json!(
                r.map(|r| text(r, "motor"))
                    .filter(|s| !s.is_empty())
                    .unwrap_or(value)
            ),
        );
    }
    let c = choices(registry, &next);
    if ["toHarness", "motor"].contains(&field)
        && !array(at(&c, "models"))
            .iter()
            .any(|m| m.get("id") == next.get("model"))
    {
        put(
            &mut next,
            "model",
            json!(
                array(at(&c, "models"))
                    .first()
                    .map(|m| text(m, "id"))
                    .unwrap_or("")
            ),
        );
    }
    if ["toHarness", "motor", "model"].contains(&field) {
        let models = array(at(&c, "models"));
        let m = models
            .iter()
            .find(|m| m.get("id") == next.get("model"))
            .cloned()
            .unwrap_or(Value::Null);
        if !array(at(&m, "efforts")).contains(at(&next, "effort")) {
            put(
                &mut next,
                "effort",
                m.get("defaultEffort")
                    .filter(|v| v.as_str().is_some_and(|s| !s.is_empty()))
                    .cloned()
                    .or_else(|| array(at(&m, "efforts")).first().cloned())
                    .unwrap_or(json!("")),
            );
        }
    }
    if text(&next, "toHarness") == "acp" {
        put(&mut next, "harnessAccount", json!("main"));
    } else if next.get("motor") == next.get("toHarness") {
        let account = at(&next, "harnessAccount").clone();
        put(&mut next, "motorAccount", account);
    }
    next
}
pub fn switch_error(registry: &Value, state: &Value, source: &Value) -> String {
    if source.is_null() {
        return String::new();
    }
    let agent = text(source, "agent");
    if !agent.is_empty() && !["shell", "claude", "codex", "grok", "acp"].contains(&agent) {
        return "Este CLI no ofrece recuperación exacta. Puedes iniciar una sesión nueva con otro CLI.".into();
    }
    let r = route(registry, state);
    let id = if text(&r, "id").is_empty() {
        format!("{}:{}", text(state, "toHarness"), text(state, "motor"))
    } else {
        text(&r, "id").into()
    };
    let rule = &at(at(registry, "midSessionRoutes"), &id);
    if rule.is_null() || yes(rule, "selectable") {
        return String::new();
    }
    let observed = &at(source, "observedConfig");
    if text(at(rule, "reason"), "code") == "acp_effort_unobserved"
        && text(observed, "harness") == "acp"
        && observed.get("motor") == state.get("motor")
        && text(observed, "effortSource") == "acp-config-options"
    {
        return String::new();
    }
    let msg = text(at(rule, "reason"), "message");
    if msg.is_empty() {
        "Este cambio no está disponible en una sesión abierta".into()
    } else {
        msg.into()
    }
}
pub fn validate(registry: &Value, state: &Value, source: &Value) -> String {
    let c = choices(registry, state);
    if !yes(at(&c, "route"), "selectable") {
        let msg = text(at(at(&c, "route"), "reason"), "message");
        return if msg.is_empty() {
            "Esta combinación no está disponible".into()
        } else {
            msg.into()
        };
    }
    let unsupported = switch_error(registry, state, source);
    if !unsupported.is_empty() {
        return unsupported;
    }
    let models = array(at(&c, "models"));
    if !models.is_empty()
        && !models
            .iter()
            .any(|m| m.get("id") == state.get("model") && !yes(m, "soon"))
    {
        return "Selecciona un modelo disponible".into();
    }
    if !text(state, "effort").is_empty() && !array(at(&c, "efforts")).contains(at(state, "effort"))
    {
        return "Selecciona un esfuerzo compatible".into();
    }
    let account = if text(state, "toHarness") == "acp" {
        "motorAccount"
    } else {
        "harnessAccount"
    };
    let accounts = array(at(&c, "accounts"));
    if !accounts.is_empty()
        && !accounts
            .iter()
            .any(|a| a.get("alias") == state.get(account) && yes(a, "selectable"))
    {
        return "Selecciona una cuenta con sesión iniciada".into();
    }
    let accounts = array(at(&c, "motorAccounts"));
    if state.get("motor") != state.get("toHarness")
        && text(state, "toHarness") != "acp"
        && !accounts.is_empty()
        && !accounts
            .iter()
            .any(|a| a.get("alias") == state.get("motorAccount") && yes(a, "motorSelectable"))
    {
        return "Selecciona una cuenta compatible para el motor".into();
    }
    String::new()
}
pub fn field_options(registry: &Value, state: &Value, field: &str, source: &Value) -> Value {
    let c = choices(registry, state);
    let rows=match field {
        "model"=>array(at(&c, "models")).iter().map(|m|json!({"value":at(m, "id"),"label":if text(m,"name").is_empty(){text(m,"id")}else{text(m,"name")},"disabled":yes(m,"soon"),"reason":if yes(m,"soon"){"Próximamente"}else{""}})).collect(),
        "effort"=>array(at(&c, "efforts")).iter().map(|v|json!({"value":v,"label":v,"disabled":false})).collect(),
        "toHarness"|"motor"=>{
            let mut values=Vec::new();for r in array(at(registry, "matrix")) {if field=="toHarness"||r.get("harness")==state.get("toHarness") {let v=text(&r,if field=="toHarness"{"harness"}else{"motor"}).to_string();if !values.contains(&v){values.push(v);}}}
            values.iter().map(|v|{let next=update(registry,state,field,v);let r=route(registry,&next);let reason=if !yes(&r,"selectable"){let m=text(at(&r, "reason"),"message");if m.is_empty(){"Ruta no disponible".into()}else{m.into()}}else{switch_error(registry,&next,source)};
                let label=text(at(at(registry, if field=="toHarness"{"harnesses"}else{"motors"}), v),"label").to_string();json!({"value":v,"label":if label.is_empty(){v.clone()}else{label},"disabled":!reason.is_empty(),"reason":reason})}).collect()
        },
        _=>{let motor=field=="motorAccount"&&text(state,"toHarness")!="acp"&&state.get("motor")!=state.get("toHarness");array(at(&c, if motor{"motorAccounts"}else{"accounts"})).iter().map(|a|json!({"value":at(a, "alias"),"label":at(a, "alias"),"detail":text(a,"identity"),"disabled":!yes(a,if motor{"motorSelectable"}else{"selectable"}),"reason":"Cuenta sin sesión compatible"})).collect()}
    };
    Value::Array(rows)
}
pub fn cycle(
    registry: &Value,
    state: &Value,
    field: &str,
    direction: f64,
    source: &Value,
) -> Value {
    let rows = array(&field_options(registry, state, field, source))
        .into_iter()
        .filter(|o| !yes(o, "disabled"))
        .collect::<Vec<_>>();
    if rows.is_empty() {
        return Value::Null;
    }
    let at = rows.iter().position(|o| o.get("value") == state.get(field));
    let i = at
        .map(|i| {
            if direction < 0.0 {
                (i + rows.len() - 1) % rows.len()
            } else {
                (i + 1) % rows.len()
            }
        })
        .unwrap_or(0);
    let next = rows
        .get(i)
        .and_then(|r| r.get("value"))
        .cloned()
        .unwrap_or(Value::Null);
    if Some(&next) == state.get(field) {
        Value::Null
    } else {
        next
    }
}
pub fn recommendations(registry: &Value, state: &Value, rows: &Value, source: &Value) -> Value {
    let mut candidates = array(rows)
        .into_iter()
        .filter(|r| {
            r.get("config").is_some()
                && r.get("count")
                    .and_then(Value::as_f64)
                    .is_some_and(|n| n.is_finite() && n > 0.0)
        })
        .collect::<Vec<_>>();
    let key = |v: &Value| {
        serde_json::to_string(
            &CONFIG_FIELDS
                .iter()
                .map(|k| text(at(v, "config"), k))
                .collect::<Vec<_>>(),
        )
        .unwrap_or_default()
    };
    candidates.sort_by(|a, b| {
        at(b, "count")
            .as_f64()
            .unwrap_or(0.0)
            .total_cmp(&at(a, "count").as_f64().unwrap_or(0.0))
            .then_with(|| {
                at(b, "lastUsed")
                    .as_f64()
                    .unwrap_or(0.0)
                    .total_cmp(&at(a, "lastUsed").as_f64().unwrap_or(0.0))
            })
            .then_with(|| key(a).cmp(&key(b)))
    });
    let mut seen = std::collections::BTreeSet::new();
    let mut result = Vec::new();
    for mut r in candidates {
        if !CONFIG_FIELDS
            .iter()
            .all(|k| at(&r, "config").get(*k).is_some_and(Value::is_string))
        {
            continue;
        }
        let config = configuration(state, at(&r, "config"));
        let k = key(&json!({"config":config}));
        if seen.contains(&k)
            || !changed(state, &config)
            || !validate(registry, &config, source).is_empty()
        {
            continue;
        }
        let contains = |field: &str| {
            array(&field_options(registry, &config, field, source))
                .iter()
                .any(|o| o.get("value") == config.get(field) && !yes(o, "disabled"))
        };
        if !contains("model") {
            continue;
        }
        let fields = if text(&config, "toHarness") == "acp" {
            vec!["motorAccount"]
        } else if config.get("motor") == config.get("toHarness") {
            vec!["harnessAccount"]
        } else {
            vec!["harnessAccount", "motorAccount"]
        };
        if !fields.iter().all(|f| contains(f)) {
            continue;
        }
        seen.insert(k);
        put(&mut r, "config", config);
        result.push(r);
        if result.len() == 3 {
            break;
        }
    }
    Value::Array(result)
}
#[cfg(target_arch = "wasm32")]
pub fn mount() -> Result<(), wasm_bindgen::JsValue> {
    use comandos_web_dom::{bridge::global_set, port::*};
    let api = object();
    for name in [
        "draft",
        "route",
        "choices",
        "update",
        "validate",
        "changed",
        "switchError",
        "configuration",
        "requiresConfirmation",
        "fieldOptions",
        "cycle",
        "recommendations",
    ] {
        method(&api, name, move |args| {
            let a = to_json(&args.get(0));
            let b = to_json(&args.get(1));
            let c = to_json(&args.get(2));
            let d = to_json(&args.get(3));
            let e = to_json(&args.get(4));
            let v = match name {
                "draft" => {
                    let mut v = draft(&a);
                    if let Some(o) = v.as_object_mut() {
                        o.retain(|k, v| {
                            !(["expectedIdentity", "expectedConversationId"].contains(&k.as_str())
                                && v.is_null())
                        });
                    }
                    v
                }
                "route" => route(&a, &b),
                "choices" => choices(&a, &b),
                "update" => update(&a, &b, c.as_str().unwrap_or(""), d.as_str().unwrap_or("")),
                "validate" => json!(validate(&a, &b, &c)),
                "changed" => json!(changed(&a, &b)),
                "switchError" => json!(switch_error(&a, &b, &c)),
                "configuration" => configuration(&a, &b),
                "requiresConfirmation" => json!(requires_confirmation(&a, &b)),
                "fieldOptions" => field_options(&a, &b, c.as_str().unwrap_or(""), &d),
                "cycle" => cycle(
                    &a,
                    &b,
                    c.as_str().unwrap_or(""),
                    d.as_f64().unwrap_or(1.0),
                    &e,
                ),
                "recommendations" => recommendations(&a, &b, &c, &d),
                _ => Value::Null,
            };
            if name == "route" && v.is_null() {
                return Ok(wasm_bindgen::JsValue::UNDEFINED);
            }
            let out = from_json(&v)?;
            if name == "choices" && at(&v, "route").is_null() {
                set(&out, "route", &wasm_bindgen::JsValue::UNDEFINED)?;
            }
            Ok(out)
        })?;
    }
    global_set("SessionConfig", &api)
}

#[cfg(not(target_arch = "wasm32"))]
pub fn mount() -> Result<(), wasm_bindgen::JsValue> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{at, changed, configuration, draft, requires_confirmation};
    use serde_json::json;

    #[test]
    fn draft_preserves_accounts_and_observed_identity() {
        let got = draft(
            &json!({"agent":"codex","model":"gpt-5","account":"work","observedConfig":{"identity":"i","conversationId":"c"}}),
        );
        assert_eq!(at(&got, "toHarness"), "codex");
        assert_eq!(at(&got, "motor"), "codex");
        assert_eq!(at(&got, "harnessAccount"), "work");
        assert_eq!(at(&got, "motorAccount"), "work");
        assert_eq!(at(&got, "expectedIdentity"), "i");
    }

    #[test]
    fn configuration_copies_only_string_config_fields_and_resets_interrupt() {
        let base = json!({"toHarness":"claude","motor":"claude","model":"old","interrupt":true});
        let next = configuration(&base, &json!({"model":"new","interrupt":"ignored"}));
        assert_eq!(at(&next, "model"), "new");
        assert_eq!(at(&next, "interrupt"), false);
        assert!(changed(&base, &next));
        assert!(!requires_confirmation(&base, &next));
    }
}

#[cfg(test)]
#[test]
#[allow(clippy::unwrap_used, clippy::panic)]
fn original_session_config_ui_assertion_inputs_match_node_oracle() {
    let cases: Value = serde_json::from_str(include_str!(
        "../../../../xtask/web/fixtures/sounds/session-cases.json"
    ))
    .unwrap();
    for case in cases.as_array().unwrap() {
        let args = case.get("args").and_then(Value::as_array).unwrap();
        let arg = |i| args.get(i).unwrap_or(&Value::Null);
        let name = text(case, "function");
        let mut got = match name {
            "draft" => draft(arg(0)),
            "route" => route(arg(0), arg(1)),
            "choices" => choices(arg(0), arg(1)),
            "update" => update(
                arg(0),
                arg(1),
                arg(2).as_str().unwrap(),
                arg(3).as_str().unwrap(),
            ),
            "validate" => json!(validate(arg(0), arg(1), arg(2))),
            "changed" => json!(changed(arg(0), arg(1))),
            "switchError" => json!(switch_error(arg(0), arg(1), arg(2))),
            "configuration" => configuration(arg(0), arg(1)),
            "requiresConfirmation" => json!(requires_confirmation(arg(0), arg(1))),
            "fieldOptions" => field_options(arg(0), arg(1), arg(2).as_str().unwrap(), arg(3)),
            "cycle" => cycle(
                arg(0),
                arg(1),
                arg(2).as_str().unwrap(),
                arg(3).as_f64().unwrap(),
                arg(4),
            ),
            "recommendations" => recommendations(arg(0), arg(1), arg(2), arg(3)),
            _ => panic!("unexpected original API {name}"),
        };
        // JS omits undefined identity values. The native model uses null.
        if name == "draft"
            && let Some(o) = got.as_object_mut()
        {
            o.retain(|k, v| {
                !(["expectedIdentity", "expectedConversationId"].contains(&k.as_str())
                    && v.is_null())
            });
        }
        assert_eq!(&got, case.get("expected").unwrap(), "{name}: {args:?}");
    }
}
