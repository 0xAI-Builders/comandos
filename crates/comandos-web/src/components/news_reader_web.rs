//! Lightweight news entrypoints defer the native reader until actual use.
use comandos_web_dom::{bridge::global_set, port::*};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};
use wasm_bindgen::JsValue;
thread_local! {static COLD_CLICK:RefCell<Option<(JsValue,JsValue)>>=const{RefCell::new(None)};}
fn global(key: &str) -> JsValue {
    get(&js_sys::global(), key)
}
fn instance() -> JsValue {
    get(&global("NewsReader"), "instance")
}
pub fn mount() -> Result<(), JsValue> {
    super::super::content_runtime::declare("news")?;
    global_set("__comandosDeferredNewsReader", &JsValue::TRUE)?;
    super::super::content_runtime::factory(
        "NewsReader",
        &[
            "statusLabel",
            "modelName",
            "safeHref",
            "clampShare",
            "sourceDateLabel",
            "provenance",
            "storyKicker",
            "inlineText",
            "blocksHtml",
            "noteCommand",
            "dayLine",
            "notesByDay",
            "createStore",
            "createRenderer",
            "mount",
            "install",
        ],
    )?;
    Ok(())
}
pub fn retire_vendor() -> Result<(), JsValue> {
    Ok(())
}
pub fn remove_cold_listener() -> Result<(), JsValue> {
    if let Some((target, callback)) = COLD_CLICK
        .try_with(|slot| slot.try_borrow_mut().ok().and_then(|mut slot| slot.take()))
        .ok()
        .flatten()
    {
        call(&target, "removeEventListener", &["click".into(), callback])?;
    }
    Ok(())
}
pub fn attach() -> Result<(), JsValue> {
    let wanted = Rc::new(Cell::new(false));
    let generation = Rc::new(Cell::new(0u64));
    let disposed = Rc::new(Cell::new(false));
    let params = js_sys::Reflect::construct(
        &global("URLSearchParams").into(),
        &[get(&global("location"), "search")].into_iter().collect(),
    )?;
    let panel = call(&params, "get", &["panel".into()])?;
    let bridge = get(
        &get(&get(&global("window"), "webkit"), "messageHandlers"),
        "centro",
    );
    let desktop = string(&panel) == "news" && truthy(&bridge);
    let app_only = truthy(&bridge) && !desktop;
    let timer = Rc::new(RefCell::new(JsValue::UNDEFINED));
    let api = object();
    for action in ["open", "toggle", "close"] {
        let wanted = wanted.clone();
        let generation = generation.clone();
        let disposed = disposed.clone();
        let bridge = bridge.clone();
        method(&api, action, move |args| {
            if disposed.get() {
                return Err(js_sys::Error::new("Lector cerrado").into());
            }
            if app_only {
                let message = object();
                set(&message, "type", &"reader".into())?;
                set(
                    &message,
                    "action",
                    &if action == "close" { "close" } else { "open" }.into(),
                )?;
                if action != "close" {
                    let target = args.get(0);
                    set(
                        &message,
                        "id",
                        &if truthy(&target) { target } else { "".into() },
                    )?;
                }
                let _ = call(
                    &bridge,
                    "postMessage",
                    &[js_sys::JSON::stringify(&message)?.into()],
                );
                return Ok(JsValue::UNDEFINED);
            }
            if super::super::content_runtime::ready() {
                let result = call(&instance(), action, &args.iter().collect::<Vec<_>>())?;
                return Ok(if action == "toggle" {
                    JsValue::UNDEFINED
                } else {
                    result
                });
            }
            let untouched = generation.get() == 0;
            let open = match action {
                "open" => true,
                "close" => false,
                _ => !wanted.get(),
            };
            wanted.set(open);
            let version = generation.get().wrapping_add(1);
            generation.set(version);
            if !open {
                return Ok(if action == "close" && untouched {
                    JsValue::NULL
                } else {
                    JsValue::UNDEFINED
                });
            }
            let target = args.get(0);
            let wanted = wanted.clone();
            let generation = generation.clone();
            let disposed = disposed.clone();
            let prepared = super::super::content_runtime::prepare_content("news");
            let opened = wasm_bindgen_futures::future_to_promise(async move {
                wait(Ok(prepared.into())).await?;
                if disposed.get() || !wanted.get() || generation.get() != version {
                    return Ok(JsValue::UNDEFINED);
                }
                wait(call(&instance(), "open", &[target])).await
            });
            if action == "toggle" {
                call(&opened, "catch", &[function(|_| Ok(JsValue::UNDEFINED))])?;
                Ok(JsValue::UNDEFINED)
            } else {
                Ok(opened.into())
            }
        })?;
    }
    if !app_only {
        getter(&api, "reader", || {
            if super::super::content_runtime::ready() {
                get(&instance(), "reader")
            } else {
                JsValue::NULL
            }
        })?;
    }
    let stop = disposed.clone();
    let gen2 = generation.clone();
    let timer2 = timer.clone();
    method(&api, "dispose", move |_| {
        stop.set(true);
        global_set("__comandosNewsDisposed", &JsValue::TRUE)?;
        if super::super::content_runtime::ready() {
            call(&instance(), "dispose", &[])?;
        }
        gen2.set(gen2.get().wrapping_add(1));
        remove_cold_listener()?;
        if let Ok(timer) = timer2.try_borrow() {
            let _ = call(
                &js_sys::global(),
                "clearTimeout",
                std::slice::from_ref(&timer),
            );
        }
        Ok(JsValue::UNDEFINED)
    })?;
    set(&global("NewsReader"), "instance", &api)?;
    let toggle = get(&api, "toggle");
    let callback = function(move |_| {
        let opened = invoke(&toggle, &[])?;
        if get(&opened, "catch").is_function() {
            call(&opened, "catch", &[function(|_| Ok(JsValue::UNDEFINED))])?;
        }
        Ok(JsValue::UNDEFINED)
    });
    let button = call(&global("document"), "getElementById", &["btn-news".into()])?;
    if button.is_object() {
        call(
            &button,
            "addEventListener",
            &["click".into(), callback.clone()],
        )?;
        let _ = COLD_CLICK.try_with(|slot| {
            if let Ok(mut slot) = slot.try_borrow_mut() {
                *slot = Some((button.clone(), callback));
            }
        });
    }
    let news = call(&params, "get", &["news".into()])?;
    if !app_only && (desktop || !news.is_null()) {
        let pattern = js_sys::RegExp::new(r"^\d{4}-\d{2}-\d{2}@\d{2}:\d{2}$", "");
        let target = if truthy(&news) && pattern.test(&string(&news)) {
            news
        } else {
            JsValue::UNDEFINED
        };
        let open = get(&api, "open");
        let handle = call(
            &js_sys::global(),
            "setTimeout",
            &[
                function(move |_| {
                    let opened = invoke(&open, std::slice::from_ref(&target))?;
                    let _ = call(&opened, "catch", &[function(|_| Ok(JsValue::UNDEFINED))]);
                    Ok(JsValue::UNDEFINED)
                }),
                if desktop { 0.0 } else { 400.0 }.into(),
            ],
        )?;
        if let Ok(mut timer) = timer.try_borrow_mut() {
            *timer = handle;
        }
    }
    global_set("__comandosNewsDeferred", &JsValue::TRUE)
}
