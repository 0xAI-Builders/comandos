use super::{DENIED_HELP, DEVICE_KEY, ENABLED_KEY, support as check_support, url_base64_to_bytes};
use comandos_web_dom::{bridge::global_set, port::*};
use wasm_bindgen::JsValue;
fn support(env: &JsValue) -> JsValue {
    let s = check_support(
        truthy(&get(env, "isSecureContext")),
        truthy(&get(&get(env, "navigator"), "serviceWorker")),
        truthy(&get(env, "PushManager")),
        truthy(&get(env, "Notification")),
    );
    from_json(&serde_json::json!({"ok":s.ok,"reason":s.reason})).unwrap_or(JsValue::NULL)
}
fn response(state: &str, message: &str) -> Result<JsValue, JsValue> {
    from_json(&serde_json::json!({"state":state,"message":message}))
}
fn store_get(env: &JsValue, key: &str) -> JsValue {
    call(&get(env, "storage"), "getItem", &[key.into()]).unwrap_or(JsValue::NULL)
}
fn store_set(env: &JsValue, key: &str, value: Option<&str>) {
    let _ = if let Some(value) = value {
        call(&get(env, "storage"), "setItem", &[key.into(), value.into()])
    } else {
        call(&get(env, "storage"), "removeItem", &[key.into()])
    };
}
fn device_id(env: &JsValue) -> JsValue {
    let id = store_get(env, DEVICE_KEY);
    if truthy(&id) {
        return id;
    }
    let id = call(&get(env, "crypto"), "randomUUID", &[]).unwrap_or_else(|_| {
        format!("dev-{}{}", js_sys::Date::now(), js_sys::Math::random()).into()
    });
    let _ = call(
        &get(env, "storage"),
        "setItem",
        &[DEVICE_KEY.into(), id.clone()],
    );
    id
}
async fn registration(env: &JsValue) -> Result<JsValue, JsValue> {
    wait(Ok(get(
        &get(&get(env, "navigator"), "serviceWorker"),
        "ready",
    )))
    .await
}
async fn subscription(reg: &JsValue) -> Result<JsValue, JsValue> {
    wait(call(&get(reg, "pushManager"), "getSubscription", &[])).await
}
async fn fetch(
    env: &JsValue,
    method: &str,
    url: &str,
    body: Option<JsValue>,
) -> Result<JsValue, JsValue> {
    let mut args = vec![method.into(), url.into()];
    if let Some(body) = body {
        args.push(body);
    }
    wait(call(env, "fetchJson", &args)).await
}
fn subscription_body(env: &JsValue, sub: &JsValue, user_agent: bool) -> Result<JsValue, JsValue> {
    let o = object();
    set(&o, "subscription", &call(sub, "toJSON", &[])?)?;
    set(&o, "deviceId", &device_id(env))?;
    if user_agent {
        let raw = get(&get(env, "navigator"), "userAgent");
        let s = if truthy(&raw) {
            string(&raw)
        } else {
            String::new()
        };
        set(
            &o,
            "userAgent",
            &String::from_utf16_lossy(&s.encode_utf16().take(200).collect::<Vec<_>>()).into(),
        )?;
    }
    Ok(o)
}
async fn operation(
    env: JsValue,
    op: &str,
    prompt: Option<Result<JsValue, JsValue>>,
) -> Result<JsValue, JsValue> {
    let notification = get(&env, "Notification");
    if op == "current" || op == "enable" {
        let s = support(&env);
        if !truthy(&get(&s, "ok")) {
            return response("unsupported", &string(&get(&s, "reason")));
        }
        if get(&notification, "permission").as_string().as_deref() == Some("denied") {
            return response("denied", DENIED_HELP);
        }
    }
    match op {
        "current" => {
            let reg = registration(&env).await?;
            let sub = subscription(&reg).await?;
            if truthy(&sub)
                && get(&notification, "permission").as_string().as_deref() == Some("granted")
            {
                let r = response(
                    "enabled",
                    "Este dispositivo recibe avisos cuando CommandOS no está visible.",
                )?;
                set(&r, "subscription", &sub)?;
                Ok(r)
            } else {
                response(
                    "disabled",
                    "Recibe un aviso breve (proyecto y título) cuando ningún CommandOS esté visible.",
                )
            }
        }
        "enable" => {
            let permission = if let Some(p) = prompt {
                wait(p).await?
            } else {
                get(&notification, "permission")
            };
            if permission.as_string().as_deref() != Some("granted") {
                store_set(&env, ENABLED_KEY, None);
                return if permission.as_string().as_deref() == Some("denied") {
                    response("denied", DENIED_HELP)
                } else {
                    response(
                        "disabled",
                        "No se concedió el permiso. Puedes intentarlo de nuevo cuando quieras.",
                    )
                };
            }
            let key = fetch(&env, "GET", "/push/key", None).await?;
            if !truthy(&get(&key, "available")) || !truthy(&get(&key, "publicKey")) {
                let error = get(&key, "error");
                let msg = if truthy(&error) {
                    string(&error)
                } else {
                    "El servidor no tiene push disponible.".into()
                };
                return response("error", &msg);
            }
            let reg = registration(&env).await?;
            let mut sub = subscription(&reg).await?;
            if !truthy(&sub) {
                let options = object();
                set(&options, "userVisibleOnly", &true.into())?;
                set(
                    &options,
                    "applicationServerKey",
                    &js_sys::Uint8Array::from(
                        url_base64_to_bytes(&string(&get(&key, "publicKey"))).as_slice(),
                    )
                    .into(),
                )?;
                sub = wait(call(&get(&reg, "pushManager"), "subscribe", &[options])).await?;
            }
            fetch(
                &env,
                "POST",
                "/push/subscription",
                Some(subscription_body(&env, &sub, true)?),
            )
            .await?;
            store_set(&env, ENABLED_KEY, Some("1"));
            let r = response(
                "enabled",
                "Listo. Pulsa «Enviar aviso de prueba» con la app en segundo plano o la pantalla bloqueada.",
            )?;
            set(&r, "subscription", &sub)?;
            Ok(r)
        }
        "disable" => {
            let reg = registration(&env).await?;
            let sub = subscription(&reg).await?;
            store_set(&env, ENABLED_KEY, None);
            if truthy(&sub) {
                let body = object();
                set(&body, "endpoint", &get(&sub, "endpoint"))?;
                let _ = fetch(&env, "DELETE", "/push/subscription", Some(body)).await;
                let _ = wait(call(&sub, "unsubscribe", &[])).await;
            }
            response("disabled", "Avisos push desactivados en este dispositivo.")
        }
        "test" => {
            let reg = registration(&env).await?;
            let sub = subscription(&reg).await?;
            if !truthy(&sub) {
                return response("disabled", "Primero activa los avisos en este dispositivo.");
            }
            let body = object();
            set(&body, "endpoint", &get(&sub, "endpoint"))?;
            let r = fetch(&env, "POST", "/push/test", Some(body)).await?;
            if truthy(&get(&r, "removed")) {
                store_set(&env, ENABLED_KEY, None);
                return response(
                    "disabled",
                    "El servicio push dio la suscripción por caducada; actívala de nuevo.",
                );
            }
            response(
                "enabled",
                &if truthy(&get(&r, "ok")) {
                    "Aviso de prueba enviado al servicio push. Si no llega, revisa el modo No molestar y los ajustes de Chrome.".into()
                } else {
                    format!("El servicio push respondió {}.", string(&get(&r, "status")))
                },
            )
        }
        "resync" => {
            if store_get(&env, ENABLED_KEY).as_string().as_deref() != Some("1")
                || !truthy(&get(&support(&env), "ok"))
                || get(&notification, "permission").as_string().as_deref() != Some("granted")
            {
                return Ok(false.into());
            }
            let reg = registration(&env).await?;
            let sub = subscription(&reg).await?;
            if !truthy(&sub) {
                store_set(&env, ENABLED_KEY, None);
                return Ok(false.into());
            }
            fetch(
                &env,
                "POST",
                "/push/subscription",
                Some(subscription_body(&env, &sub, false)?),
            )
            .await?;
            Ok(true.into())
        }
        _ => Err(js_sys::Error::new("unknown push operation").into()),
    }
}
fn create_controller(env: JsValue) -> Result<JsValue, JsValue> {
    let o = object();
    let e = env.clone();
    method(&o, "deviceId", move |_| Ok(device_id(&e)))?;
    for name in ["current", "enable", "disable", "test", "resync"] {
        let env = env.clone();
        method(&o, name, move |_| {
            // Request permission in the original gesture's call stack, before yielding.
            let prompt = if name == "enable" && truthy(&get(&support(&env), "ok")) {
                let n = get(&env, "Notification");
                let p = get(&n, "permission");
                if p.as_string()
                    .is_some_and(|s| s != "denied" && s != "granted")
                {
                    Some(call(&n, "requestPermission", &[]))
                } else {
                    None
                }
            } else {
                None
            };
            Ok(
                wasm_bindgen_futures::future_to_promise(operation(env.clone(), name, prompt))
                    .into(),
            )
        })?;
    }
    Ok(o)
}
fn route_event(win: &JsValue, id: JsValue) -> Result<(), JsValue> {
    if !truthy(&id) {
        return Ok(());
    }
    let detail = object();
    set(&detail, "eventId", &id)?;
    let init = object();
    set(&init, "detail", &detail)?;
    let event = js_sys::Reflect::construct(
        &get(win, "CustomEvent").into(),
        &["comandos:open-event".into(), init]
            .into_iter()
            .collect::<js_sys::Array>(),
    )?;
    call(win, "dispatchEvent", &[event])?;
    Ok(())
}
fn install_routing(win: JsValue) -> Result<(), JsValue> {
    let location = get(&win, "location");
    let params = js_sys::Reflect::construct(
        &get(&js_sys::global(), "URLSearchParams").into(),
        &[get(&location, "search")]
            .into_iter()
            .collect::<js_sys::Array>(),
    )?;
    let params: JsValue = params;
    let id = call(&params, "get", &["event".into()])?;
    if !id.is_null() {
        call(&params, "delete", &["event".into()])?;
        let qs = string(&call(&params, "toString", &[])?);
        call(
            &get(&win, "history"),
            "replaceState",
            &[
                get(&get(&win, "history"), "state"),
                "".into(),
                format!(
                    "{}{}{}",
                    string(&get(&location, "pathname")),
                    if qs.is_empty() {
                        String::new()
                    } else {
                        format!("?{qs}")
                    },
                    string(&get(&location, "hash"))
                )
                .into(),
            ],
        )?;
        let w = win.clone();
        let id = string(&id).encode_utf16().take(128).collect::<Vec<_>>();
        let cb = function(move |_| {
            route_event(&w, String::from_utf16_lossy(&id).into())?;
            Ok(JsValue::UNDEFINED)
        });
        call(&win, "setTimeout", &[cb, 0.into()])?;
    }
    let sw = get(&get(&win, "navigator"), "serviceWorker");
    if truthy(&sw) {
        let cb = function(move |args| {
            let m = get(&args.get(0), "data");
            if get(&m, "type").as_string().as_deref() == Some("comandos:open-event") {
                let raw = get(&m, "eventId");
                let s = if truthy(&raw) {
                    string(&raw)
                } else {
                    String::new()
                };
                route_event(
                    &win,
                    String::from_utf16_lossy(&s.encode_utf16().take(128).collect::<Vec<_>>())
                        .into(),
                )?;
            }
            Ok(JsValue::UNDEFINED)
        });
        call(&sw, "addEventListener", &["message".into(), cb])?;
    }
    Ok(())
}
pub fn mount() -> Result<(), JsValue> {
    let api = object();
    set(&api, "DENIED_HELP", &DENIED_HELP.into())?;
    method(&api, "support", |a| Ok(support(&a.get(0))))?;
    method(&api, "urlBase64ToUint8Array", |a| {
        let value = string(&a.get(0));
        let mut padded = value.replace('-', "+").replace('_', "/");
        while !padded.len().is_multiple_of(4) {
            padded.push('=');
        }
        call(&js_sys::global(), "atob", &[padded.into()])?;
        Ok(js_sys::Uint8Array::from(url_base64_to_bytes(&value).as_slice()).into())
    })?;
    method(&api, "createController", |a| create_controller(a.get(0)))?;
    method(&api, "installEventRouting", |a| {
        install_routing(a.get(0))?;
        Ok(JsValue::UNDEFINED)
    })?;
    global_set("PushSettings", &api)
}
fn show(box_: &JsValue, result: &JsValue) -> Result<(), JsValue> {
    let status = call(box_, "querySelector", &["#push-status".into()])?;
    let message = get(result, "message");
    set(
        &status,
        "textContent",
        &if truthy(&message) { message } else { "".into() },
    )?;
    let state = get(result, "state");
    let enabled = state.as_string().as_deref() == Some("enabled");
    for (selector, hidden) in [
        (
            "#push-enable",
            enabled || state.as_string().as_deref() == Some("unsupported"),
        ),
        ("#push-disable", !enabled),
        ("#push-test", !enabled),
    ] {
        set(
            &call(box_, "querySelector", &[selector.into()])?,
            "hidden",
            &hidden.into(),
        )?;
    }
    set(&get(box_, "dataset"), "state", &state)
}
fn browser_env() -> Result<JsValue, JsValue> {
    let global = js_sys::global();
    let env = object();
    for k in [
        "navigator",
        "Notification",
        "PushManager",
        "isSecureContext",
        "crypto",
    ] {
        set(&env, k, &get(&global, k))?;
    }
    let storage = js_sys::Reflect::get(&global, &"localStorage".into())?;
    set(&env, "storage", &storage)?;
    method(&env, "fetchJson", move |a| {
        let options = object();
        set(&options, "method", &a.get(0))?;
        let headers = object();
        let token = call(&storage, "getItem", &["cc_token".into()]).unwrap_or(JsValue::NULL);
        if truthy(&token) {
            set(&headers, "X-Comandos-Token", &token)?;
        }
        let body = a.get(2);
        if truthy(&body) {
            set(&headers, "Content-Type", &"application/json".into())?;
            set(&options, "body", &js_sys::JSON::stringify(&body)?.into())?;
        }
        set(&options, "headers", &headers)?;
        let request = call(&global, "fetch", &[a.get(1), options]);
        Ok(wasm_bindgen_futures::future_to_promise(async move {
            let response = wait(request).await?;
            let json = wait(call(&response, "json", &[]))
                .await
                .unwrap_or_else(|_| object());
            if !truthy(&get(&response, "ok")) && get(&response, "status").as_f64() != Some(503.0) {
                let error = get(&json, "error");
                let status = get(&response, "statusText");
                let msg = if truthy(&error) {
                    string(&error)
                } else if truthy(&status) {
                    string(&status)
                } else {
                    "No se completó".into()
                };
                return Err(js_sys::Error::new(&msg).into());
            }
            Ok(json)
        })
        .into())
    })?;
    Ok(env)
}
pub fn attach() -> Result<(), JsValue> {
    let global = js_sys::global();
    install_routing(global.clone().into())?;
    let env = match browser_env() {
        Ok(e) => e,
        Err(_) => return Ok(()),
    };
    let controller = create_controller(env)?;
    let box_ = call(
        &get(&global, "document"),
        "getElementById",
        &["push-settings".into()],
    )?;
    if !box_.is_null() {
        for (selector, op) in [
            ("#push-enable", "enable"),
            ("#push-disable", "disable"),
            ("#push-test", "test"),
        ] {
            let button = call(&box_, "querySelector", &[selector.into()])?;
            let b = box_.clone();
            let c = controller.clone();
            let callback = function(move |_| {
                let mut buttons = Vec::new();
                for id in ["#push-enable", "#push-disable", "#push-test"] {
                    let button = call(&b, "querySelector", &[id.into()])?;
                    set(&button, "disabled", &true.into())?;
                    buttons.push(button);
                }
                let result = call(&c, op, &[]);
                let b = b.clone();
                wasm_bindgen_futures::spawn_local(async move {
                    match wait(result).await {
                        Ok(r) => {
                            let _ = show(&b, &r);
                        }
                        Err(e) => {
                            let message = get(&e, "message");
                            if let Ok(status) = call(&b, "querySelector", &["#push-status".into()])
                            {
                                let _ = set(
                                    &status,
                                    "textContent",
                                    &format!(
                                        "No se pudo completar: {}",
                                        string(&if truthy(&message) { message } else { e })
                                    )
                                    .into(),
                                );
                            }
                        }
                    }
                    for button in buttons {
                        let _ = set(&button, "disabled", &false.into());
                    }
                });
                Ok(JsValue::UNDEFINED)
            });
            call(&button, "addEventListener", &["click".into(), callback])?;
        }
        let current = call(&controller, "current", &[]);
        wasm_bindgen_futures::spawn_local(async move {
            if let Ok(r) = wait(current).await {
                let _ = show(&box_, &r);
            }
        });
    }
    let resync = call(&controller, "resync", &[]);
    wasm_bindgen_futures::spawn_local(async move {
        let _ = wait(resync).await;
    });
    Ok(())
}
