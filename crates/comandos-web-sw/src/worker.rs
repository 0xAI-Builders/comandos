use crate::policy;
use js_sys::{Array, Function, JsString, Object, Promise, Reflect};
use wasm_bindgen::{JsCast, closure::Closure, prelude::*};
use wasm_bindgen_futures::{JsFuture, future_to_promise};

const NATIVE_PREFIX: &str = "comandos-native-shell-v1-";
fn get(value: &JsValue, key: &str) -> JsValue {
    Reflect::get(value, &key.into()).unwrap_or(JsValue::UNDEFINED)
}
fn global(key: &str) -> JsValue {
    get(&js_sys::global(), key)
}
fn call(value: &JsValue, key: &str, args: &[JsValue]) -> Result<JsValue, JsValue> {
    let f: Function = get(value, key).dyn_into()?;
    f.apply(value, &args.iter().cloned().collect::<Array>())
}
fn object(fields: &[(&str, JsValue)]) -> Result<JsValue, JsValue> {
    let out = Object::new();
    for (key, value) in fields {
        Reflect::set(&out, &(*key).into(), value)?;
    }
    Ok(out.into())
}
async fn resolved(value: JsValue) -> Result<JsValue, JsValue> {
    JsFuture::from(Promise::resolve(&value)).await
}
fn listen<F: Fn(JsValue) -> Result<(), JsValue> + 'static>(
    scope: &JsValue,
    name: &str,
    f: F,
) -> Result<(), JsValue> {
    let f = Closure::<dyn Fn(JsValue) -> Result<(), JsValue>>::new(f);
    call(
        scope,
        "addEventListener",
        &[name.into(), f.as_ref().clone()],
    )?;
    f.forget();
    Ok(())
}
fn url(value: &JsValue, base: Option<&JsValue>) -> Result<JsValue, JsValue> {
    let args = Array::new();
    args.push(value);
    if let Some(b) = base {
        args.push(b);
    }
    Reflect::construct(&global("URL").dyn_into::<Function>()?, &args)
}
fn event_id(value: JsValue) -> JsValue {
    if !value.is_string() {
        return "".into();
    }
    let text: JsString = value.clone().unchecked_into();
    if text.length() > 128 || (0..text.length()).any(|i| text.char_code_at(i) <= 31.) {
        "".into()
    } else {
        value
    }
}
fn string(value: JsValue) -> Result<JsString, JsValue> {
    Ok(global("String")
        .dyn_into::<Function>()?
        .call1(&JsValue::UNDEFINED, &value)?
        .unchecked_into())
}
fn sliced(value: JsValue, fallback: &str, len: u32) -> Result<JsValue, JsValue> {
    Ok(string(if value.is_falsy() {
        fallback.into()
    } else {
        value
    })?
    .substring(0, len)
    .into())
}

