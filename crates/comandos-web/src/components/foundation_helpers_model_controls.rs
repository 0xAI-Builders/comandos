use super::super::*;
fn menu() -> JsValue {
    id("mdl-menu")
}
fn close() {
    classes(&menu(), "hidden", true);
    let b = global("mdlMenuBtn");
    if truthy(&b) {
        attr(&b, "aria-expanded", "false")
    }
    let _ = global_set("mdlMenuBtn", &JsValue::NULL);
}
fn choices(provider: &str) -> String {
    let choices = json_global("MODEL_CHOICES");
    list(choices.get(provider).unwrap_or(at(&choices,"claude"))).iter().map(|m|format!("<button type=\"button\" class=\"mdl-opt\" role=\"option\" data-model=\"{}\"{}><i style=\"background:{}\"></i><b>{}</b><span>{}</span></button>",attr_esc(&s(at(m,"model"))),if truth(at(m,"effort")){format!(" data-effort=\"{}\"",attr_esc(&s(at(m,"effort"))))}else{String::new()},s(at(m,"dot")),md_esc(&s(at(m,"name"))),md_esc(&s(at(m,"intent"))))).collect()
}
fn opencode_html() -> String {
    let data = global("ocModels");
    if !truthy(&data) {
        return format!(
            "<div class=\"mdl-head\">{}</div>",
            tf("Cargando modelos…", "Loading models…")
        );
    }
    let providers = list(&to_utf16_json(&data));
    if providers.is_empty() {
        return format!(
            "<div class=\"mdl-head\">{}</div>",
            tf(
                "Sin modelos — revisa las keys en providers.env",
                "No models — check keys in providers.env"
            )
        );
    }
    providers.iter().map(|p|format!("<div class=\"mdl-head\">{}</div>{}",md_esc(&s(at(p,"provider"))),list(at(p,"models")).iter().map(|m|format!("<button type=\"button\" class=\"mdl-opt\" role=\"option\" data-model=\"{}\" data-name=\"{}\"><i style=\"background:{}\"></i><b>{}</b><span>{}</span></button>",attr_esc(&s(at(m,"id"))),attr_esc(&s(at(m,"name"))),if truth(at(m,"free")){"var(--done)"}else{"var(--waiting)"},md_esc(&s(at(m,"name"))),if truth(at(m,"free")){"gratis"}else{"de pago"})).collect::<String>())).collect()
}
fn position(button: &JsValue) {
    let r = call(button, "getBoundingClientRect", &[]).unwrap_or(JsValue::NULL);
    let m = menu();
    let width = number(&get(&m, "offsetWidth"));
    let height = number(&get(&m, "offsetHeight"));
    let left = (number(&get(&r, "right")) - width)
        .min(number(&global("innerWidth")) - width - 8.0)
        .max(8.0);
    let top = number(&get(&r, "bottom")) + 6.0;
    let top = if top + height > number(&global("innerHeight")) - 8.0 {
        (number(&get(&r, "top")) - height - 6.0).max(8.0)
    } else {
        top
    };
    style(&m, "left", &format!("{left}px"));
    style(&m, "top", &format!("{top}px"));
}
async fn load_models() -> Result<JsValue, JsValue> {
    if truthy(&global("ocModels")) || truthy(&global("ocModelsLoading")) {
        return Ok(JsValue::UNDEFINED);
    }
    global_set("ocModelsLoading", &true.into())?;
    let result = api("/opencode/models", None).await;
    global_set(
        "ocModels",
        &result
            .map(|r| get(&r, "providers"))
            .unwrap_or(JsValue::NULL),
    )?;
    global_set("ocModelsLoading", &false.into())?;
    Ok(JsValue::UNDEFINED)
}
fn open(button: JsValue) {
    let provider = data(&button, "provider");
    let provider = if provider.is_empty() {
        "claude"
    } else {
        &provider
    };
    let is_opencode = provider == "opencode";
    let m = menu();
    html(
        &m,
        if is_opencode {
            opencode_html()
        } else {
            choices(provider)
        },
    );
    classes(&m, "hidden", false);
    position(&button);
    let _ = global_set("mdlMenuBtn", &button);
    attr(&button, "aria-expanded", "true");
    for option in all(&m, ".mdl-opt") {
        let o = option.clone();
        listen(
            &option,
            "click",
            event(move |e| {
                call(&e, "stopPropagation", &[])?;
                let target = global("mdlMenuBtn");
                close();
                if truthy(&target) {
                    let model = data(&o, "model");
                    let name = data(&o, "name");
                    let effort = data(&o, "effort");
                    wasm_bindgen_futures::spawn_local(async move {
                        let _ = switch(target, model, name, effort).await;
                    });
                }
                Ok(())
            }),
        );
    }
    if is_opencode && !truthy(&global("ocModels")) {
        wasm_bindgen_futures::spawn_local(async move {
            let _ = load_models().await;
            if global("mdlMenuBtn") == button && !has_class(&menu(), "hidden") {
                open(button);
            }
        });
    }
}
async fn switch(
    button: JsValue,
    model: String,
    name: String,
    effort: String,
) -> Result<JsValue, JsValue> {
    let card = closest(&button, "article[data-session]");
    if !card.is_null() && !data(&card, "pane").is_empty() {
        set(
            &get(&button, "dataset"),
            "pane",
            &utf16_value(&data(&card, "pane")),
        )?;
    }
    let provider = data(&button, "provider");
    let mut body = json!({"session":data(&button,"session"),"provider":if provider.is_empty(){"claude"}else{&provider},"model":model});
    if let Some(m) = body.as_object_mut() {
        for (k, v) in [
            ("pane", data(&button, "pane")),
            ("model_name", name.clone()),
            ("effort", effort),
        ] {
            if !v.is_empty() {
                m.insert(k.into(), json!(v));
            }
        }
    }
    match api("/model/switch", Some(body)).await {
        Ok(_) => {
            let shown = if name.is_empty() { model } else { name };
            let current = query(&button, ".mdl-cur");
            if !current.is_null() {
                txt(&current, shown.clone());
                classes(&current, "pending", true);
            }
            toast(
                tf(
                    &format!("Modelo {shown} → {}", data(&button, "session")),
                    &format!("Model {shown} → {}", data(&button, "session")),
                ),
                false,
            );
        }
        Err(e) => toast(err(e), true),
    }
    Ok(JsValue::UNDEFINED)
}
async fn poll() -> Result<JsValue, JsValue> {
    let pending = global("MOTOR_PENDING");
    if number(&get(&pending, "size")) == 0.0 || truthy(&global("MOTOR_STATUS_BUSY")) {
        return Ok(JsValue::UNDEFINED);
    }
    global_set("MOTOR_STATUS_BUSY", &true.into())?;
    let keys = call(&pending, "keys", &[])
        .map(|v| js_sys::Array::from(&v))
        .unwrap_or_default();
    let requests = js_sys::Array::new();
    for key in keys.iter() {
        let key = key.as_string().unwrap_or_default();
        let p = call(&pending, "get", &[utf16_value(&key)]).unwrap_or(JsValue::NULL);
        let operation_id = get(&p, "operationId");
        let request = promise(async move {
            let params = js_sys::Reflect::construct(
                &global("URLSearchParams").into(),
                &js_sys::Array::new(),
            )?;
            call(&params, "set", &["operationKey".into(), utf16_value(&key)])?;
            if truthy(&operation_id) {
                call(&params, "set", &["operationId".into(), operation_id])?;
            }
            let query = string(&call(&params, "toString", &[])?);
            let result = api(&format!("/model/status?{query}"), None)
                .await
                .unwrap_or(JsValue::NULL);
            let out = js_sys::Array::new();
            out.push(&utf16_value(&key));
            out.push(&result);
            Ok(out.into())
        });
        requests.push(&request);
    }
    let results = wait(Ok(js_sys::Promise::all(&requests).into())).await;
    let processed = if let Ok(results) = results {
        process_status(results)
    } else {
        Ok(())
    };
    let _ = global_set("MOTOR_STATUS_BUSY", &false.into());
    processed?;
    Ok(JsValue::UNDEFINED)
}
fn process_status(results: JsValue) -> Result<(), JsValue> {
    for row in js_rows(&results) {
        let row = js_sys::Array::from(&row);
        let key = row.get(0);
        let r = row.get(1);
        if !truthy(&r) {
            continue;
        }
        let pending = global("MOTOR_PENDING");
        let pend = call(&pending, "get", std::slice::from_ref(&key)).unwrap_or(JsValue::NULL);
        let pending_id = get(&pend, "operationId");
        if truthy(&pending_id) && get(&r, "operationId") != pending_id {
            continue;
        }
        let revision = js_sys::JSON::stringify(
            &[
                get(&r, "operationId"),
                get(&r, "state"),
                get(&r, "ok"),
                get(&r, "ts"),
            ]
            .into_iter()
            .collect::<js_sys::Array>(),
        )?;
        if call(&global("SWITCH_SEEN"), "get", std::slice::from_ref(&key)).ok()
            == Some(revision.clone().into())
        {
            continue;
        }
        let rows = js_rows(&get(&state(), "list"));
        let it = rows
            .iter()
            .find(|it| row_key(&to_utf16_json(it)) == key.as_string().unwrap_or_default())
            .or_else(|| rows.iter().find(|it| get(it, "session") == key))
            .cloned()
            .unwrap_or(JsValue::NULL);
        if !get(&r, "stage").is_undefined() && get(&r, "ok").is_undefined() {
            if truthy(&pend) {
                let stage = stage_label(&to_utf16_json(&r));
                let code = get(&r, "stageCode");
                let code = if truthy(&code) {
                    code
                } else {
                    let stage = get(&r, "stage");
                    if truthy(&stage) {
                        stage
                    } else {
                        get(&r, "state")
                    }
                };
                if get(&pend, "stageTxt").as_string().as_deref() != Some(&stage)
                    || get(&pend, "stageCode") != code
                {
                    set(&pend, "stageTxt", &utf16_value(&stage))?;
                    set(
                        &pend,
                        "stageCode",
                        &if truthy(&code) { code } else { "".into() },
                    )?;
                }
            }
            continue;
        }
        call(
            &global("SWITCH_SEEN"),
            "set",
            &[key.clone(), revision.into()],
        )?;
        let name = get(&it, "project")
            .as_string()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| key.as_string().unwrap_or_default());
        if (truthy(&get(&r, "ok")) || truthy(&get(&r, "rolledBack"))) && truthy(&it) {
            if truthy(&get(&r, "harness")) {
                set(&it, "agent", &get(&r, "harness"))?;
            }
            for field in ["motor", "model", "effort", "harnessAccount", "motorAccount"] {
                let v = get(&r, field);
                if !v.is_undefined() {
                    set(&it, field, &v)?;
                }
            }
            if truthy(&get(&r, "harnessAccount")) {
                set(&it, "account", &get(&r, "harnessAccount"))?;
            }
            let route_id = get(&r, "route_id");
            let route_id = if truthy(&route_id) {
                route_id
            } else {
                get(&r, "routeId")
            };
            if truthy(&route_id) {
                set(&it, "routeId", &route_id)?;
            }
        }
        if truthy(&get(&r, "ok")) {
            let ms = get(&r, "applyMs");
            toast(
                format!(
                    "✓ {name}: {}{}",
                    get(&r, "detail").as_string().unwrap_or_default(),
                    if ms.is_null() || ms.is_undefined() {
                        String::new()
                    } else {
                        format!(" · {}s", (number(&ms) / 100.0).round() / 10.0)
                    }
                ),
                false,
            );
            call_global("tick", &[]);
        } else if truthy(&get(&r, "rolledBack")) {
            toast(
                if truthy(&get(&pend, "recovering")) {
                    tf(
                        &format!("{name}: sesión original recuperada"),
                        &format!("{name}: original session restored"),
                    )
                } else {
                    tf(
                        &format!("{name}: el cambio no se aplicó; sesión original recuperada"),
                        &format!("{name}: switch did not apply; original session restored"),
                    )
                },
                false,
            );
            call_global("tick", &[]);
        } else {
            let detail = get(&r, "detail").as_string().unwrap_or_default();
            toast(
                tf(
                    &format!("✗ {name}: el cambio NO entró ({detail}) — reinténtalo"),
                    &format!("✗ {name}: switch did NOT apply ({detail}) — retry"),
                ),
                true,
            );
        }
        call(&pending, "delete", &[key])?;
    }
    Ok(())
}
pub fn mount() -> Result<(), JsValue> {
    let scope: JsValue = js_sys::global().into();
    method(&scope, "modelMenuEl", |a| {
        let session = a.get(0).as_string().unwrap_or_default();
        let pane = a.get(1).as_string().unwrap_or_default();
        let provider = a.get(2).as_string().unwrap_or_default();
        let current = short_model(&a.get(3).as_string().unwrap_or_default());
        let current = if current.is_empty() {
            tf("modelo", "model")
        } else {
            current
        };
        Ok(utf16_value(&format!(
            "<span class=\"mdl-wrap\"><button type=\"button\" class=\"mdl-btn\" data-session=\"{}\" data-pane=\"{}\" data-provider=\"{}\" aria-haspopup=\"listbox\" aria-expanded=\"false\" title=\"{}\"><span class=\"mdl-cur\">{}</span><span class=\"mdl-caret\">▾</span></button></span>",
            attr_esc(&session),
            attr_esc(&pane),
            attr_esc(&provider),
            tf(
                "Cambiar el modelo de esta sesion",
                "Switch this session's model"
            ),
            md_esc(&current)
        )))
    })?;
    {
        let (name, fn_) = ("choicesMenuHtml", choices as fn(&str) -> String);
        method(&scope, name, move |a| {
            Ok(utf16_value(&fn_(&a.get(0).as_string().unwrap_or_default())))
        })?;
    }
    method(&scope, "opencodeMenuHtml", |_| {
        Ok(utf16_value(&opencode_html()))
    })?;
    method(&scope, "closeModelMenus", |_| {
        close();
        Ok(JsValue::UNDEFINED)
    })?;
    method(&scope, "loadOpencodeModels", |_| Ok(promise(load_models())))?;
    method(&scope, "positionModelMenu", |a| {
        position(&a.get(0));
        Ok(JsValue::UNDEFINED)
    })?;
    method(&scope, "openModelMenu", |a| {
        open(a.get(0));
        Ok(JsValue::UNDEFINED)
    })?;
    method(&scope, "switchModel", |a| {
        Ok(promise(switch(
            a.get(0),
            a.get(1).as_string().unwrap_or_default(),
            a.get(2).as_string().unwrap_or_default(),
            a.get(3).as_string().unwrap_or_default(),
        )))
    })?;
    method(&scope, "wireModelMenu", |a| {
        let button = query(&a.get(0), ".mdl-btn");
        if button.is_null() || truthy(&get(&button, "_wired")) {
            return Ok(JsValue::UNDEFINED);
        }
        set(&button, "_wired", &true.into())?;
        let b = button.clone();
        listen(
            &button,
            "click",
            event(move |e| {
                call(&e, "stopPropagation", &[])?;
                if !truthy(
                    &call(&b, "hasAttribute", &["aria-haspopup".into()]).unwrap_or(JsValue::FALSE),
                ) {
                    let b = b.clone();
                    wasm_bindgen_futures::spawn_local(async move {
                        let _ = switch(b, "picker".into(), String::new(), String::new()).await;
                    });
                    return Ok(());
                }
                let reopen = global("mdlMenuBtn") != b;
                close();
                if reopen {
                    open(b.clone());
                }
                Ok(())
            }),
        );
        Ok(JsValue::UNDEFINED)
    })?;
    Ok(())
}
pub fn attach() -> Result<(), JsValue> {
    listen(
        &doc(),
        "click",
        event(|_| {
            close();
            Ok(())
        }),
    );
    listen(
        &doc(),
        "keydown",
        event(|e| {
            if get(&e, "key").as_string().as_deref() == Some("Escape") {
                close();
            }
            Ok(())
        }),
    );
    call(
        &doc(),
        "addEventListener",
        &[
            "scroll".into(),
            event(|_| {
                close();
                Ok(())
            }),
            true.into(),
        ],
    )?;
    interval(450.0, || {
        wasm_bindgen_futures::spawn_local(async {
            let _ = poll().await;
        });
        Ok(())
    });
    Ok(())
}
