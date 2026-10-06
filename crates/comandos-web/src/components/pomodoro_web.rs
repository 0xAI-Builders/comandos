use super::super::web_support::*;
use super::*;
use comandos_web_dom::port::utf16_string as string;
use comandos_web_dom::port::{from_utf16_json as from_json, to_utf16_json as to_json};
use comandos_web_dom::{bridge::global_set, port::*};
use comandos_web_view::pomodoro as art;
use std::{cell::RefCell, rc::Rc};
use wasm_bindgen::JsValue;
struct Client {
    confirmed: JsValue,
    offset: f64,
    pending: JsValue,
    error: JsValue,
    opts: JsValue,
}
fn borrowed() -> JsValue {
    js_sys::Error::new("Pomodoro state busy").into()
}
fn change(st: &Rc<RefCell<Client>>) {
    let cb = st
        .try_borrow()
        .map(|s| get(&s.opts, "onChange"))
        .unwrap_or(JsValue::NULL);
    if cb.is_function() {
        let _ = invoke(&cb, &[]);
    }
}
fn now(st: &Rc<RefCell<Client>>) -> f64 {
    let opts = st
        .try_borrow()
        .map(|s| s.opts.clone())
        .unwrap_or(JsValue::NULL);
    invoke(&get(&opts, "now"), &[])
        .ok()
        .and_then(|v| v.as_f64())
        .unwrap_or_else(js_sys::Date::now)
}
fn accept(st: &Rc<RefCell<Client>>, snap: JsValue, at: f64) -> Result<JsValue, JsValue> {
    let revision = get(&snap, "revision");
    if revision.as_f64().is_none() {
        return Ok(false.into());
    }
    let mut s = st.try_borrow_mut().map_err(|_| borrowed())?;
    if truthy(&s.confirmed) && number(&revision) < number(&get(&s.confirmed, "revision")) {
        return Ok(false.into());
    }
    let copy = call(&global("Object"), "assign", &[object(), snap.clone()])?;
    for k in ["settings", "progress", "queue"] {
        if get(&snap, k).is_undefined() && !get(&s.confirmed, k).is_undefined() {
            set(&copy, k, &get(&s.confirmed, k))?;
        }
    }
    s.confirmed = copy;
    if let Some(n) = get(&snap, "serverNowMs").as_f64() {
        s.offset = n - at;
    }
    Ok(true.into())
}
fn server_now(st: &Rc<RefCell<Client>>) -> f64 {
    now(st) + st.try_borrow().map(|s| s.offset).unwrap_or(0.)
}
fn view(st: &Rc<RefCell<Client>>) -> Result<JsValue, JsValue> {
    let at = server_now(st);
    let s = st.try_borrow().map_err(|_| borrowed())?;
    let block = if truthy(&get(&s.confirmed, "block")) {
        get(&s.confirmed, "block")
    } else {
        JsValue::NULL
    };
    let status = if truthy(&block) {
        string(&get(&block, "status"))
    } else {
        "idle".into()
    };
    let live = truthy(&block) && ["running", "paused"].contains(&status.as_str());
    let rem = if live {
        remaining(&to_json(&block), at)
    } else {
        0.
    };
    let out = object();
    for (k, v) in [
        (
            "revision",
            if truthy(&s.confirmed) {
                get(&s.confirmed, "revision")
            } else {
                JsValue::NULL
            },
        ),
        ("block", block.clone()),
        ("live", live.into()),
        ("status", utf16_value(&status)),
        ("remainingMs", rem.into()),
        ("elapsedMs", elapsed(&to_json(&block), at).into()),
        ("due", (live && status == "running" && rem == 0.).into()),
        (
            "pending",
            if truthy(&s.pending) {
                get(&s.pending, "action")
            } else {
                JsValue::NULL
            },
        ),
        ("error", s.error.clone()),
        ("serverNowMs", at.into()),
        ("snapshot", s.confirmed.clone()),
    ] {
        set(&out, k, &v)?;
    }
    Ok(out)
}
fn new_id() -> JsValue {
    call(&global("crypto"), "randomUUID", &[])
        .unwrap_or_else(|_| format!("r-{}-{}", js_sys::Date::now(), js_sys::Math::random()).into())
}
async fn transport(
    st: &Rc<RefCell<Client>>,
    method: &str,
    body: JsValue,
) -> Result<JsValue, JsValue> {
    let f = st.try_borrow().map_err(|_| borrowed())?.opts.clone();
    let args = if body.is_undefined() {
        vec![method.into(), "/pomodoro".into()]
    } else {
        vec![method.into(), "/pomodoro".into(), body]
    };
    wait(invoke(&get(&f, "transport"), &args)).await
}
async fn refresh(st: Rc<RefCell<Client>>) -> Result<JsValue, JsValue> {
    let at = now(&st);
    let r = transport(&st, "GET", JsValue::UNDEFINED).await;
    let Ok(r) = r else {
        let mut s = st.try_borrow_mut().map_err(|_| borrowed())?;
        if !truthy(&s.pending) {
            s.error = from_json(
                &serde_json::json!({"code":"network","message":"Sin conexión con CommandOS. Se muestra el último estado confirmado.","retryable":false}),
            )?;
        }
        drop(s);
        change(&st);
        return Ok(false.into());
    };
    let ok = number(&get(&r, "status")) == 200.;
    if ok {
        accept(&st, get(&r, "body"), at)?;
        let mut s = st.try_borrow_mut().map_err(|_| borrowed())?;
        if !truthy(&s.pending) && get(&s.error, "code") == "network" {
            s.error = JsValue::NULL;
        }
    }
    change(&st);
    Ok(ok.into())
}
async fn dispatch(st: Rc<RefCell<Client>>) -> Result<JsValue, JsValue> {
    let request = st.try_borrow().map_err(|_| borrowed())?.pending.clone();
    let at = now(&st);
    let r = transport(&st, "POST", request.clone()).await;
    let ok = false;
    let error = if let Ok(r) = r {
        let status = number(&get(&r, "status"));
        let body = get(&r, "body");
        if status == 200. && get(&body, "ok") != JsValue::FALSE {
            accept(&st, body.clone(), at)?;
            let mut s = st.try_borrow_mut().map_err(|_| borrowed())?;
            s.pending = JsValue::NULL;
            s.error = JsValue::NULL;
            drop(s);
            change(&st);
            let out = object();
            set(&out, "ok", &true.into())?;
            set(&out, "result", &body)?;
            set(&out, "request", &request)?;
            return Ok(out);
        }
        let state = get(&body, "state");
        if truthy(&state) {
            accept(&st, state, at)?;
        }
        let retry = status >= 500.;
        if !retry {
            st.try_borrow_mut().map_err(|_| borrowed())?.pending = JsValue::NULL;
        }
        let code = get(&body, "code")
            .as_string()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| if retry { "server" } else { "rejected" }.into());
        let msg = get(&body, "error")
            .as_string()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| {
                if retry {
                    "El servidor no confirmó la acción."
                } else {
                    "Acción rechazada"
                }
                .into()
            });
        from_json(&serde_json::json!({"code":code,"message":msg,"retryable":retry}))?
    } else {
        from_json(
            &serde_json::json!({"code":"network","message":"No se pudo confirmar. El último estado confirmado sigue visible.","retryable":true}),
        )?
    };
    st.try_borrow_mut().map_err(|_| borrowed())?.error = error.clone();
    change(&st);
    let out = object();
    set(&out, "ok", &ok.into())?;
    set(&out, "error", &error)?;
    set(&out, "request", &request)?;
    Ok(out)
}
fn send(st: Rc<RefCell<Client>>, action: JsValue, fields: JsValue) -> Result<JsValue, JsValue> {
    let mut s = st.try_borrow_mut().map_err(|_| borrowed())?;
    if truthy(&s.pending) {
        return Ok(js_sys::Promise::resolve(&from_json(
            &serde_json::json!({"ok":false,"busy":true}),
        )?)
        .into());
    }
    let req = object();
    let id = invoke(&get(&s.opts, "requestId"), &[]).unwrap_or_else(|_| new_id());
    set(&req, "requestId", &id)?;
    set(
        &req,
        "expectedRevision",
        &if truthy(&s.confirmed) {
            get(&s.confirmed, "revision")
        } else {
            0.into()
        },
    )?;
    set(&req, "action", &action)?;
    let req = call(&global("Object"), "assign", &[req, fields])?;
    s.pending = req;
    s.error = JsValue::NULL;
    drop(s);
    change(&st);
    Ok(promise(dispatch(st)))
}
fn create_client(opts: JsValue) -> Result<JsValue, JsValue> {
    let st = Rc::new(RefCell::new(Client {
        confirmed: JsValue::NULL,
        offset: 0.,
        pending: JsValue::NULL,
        error: JsValue::NULL,
        opts,
    }));
    let out = object();
    let s = st.clone();
    method(&out, "refresh", move |_| Ok(promise(refresh(s.clone()))))?;
    let s = st.clone();
    method(&out, "send", move |a| {
        send(
            s.clone(),
            a.get(0),
            if a.get(1).is_undefined() {
                object()
            } else {
                a.get(1)
            },
        )
    })?;
    let s = st.clone();
    method(&out, "retry", move |_| {
        if truthy(&s.try_borrow().map_err(|_| borrowed())?.pending) {
            Ok(promise(dispatch(s.clone())))
        } else {
            Ok(js_sys::Promise::resolve(&from_json(&serde_json::json!({"ok":false}))?).into())
        }
    })?;
    let s = st.clone();
    method(&out, "discard", move |_| {
        let mut state = s.try_borrow_mut().map_err(|_| borrowed())?;
        state.pending = JsValue::NULL;
        state.error = JsValue::NULL;
        drop(state);
        change(&s);
        Ok(JsValue::UNDEFINED)
    })?;
    let s = st.clone();
    method(&out, "view", move |_| view(&s))?;
    let s = st.clone();
    method(&out, "accept", move |a| {
        accept(&s, a.get(0), number(&a.get(1)))
    })?;
    let s = st.clone();
    method(&out, "serverNow", move |_| Ok(server_now(&s).into()))?;
    method(&out, "snapshot", move |_| {
        Ok(st.try_borrow().map_err(|_| borrowed())?.confirmed.clone())
    })?;
    Ok(out)
}
pub fn mount() -> Result<(), JsValue> {
    let api = object();
    for (k, v) in [
        ("MIN", MIN),
        ("MIN_TARGET_MS", MIN),
        ("MAX_TARGET_MS", 180. * MIN),
    ] {
        set(&api, k, &v.into())?;
    }
    let data = art::catalog();
    for k in ["STYLES", "STYLE_ORDER", "ART_SOURCES"] {
        set(&api, k, &from_json(data.get(k).unwrap_or(&Value::Null))?)?;
    }
    // Optional animation properties are own undefined properties in the original catalog.
    let styles = get(&api, "STYLES");
    for name in art::STYLE_ORDER {
        let assets = get(&get(&styles, name), "assets");
        for role in ["clock", "crystal", "first", "hundred", "streak", "level"] {
            let a = get(&assets, role);
            if !get(&a, "width").is_undefined() && get(&a, "motion").is_undefined() {
                set(&a, "motion", &JsValue::UNDEFINED)?;
            }
        }
    }
    set(&api, "DEFAULT_STYLE", &"alchemy".into())?;
    method(&api, "elapsedMs", |a| {
        Ok(elapsed(&to_json(&a.get(0)), number(&a.get(1))).into())
    })?;
    method(&api, "remainingMs", |a| {
        Ok(remaining(&to_json(&a.get(0)), number(&a.get(1))).into())
    })?;
    method(&api, "fmt", |a| Ok(fmt(number(&a.get(0))).into()))?;
    method(&api, "deltaForRemaining", |a| {
        Ok(delta(&to_json(&a.get(0)), number(&a.get(1)), number(&a.get(2))).into())
    })?;
    method(&api, "hourglassFrame", |a| {
        Ok(hourglass(
            number(&a.get(1)),
            if a.get(2).is_null() || a.get(2).is_undefined() {
                None
            } else {
                Some(number(&a.get(2)))
            },
        )
        .into())
    })?;
    method(&api, "styleOf", |a| {
        Ok(art::style(&string(&a.get(0))).into())
    })?;
    method(&api, "assetHtml", |a| {
        let class = if a.get(2).is_string() {
            string(&a.get(2))
        } else {
            String::new()
        };
        Ok(utf16_value(&art::asset(
            &string(&a.get(0)),
            &string(&a.get(1)),
            &class,
        )))
    })?;
    method(&api, "artFiles", |_| from_json(&art::files()))?;
    method(&api, "newRequestId", |_| Ok(new_id()))?;
    method(&api, "soundWhere", |a| {
        let sound = a.get(0);
        let device = get(&sound, "device");
        let (where_, enable) = if !truthy(&get(&sound, "enabled")) {
            (
                "ningún dispositivo (el aviso de foco es solo visual)",
                false,
            )
        } else if truthy(&device) && device == a.get(1) {
            ("este dispositivo", false)
        } else {
            (
                if device == get(&sound, "desktopDevice") || device == "local-speaker" {
                    "la compu"
                } else if truthy(&device) {
                    "otro dispositivo"
                } else {
                    "ningún dispositivo"
                },
                !truthy(&a.get(2)),
            )
        };
        from_json(&serde_json::json!({"where":where_,"canEnableHere":enable}))
    })?;
    method(&api, "createClient", |a| create_client(a.get(0)))?;
    global_set("ComandosPomodoro", &api)?;
    mount_ui(api)
}
#[path = "pomodoro_ui_web.rs"]
mod ui;
pub use ui::attach;
use ui::mount_ui;