pub fn boot(native: bool, precache: JsValue) -> Result<(), JsValue> {
    let scope = global("self");
    let origin = get(&get(&scope, "location"), "origin");
    // The versioned manifest set separates releases and avoids overwriting a
    // legacy shell's cache during an explicitly selected native preview.
    let cache_key = if native {
        let assets = Array::from(&precache);
        let fingerprint = assets
            .iter()
            .filter_map(|v| v.as_string())
            .collect::<Vec<_>>()
            .join("|");
        let hash = fingerprint.bytes().fold(0xcbf29ce484222325u64, |h, b| {
            (h ^ u64::from(b)).wrapping_mul(0x100000001b3)
        });
        format!("{NATIVE_PREFIX}{hash:x}")
    } else {
        policy::SHELL.into()
    };
    let known_wasm = Array::from(&precache)
        .iter()
        .filter_map(|v| v.as_string())
        .filter(|p| p.ends_with(".wasm"))
        .collect::<Vec<_>>();
    let urls = if native {
        precache
    } else {
        Array::of2(&"/".into(), &"/manifest.webmanifest".into()).into()
    };
    let install_scope = scope.clone();
    let key = cache_key.clone();
    listen(&scope, "install", move |e| {
        call(&install_scope, "skipWaiting", &[])?;
        let key = key.clone();
        let urls = urls.clone();
        let promise = future_to_promise(async move {
            let c = resolved(call(&global("caches"), "open", &[key.into()])?).await?;
            resolved(call(&c, "addAll", &[urls])?).await
        });
        call(&e, "waitUntil", &[promise.into()])?;
        Ok(())
    })?;
    let activate_scope = scope.clone();
    let key = cache_key.clone();
    listen(&scope, "activate", move |e| {
        let key = key.clone();
        let promise = future_to_promise(async move {
            let caches = global("caches");
            let keys = Array::from(&resolved(call(&caches, "keys", &[])?).await?);
            let deletes = Array::new();
            for k in keys.iter() {
                let name = k.as_string().unwrap_or_default();
                if name != key && (!native || name.starts_with(NATIVE_PREFIX)) {
                    deletes.push(&call(&caches, "delete", &[k])?);
                }
            }
            resolved(Promise::all(&deletes).into()).await
        });
        call(&e, "waitUntil", &[promise.into()])?;
        call(&get(&activate_scope, "clients"), "claim", &[])?;
        Ok(())
    })?;
    let key = cache_key.clone();
    let fetch_origin = origin.clone();
    listen(&scope, "fetch", move |e| {
        let request = get(&e, "request");
        let parsed = url(&get(&request, "url"), None)?;
        let path = get(&parsed, "pathname").as_string().unwrap_or_default();
        let asset_wasm = native
            && known_wasm.contains(&path)
            && get(&parsed, "origin") == fetch_origin
            && get(&request, "method") == "GET";
        if !asset_wasm
            && !policy::intercepts(
                &get(&parsed, "origin").as_string().unwrap_or_default(),
                &fetch_origin.as_string().unwrap_or_default(),
                &get(&request, "method").as_string().unwrap_or_default(),
                &path,
            )
        {
            return Ok(());
        }
        let token = path == "/"
            && call(&get(&parsed, "searchParams"), "has", &["token".into()])?.is_truthy();
        let key = key.clone();
        let lifetime = e.clone();
        let promise = future_to_promise(async move {
            let opts = object(&[("cache", "no-cache".into())])?;
            let fetched =
                global("fetch")
                    .dyn_into::<Function>()?
                    .call2(&JsValue::UNDEFINED, &request, &opts);
            match match fetched {
                Ok(p) => resolved(p).await,
                Err(e) => Err(e),
            } {
                Ok(response) => {
                    if !token {
                        let copy = call(&response, "clone", &[])?;
                        let req = request.clone();
                        // Original network-first behaviour does not block the
                        // response on a cache write. Keep it alive for native SW.
                        let cache = future_to_promise(async move {
                            let c =
                                resolved(call(&global("caches"), "open", &[key.into()])?).await?;
                            resolved(call(&c, "put", &[req, copy])?).await
                        });
                        if native {
                            call(&lifetime, "waitUntil", &[cache.into()])?;
                        }
                    }
                    Ok(response)
                }
                Err(_) => {
                    let caches = global("caches");
                    let fallback = if native { "/?web=native" } else { "/" };
                    if token {
                        return resolved(call(&caches, "match", &[fallback.into()])?).await;
                    }
                    let found = resolved(call(&caches, "match", &[request])?).await?;
                    if !found.is_falsy() {
                        Ok(found)
                    } else {
                        resolved(call(&caches, "match", &[fallback.into()])?).await
                    }
                }
            }
        });
        call(&e, "respondWith", &[promise.into()])?;
        Ok(())
    })?;
    let push_scope = scope.clone();
    listen(&scope, "push", move |e| {
        let data = get(&e, "data");
        let data = if data.is_falsy() {
            Object::new().into()
        } else {
            call(&data, "json", &[]).unwrap_or_else(|_| Object::new().into())
        };
        let data = if data.is_object() && !data.is_null() {
            data
        } else {
            Object::new().into()
        };
        let id = event_id(get(&data, "eventId"));
        let title = sliced(get(&data, "title"), "CommandOS", 60)?;
        let body = sliced(get(&data, "body"), "", 120)?;
        let tag = if !id.is_falsy() {
            string("comandos-event-".into())?.concat(&id).into()
        } else if get(&data, "tag") == "comandos-test" {
            "comandos-test".into()
        } else {
            "comandos".into()
        };
        let options = object(&[
            ("body", body),
            ("tag", tag),
            ("renotify", false.into()),
            ("icon", "/icon-192.png".into()),
            ("badge", "/icon-192.png".into()),
            ("data", object(&[("eventId", id)])?),
        ])?;
        let p = call(
            &get(&push_scope, "registration"),
            "showNotification",
            &[title, options],
        )?;
        call(&e, "waitUntil", &[p])?;
        Ok(())
    })?;
    let click_scope = scope.clone();
    listen(&scope, "notificationclick", move |e| {
        let note = get(&e, "notification");
        call(&note, "close", &[])?;
        let id = event_id(get(&get(&note, "data"), "eventId"));
        let path = if id.is_falsy() {
            if native {
                "/?web=native".into()
            } else {
                "/".into()
            }
        } else {
            string(if native {
                "/?web=native&event=".into()
            } else {
                "/?event=".into()
            })?
            .concat(
                &global("encodeURIComponent")
                    .dyn_into::<Function>()?
                    .call1(&JsValue::UNDEFINED, &id)?,
            )
            .into()
        };
        let target = get(&url(&path, Some(&origin))?, "href");
        let origin = origin.clone();
        let clients = get(&click_scope, "clients");
        let promise = future_to_promise(async move {
            let opts = object(&[
                ("type", "window".into()),
                ("includeUncontrolled", true.into()),
            ])?;
            let windows = Array::from(&resolved(call(&clients, "matchAll", &[opts])?).await?);
            for window in windows.iter() {
                let Ok(parsed) = url(&get(&window, "url"), None) else {
                    continue;
                };
                if get(&parsed, "origin") != origin {
                    continue;
                }
                if get(&window, "focus").is_function() {
                    resolved(call(&window, "focus", &[])?).await?;
                }
                call(
                    &window,
                    "postMessage",
                    &[object(&[
                        ("type", "comandos:open-event".into()),
                        ("eventId", id),
                    ])?],
                )?;
                return Ok(JsValue::UNDEFINED);
            }
            if get(&clients, "openWindow").is_truthy() {
                resolved(call(&clients, "openWindow", &[target])?).await?;
            }
            Ok(JsValue::UNDEFINED)
        });
        call(&e, "waitUntil", &[promise.into()])?;
        Ok(())
    })?;
    Ok(())
}
