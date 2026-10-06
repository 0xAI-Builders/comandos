use super::{DEFAULT_CATALOG, LOOP_CUES, MAX_VOLUME};
use comandos_web_dom::{audio, bridge::global_set, port::*};
use std::{cell::RefCell, collections::BTreeMap, rc::Rc};
use wasm_bindgen::JsValue;
const ENABLED: &str = "comandos:ui-sounds:enabled";
const VOLUME: &str = "comandos:ui-sounds:volume";
const PLAYED: &str = "comandos:ui-sounds:played";
struct State {
    enabled: bool,
    volume: f64,
    armed: bool,
    disposed: bool,
    engine: JsValue,
    player: JsValue,
    names: BTreeMap<String, String>,
    order: Vec<String>,
    played: std::collections::BTreeSet<String>,
}
fn read(storage: &JsValue, key: &str, default: &str) -> String {
    call(storage, "getItem", &[key.into()])
        .ok()
        .filter(|v| !v.is_null() && !v.is_undefined())
        .map(|v| string(&v))
        .unwrap_or(default.into())
}
fn write(storage: &JsValue, key: &str, value: &str) {
    let _ = call(storage, "setItem", &[key.into(), value.into()]);
}
fn visible(doc: &JsValue) -> bool {
    get(doc, "visibilityState").as_string().as_deref() != Some("hidden")
}
fn trusted(win: &JsValue, event: &JsValue) -> bool {
    get(event, "isTrusted") == JsValue::TRUE
        || truthy(&get(
            &get(&get(win, "navigator"), "userActivation"),
            "isActive",
        ))
}
fn borrowed() -> JsValue {
    js_sys::Error::new("sound state busy").into()
}
struct Player {
    context: JsValue,
    master: JsValue,
    buffers: BTreeMap<String, JsValue>,
    voices: BTreeMap<String, (JsValue, JsValue, f64)>,
    volume: f64,
    last_started: BTreeMap<String, f64>,
}
fn rust_player(volume: f64, pack: String) -> Result<JsValue, JsValue> {
    let state = Rc::new(RefCell::new(Player {
        context: JsValue::NULL,
        master: JsValue::NULL,
        buffers: BTreeMap::new(),
        voices: BTreeMap::new(),
        last_started: BTreeMap::new(),
        volume,
    }));
    let o = object();
    let st = state.clone();
    method(&o, "unlock", move |_| {
        let mut s = st.try_borrow_mut().map_err(|_| borrowed())?;
        if s.context.is_null() {
            let g = js_sys::global();
            let class = get(&g, "AudioContext");
            let class = if class.is_undefined() {
                get(&g, "webkitAudioContext")
            } else {
                class
            };
            if !class.is_function() {
                return Ok(js_sys::Promise::resolve(&JsValue::FALSE).into());
            }
            s.context = js_sys::Reflect::construct(&class.into(), &js_sys::Array::new())?;
            s.master = call(&s.context, "createGain", &[])?;
            set(&get(&s.master, "gain"), "value", &s.volume.into())?;
            call(&s.master, "connect", &[get(&s.context, "destination")])?;
        }
        let context = s.context.clone();
        drop(s);
        let resumed = call(&context, "resume", &[]);
        Ok(wasm_bindgen_futures::future_to_promise(async move {
            Ok((wait(resumed).await.is_ok()
                && get(&context, "state").as_string().as_deref() == Some("running"))
            .into())
        })
        .into())
    })?;
    let st = state.clone();
    method(&o, "setVolume", move |a| {
        let mut s = st.try_borrow_mut().map_err(|_| borrowed())?;
        s.volume = a.get(0).as_f64().unwrap_or(s.volume);
        if !s.master.is_null() {
            let _ = call(
                &get(&s.master, "gain"),
                "linearRampToValueAtTime",
                &[
                    s.volume.into(),
                    (get(&s.context, "currentTime").as_f64().unwrap_or(0.0) + 0.02).into(),
                ],
            );
        }
        Ok(JsValue::UNDEFINED)
    })?;
    let st = state.clone();
    let stop = function(move |_| {
        let mut s = st.try_borrow_mut().map_err(|_| borrowed())?;
        for (_, (_, handle, _)) in std::mem::take(&mut s.voices) {
            let _ = call(&handle, "stop", &[]);
        }
        Ok(JsValue::UNDEFINED)
    });
    set(&o, "stopAll", &stop)?;
    let st = state.clone();
    method(&o, "destroy", move |_| {
        invoke(&stop, &[])?;
        let mut s = st.try_borrow_mut().map_err(|_| borrowed())?;
        s.buffers.clear();
        let c = s.context.clone();
        s.context = JsValue::NULL;
        s.master = JsValue::NULL;
        Ok(if c.is_null() {
            js_sys::Promise::resolve(&JsValue::UNDEFINED).into()
        } else {
            call(&c, "close", &[])?
        })
    })?;
    method(&o, "play", move |a| {
        let cue = string(&a.get(0));
        let options = a.get(1);
        let recipe = audio::pack_recipe(&pack, &cue);
        let Some(recipe) = recipe else {
            return Ok(JsValue::NULL);
        };
        let mut s = state.try_borrow_mut().map_err(|_| borrowed())?;
        if s.context.is_null() {
            return Ok(JsValue::NULL);
        }
        let now = js_sys::Date::now();
        if s.last_started
            .get(&cue)
            .is_some_and(|last| now - last < 180.0)
        {
            return Ok(s
                .voices
                .get(&cue)
                .map(|(_, h, _)| h.clone())
                .unwrap_or(JsValue::NULL));
        }
        if let Some((_, handle, at)) = s.voices.get(&cue)
            && (now - *at < 180.0
                || get(&options, "retrigger").as_string().as_deref() == Some("ignore"))
        {
            return Ok(handle.clone());
        }
        if s.voices.len() >= 2 {
            let oldest = s
                .voices
                .iter()
                .min_by(|a, b| a.1.2.total_cmp(&b.1.2))
                .map(|(k, _)| k.clone());
            if let Some(k) = oldest
                && let Some((_, handle, _)) = s.voices.remove(&k)
            {
                let _ = call(&handle, "stop", &[]);
            }
        }
        let buffer = if let Some(b) = s.buffers.get(&cue) {
            b.clone()
        } else {
            let rate = get(&s.context, "sampleRate").as_f64().unwrap_or(48_000.0);
            let (left, right) = audio::render_pack_cue(&pack, &cue, rate as f32);
            let b = call(
                &s.context,
                "createBuffer",
                &[2.into(), (left.len() as f64).into(), rate.into()],
            )?;
            for (channel, samples) in [(0, left), (1, right)] {
                let array = call(&b, "getChannelData", &[channel.into()])?;
                call(
                    &array,
                    "set",
                    &[js_sys::Float32Array::from(samples.as_slice()).into()],
                )?;
            }
            s.buffers.insert(cue.clone(), b.clone());
            b
        };
        let source = call(&s.context, "createBufferSource", &[])?;
        set(&source, "buffer", &buffer)?;
        set(&source, "loop", &false.into())?;
        let gain = call(&s.context, "createGain", &[])?;
        set(
            &get(&gain, "gain"),
            "value",
            &get(&options, "volume")
                .as_f64()
                .unwrap_or_else(|| {
                    recipe
                        .get("defaultVolume")
                        .and_then(serde_json::Value::as_f64)
                        .unwrap_or(1.0)
                })
                .into(),
        )?;
        call(&source, "connect", std::slice::from_ref(&gain))?;
        call(&gain, "connect", std::slice::from_ref(&s.master))?;
        let handle = object();
        let src = source.clone();
        let stop_gain = gain.clone();
        let context = s.context.clone();
        let stopped = std::cell::Cell::new(false);
        let player_state = state.clone();
        let stop_key = cue.clone();
        method(&handle, "stop", move |_| {
            if stopped.replace(true) {
                return Ok(JsValue::UNDEFINED);
            }
            if let Ok(mut s) = player_state.try_borrow_mut()
                && s.voices.get(&stop_key).is_some_and(|(current, _, _)| current == &src) {
                s.voices.remove(&stop_key);
            }
            let now = get(&context, "currentTime").as_f64().unwrap_or(0.0);
            let param = get(&stop_gain, "gain");
            let _ = call(&param, "cancelScheduledValues", &[now.into()]);
            let _ = call(
                &param,
                "setValueAtTime",
                &[get(&param, "value"), now.into()],
            );
            let _ = call(
                &param,
                "linearRampToValueAtTime",
                &[0.into(), (now + 0.015).into()],
            );
            let _ = call(&src, "stop", &[(now + 0.018).into()]);
            Ok(JsValue::UNDEFINED)
        })?;
        let holder = Rc::new(RefCell::new(JsValue::UNDEFINED));
        let holder2 = holder.clone();
        let ended = js_sys::Promise::new(&mut move |resolve, _| {
            if let Ok(mut h) = holder2.try_borrow_mut() {
                *h = resolve.into();
            }
        });
        set(&handle, "ended", &ended.into())?;
        let st = state.clone();
        let key = cue.clone();
        let src = source.clone();
        let ended_cb = function(move |_| {
            if let Ok(mut s) = st.try_borrow_mut()
                && s.voices
                    .get(&key)
                    .is_some_and(|(current, _, _)| current == &src)
            {
                s.voices.remove(&key);
            }
            let _ = call(&src, "disconnect", &[]);
            let _ = call(&gain, "disconnect", &[]);
            if let Ok(resolve) = holder.try_borrow() {
                let _ = invoke(&resolve, &[]);
            }
            Ok(JsValue::UNDEFINED)
        });
        let once = object();
        set(&once, "once", &true.into())?;
        call(
            &source,
            "addEventListener",
            &["ended".into(), ended_cb, once],
        )?;
        s.last_started.insert(cue.clone(), now);
        s.voices.insert(cue, (source.clone(), handle.clone(), now));
        call(&source, "start", &[])?;
        Ok(handle)
    })?;
    Ok(o)
}
fn ensure_player(s: &mut State, pack: &JsValue) -> JsValue {
    if !s.player.is_null() {
        return s.player.clone();
    }
    if s.engine.is_null() {
        return JsValue::NULL;
    }
    let options = object();
    let _ = set(&options, "pack", pack);
    let _ = set(&options, "enabled", &true.into());
    let _ = set(&options, "volume", &(s.volume * MAX_VOLUME).into());
    let _ = set(&options, "maxVoices", &2.into());
    let _ = set(&options, "cooldownMs", &180.into());
    s.player = call(&s.engine, "createUISFX", &[options]).unwrap_or(JsValue::NULL);
    s.player.clone()
}
fn resolve(s: &State, cue: &JsValue) -> Option<String> {
    let raw = cue.as_string()?;
    let name = s.names.get(&raw).cloned().unwrap_or(raw);
    if name.is_empty() || LOOP_CUES.contains(&name.as_str()) {
        return None;
    }
    let cues = get(&s.engine, "cueNames");
    if js_sys::Array::is_array(&cues)
        && !js_sys::Array::from(&cues).includes(&name.clone().into(), 0)
    {
        return None;
    }
    Some(name)
}
fn unlock(
    state: &Rc<RefCell<State>>,
    win: &JsValue,
    doc: &JsValue,
    pack: &JsValue,
    event: &JsValue,
) -> Result<bool, JsValue> {
    let mut s = state.try_borrow_mut().map_err(|_| borrowed())?;
    if s.disposed || !trusted(win, event) || !visible(doc) || s.engine.is_null() {
        return Ok(false);
    }
    let p = ensure_player(&mut s, pack);
    if p.is_null() {
        return Ok(false);
    }
    drop(s);
    let attempt = call(&p, "unlock", &[]);
    let st = state.clone();
    wasm_bindgen_futures::spawn_local(async move {
        if wait(attempt).await == Ok(JsValue::TRUE)
            && let Ok(mut s) = st.try_borrow_mut()
            && !s.disposed
        {
            s.armed = true;
        }
    });
    Ok(true)
}
fn create(options: JsValue, internal_engine: bool) -> Result<JsValue, JsValue> {
    let win = get(&options, "win");
    let doc = get(&options, "doc");
    let storage = get(&options, "storage");
    let pack = get(&options, "pack");
    let pack = if pack.is_undefined() {
        "arcade".into()
    } else {
        pack
    };
    let catalog = get(&options, "catalog");
    let catalog = if catalog.is_undefined() {
        from_json(&serde_json::Value::Object(
            DEFAULT_CATALOG
                .iter()
                .map(|(k, v)| (k.to_string(), (*v).into()))
                .collect(),
        ))?
    } else {
        catalog
    };
    let mut names = BTreeMap::new();
    let mut order = Vec::new();
    for entry in js_sys::Object::entries(&catalog.into()).iter() {
        let pair = js_sys::Array::from(&entry);
        let k = string(&pair.get(0));
        names.insert(k.clone(), string(&pair.get(1)));
        order.push(k);
    }
    let volume = number(&JsValue::from(read(&storage, VOLUME, "0.6"))).clamp(0.0, 1.0);
    let volume = if volume.is_finite() { volume } else { 0.6 };
    let state = Rc::new(RefCell::new(State {
        enabled: read(&storage, ENABLED, "off") == "on",
        volume,
        armed: false,
        disposed: false,
        engine: JsValue::NULL,
        player: JsValue::NULL,
        names,
        order,
        played: Default::default(),
    }));
    let loader = get(&options, "loadEngine");
    if loader.is_function() {
        let loaded = invoke(&loader, &[]);
        let st = state.clone();
        wasm_bindgen_futures::spawn_local(async move {
            if let Ok(engine) = wait(loaded).await
                && let Ok(mut s) = st.try_borrow_mut()
            {
                s.engine = engine;
            }
        });
    } else if internal_engine {
        let engine = object();
        method(&engine, "createUISFX", |a| {
            rust_player(
                get(&a.get(0), "volume").as_f64().unwrap_or(0.18),
                get(&a.get(0), "pack")
                    .as_string()
                    .unwrap_or("arcade".into()),
            )
        })?;
        set(
            &engine,
            "cueNames",
            &from_json(&serde_json::json!(audio::cue_names()))?,
        )?;
        state.try_borrow_mut().map_err(|_| borrowed())?.engine = engine;
    }
    let out = object();
    let st = state.clone();
    let d = doc.clone();
    let w = win.clone();
    let p = pack.clone();
    method(&out, "unlock", move |a| {
        Ok(unlock(&st, &w, &d, &p, &a.get(0))?.into())
    })?;
    for name in ["play", "preview"] {
        let st = state.clone();
        let d = doc.clone();
        let w = win.clone();
        let p = pack.clone();
        let store = storage.clone();
        method(&out, name, move |a| {
            {
                let s = st.try_borrow().map_err(|_| borrowed())?;
                if s.disposed || !visible(&d) || (name == "play" && (!s.enabled || !s.armed)) {
                    return Ok(JsValue::NULL);
                }
            }
            let cue = {
                let s = st.try_borrow().map_err(|_| borrowed())?;
                resolve(&s, &a.get(0))
            };
            let Some(cue) = cue else {
                return Ok(JsValue::NULL);
            };
            if name == "preview" {
                unlock(&st, &w, &d, &p, &JsValue::UNDEFINED)?;
            }
            let mut s = st.try_borrow_mut().map_err(|_| borrowed())?;
            if name == "play" {
                let id = get(&a.get(1), "eventId");
                if !id.is_null() && !id.is_undefined() {
                    let id = string(&id);
                    let list: Vec<String> =
                        serde_json::from_str(&read(&store, PLAYED, "[]")).unwrap_or_default();
                    if s.played.contains(&id) || list.contains(&id) {
                        return Ok(JsValue::NULL);
                    }
                    s.played.insert(id.clone());
                    let mut list = list.into_iter().filter(|x| x != &id).collect::<Vec<_>>();
                    list.push(id);
                    let limit = list.len().saturating_sub(300);
                    write(
                        &store,
                        PLAYED,
                        &serde_json::to_string(&list.into_iter().skip(limit).collect::<Vec<_>>())
                            .unwrap_or_default(),
                    );
                }
            }
            let player = if name == "preview" {
                s.player.clone()
            } else {
                ensure_player(&mut s, &p)
            };
            if player.is_null() {
                return Ok(JsValue::NULL);
            }
            if name == "preview" {
                s.armed = true;
            }
            let options = object();
            set(&options, "loop", &false.into())?;
            set(&options, "retrigger", &"ignore".into())?;
            let gain = get(&a.get(1), "volume");
            if name == "play"
                && let Some(g) = gain.as_f64().filter(|n| n.is_finite())
            {
                set(
                    &options,
                    "volume",
                    &(g.clamp(0.0, 1.0) * s.volume * MAX_VOLUME).into(),
                )?;
            }
            drop(s);
            Ok(call(&player, "play", &[cue.into(), options]).unwrap_or(JsValue::NULL))
        })?;
    }
    let st = state.clone();
    let stop = function(move |_| {
        let player = st.try_borrow().map_err(|_| borrowed())?.player.clone();
        let _ = call(&player, "stopAll", &[]);
        Ok(JsValue::UNDEFINED)
    });
    set(&out, "stop", &stop)?;
    let st = state.clone();
    let store = storage.clone();
    let stop2 = stop.clone();
    method(&out, "setEnabled", move |a| {
        let enabled = truthy(&a.get(0));
        st.try_borrow_mut().map_err(|_| borrowed())?.enabled = enabled;
        write(&store, ENABLED, if enabled { "on" } else { "off" });
        if !enabled {
            invoke(&stop2, &[])?;
        }
        Ok(enabled.into())
    })?;
    let st = state.clone();
    let store = storage.clone();
    method(&out, "setVolume", move |a| {
        let v = number(&a.get(0));
        let mut s = st.try_borrow_mut().map_err(|_| borrowed())?;
        if v.is_finite() {
            s.volume = v.clamp(0.0, 1.0);
            write(&store, VOLUME, &s.volume.to_string());
            let _ = call(&s.player, "setVolume", &[(s.volume * MAX_VOLUME).into()]);
        }
        Ok(s.volume.into())
    })?;
    let st = state.clone();
    method(&out, "register", move |a| {
        let Some(name) = a.get(0).as_string().filter(|s| !s.is_empty()) else {
            return Ok(false.into());
        };
        let Some(cue) = a
            .get(1)
            .as_string()
            .filter(|s| !LOOP_CUES.contains(&s.as_str()))
        else {
            return Ok(false.into());
        };
        let mut s = st.try_borrow_mut().map_err(|_| borrowed())?;
        if !s.names.contains_key(&name) {
            s.order.push(name.clone());
        }
        s.names.insert(name, cue);
        Ok(true.into())
    })?;
    for name in ["isEnabled", "getVolume", "isReady", "cues"] {
        let st = state.clone();
        let d = doc.clone();
        method(&out, name, move |_| {
            let s = st.try_borrow().map_err(|_| borrowed())?;
            Ok(match name {
                "isEnabled" => s.enabled.into(),
                "getVolume" => s.volume.into(),
                "isReady" => (s.enabled && s.armed && visible(&d)).into(),
                _ => s
                    .order
                    .iter()
                    .map(|x| JsValue::from(x.as_str()))
                    .collect::<js_sys::Array>()
                    .into(),
            })
        })?;
    }
    let mut listeners = Vec::new();
    for (target, kind) in [
        (doc.clone(), "pointerdown"),
        (doc.clone(), "keydown"),
        (doc.clone(), "visibilitychange"),
        (win.clone(), "pagehide"),
    ] {
        let st = state.clone();
        let w = win.clone();
        let d = doc.clone();
        let p = pack.clone();
        let halt = stop.clone();
        let callback = function(move |a| {
            if kind == "pagehide" || (kind == "visibilitychange" && !visible(&d)) {
                invoke(&halt, &[])?;
            } else if (kind == "pointerdown" || kind == "keydown")
                && st.try_borrow().map(|s| s.enabled).unwrap_or(false)
            {
                unlock(&st, &w, &d, &p, &a.get(0))?;
            }
            Ok(JsValue::UNDEFINED)
        });
        let _ = call(
            &target,
            "addEventListener",
            &[kind.into(), callback.clone(), true.into()],
        );
        listeners.push((target, kind, callback));
    }
    method(&out, "dispose", move |_| {
        state.try_borrow_mut().map_err(|_| borrowed())?.disposed = true;
        invoke(&stop, &[])?;
        for (target, kind, callback) in &listeners {
            let _ = call(
                target,
                "removeEventListener",
                &[(*kind).into(), callback.clone(), true.into()],
            );
        }
        let player = state.try_borrow().map_err(|_| borrowed())?.player.clone();
        state.try_borrow_mut().map_err(|_| borrowed())?.player = JsValue::NULL;
        let destroy = call(&player, "destroy", &[]);
        Ok(wasm_bindgen_futures::future_to_promise(async move {
            let _ = wait(destroy).await;
            Ok(JsValue::UNDEFINED)
        })
        .into())
    })?;
    Ok(out)
}
pub fn mount() -> Result<(), JsValue> {
    let api = object();
    method(&api, "createUISounds", |a| create(a.get(0), false))?;
    set(
        &api,
        "DEFAULT_CATALOG",
        &from_json(&serde_json::Value::Object(
            DEFAULT_CATALOG
                .iter()
                .map(|(k, v)| (k.to_string(), (*v).into()))
                .collect(),
        ))?,
    )?;
    set(
        &api,
        "LOOP_CUES",
        &from_json(&serde_json::json!(LOOP_CUES))?,
    )?;
    for name in ["DEFAULT_CATALOG", "LOOP_CUES"] {
        js_sys::Object::freeze(&get(&api, name).into());
    }
    set(&api, "MAX_VOLUME", &MAX_VOLUME.into())?;
    set(
        &api,
        "KEYS",
        &from_json(&serde_json::json!({"enabled":ENABLED,"volume":VOLUME,"played":PLAYED}))?,
    )?;
    global_set("ComandosUISounds", &api)?;
    let global = js_sys::global();
    let opts = object();
    set(&opts, "win", &global)?;
    set(&opts, "doc", &get(&global, "document"))?;
    set(&opts, "storage", &get(&global, "localStorage"))?;
    if !truthy(&get(&global, "uiSounds")) {
        global_set("uiSounds", &create(opts, true)?)?;
    }
    if get(&global, "COMANDOS_DASH_TEST_HOOKS") == JsValue::TRUE {
        let hook = function(|a| {
            let pack = a.get(2).as_string().unwrap_or("arcade".into());
            let (left, right) = audio::render_pack_cue(
                &pack,
                &string(&a.get(0)),
                a.get(1).as_f64().unwrap_or(48000.0) as f32,
            );
            let samples = if a.get(3).as_f64() == Some(1.0) {
                right
            } else {
                left
            };
            Ok(js_sys::Float32Array::from(samples.as_slice()).into())
        });
        global_set("__comandosRenderCue", &hook)?;
    }
    Ok(())
}
