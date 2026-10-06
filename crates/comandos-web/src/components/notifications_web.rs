use super::super::web_support::*;
use comandos_web_dom::{bridge::global_set, port::*};
use comandos_web_view::notifications as view;
use js_sys::{Array, Map, Set};
use serde_json::{Value, json};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};
use wasm_bindgen::JsValue;
fn rows(v: &JsValue) -> Vec<JsValue> {
    if v.is_null() || v.is_undefined() {
        Vec::new()
    } else {
        Array::from(v).iter().collect()
    }
}
fn arr(v: Vec<JsValue>) -> JsValue {
    v.into_iter().collect::<Array>().into()
}
fn clone(v: &JsValue) -> JsValue {
    call(&global("Object"), "assign", &[object(), v.clone()]).unwrap_or_else(|_| object())
}
fn has(s: &JsValue, id: &JsValue) -> bool {
    call(s, "has", std::slice::from_ref(id))
        .map(|v| truthy(&v))
        .unwrap_or(false)
}
fn default(v: &JsValue, key: &str, fallback: JsValue) -> JsValue {
    let val = get(v, key);
    if truthy(&val) { val } else { fallback }
}
fn norm(p: &JsValue) -> JsValue {
    let out = from_json(&view::normalize(&to_json(p))).unwrap_or_else(|_| object());
    let volume = number(&get(p, "volume"));
    let _ = set(
        &out,
        "volume",
        &if volume.is_finite() {
            volume.clamp(0., 1.)
        } else {
            0.6
        }
        .into(),
    );
    let _ = set(&out, "muted", &truthy(&get(p, "muted")).into());
    out
}
fn category(n: &JsValue) -> String {
    view::category(&to_json(n))
}
fn rank(n: &JsValue) -> usize {
    view::CATEGORIES
        .iter()
        .position(|c| *c == category(n))
        .unwrap_or(6)
}
fn timer(opts: &JsValue, key: &str, global_key: &str, args: &[JsValue]) -> JsValue {
    let f = get(opts, key);
    let f = if f.is_function() {
        f
    } else {
        global(global_key)
    };
    invoke(&f, args).unwrap_or(JsValue::NULL)
}
fn encode(v: &JsValue) -> String {
    invoke(&global("encodeURIComponent"), std::slice::from_ref(v))
        .map(|v| string(&v))
        .unwrap_or_default()
}
fn rel(ms: JsValue, at: f64) -> String {
    let m = number(&ms);
    if !truthy(&ms) || !at.is_finite() || !m.is_finite() {
        return String::new();
    }
    let result = view::rel_time(m, at);
    if !result.is_empty() || m == 0. || at == 0. {
        return result;
    }
    let d = js_sys::Date::new(&ms);
    format!("{:02}/{:02}", d.get_date(), d.get_month() + 1)
}
fn rendering(v: &JsValue) -> Value {
    let mut out = to_json(v);
    view::put(&mut out, "pending", to_json(&arr(rows(&get(v, "pending")))));
    let describe = get(v, "describe");
    let at = number(&get(v, "now"));
    let mut notices = Vec::new();
    for n in rows(&get(v, "notices")) {
        let mut row = to_json(&n);
        if describe.is_function() {
            let origin = invoke(&describe, std::slice::from_ref(&n))
                .map(|v| string(&v))
                .unwrap_or_default()
                .into();
            view::put(&mut row, "_origin", origin);
        }
        view::put(&mut row, "_time", rel(get(&n, "occurredAtMs"), at).into());
        notices.push(row);
    }
    view::put(&mut out, "notices", json!(notices));
    out
}
struct Controller {
    opts: JsValue,
    state: JsValue,
    by_id: Map,
    after: Cell<f64>,
    inflight: RefCell<JsValue>,
    watch: RefCell<JsValue>,
    watch_rev: RefCell<String>,
    watch_cancel: RefCell<JsValue>,
    watch_abort: RefCell<JsValue>,
    float_timer: RefCell<JsValue>,
    float_token: Cell<usize>,
    sound_busy: Cell<bool>,
    pending_open: RefCell<JsValue>,
    stopped: Cell<bool>,
}
impl Controller {
    fn emit(&self) {
        let cb = get(&self.opts, "onChange");
        if cb.is_function() {
            let _ = invoke(&cb, std::slice::from_ref(&self.state));
        }
    }
    fn now(&self) -> f64 {
        invoke(&get(&self.opts, "now"), &[])
            .map(|v| number(&v))
            .unwrap_or_else(|_| js_sys::Date::now())
    }
    fn notices(&self) -> Vec<JsValue> {
        rows(&get(&self.state, "notices"))
    }
    fn quiet(&self) -> bool {
        truthy(&get(&self.state, "loaded"))
            && !truthy(&get(&self.state, "error"))
            && !truthy(&get(&self.state, "unavailable"))
            && !truthy(&get(&self.state, "opened"))
            && string(&get(&self.state, "filter")) == "all"
            && !self.notices().iter().any(|n| {
                has(&get(&self.state, "pending"), &get(n, "eventId"))
                    || (!truthy(&get(n, "read"))
                        && ["attention", "error"].contains(&category(n).as_str()))
            })
    }
    fn view(&self) -> Result<JsValue, JsValue> {
        let v = object();
        for k in [
            "notices",
            "pending",
            "badge",
            "filter",
            "float",
            "unavailable",
            "prefs",
            "loaded",
            "error",
        ] {
            set(&v, k, &get(&self.state, k))?;
        }
        set(
            &v,
            "collapsed",
            &(truthy(&get(&self.state, "collapsed")) || self.quiet()).into(),
        )?;
        set(&v, "now", &self.now().into())?;
        set(
            &v,
            "describe",
            &default(&self.opts, "describe", JsValue::NULL),
        )?;
        Ok(v)
    }
    async fn request(&self, method: &str, path: &str, body: JsValue) -> Result<JsValue, JsValue> {
        let cb = get(&self.opts, "transport");
        let args = if body.is_undefined() {
            vec![method.into(), path.into()]
        } else {
            vec![method.into(), path.into(), body]
        };
        wait(invoke(&cb, &args)).await
    }
    fn apply_meta(&self, r: &JsValue) -> Result<(), JsValue> {
        let prefs = get(r, "prefs");
        if truthy(&prefs) {
            set(&self.state, "prefs", &norm(&prefs))?;
        }
        set(
            &self.state,
            "focusActive",
            &truthy(&get(r, "focusActive")).into(),
        )?;
        let pending = get(r, "pending");
        if Array::is_array(&pending) {
            set(&self.state, "pending", &Set::new(&pending).into())?;
        }
        let badge = get(r, "badge");
        if !badge.is_null() && !badge.is_undefined() && number(&badge).is_finite() {
            set(&self.state, "badge", &number(&badge).into())?;
        }
        let next = number(&get(r, "nextAfter"));
        if next.is_finite() {
            self.after.set(self.after.get().max(next));
        }
        Ok(())
    }
    fn ingest(
        &self,
        list: &JsValue,
        live: bool,
        arrivals: &mut Vec<JsValue>,
    ) -> Result<(), JsValue> {
        for raw in rows(list) {
            let id = get(&raw, "eventId");
            if !truthy(&raw) || !truthy(&id) {
                continue;
            }
            if self.by_id.has(&id) {
                call(&global("Object"), "assign", &[self.by_id.get(&id), raw])?;
                continue;
            }
            let n = clone(&raw);
            self.by_id.set(&id, &n);
            call(
                &get(&self.state, "notices"),
                "push",
                std::slice::from_ref(&n),
            )?;
            let seq = number(&get(&n, "sequence"));
            if seq.is_finite() {
                self.after.set(self.after.get().max(seq));
            }
            if live && !truthy(&get(&n, "read")) {
                arrivals.push(n);
            }
        }
        Ok(())
    }
    fn trim(&self) -> Result<(), JsValue> {
        let mut list = self.notices();
        list.sort_by(|a, b| {
            number(&default(b, "sequence", 0.into())).total_cmp(&number(&default(
                a,
                "sequence",
                0.into(),
            )))
        });
        while list.len() > 500 {
            let Some(n) = list.pop() else { break };
            let id = get(&n, "eventId");
            if has(&get(&self.state, "pending"), &id)
                || rows(&get(&get(&self.state, "float"), "eventIds")).contains(&id)
            {
                list.push(n);
                break;
            }
            self.by_id.delete(&id);
        }
        let notices = get(&self.state, "notices");
        call(&notices, "splice", &[0.into(), get(&notices, "length")])?;
        for n in list {
            call(&notices, "push", &[n])?;
        }
        Ok(())
    }
    fn clear_float(&self) {
        if let Ok(mut timer_id) = self.float_timer.try_borrow_mut()
            && !timer_id.is_null()
        {
            timer(
                &self.opts,
                "clearTimer",
                "clearTimeout",
                std::slice::from_ref(&timer_id),
            );
            *timer_id = JsValue::NULL;
        }
    }
    fn arm_float(self: &Rc<Self>, ms: JsValue) {
        self.clear_float();
        let float = get(&self.state, "float");
        if !truthy(&float) || truthy(&get(&float, "persistent")) {
            return;
        }
        let token = get(&float, "token");
        let c = self.clone();
        let callback = function(move |_| {
            if let Ok(mut timer) = c.float_timer.try_borrow_mut() {
                *timer = JsValue::NULL;
            }
            if !c.stopped.get() && get(&get(&c.state, "float"), "token") == token {
                let _ = set(&c.state, "float", &JsValue::NULL);
                c.emit();
            }
            Ok(JsValue::UNDEFINED)
        });
        let millis = number(&ms);
        let duration = if millis.is_finite() && millis != 0. {
            millis
        } else {
            number(&default(&get(&self.state, "prefs"), "floatMs", 6000.into()))
        };
        let id = timer(
            &self.opts,
            "setTimer",
            "setTimeout",
            &[callback, duration.max(1000.).into()],
        );
        if let Ok(mut t) = self.float_timer.try_borrow_mut() {
            *t = id;
        }
    }
    fn maybe_float(self: &Rc<Self>, n: &JsValue) -> Result<(), JsValue> {
        let config = get(n, "float");
        if !truthy(&config) || !truthy(&get(&config, "show")) {
            return Ok(());
        }
        let ms = get(&config, "ms");
        let persistent = ms.is_null() || ms.is_undefined();
        let current = get(&self.state, "float");
        let group = get(n, "group");
        if truthy(&current) && truthy(&group) && get(&current, "group") == group {
            let ids = get(&current, "eventIds");
            let id = get(n, "eventId");
            if !rows(&ids).contains(&id) {
                call(&ids, "push", &[id])?;
            }
            set(
                &current,
                "persistent",
                &(truthy(&get(&current, "persistent")) || persistent).into(),
            )?;
            self.arm_float(ms);
            return Ok(());
        }
        if truthy(&current) && truthy(&get(&current, "persistent")) && !persistent {
            return Ok(());
        }
        self.float_token.set(self.float_token.get() + 1);
        let f = object();
        set(&f, "token", &(self.float_token.get() as f64).into())?;
        set(&f, "eventIds", &Array::of1(&get(n, "eventId")).into())?;
        set(
            &f,
            "group",
            &if truthy(&group) { group } else { JsValue::NULL },
        )?;
        set(&f, "persistent", &persistent.into())?;
        set(&self.state, "float", &f)?;
        self.arm_float(ms);
        Ok(())
    }
    async fn sound(self: &Rc<Self>, arrivals: Vec<JsValue>) {
        if self.stopped.get() || self.sound_busy.get() {
            return;
        }
        let prefs = get(&self.state, "prefs");
        if truthy(&get(&prefs, "muted")) {
            return;
        }
        let visible = get(&self.opts, "isVisible");
        if visible.is_function() && !truthy(&invoke(&visible, &[]).unwrap_or(JsValue::FALSE)) {
            return;
        }
        let sounds = get(&self.opts, "sounds");
        if !get(&sounds, "play").is_function()
            || !get(&sounds, "isReady").is_function()
            || !truthy(&call(&sounds, "isReady", &[]).unwrap_or(JsValue::FALSE))
        {
            return;
        }
        let mut candidates: Vec<_> = arrivals
            .into_iter()
            .filter(|n| {
                !truthy(&get(n, "read"))
                    && string(&get(&get(&prefs, "modes"), &category(n))) == "sound"
            })
            .collect();
        candidates.sort_by(|a, b| {
            rank(a)
                .cmp(&rank(b))
                .then_with(|| number(&get(b, "sequence")).total_cmp(&number(&get(a, "sequence"))))
        });
        let Some(pick) = candidates.first() else {
            return;
        };
        self.sound_busy.set(true);
        let body = object();
        let _ = set(&body, "eventId", &get(pick, "eventId"));
        let _ = set(
            &body,
            "deviceId",
            &default(&self.opts, "deviceId", "".into()),
        );
        if let Ok(r) = self.request("POST", "/notices/sound", body).await
            && !self.stopped.get()
            && get(&r, "play") == JsValue::TRUE
        {
            let cat = category(pick);
            let cue = default(
                &r,
                "cue",
                view::TYPES
                    .iter()
                    .find(|(c, _, _)| *c == cat)
                    .map(|(_, _, cue)| (*cue).into())
                    .unwrap_or("attention".into()),
            );
            let options = object();
            let _ = set(&options, "eventId", &get(pick, "eventId"));
            let _ = set(&options, "volume", &get(&prefs, "volume"));
            let _ = call(&sounds, "play", &[cue, options]);
        }
        self.sound_busy.set(false);
    }
    fn poll(self: &Rc<Self>) -> JsValue {
        if let Ok(p) = self.inflight.try_borrow()
            && truthy(&p)
        {
            return p.clone();
        }
        let c = self.clone();
        let job = promise(async move {
            let result = c.poll_inner().await;
            if let Ok(mut p) = c.inflight.try_borrow_mut() {
                *p = JsValue::NULL;
            }
            result
        });
        if let Ok(mut p) = self.inflight.try_borrow_mut() {
            *p = job.clone();
        }
        job
    }
    async fn poll_inner(self: &Rc<Self>) -> Result<JsValue, JsValue> {
        if self.stopped.get() {
            return Ok(JsValue::UNDEFINED);
        }
        let first = !truthy(&get(&self.state, "loaded"));
        let mut arrivals = Vec::new();
        for page in 0..20 {
            let limit = if first || page > 0 { 200 } else { 50 };
            let path = format!(
                "/notices?after={}&limit={limit}&deviceId={}",
                encode(&self.after.get().into()),
                encode(&default(&self.opts, "deviceId", "".into()))
            );
            let result = self.request("GET", &path, JsValue::UNDEFINED).await;
            let r = match result {
                Ok(r) => r,
                Err(e) => {
                    set(
                        &self.state,
                        "error",
                        &default(&e, "message", "error".into()),
                    )?;
                    self.emit();
                    return Ok(JsValue::UNDEFINED);
                }
            };
            if self.stopped.get() {
                return Ok(JsValue::UNDEFINED);
            }
            self.apply_meta(&r)?;
            let list = get(&r, "notices");
            let count = rows(&list).len();
            self.ingest(&list, !first && page == 0 && count < limit, &mut arrivals)?;
            if count < limit {
                break;
            }
        }
        set(&self.state, "error", &JsValue::NULL)?;
        set(&self.state, "loaded", &true.into())?;
        self.trim()?;
        arrivals
            .sort_by(|a, b| number(&get(a, "sequence")).total_cmp(&number(&get(b, "sequence"))));
        for n in &arrivals {
            self.maybe_float(n)?;
        }
        self.emit();
        let pending = self
            .pending_open
            .try_borrow()
            .map(|v| v.clone())
            .unwrap_or(JsValue::NULL);
        if truthy(&pending) && self.by_id.has(&pending) {
            if let Ok(mut p) = self.pending_open.try_borrow_mut() {
                *p = JsValue::NULL;
            }
            self.open(pending).await?;
        }
        if !arrivals.is_empty() {
            self.sound(arrivals).await;
        }
        Ok(JsValue::UNDEFINED)
    }
    fn hide_float(&self, id: &JsValue) {
        if rows(&get(&get(&self.state, "float"), "eventIds")).contains(id) {
            self.clear_float();
            let _ = set(&self.state, "float", &JsValue::NULL);
        }
    }
    fn badge(self: &Rc<Self>) {
        let c = self.clone();
        wasm_bindgen_futures::spawn_local(async move {
            if let Ok(r) = c.request("GET", "/notifs/count", JsValue::UNDEFINED).await {
                let count = number(&get(&r, "count"));
                if count.is_finite() && !c.stopped.get() {
                    let _ = set(&c.state, "badge", &count.into());
                    c.emit();
                }
            }
        });
    }
    async fn read(self: &Rc<Self>, ids: Vec<JsValue>) -> Result<JsValue, JsValue> {
        let wanted: Vec<_> = ids
            .into_iter()
            .filter(|id| self.by_id.has(id) && !truthy(&get(&self.by_id.get(id), "read")))
            .collect();
        if wanted.is_empty() || !get(&self.opts, "transport").is_function() {
            return Ok(JsValue::UNDEFINED);
        }
        for id in &wanted {
            set(&self.by_id.get(id), "read", &true.into())?;
        }
        let badge = get(&self.state, "badge");
        if let Some(badge) = badge.as_f64() {
            let delta = wanted
                .iter()
                .filter(|id| !has(&get(&self.state, "pending"), id))
                .count();
            set(&self.state, "badge", &(badge - delta as f64).max(0.).into())?;
        }
        self.emit();
        let body = object();
        set(&body, "eventIds", &arr(wanted.clone()))?;
        match self.request("POST", "/notices/read", body).await {
            Ok(r) => {
                let done = get(&r, "read");
                let done = if truthy(&done) {
                    rows(&done)
                } else {
                    wanted.clone()
                };
                for id in &wanted {
                    if !done.contains(id) {
                        set(&self.by_id.get(id), "read", &false.into())?;
                    }
                }
            }
            Err(e) => {
                for id in wanted {
                    set(&self.by_id.get(&id), "read", &false.into())?;
                }
                set(
                    &self.state,
                    "error",
                    &default(&e, "message", "error".into()),
                )?;
            }
        }
        self.emit();
        self.badge();
        Ok(JsValue::UNDEFINED)
    }
    async fn read_all(self: &Rc<Self>, project: JsValue) -> Result<JsValue, JsValue> {
        let hit: Vec<_> = self
            .notices()
            .into_iter()
            .filter(|n| {
                !truthy(&get(n, "read"))
                    && (project.is_null() || project.is_undefined() || get(n, "project") == project)
            })
            .collect();
        if !get(&self.opts, "transport").is_function() {
            return Ok(JsValue::UNDEFINED);
        }
        for n in &hit {
            set(n, "read", &true.into())?;
        }
        let before = get(&self.state, "badge");
        let project = if project.is_undefined() {
            JsValue::NULL
        } else {
            project
        };
        if project.is_null() && before.as_f64().is_some() {
            set(
                &self.state,
                "badge",
                &get(&get(&self.state, "pending"), "size"),
            )?;
        }
        self.emit();
        let body = object();
        set(&body, "all", &true.into())?;
        set(&body, "project", &project)?;
        match self.request("POST", "/notices/read", body).await {
            Ok(r) => {
                let ids = get(&r, "read");
                if Array::is_array(&ids) {
                    for id in rows(&ids) {
                        if self.by_id.has(&id) {
                            set(&self.by_id.get(&id), "read", &true.into())?;
                        }
                    }
                }
            }
            Err(e) => {
                for n in hit {
                    set(&n, "read", &false.into())?;
                }
                set(&self.state, "badge", &before)?;
                set(
                    &self.state,
                    "error",
                    &default(&e, "message", "error".into()),
                )?;
            }
        }
        self.emit();
        self.badge();
        Ok(JsValue::UNDEFINED)
    }
    async fn open(self: &Rc<Self>, id: JsValue) -> Result<JsValue, JsValue> {
        if !self.by_id.has(&id) {
            if let Ok(mut p) = self.pending_open.try_borrow_mut() {
                *p = id;
            }
            return Ok(false.into());
        }
        let n = self.by_id.get(&id);
        let news = view::is_news(&to_json(&n));
        if !news {
            let session = get(&n, "sessionKey");
            let live = invoke(
                &get(&self.opts, "isSessionLive"),
                std::slice::from_ref(&session),
            )
            .unwrap_or(JsValue::FALSE);
            if !truthy(&session) || !truthy(&live) {
                set(&self.state, "unavailable", &id)?;
                set(&self.state, "collapsed", &false.into())?;
                self.emit();
                return Ok(false.into());
            }
        }
        set(&self.state, "unavailable", &JsValue::NULL)?;
        self.hide_float(&id);
        self.emit();
        let callback = get(&self.opts, if news { "openNews" } else { "openSource" });
        if callback.is_function() {
            invoke(&callback, &[n])?;
        }
        self.read(vec![id]).await?;
        Ok(true.into())
    }
    fn apply_watch(self: &Rc<Self>, r: &JsValue) -> Result<(), JsValue> {
        if !r.is_object() {
            return Ok(());
        }
        if let Some(rev) = get(r, "rev").as_string()
            && let Ok(mut s) = self.watch_rev.try_borrow_mut()
        {
            *s = rev;
        }
        let badge = get(r, "badge");
        if !badge.is_null() && !badge.is_undefined() && number(&badge).is_finite() {
            set(&self.state, "badge", &number(&badge).into())?;
        }
        let pending = get(r, "pending");
        if Array::is_array(&pending) {
            set(&self.state, "pending", &Set::new(&pending).into())?;
        }
        let unread = get(r, "unread");
        if Array::is_array(&unread) {
            let ids = rows(&unread);
            for n in self.notices() {
                set(&n, "read", &(!ids.contains(&get(&n, "eventId"))).into())?;
            }
        }
        self.emit();
        if number(&get(r, "latest")) > self.after.get() {
            self.poll();
        }
        Ok(())
    }
    fn watch_once(self: &Rc<Self>, seconds: JsValue) -> JsValue {
        if let Ok(p) = self.watch.try_borrow()
            && truthy(&p)
        {
            return p.clone();
        }
        if !get(&self.opts, "transport").is_function() {
            return js_sys::Promise::resolve(&JsValue::NULL).into();
        }
        let c = self.clone();
        let rev = self
            .watch_rev
            .try_borrow()
            .map(|s| s.clone())
            .unwrap_or_default();
        let path = format!(
            "/notices/watch?rev={}&wait={}",
            encode(&rev.into()),
            if seconds.is_undefined() {
                "25".into()
            } else {
                string(&seconds)
            }
        );
        let abort = call(
            &global("Reflect"),
            "construct",
            &[global("AbortController"), Array::new().into()],
        )
        .unwrap_or(JsValue::NULL);
        if let Ok(mut slot) = self.watch_abort.try_borrow_mut() {
            *slot = abort.clone();
        }
        let options = object();
        let _ = set(&options, "signal", &get(&abort, "signal"));
        let response = invoke(
            &get(&self.opts, "transport"),
            &["GET".into(), path.into(), JsValue::UNDEFINED, options],
        );
        let response = match response {
            Ok(p) => js_sys::Promise::resolve(&p),
            Err(e) => js_sys::Promise::reject(&e),
        };
        let owner = self.clone();
        let cancelled = js_sys::Promise::new(&mut move |resolve, _| {
            if let Ok(mut slot) = owner.watch_cancel.try_borrow_mut() {
                *slot = resolve.into();
            }
        });
        let race = js_sys::Promise::race(&Array::of2(&response.into(), &cancelled.into()));
        let p = promise(async move {
            let result = wait(Ok(race.into())).await;
            if let Ok(mut w) = c.watch.try_borrow_mut() {
                *w = JsValue::NULL;
            }
            let r = result?;
            if !c.stopped.get() {
                c.apply_watch(&r)?;
            }
            Ok(r)
        });
        if let Ok(mut w) = self.watch.try_borrow_mut() {
            *w = p.clone();
        }
        p
    }
    fn dispose(&self) {
        self.stopped.set(true);
        self.clear_float();
        if let Ok(abort) = self.watch_abort.try_borrow() {
            let _ = call(&abort, "abort", &[]);
        }
        if let Ok(mut cancel) = self.watch_cancel.try_borrow_mut() {
            let _ = invoke(&cancel, &[JsValue::NULL]);
            *cancel = JsValue::NULL;
        }
    }
}
fn controller(opts: JsValue) -> Result<(JsValue, Rc<Controller>), JsValue> {
    let state = from_json(
        &json!({"notices":[],"prefs":view::defaults(),"focusActive":false,"loaded":false,"error":null,"float":null,"unavailable":null,"filter":"all","collapsed":truthy(&get(&opts,"collapsed")),"opened":false}),
    )?;
    set(&state, "pending", &Set::new(&JsValue::UNDEFINED).into())?;
    let c = Rc::new(Controller {
        opts,
        state,
        by_id: Map::new(),
        after: Cell::new(0.),
        inflight: RefCell::new(JsValue::NULL),
        watch: RefCell::new(JsValue::NULL),
        watch_rev: RefCell::new(String::new()),
        watch_cancel: RefCell::new(JsValue::NULL),
        watch_abort: RefCell::new(JsValue::NULL),
        float_timer: RefCell::new(JsValue::NULL),
        float_token: Cell::new(0),
        sound_busy: Cell::new(false),
        pending_open: RefCell::new(JsValue::NULL),
        stopped: Cell::new(false),
    });
    let out = object();
    set(&out, "state", &c.state)?;
    let u = c.clone();
    method(&out, "view", move |_| u.view())?;
    let u = c.clone();
    method(&out, "poll", move |_| Ok(u.poll()))?;
    let u = c.clone();
    method(&out, "watchOnce", move |a| Ok(u.watch_once(a.get(0))))?;
    let u = c.clone();
    method(&out, "applyWatch", move |a| {
        u.apply_watch(&a.get(0))?;
        Ok(JsValue::UNDEFINED)
    })?;
    let u = c.clone();
    method(&out, "dismissFloat", move |_| {
        u.clear_float();
        set(&u.state, "float", &JsValue::NULL)?;
        u.emit();
        Ok(JsValue::UNDEFINED)
    })?;
    let u = c.clone();
    method(&out, "open", move |a| {
        let c = u.clone();
        let id = a.get(0);
        Ok(promise(async move { c.open(id).await }))
    })?;
    let u = c.clone();
    method(&out, "openFloat", move |_| {
        let ids = rows(&get(&get(&u.state, "float"), "eventIds"));
        let id = ids.iter().rev().find(|id| u.by_id.has(id)).cloned();
        let c = u.clone();
        Ok(promise(async move {
            if let Some(id) = id {
                c.open(id).await
            } else {
                Ok(false.into())
            }
        }))
    })?;
    let u = c.clone();
    method(&out, "requestOpen", move |a| {
        let id = a.get(0);
        if truthy(&id) {
            if truthy(&get(&u.state, "loaded")) && u.by_id.has(&id) {
                let c = u.clone();
                wasm_bindgen_futures::spawn_local(async move {
                    let _ = c.open(id).await;
                });
            } else if let Ok(mut p) = u.pending_open.try_borrow_mut() {
                *p = id;
            }
        }
        Ok(JsValue::UNDEFINED)
    })?;
    let u = c.clone();
    method(&out, "markRead", move |a| {
        let c = u.clone();
        let ids = rows(&a.get(0));
        Ok(promise(async move { c.read(ids).await }))
    })?;
    let u = c.clone();
    method(&out, "markAllRead", move |a| {
        let c = u.clone();
        let p = a.get(0);
        Ok(promise(async move { c.read_all(p).await }))
    })?;
    let u = c.clone();
    method(&out, "markGroupRead", move |a| {
        let key = string(&a.get(0));
        let groups = view::groups(
            u.notices().iter().map(to_json).collect(),
            "all",
            &view::array(&to_json(&arr(rows(&get(&u.state, "pending"))))),
        );
        let group = groups.into_iter().find(|g| g["key"] == key);
        let c = u.clone();
        Ok(promise(async move {
            if let Some(g) = group {
                if view::truth(view::at(&g, "project")) {
                    c.read_all(from_json(view::at(&g, "project"))?).await
                } else {
                    let ids = view::array(view::at(&g, "notices"))
                        .iter()
                        .filter(|n| !view::truth(&n["read"]))
                        .filter_map(|n| from_json(&n["eventId"]).ok())
                        .collect();
                    c.read(ids).await
                }
            } else {
                Ok(JsValue::UNDEFINED)
            }
        }))
    })?;
    let u = c.clone();
    method(&out, "setPrefs", move |a| {
        let partial = a.get(0);
        let c = u.clone();
        Ok(promise(async move {
            if !get(&c.opts, "transport").is_function() {
                return Ok(get(&c.state, "prefs"));
            }
            let r = c.request("POST", "/notices/prefs", partial.clone()).await?;
            let src = if truthy(&get(&r, "modes")) {
                r
            } else {
                call(
                    &global("Object"),
                    "assign",
                    &[object(), get(&c.state, "prefs"), partial],
                )?
            };
            let prefs = norm(&src);
            set(&c.state, "prefs", &prefs)?;
            c.emit();
            Ok(prefs)
        }))
    })?;
    let u = c.clone();
    method(&out, "setFilter", move |a| {
        let f = string(&a.get(0));
        set(
            &u.state,
            "filter",
            &if view::FILTERS.iter().any(|(id, _)| *id == f) {
                f
            } else {
                "all".into()
            }
            .into(),
        )?;
        u.emit();
        Ok(JsValue::UNDEFINED)
    })?;
    let u = c.clone();
    method(&out, "setCollapsed", move |a| {
        let collapsed = truthy(&a.get(0));
        set(&u.state, "collapsed", &collapsed.into())?;
        if collapsed {
            set(&u.state, "opened", &false.into())?;
        }
        u.emit();
        Ok(JsValue::UNDEFINED)
    })?;
    let u = c.clone();
    method(&out, "toggle", move |_| {
        let open = truthy(&get(&u.state, "collapsed")) || u.quiet();
        set(&u.state, "collapsed", &(!open).into())?;
        set(&u.state, "opened", &open.into())?;
        u.emit();
        Ok((!open).into())
    })?;
    let u = c.clone();
    method(&out, "closeUnavailable", move |_| {
        set(&u.state, "unavailable", &JsValue::NULL)?;
        u.emit();
        Ok(JsValue::UNDEFINED)
    })?;
    let u = c.clone();
    method(&out, "unreadCount", move |_| {
        Ok((u
            .notices()
            .iter()
            .filter(|n| !truthy(&get(n, "read")))
            .count() as f64)
            .into())
    })?;
    let u = c.clone();
    method(&out, "pendingCount", move |_| {
        Ok(get(&get(&u.state, "pending"), "size"))
    })?;
    let u = c.clone();
    method(&out, "dispose", move |_| {
        u.dispose();
        Ok(JsValue::UNDEFINED)
    })?;
    Ok((out, c))
}
#[path = "notifications_mount.rs"]
mod browser;
thread_local! {static INSTANCE:RefCell<JsValue>=const{RefCell::new(JsValue::NULL)};}
pub fn mount() -> Result<(), JsValue> {
    let api = object();
    for (k, v) in [
        ("GROUP_NEWS", json!("__news__")),
        ("GROUP_GENERAL", json!("__general__")),
        ("DEFAULT_PREFS", view::defaults()),
        ("CATEGORIES", json!(view::CATEGORIES)),
        ("FILTERS", json!(view::FILTERS)),
        ("TYPES", json!(view::TYPES)),
        ("DEFAULT_HEIGHT", json!(260)),
    ] {
        set(&api, k, &from_json(&v)?)?;
    }
    method(&api, "isNews", |a| {
        Ok(view::is_news(&to_json(&a.get(0))).into())
    })?;
    method(&api, "groupKeyOf", |a| {
        Ok(view::group_key(&to_json(&a.get(0))).into())
    })?;
    method(&api, "matchesFilter", |a| {
        Ok(view::matches(
            &to_json(&a.get(0)),
            &string(&a.get(1)),
            &view::array(&to_json(&arr(rows(&a.get(2))))),
        )
        .into())
    })?;
    method(&api, "groupNotices", |a| {
        let options = a.get(1);
        let filter = string(&default(&options, "filter", "all".into()));
        let ids = view::array(&to_json(&arr(rows(&get(&options, "pending")))));
        let mut originals = rows(&a.get(0));
        originals.sort_by(|a, b| {
            number(&default(b, "sequence", 0.into())).total_cmp(&number(&default(
                a,
                "sequence",
                0.into(),
            )))
        });
        let groups = from_json(&json!(view::groups(
            originals.iter().map(to_json).collect(),
            &filter,
            &ids
        )))?;
        for group in rows(&groups) {
            let key = string(&get(&group, "key"));
            let notices = originals
                .iter()
                .filter(|n| {
                    let model = to_json(n);
                    view::group_key(&model) == key && view::matches(&model, &filter, &ids)
                })
                .cloned()
                .collect();
            set(&group, "notices", &arr(notices))?;
        }
        Ok(groups)
    })?;
    method(&api, "normalizePrefs", |a| Ok(norm(&a.get(0))))?;
    method(&api, "relTime", |a| {
        Ok(rel(a.get(0), number(&a.get(1))).into())
    })?;
    method(&api, "clampHeight", |a| {
        Ok(view::clamp_height(number(&a.get(0)), number(&a.get(1))).into())
    })?;
    method(&api, "renderStrip", |a| {
        Ok(view::render_strip(&rendering(&a.get(0))).into())
    })?;
    method(&api, "renderFloat", |a| {
        Ok(view::render_float(&rendering(&a.get(0))).into())
    })?;
    method(&api, "renderSettings", |a| {
        Ok(view::render_settings(&to_json(&a.get(0))).into())
    })?;
    method(&api, "floatSummary", |a| {
        Ok(view::float_summary(&view::array(&to_json(&a.get(0)))).into())
    })?;
    method(&api, "createController", |a| {
        controller(a.get(0)).map(|(out, _)| out)
    })?;
    method(&api, "createPresence", |a| browser::presence(a.get(0)))?;
    method(&api, "mount", |a| browser::mount(a.get(0)))?;
    method(&api, "install", |a| {
        if !truthy(&doc()) {
            return Ok(JsValue::NULL);
        }
        let opts = clone(&a.get(0));
        if get(&opts, "sounds").is_undefined() {
            set(
                &opts,
                "sounds",
                &default(&global("window"), "uiSounds", JsValue::NULL),
            )?;
        }
        if !get(&opts, "transport").is_function() {
            let callback = get(&opts, "api");
            let f = function(move |args| {
                let method = string(&args.get(0));
                let path = args.get(1);
                if method == "GET" && string(&path).starts_with("/notices/watch?") {
                    Ok(watch_fetch(string(&path), get(&args.get(3), "signal")))
                } else if method == "GET" {
                    invoke(&callback, &[path])
                } else {
                    invoke(
                        &callback,
                        &[
                            path,
                            if truthy(&args.get(2)) {
                                args.get(2)
                            } else {
                                object()
                            },
                        ],
                    )
                }
            });
            set(&opts, "transport", &f)?;
        }
        let inst = browser::mount(opts)?;
        INSTANCE.with(|slot| {
            if let Ok(mut s) = slot.try_borrow_mut() {
                *s = inst.clone();
            }
        });
        Ok(inst)
    })?;
    getter(&api, "instance", || {
        INSTANCE.with(|slot| {
            slot.try_borrow()
                .map(|v| v.clone())
                .unwrap_or(JsValue::NULL)
        })
    })?;
    global_set("ComandosNotices", &api)
}

fn watch_fetch(path: String, signal: JsValue) -> JsValue {
    promise(async move {
        let opts = object();
        set(&opts, "signal", &signal)?;
        let headers = object();
        if let Ok(token) = invoke(&global("authToken"), &[])
            && truthy(&token)
        {
            set(&headers, "X-Comandos-Token", &token)?;
        }
        set(&opts, "headers", &headers)?;
        let r = wait(call(&global("window"), "fetch", &[path.into(), opts])).await?;
        let data = wait(call(&r, "json", &[]))
            .await
            .unwrap_or_else(|_| object());
        if !truthy(&get(&r, "ok")) {
            let message = default(
                &data,
                "error",
                default(
                    &data,
                    "message",
                    default(&r, "statusText", "La acción no se completó".into()),
                ),
            );
            return Err(js_sys::Error::new(&string(&message)).into());
        }
        Ok(data)
    })
}
