use crate::web_support::*;
use comandos_web_dom::port::{
    from_utf16_json as from_json, to_utf16_json as to_json, utf16_get as get, utf16_set as set,
    utf16_string as string,
};
use comandos_web_dom::{bridge::global_set, port::*};
use comandos_web_view::news as view;
use js_sys::{Array, Map, Promise, Set};
use serde_json::{Value, json};
use std::{
    cell::{Cell, RefCell},
    collections::{HashMap, VecDeque},
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
fn assign(v: &JsValue) -> JsValue {
    call(&global("Object"), "assign", &[object(), v.clone()]).unwrap_or_else(|_| object())
}
fn encoded(v: &JsValue) -> Result<String, JsValue> {
    invoke(&global("encodeURIComponent"), std::slice::from_ref(v)).map(|v| string(&v))
}
fn error(message: &str) -> JsValue {
    js_sys::Error::new(message).into()
}
fn err_text(e: &JsValue) -> String {
    let m = get(e, "message");
    if truthy(&m) { string(&m) } else { string(e) }
}
fn construct(name: &str, args: &[JsValue]) -> Result<JsValue, JsValue> {
    call(
        &global("Reflect"),
        "construct",
        &[global(name), args.iter().cloned().collect::<Array>().into()],
    )
}
fn token_headers() -> JsValue {
    let h = object();
    if let Ok(token) = call(&global("localStorage"), "getItem", &["cc_token".into()])
        && truthy(&token)
    {
        let _ = set(&h, "X-Comandos-Token", &token);
    }
    h
}
fn read_store(key: &str, fallback: JsValue) -> JsValue {
    call(&global("localStorage"), "getItem", &[key.into()])
        .ok()
        .filter(|v| !v.is_null())
        .and_then(|v| js_sys::JSON::parse(&comandos_web_dom::port::string(&v)).ok())
        .unwrap_or(fallback)
}
fn write_store(key: &str, v: JsValue) {
    if let Ok(text) = js_sys::JSON::stringify(&v) {
        let _ = call(
            &global("localStorage"),
            "setItem",
            &[key.into(), text.into()],
        );
    }
}
fn html(el: &JsValue, text: &str) {
    let _ = set(el, "innerHTML", &utf16_value(text));
}
fn text(el: &JsValue, value: &str) {
    let _ = set(el, "textContent", &utf16_value(value));
}
fn data(el: &JsValue, key: &str) -> JsValue {
    get(&get(el, "dataset"), key)
}
fn safe_url(text: &str) -> JsValue {
    construct("URL", &[utf16_value(text)]).unwrap_or(JsValue::NULL)
}
fn safe_href(text: &str) -> Option<String> {
    let u = safe_url(text);
    if matches!(string(&get(&u, "protocol")).as_str(), "http:" | "https:")
        && !truthy(&get(&u, "username"))
        && !truthy(&get(&u, "password"))
    {
        Some(string(&get(&u, "href")))
    } else {
        None
    }
}
fn host(text: &str) -> String {
    string(&get(&safe_url(text), "hostname"))
        .trim_start_matches("www.")
        .to_lowercase()
}
fn date(ms: f64, tz: &str) -> String {
    if ms == 0. {
        return String::new();
    }
    let opts=from_json(&json!({"timeZone":if tz.is_empty(){"America/Mexico_City"}else{tz},"day":"numeric","month":"short","hour":"2-digit","minute":"2-digit"})).unwrap_or_else(|_|object());
    let dt = js_sys::Date::new(&ms.into());
    call(
        &get(&global("Intl"), "DateTimeFormat"),
        "call",
        &[JsValue::NULL, "es-MX".into(), opts],
    )
    .and_then(|f| call(&f, "format", &[dt.clone().into()]))
    .map(|v| string(&v))
    .unwrap_or_else(|_| dt.to_iso_string().as_string().unwrap_or_default())
}
fn date_day(ms: f64, tz: &str, long: bool) -> String {
    let opts = if long {
        json!({"timeZone":tz,"day":"numeric","month":"long"})
    } else {
        json!({"timeZone":tz})
    };
    let dt = js_sys::Date::new(&ms.into());
    call(
        &get(&global("Intl"), "DateTimeFormat"),
        "call",
        &[
            JsValue::NULL,
            if long { "es-MX".into() } else { "en-CA".into() },
            from_json(&opts).unwrap_or_else(|_| object()),
        ],
    )
    .and_then(|f| call(&f, "format", &[dt.clone().into()]))
    .map(|v| string(&v))
    .unwrap_or_else(|_| {
        dt.to_iso_string()
            .as_string()
            .unwrap_or_default()
            .chars()
            .take(10)
            .collect()
    })
}
fn groups(notes: &Value, now: f64, tz: &str) -> Value {
    let tz = if tz.is_empty() {
        "America/Mexico_City"
    } else {
        tz
    };
    let today = date_day(now, tz, false);
    let yesterday = date_day(now - 86400000., tz, false);
    let mut out: Vec<Value> = Vec::new();
    for note in view::arr(notes) {
        let ms = view::n(view::at(note, "createdAt"));
        let d = date_day(ms, tz, false);
        let label = if d == today {
            "Hoy".into()
        } else if d == yesterday {
            "Ayer".into()
        } else {
            date_day(ms, tz, true)
        };
        if let Some(last) = out.last_mut()
            && view::field(last, "label") == label
        {
            if let Some(a) = last.get_mut("notes").and_then(Value::as_array_mut) {
                a.push(note.clone());
            }
        } else {
            out.push(json!({"label":label,"notes":[note]}));
        }
    }
    out.into()
}
#[derive(Default)]
struct Owner {
    serial: Cell<u64>,
    paused: Cell<bool>,
    jobs: RefCell<HashMap<u64, (JsValue, JsValue)>>,
}
impl Owner {
    async fn borrowed(
        self: &Rc<Self>,
        result: Result<JsValue, JsValue>,
    ) -> Result<JsValue, JsValue> {
        if self.paused.get() {
            return Err(error("Lectura cancelada"));
        }
        let id = self.serial.get().wrapping_add(1);
        self.serial.set(id);
        let owner = self.clone();
        let cancelled = Promise::new(&mut move |_, reject| {
            if let Ok(mut jobs) = owner.jobs.try_borrow_mut() {
                jobs.insert(id, (JsValue::NULL, reject.into()));
            } else {
                let _ = reject.call1(&JsValue::UNDEFINED, &error("Lectura cancelada"));
            }
        });
        let response = match result {
            Ok(value) => Promise::resolve(&value),
            Err(error) => Promise::reject(&error),
        };
        let result = wait(Ok(Promise::race(&Array::of2(
            &response.into(),
            &cancelled.into(),
        ))
        .into()))
        .await;
        if let Ok(mut jobs) = self.jobs.try_borrow_mut() {
            jobs.remove(&id);
        }
        result
    }
    fn cancel(&self) {
        if let Ok(mut jobs) = self.jobs.try_borrow_mut() {
            for (_, (abort, reject)) in jobs.drain() {
                let _ = call(&abort, "abort", &[]);
                let _ = invoke(&reject, &[error("Lectura cancelada")]);
            }
        }
    }
    async fn run(
        self: &Rc<Self>,
        fetch: &JsValue,
        path: &str,
        body: JsValue,
        blob: bool,
    ) -> Result<JsValue, JsValue> {
        if self.paused.get() {
            return Err(error("Lectura cancelada"));
        }
        let id = self.serial.get().wrapping_add(1);
        self.serial.set(id);
        let abort = construct("AbortController", &[]).unwrap_or(JsValue::NULL);
        let owner = self.clone();
        let a = abort.clone();
        let cancelled = Promise::new(&mut move |_, reject| {
            if let Ok(mut jobs) = owner.jobs.try_borrow_mut() {
                jobs.insert(id, (a.clone(), reject.into()));
            }
        });
        let opts = object();
        set(&opts, "signal", &get(&abort, "signal"))?;
        let response = if fetch.is_function() && !blob {
            invoke(fetch, &[utf16_value(path), body, opts])
        } else {
            let headers = token_headers();
            set(&opts, "headers", &headers)?;
            if !body.is_undefined() {
                set(&opts, "method", &"POST".into())?;
                set(&headers, "Content-Type", &"application/json".into())?;
                set(&opts, "body", &js_sys::JSON::stringify(&body)?.into())?;
            }
            let p = call(&js_sys::global(), "fetch", &[utf16_value(path), opts]);
            Ok(promise(async move {
                let r = wait(p).await?;
                let parsed = if blob {
                    wait(call(&r, "blob", &[])).await?
                } else {
                    wait(call(&r, "json", &[]))
                        .await
                        .unwrap_or_else(|_| object())
                };
                if !truthy(&get(&r, "ok")) {
                    return Err(error(&{
                        let m = get(&parsed, "error");
                        if truthy(&m) {
                            comandos_web_dom::port::string(&m)
                        } else {
                            comandos_web_dom::port::string(&get(&r, "statusText"))
                        }
                    }));
                }
                Ok(parsed)
            }))
        };
        let response = match response {
            Ok(p) => Promise::resolve(&p),
            Err(e) => Promise::reject(&e),
        };
        let result = wait(Ok(Promise::race(&Array::of2(
            &response.into(),
            &cancelled.into(),
        ))
        .into()))
        .await;
        if let Ok(mut jobs) = self.jobs.try_borrow_mut() {
            jobs.remove(&id);
        }
        result
    }
}
struct Renderer {
    owner: Rc<Owner>,
    fetch: JsValue,
    purify: JsValue,
    plain: bool,
    cache: RefCell<VecDeque<(u64, String, String)>>,
    pending: RefCell<HashMap<String, JsValue>>,
    epoch: Cell<u64>,
}
fn hash(text: &str) -> u64 {
    text.bytes().fold(14695981039346656037u64, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(1099511628211)
    })
}
impl Renderer {
    fn new(fetch: JsValue, purify: JsValue, plain: bool) -> Rc<Self> {
        Rc::new(Self {
            owner: Rc::new(Owner::default()),
            fetch,
            purify,
            plain,
            cache: RefCell::new(VecDeque::new()),
            pending: RefCell::new(HashMap::new()),
            epoch: Cell::new(0),
        })
    }
    fn cached(&self, text: &str) -> Option<String> {
        self.cache
            .try_borrow()
            .ok()?
            .iter()
            .find(|(h, t, _)| *h == hash(text) && t == text)
            .map(|(_, _, html)| html.clone())
    }
    fn cancel(&self) {
        self.epoch.set(self.epoch.get().wrapping_add(1));
        self.owner.cancel();
        if let Ok(mut p) = self.pending.try_borrow_mut() {
            p.clear();
        }
    }
    fn render(self: &Rc<Self>, text: String) -> JsValue {
        if self.plain {
            return Promise::resolve(&utf16_value(&view::plain(&text))).into();
        }
        if let Some(html) = self.cached(&text) {
            return Promise::resolve(&utf16_value(&html)).into();
        }
        if let Ok(p) = self.pending.try_borrow()
            && let Some(p) = p.get(&text)
        {
            return p.clone();
        }
        let c = self.clone();
        let key = text.clone();
        let epoch = self.epoch.get();
        let pending_key = text.clone();
        let p = promise(async move {
            let result=async{if epoch != c.epoch.get(){return Err(error("Lectura cancelada"))}let body=from_json(&json!({"text":text,"profile":"news"}))?;let response=c.owner.run(&c.fetch,"/web/markdown",body,false).await?;let h=get(&response,"html");if !h.is_string(){return Err(error("Respuesta Markdown inválida"))}let h=if get(&c.purify,"sanitize").is_function(){let cfg=from_json(&json!({"ALLOWED_TAGS":["p","br","strong","em","del","s","a","ul","ol","li","blockquote","code","pre","h1","h2","h3","h4","h5","h6","hr","table","thead","tbody","tr","th","td","div","span"],"ALLOWED_ATTR":["href","target","rel","class","title","start"],"ALLOW_DATA_ATTR":false,"ADD_URI_SAFE_ATTR":["target","rel"]}))?;set(&cfg,"ALLOWED_URI_REGEXP",&construct("RegExp",&["^https?://".into(),"i".into()])?)?;call(&c.purify,"sanitize",&[h,cfg])?}else{h};if epoch!=c.epoch.get(){return Err(error("Lectura cancelada"))}let h=string(&h);if let Ok(mut cache)=c.cache.try_borrow_mut(){cache.push_back((hash(&text),text.clone(),h.clone()));while cache.len()>64{cache.pop_front();}}Ok(utf16_value(&h))}.await;
            if epoch == c.epoch.get()
                && let Ok(mut p) = c.pending.try_borrow_mut()
            {
                p.remove(&key);
            }
            result
        });
        if let Ok(mut pending) = self.pending.try_borrow_mut() {
            pending.insert(pending_key, p.clone());
        }
        p
    }
    fn api(self: &Rc<Self>) -> Result<JsValue, JsValue> {
        let c = self.clone();
        let f = function(move |a| {
            Ok(c.render(if truthy(&a.get(0)) {
                string(&a.get(0))
            } else {
                String::new()
            }))
        });
        let c = self.clone();
        method(&f, "dispose", move |_| {
            c.cancel();
            Ok(JsValue::UNDEFINED)
        })?;
        Ok(f)
    }
}
fn store(fetch: JsValue, owner: Rc<Owner>) -> Result<JsValue, JsValue> {
    let api = object();
    let cache = Rc::new(RefCell::new(VecDeque::<(String, JsValue)>::new()));
    let f = fetch.clone();
    let o = owner.clone();
    method(&api, "list", move |_| {
        let f = f.clone();
        let o = o.clone();
        Ok(promise(async move {
            o.run(&f, "/news/editions", JsValue::UNDEFINED, false).await
        }))
    })?;
    method(&api, "edition", move |a| {
        let id = a.get(0);
        let key = string(&id);
        if !truthy(&a.get(1))
            && let Ok(cache) = cache.try_borrow()
            && let Some((_, value)) = cache.iter().find(|(k, _)| k == &key)
        {
            return Ok(Promise::resolve(value).into());
        }
        let path = format!("/news/edition?id={}", encoded(&id)?);
        let f = fetch.clone();
        let o = owner.clone();
        let cache = cache.clone();
        Ok(promise(async move {
            let value = o.run(&f, &path, JsValue::UNDEFINED, false).await?;
            if view::terminal_status(&string(&get(&get(&value, "edition"), "status")))
                && let Ok(mut cache) = cache.try_borrow_mut()
            {
                cache.retain(|(k, _)| k != &key);
                cache.push_back((key, value.clone()));
                while cache.len() > 64 {
                    cache.pop_front();
                }
            }
            Ok(value)
        }))
    })?;
    Ok(api)
}
struct Reader {
    opts: JsValue,
    doc: JsValue,
    el: JsValue,
    state: JsValue,
    owner: Rc<Owner>,
    renderer: Rc<Renderer>,
    store: JsValue,
    listeners: RefCell<Vec<(JsValue, String, JsValue)>>,
    timers: RefCell<HashMap<String, JsValue>>,
    media: RefCell<HashMap<String, String>>,
    media_pending: RefCell<HashMap<String, JsValue>>,
    raf: RefCell<Vec<JsValue>>,
    generation: Cell<u64>,
    edition_generation: Cell<u64>,
    open_epoch: Cell<u64>,
    stopped: Cell<bool>,
    drag: RefCell<Option<(JsValue, f64)>>,
    busy: RefCell<HashMap<String, u64>>,
    edition_html: RefCell<Option<String>>,
}
impl Reader {
    fn q(&self, sel: &str) -> JsValue {
        utf16_query(&self.el, sel)
    }
    fn st(&self, key: &str) -> JsValue {
        get(&self.state, key)
    }
    fn put(&self, key: &str, v: JsValue) {
        let _ = set(&self.state, key, &v);
    }
    fn fetch(&self) -> JsValue {
        get(&self.opts, "fetchJson")
    }
    fn active(&self, epoch: u64) -> bool {
        !self.stopped.get() && self.open_epoch.get() == epoch
    }
    fn story(&self, id: &JsValue) -> JsValue {
        rows(&get(&self.st("current"), "stories"))
            .into_iter()
            .find(|s| get(s, "id") == *id)
            .unwrap_or(JsValue::NULL)
    }
    fn snapshot(&self) -> Value {
        let out = assign(&self.state);
        for key in ["chat", "sources"] {
            let map = self.st(key);
            let entries = call(&map, "entries", &[]).unwrap_or(JsValue::NULL);
            let obj =
                call(&global("Object"), "fromEntries", &[entries]).unwrap_or_else(|_| object());
            let _ = set(&out, key, &obj);
        }
        let _ = set(&out, "original", &Array::from(&self.st("original")).into());
        to_json(&out)
    }
    fn replace_timer(
        self: &Rc<Self>,
        key: &str,
        ms: f64,
        f: impl Fn(Rc<Self>) -> Result<JsValue, JsValue> + 'static,
    ) {
        self.clear_timer(key);
        let c = self.clone();
        let name = key.to_owned();
        let timer = later(
            function(move |_| {
                if let Ok(mut timers) = c.timers.try_borrow_mut() {
                    timers.remove(&name);
                }
                if c.stopped.get() || truthy(&get(&c.el, "hidden")) {
                    return Ok(JsValue::UNDEFINED);
                }
                f(c.clone())
            }),
            ms,
        );
        if let Ok(mut timers) = self.timers.try_borrow_mut() {
            timers.insert(key.into(), timer);
        }
    }
    fn clear_timer(&self, key: &str) {
        if let Ok(mut timers) = self.timers.try_borrow_mut()
            && let Some(timer) = timers.remove(key)
        {
            cancel(timer)
        }
    }
    fn toast(self: &Rc<Self>, message: &str) {
        let el = self.q(".nr-toast");
        text(&el, message);
        classes(&el, "show", true);
        self.replace_timer("toast", 1800., |c| {
            classes(&c.q(".nr-toast"), "show", false);
            Ok(JsValue::UNDEFINED)
        });
    }
    fn count(&self, key: &str, value: f64) {
        text(
            &self.q(&format!("[data-count=\"{key}\"]")),
            &if value == 0. {
                String::new()
            } else {
                value.to_string()
            },
        );
    }
    fn counts(&self) {
        if truthy(&self.st("allNotes")) {
            self.count("notes", number(&get(&self.st("allNotes"), "total")))
        }
        if truthy(&self.st("saved")) {
            self.count("saved", rows(&self.st("saved")).len() as f64);
        }
    }
    fn sync_counts(&self, id: &JsValue) {
        let story = self.story(id);
        if story.is_null() {
            return;
        }
        let counts = get(&story, "counts");
        let counts = if truthy(&counts) {
            counts
        } else {
            let counts = object();
            let _ = set(&story, "counts", &counts);
            counts
        };
        let messages =
            call(&self.st("chat"), "get", std::slice::from_ref(id)).unwrap_or(JsValue::UNDEFINED);
        if !messages.is_undefined() {
            let _ = set(&counts, "chat", &(rows(&messages).len() as f64).into());
        }
        let notes = self.st("notes");
        if get(&notes, "storyId") == *id {
            let _ = set(
                &counts,
                "notes",
                &(rows(&get(&notes, "notes")).len() as f64).into(),
            );
        }
    }
    fn prefs(&self) {
        style(
            &self.el,
            "--read-size",
            &format!("{}px", number(&self.st("size"))),
        );
        style(
            &self.el,
            "--nr-share",
            &format!("{}%", number(&self.st("share"))),
        );
        if truthy(&get(&self.opts, "embedded")) {
            style(
                &get(&self.doc, "documentElement"),
                "--nr-share",
                &format!("{}%", number(&self.st("share"))),
            );
        }
        attr(
            &self.q(".nr-edition-divider"),
            "aria-valuenow",
            &number(&self.st("share")).round().to_string(),
        );
    }
    fn mark(&self) -> Option<(JsValue, f64)> {
        let rb = self.q(".nr-reader-body");
        let top = number(&get(&call(&rb, "getBoundingClientRect", &[]).ok()?, "top"));
        let mut mark = None;
        for n in all(&rb, ".nr-edition-head, .nr-edition-story") {
            let nt = number(&get(&call(&n, "getBoundingClientRect", &[]).ok()?, "top"));
            if nt <= top + 12. {
                mark = Some((n, nt - top))
            }
        }
        mark
    }
    fn restore(&self, mark: Option<(JsValue, f64)>) {
        if let Some((node, offset)) = mark
            && truthy(&get(&node, "isConnected"))
        {
            let rb = self.q(".nr-reader-body");
            let nt = call(&node, "getBoundingClientRect", &[])
                .map(|v| number(&get(&v, "top")))
                .unwrap_or(0.);
            let top = call(&rb, "getBoundingClientRect", &[])
                .map(|v| number(&get(&v, "top")))
                .unwrap_or(0.);
            let _ = set(
                &rb,
                "scrollTop",
                &(number(&get(&rb, "scrollTop")) + nt - top - offset).into(),
            );
        }
    }
    fn relayout(self: &Rc<Self>, change: impl FnOnce()) {
        let mark = self.mark();
        change();
        self.prefs();
        let c = self.clone();
        let f = function(move |_| {
            if !c.stopped.get() {
                c.restore(mark.clone());
            }
            Ok(JsValue::UNDEFINED)
        });
        if let Ok(id) = call(&global("window"), "requestAnimationFrame", &[f])
            && let Ok(mut raf) = self.raf.try_borrow_mut()
        {
            raf.push(id);
        }
    }
    fn narrow(&self) -> bool {
        let win = get(&self.doc, "defaultView");
        call(
            &if truthy(&win) { win } else { global("window") },
            "matchMedia",
            &["(max-width: 560px)".into()],
        )
        .map(|v| truthy(&get(&v, "matches")))
        .unwrap_or(false)
    }
    fn pane(&self, pane: &str) {
        self.put("mobilePane", pane.into());
        if truthy(&get(&self.opts, "embedded")) {
            classes(
                &get(&self.doc, "body"),
                "nr-peek",
                pane == "terminal" && self.narrow(),
            );
        } else if truthy(&self.st("terminal")) && self.narrow() {
            let stage = self.q(".nr-stage");
            let opts=from_json(&json!({"left":if pane=="reader"{number(&get(&stage,"scrollWidth"))}else{0.},"behavior":"smooth"})).unwrap_or_else(|_|object());
            let _ = call(&stage, "scrollTo", &[opts]);
        }
        for b in all(&self.el, ".nr-mobile-switch button") {
            attr(
                &b,
                "aria-pressed",
                if string(&data(&b, "pane")) == pane {
                    "true"
                } else {
                    "false"
                },
            );
        }
    }
    fn message(&self, message: &str) {
        self.edition_html.replace(None);
        html(
            &self.q(".nr-edition"),
            &format!("<div class=\"nr-state\">{message}</div>"),
        );
    }
    fn listen(
        self: &Rc<Self>,
        el: JsValue,
        event: &str,
        f: impl Fn(Rc<Self>, JsValue) -> Result<JsValue, JsValue> + 'static,
    ) {
        let c = self.clone();
        let callback = function(move |a| {
            if c.stopped.get() {
                Ok(JsValue::UNDEFINED)
            } else {
                f(c.clone(), a.get(0))
            }
        });
        listen(&el, event, callback.clone());
        if let Ok(mut list) = self.listeners.try_borrow_mut() {
            list.push((el, event.into(), callback));
        }
    }
    async fn request(self: &Rc<Self>, path: &str, body: JsValue) -> Result<JsValue, JsValue> {
        self.owner.run(&self.fetch(), path, body, false).await
    }
    fn repaint(self: &Rc<Self>) -> JsValue {
        let generation = self.generation.get().wrapping_add(1);
        self.generation.set(generation);
        let epoch = self.open_epoch.get();
        let c = self.clone();
        promise(async move {
            let snapshot = c.snapshot();
            let mut texts = Vec::new();
            fn collect(v: &Value, out: &mut Vec<String>) {
                match v {
                    Value::Object(o) => {
                        for (k, v) in o {
                            if matches!(k.as_str(), "summary" | "body" | "quote")
                                || (k == "text"
                                    && o.contains_key("role")
                                    && view::field(&Value::Object(o.clone()), "role") != "user")
                            {
                                out.push(view::s(v));
                            } else {
                                collect(v, out)
                            }
                        }
                    }
                    Value::Array(a) => {
                        for v in a {
                            collect(v, out)
                        }
                    }
                    _ => {}
                }
            }
            collect(&snapshot, &mut texts);
            texts.sort();
            texts.dedup();
            let mut rendered = HashMap::new();
            for text in texts {
                let p = c.renderer.render(text.clone());
                match wait(Ok(p)).await {
                    Ok(html) => {
                        rendered.insert(text, string(&html));
                    }
                    Err(e) => {
                        if c.active(epoch) && generation == c.generation.get() {
                            c.toast(&format!("No pude renderizar: {}", err_text(&e)));
                        }
                        return Err(e);
                    }
                }
                if !c.active(epoch) || generation != c.generation.get() {
                    return Ok(JsValue::UNDEFINED);
                }
            }
            if !c.active(epoch) || generation != c.generation.get() {
                return Ok(JsValue::UNDEFINED);
            }
            let lookup = |text: &str| rendered.get(text).cloned().unwrap_or_default();
            let d = |ms, tz: &str| date(ms, tz);
            let group = |notes: &Value| groups(notes, js_sys::Date::now(), "");
            let render = view::Render {
                state: &snapshot,
                markdown: &lookup,
                date: &d,
                href: &safe_href,
                host: &host,
                groups: &group,
            };
            let picker = render.picker();
            html(&c.q(".nr-day"), &picker);
            let _ = set(&c.q(".nr-day"), "hidden", &picker.is_empty().into());
            // Selection changes update existing cards, as the original reader does.
            // Replacing their DOM would lose focus and regenerate tooltip metadata.
            let mut edition_state = snapshot.clone();
            if let Some(state) = edition_state.as_object_mut() {
                state.insert("open".into(), Value::Null);
            }
            if let Some(stories) = edition_state
                .get_mut("current")
                .and_then(|v| v.get_mut("stories"))
                .and_then(Value::as_array_mut)
            {
                for story in stories {
                    if let Some(counts) = story.get_mut("counts").and_then(Value::as_object_mut) {
                        counts.insert("notes".into(), Value::Null);
                        counts.insert("saved".into(), Value::Null);
                    }
                }
            }
            let edition = view::Render {
                state: &edition_state,
                ..render
            }
            .edition();
            if !edition.is_empty() {
                let changed = c.edition_html.borrow().as_ref() != Some(&edition);
                if changed {
                    html(&c.q(".nr-edition"), &edition);
                    c.edition_html.replace(Some(edition));
                }
                let open = c.st("open");
                let selected = (!open.is_null() && !open.is_undefined()).then(|| string(&open));
                let story_counts: HashMap<_, _> = rows(&get(&c.st("current"), "stories"))
                    .into_iter()
                    .map(|story| (string(&get(&story, "id")), get(&story, "counts")))
                    .collect();
                for card in all(&c.el, ".nr-edition-story") {
                    let id = string(&data(&card, "story"));
                    let on = selected.as_ref() == Some(&id);
                    classes(&card, "on", on);
                    let button = utf16_query(&card, ".nr-open-btn");
                    classes(&button, "on", on);
                    let label = if on { "Abierta →" } else { "Abrir" };
                    if string(&get(&button, "textContent")) != label {
                        text(&button, label);
                    }
                    if let Some(counts) = story_counts.get(&id) {
                        let saved = truthy(&get(counts, "saved"));
                        let save = utf16_query(&card, ".nr-save");
                        attr(&save, "aria-pressed", if saved { "true" } else { "false" });
                        let label = if saved { "Guardada" } else { "Guardar" };
                        if string(&get(&save, "textContent")) != label {
                            text(&save, label);
                        }
                        let kicker = utf16_query(&card, ".nr-kicker");
                        let badge = utf16_query(&kicker, ".nr-nn");
                        let count = number(&get(counts, "notes"));
                        if count == 0. {
                            let _ = call(&badge, "remove", &[]);
                        } else {
                            let label =
                                format!("{count} nota{}", if count == 1. { "" } else { "s" });
                            if badge.is_null() || badge.is_undefined() {
                                let badge = call(&c.doc, "createElement", &["span".into()])?;
                                attr(&badge, "class", "nr-nn");
                                text(&badge, &label);
                                let _ = call(&kicker, "appendChild", &[badge]);
                            } else if string(&get(&badge, "textContent")) != label {
                                text(&badge, &label);
                            }
                        }
                    }
                }
            }
            attr(
                &c.q(".nr-saved-btn"),
                "aria-pressed",
                if string(&c.st("view")) == "saved" {
                    "true"
                } else {
                    "false"
                },
            );
            let panel = render.panel();
            let panel_el = c.q(".nr-panel");
            let _ = set(&panel_el, "hidden", &panel.is_empty().into());
            classes(&c.q(".nr-reader"), "nr-panel-open", !panel.is_empty());
            if !panel.is_empty() {
                html(&panel_el, &panel);
                if string(&c.st("tab")) == "chat" && truthy(&c.st("stickBottom")) {
                    let pb = c.q(".nr-panel .nr-pb");
                    let _ = set(&pb, "scrollTop", &get(&pb, "scrollHeight"));
                    c.put("stickBottom", false.into());
                }
                c.load_media();
            }
            c.counts();
            c.ensure_panel_data();
            Ok(JsValue::UNDEFINED)
        })
    }
    fn paint(self: &Rc<Self>) {
        let p = self.repaint();
        let _ = call(&p, "catch", &[function(|_| Ok(JsValue::UNDEFINED))]);
    }
    fn ensure_panel_data(self: &Rc<Self>) {
        let open = self.st("open");
        if open.is_null() || open.is_undefined() {
            return;
        }
        let tab = string(&self.st("tab"));
        let story = self.story(&open);
        if tab == "fuentes" && !story.is_null() {
            let sources = rows(&get(&story, "sources"));
            let current = sources
                .iter()
                .find(|s| get(s, "id") == self.st("sourceId"))
                .or_else(|| sources.iter().find(|s| truthy(&get(s, "captured"))))
                .or_else(|| sources.first());
            if let Some(source) = current {
                let id = get(source, "id");
                self.put("sourceId", id.clone());
                let loaded = call(&self.st("sources"), "has", std::slice::from_ref(&id))
                    .map(|v| truthy(&v))
                    .unwrap_or(false);
                if !loaded {
                    self.load_source(id, false);
                }
            }
        } else if tab == "chat" && !story.is_null() {
            if !call(&self.st("chat"), "has", std::slice::from_ref(&open))
                .map(|v| truthy(&v))
                .unwrap_or(false)
            {
                self.load_chat(open);
            }
        } else if tab == "notas" || string(&open) == "notes" {
            let needs = (!story.is_null() && get(&self.st("notes"), "storyId") != open)
                || ((story.is_null() || string(&self.st("scope")) == "todas")
                    && !truthy(&get(&self.st("allNotes"), "notes")));
            if needs {
                let c = self.clone();
                wasm_bindgen_futures::spawn_local(async move {
                    let _ = c
                        .refresh_notes(if story.is_null() { JsValue::NULL } else { open })
                        .await;
                    c.paint();
                });
            }
        }
    }
    fn begin(&self, key: &str) -> bool {
        if let Ok(mut busy) = self.busy.try_borrow_mut() {
            if busy.contains_key(key) {
                return false;
            }
            busy.insert(key.into(), self.open_epoch.get());
            true
        } else {
            false
        }
    }
    fn finish(&self, key: &str, epoch: u64) {
        if let Ok(mut busy) = self.busy.try_borrow_mut()
            && busy.get(key) == Some(&epoch)
        {
            busy.remove(key);
        }
    }
    async fn saved(self: &Rc<Self>) {
        let epoch = self.open_epoch.get();
        if !self.begin("saved") {
            return;
        }
        let r = self.request("/news/saved", JsValue::UNDEFINED).await;
        if self.active(epoch) {
            match r {
                Ok(v) => self.put("saved", get(&v, "stories")),
                Err(e) => {
                    if !truthy(&self.st("saved")) {
                        self.put("saved", Array::new().into());
                    }
                    self.toast(&format!("No pude leer las guardadas: {}", err_text(&e)));
                }
            }
            self.counts();
        }
        self.finish("saved", epoch);
    }
    async fn refresh_notes(self: &Rc<Self>, id: JsValue) -> Result<(), JsValue> {
        let epoch = self.open_epoch.get();
        let key = format!("notes:{}:{}", string(&id), string(&self.st("query")));
        if !self.begin(&key) {
            return Ok(());
        }
        let result = async {
            if truthy(&id) {
                let path = format!("/news/notes?story={}", encoded(&id)?);
                let data = self.request(&path, JsValue::UNDEFINED).await?;
                if !self.active(epoch) {
                    return Ok(());
                }
                let notes = object();
                set(&notes, "storyId", &id)?;
                set(&notes, "notes", &get(&data, "notes"))?;
                self.put("notes", notes);
                let all = if truthy(&self.st("allNotes")) {
                    assign(&self.st("allNotes"))
                } else {
                    let o = object();
                    set(&o, "notes", &JsValue::NULL)?;
                    o
                };
                set(&all, "total", &get(&data, "total"))?;
                self.put("allNotes", all);
                self.sync_counts(&id);
            }
            if string(&self.st("scope")) == "todas" || !truthy(&id) {
                let path = format!("/news/notes?q={}", encoded(&self.st("query"))?);
                let data = self.request(&path, JsValue::UNDEFINED).await?;
                if self.active(epoch) {
                    let all = object();
                    set(&all, "notes", &get(&data, "notes"))?;
                    set(&all, "total", &get(&data, "total"))?;
                    self.put("allNotes", all);
                }
            }
            Ok::<(), JsValue>(())
        }
        .await;
        if let Err(e) = &result
            && self.active(epoch)
        {
            if truthy(&id) && get(&self.st("notes"), "storyId") != id {
                let notes = object();
                let _ = set(&notes, "storyId", &id);
                let _ = set(&notes, "notes", &Array::new().into());
                self.put("notes", notes);
            }
            if !truthy(&get(&self.st("allNotes"), "notes")) {
                self.put(
                    "allNotes",
                    from_json(&json!({"notes":[],"total":0})).unwrap_or_else(|_| object()),
                );
            }
            self.toast(&format!("No pude leer las notas: {}", err_text(e)));
        }
        self.finish(&key, epoch);
        self.counts();
        result
    }
    fn load_source(self: &Rc<Self>, id: JsValue, _quiet: bool) {
        let key = format!("source:{}", string(&id));
        if !self.begin(&key) {
            return;
        }
        let epoch = self.open_epoch.get();
        let c = self.clone();
        wasm_bindgen_futures::spawn_local(async move {
            let result = match encoded(&id) {
                Ok(encoded) => {
                    c.request(&format!("/news/source?id={encoded}"), JsValue::UNDEFINED)
                        .await
                }
                Err(e) => Err(e),
            };
            c.finish(&key, epoch);
            if !c.active(epoch) {
                return;
            }
            let src = match result {
                Ok(v) => v,
                Err(e) => from_json(&json!({"error":err_text(&e)})).unwrap_or_else(|_| object()),
            };
            let _ = call(&c.st("sources"), "set", &[id.clone(), src.clone()]);
            let selected = string(&c.st("tab")) == "fuentes" && c.st("sourceId") == id;
            if selected {
                c.paint();
            }
            if selected {
                c.clear_timer("translate");
            }
            if string(&get(&get(&src, "translation"), "state")) == "running"
                && string(&c.st("tab")) == "fuentes"
                && c.st("sourceId") == id
            {
                c.replace_timer("translate", 3000., move |c| {
                    c.load_source(id.clone(), true);
                    Ok(JsValue::UNDEFINED)
                });
            }
        });
    }
    fn load_chat(self: &Rc<Self>, id: JsValue) {
        let key = format!("chat:{}", string(&id));
        if !self.begin(&key) {
            return;
        }
        let epoch = self.open_epoch.get();
        let c = self.clone();
        wasm_bindgen_futures::spawn_local(async move {
            let result = match encoded(&id) {
                Ok(encoded) => {
                    c.request(&format!("/news/chat?story={encoded}"), JsValue::UNDEFINED)
                        .await
                }
                Err(e) => Err(e),
            };
            c.finish(&key, epoch);
            if !c.active(epoch) {
                return;
            }
            let msgs = match result {
                Ok(v) => get(&v, "messages"),
                Err(e) => {
                    c.toast(&format!("No pude leer el chat: {}", err_text(&e)));
                    Array::new().into()
                }
            };
            let _ = call(&c.st("chat"), "set", &[id.clone(), msgs.clone()]);
            c.sync_counts(&id);
            if c.st("open") == id && string(&c.st("tab")) == "chat" {
                c.paint();
            }
            if c.st("open") == id && string(&c.st("tab")) == "chat" {
                c.clear_timer("chat");
            }
            if rows(&msgs)
                .iter()
                .any(|m| string(&get(m, "state")) == "pending")
                && c.st("open") == id
                && string(&c.st("tab")) == "chat"
            {
                c.replace_timer("chat", 2500., move |c| {
                    c.put("stickBottom", true.into());
                    c.load_chat(id.clone());
                    Ok(JsValue::UNDEFINED)
                });
            }
        });
    }
    fn load_media(self: &Rc<Self>) {
        for img in all(&self.q(".nr-panel"), "img[data-media]") {
            let name = string(&data(&img, "media"));
            if !view::media_name(&name) || truthy(&data(&img, "loaded")) {
                continue;
            }
            let _ = set(&get(&img, "dataset"), "loaded", &"1".into());
            let epoch = self.open_epoch.get();
            let c = self.clone();
            let image = img.clone();
            let callback = function(move |_| {
                let figure = call(&image, "closest", &["figure".into()]).unwrap_or(JsValue::NULL);
                classes(&figure, "nr-fig-missing", true);
                Ok(JsValue::UNDEFINED)
            });
            let once = object();
            let _ = set(&once, "once", &true.into());
            let _ = call(&img, "addEventListener", &["error".into(), callback, once]);
            let img = self.q(&format!("img[data-media=\"{name}\"]"));
            let img_owner = img.clone();
            wasm_bindgen_futures::spawn_local(async move {
                let cached = c
                    .media
                    .try_borrow()
                    .ok()
                    .and_then(|m| m.get(&name).cloned());
                let result = if let Some(url) = cached {
                    Ok(url)
                } else {
                    let pending = c
                        .media_pending
                        .try_borrow()
                        .ok()
                        .and_then(|p| p.get(&name).cloned());
                    let p = if let Some(p) = pending {
                        p
                    } else {
                        let c = c.clone();
                        let name = name.clone();
                        promise(async move {
                            let blob = c
                                .owner
                                .run(
                                    &JsValue::UNDEFINED,
                                    &format!("/news/media/{name}"),
                                    JsValue::UNDEFINED,
                                    true,
                                )
                                .await?;
                            if !c.active(epoch) {
                                return Err(error("Lectura cancelada"));
                            }
                            let url = call(&global("URL"), "createObjectURL", &[blob])?;
                            if let Ok(mut media) = c.media.try_borrow_mut() {
                                media.insert(name, string(&url));
                            }
                            Ok(url)
                        })
                    };
                    if let Ok(mut pending) = c.media_pending.try_borrow_mut() {
                        pending.insert(name.clone(), p.clone());
                    }
                    let r = wait(Ok(p)).await.map(|v| string(&v));
                    if c.active(epoch)
                        && let Ok(mut pending) = c.media_pending.try_borrow_mut()
                    {
                        pending.remove(&name);
                    }
                    r
                };
                if !c.active(epoch) || !truthy(&get(&img_owner, "isConnected")) {
                    return;
                }
                match result {
                    Ok(url) => {
                        let _ = set(&img_owner, "src", &utf16_value(&url));
                    }
                    Err(_) => {
                        let figure = call(&img_owner, "closest", &["figure".into()])
                            .unwrap_or(JsValue::NULL);
                        classes(&figure, "nr-fig-missing", true);
                    }
                }
            });
        }
    }
    async fn edition(self: &Rc<Self>, id: JsValue) -> Result<JsValue, JsValue> {
        let epoch = self.open_epoch.get();
        let selection = self.edition_generation.get().wrapping_add(1);
        self.edition_generation.set(selection);
        let result = wait(call(&self.store, "edition", &[id, true.into()])).await;
        match result {
            Ok(data) => {
                if !self.active(epoch) || selection != self.edition_generation.get() {
                    return Ok(JsValue::UNDEFINED);
                }
                self.put("current", data);
                self.put("view", "edition".into());
                if string(&self.st("open")) != "notes" && self.story(&self.st("open")).is_null() {
                    self.put("open", JsValue::NULL);
                }
                wait(Ok(self.repaint())).await?;
                let _ = set(&self.q(".nr-reader-body"), "scrollTop", &0.into());
                Ok(JsValue::UNDEFINED)
            }
            Err(e) => {
                if self.active(epoch) && selection == self.edition_generation.get() {
                    self.message(&format!(
                        "No se pudo abrir el resumen: {}",
                        comandos_web_view::escape::text(&err_text(&e))
                    ));
                }
                Err(e)
            }
        }
    }
    fn open(self: &Rc<Self>, id: JsValue) -> JsValue {
        if self.stopped.get() {
            return Promise::reject(&error("Lector cerrado")).into();
        }
        let c = self.clone();
        let epoch = c.open_epoch.get();
        c.owner.paused.set(false);
        c.put("opener", get(&c.doc, "activeElement"));
        let _ = set(&c.el, "hidden", &false.into());
        promise(async move {
            if !c.active(epoch) {
                return Err(error("Lectura cancelada"));
            }

            if truthy(&get(&c.opts, "embedded")) {
                classes(&get(&c.doc, "body"), "nr-reading", true);
                if truthy(&read_store("comandos.news.terminal", false.into()))
                    && !truthy(&c.st("terminal"))
                {
                    let _ = wait(Ok(c.terminal(true))).await;
                } else if truthy(&c.st("terminal")) {
                    classes(&get(&c.doc, "body"), "nr-docked", true);
                }
            } else {
                classes(&get(&c.doc, "documentElement"), "nr-open", true);
            }
            let cb = get(&c.opts, "onOpenChange");
            if cb.is_function() {
                invoke(&cb, &[true.into()])?;
            }
            c.prefs();
            let _ = call(
                &c.q(".nr-reader-body"),
                "focus",
                &[from_json(&json!({"preventScroll":true}))?],
            );
            let saved = c.clone();
            wasm_bindgen_futures::spawn_local(async move {
                saved.saved().await;
            });
            let notes = c.clone();
            wasm_bindgen_futures::spawn_local(async move {
                let _ = notes.refresh_notes(JsValue::NULL).await;
            });
            if truthy(&c.st("current")) && !truthy(&id) {
                c.ensure_panel_data();
                return Ok(JsValue::UNDEFINED);
            }
            if !truthy(&c.st("current")) {
                c.message("Cargando resúmenes…");
            }
            let list = match wait(call(&c.store, "list", &[])).await {
                Ok(v) => v,
                Err(e) => {
                    if c.active(epoch) {
                        c.message(&format!("No se pudo leer la lista de resúmenes: {}. Revisa la conexión con ComandOS.",comandos_web_view::escape::text(&err_text(&e))));
                    }
                    return Err(e);
                }
            };
            if !c.active(epoch) {
                return Ok(JsValue::UNDEFINED);
            }
            c.put("list", list.clone());
            let target = if truthy(&id) {
                id
            } else {
                get(&list, "latest")
            };
            if !truthy(&target) {
                c.paint();
                c.message(&if truthy(&get(&list,"configured")){"Aún no hay resúmenes. El próximo llega a su hora programada.".into()}else{format!("Resúmenes {}. No se muestran noticias inventadas; configura <code>news-editions.json</code> para activar los tres resúmenes diarios.",comandos_web_view::escape::text(&{let r=get(&list,"reason");if truthy(&r){string(&r)}else{"sin configurar".into()}}))});
                return Ok(JsValue::UNDEFINED);
            }
            c.edition(target).await
        })
    }
    fn cancel_work(&self) {
        self.owner.paused.set(true);
        self.open_epoch.set(self.open_epoch.get().wrapping_add(1));
        self.generation.set(self.generation.get().wrapping_add(1));
        self.owner.cancel();
        self.renderer.cancel();
        if let Ok(mut timers) = self.timers.try_borrow_mut() {
            for (_, timer) in timers.drain() {
                cancel(timer)
            }
        }
        if let Ok(mut busy) = self.busy.try_borrow_mut() {
            busy.clear();
        }
        if let Ok(mut pending) = self.media_pending.try_borrow_mut() {
            pending.clear();
        }
        if let Ok(mut raf) = self.raf.try_borrow_mut() {
            for id in raf.drain(..) {
                let _ = call(&global("window"), "cancelAnimationFrame", &[id]);
            }
        }
    }
    fn close(self: &Rc<Self>) -> Result<(), JsValue> {
        if truthy(&get(&self.opts, "desktop")) && get(&self.opts, "onClose").is_function() {
            self.cancel_work();
            invoke(&get(&self.opts, "onClose"), &[])?;
            return Ok(());
        }
        set(&self.el, "hidden", &true.into())?;
        self.cancel_work();
        classes(&get(&self.doc, "documentElement"), "nr-open", false);
        for class in ["nr-reading", "nr-docked", "nr-peek"] {
            classes(&get(&self.doc, "body"), class, false);
        }
        let cb = get(&self.opts, "onOpenChange");
        if cb.is_function() {
            invoke(&cb, &[false.into()])?;
        }
        let opener = self.st("opener");
        if get(&opener, "focus").is_function() {
            call(
                &opener,
                "focus",
                &[from_json(&json!({"preventScroll":true}))?],
            )?;
        }
        Ok(())
    }
    fn dispose(self: &Rc<Self>) -> Result<(), JsValue> {
        self.close()?;
        self.stopped.set(true);
        if let Ok(mut list) = self.listeners.try_borrow_mut() {
            for (el, event, f) in list.drain(..) {
                let _ = call(&el, "removeEventListener", &[event.into(), f]);
            }
        }
        if let Ok(mut media) = self.media.try_borrow_mut() {
            for (_, url) in media.drain() {
                let _ = call(&global("URL"), "revokeObjectURL", &[utf16_value(&url)]);
            }
        }
        let _ = call(&self.el, "remove", &[]);
        Ok(())
    }
    fn open_story(self: &Rc<Self>, id: JsValue, tab: JsValue) {
        if self.story(&id).is_null() {
            return;
        }
        let same = self.st("open") == id;
        if !same {
            for key in ["sourceId", "notes", "editing", "confirmDelete"] {
                self.put(key, JsValue::NULL);
            }
            self.put("scope", "esta".into());
            self.clear_timer("chat");
            self.clear_timer("translate");
        }
        self.put("open", id);
        self.put(
            "tab",
            if truthy(&tab) {
                tab
            } else if same {
                self.st("tab")
            } else {
                "resumen".into()
            },
        );
        self.paint();
    }
    fn close_panel(self: &Rc<Self>) {
        self.put("open", JsValue::NULL);
        self.clear_timer("chat");
        self.clear_timer("translate");
        self.paint();
    }
    async fn goto(
        self: &Rc<Self>,
        edition: JsValue,
        id: JsValue,
        tab: JsValue,
    ) -> Result<JsValue, JsValue> {
        self.put("view", "edition".into());
        if get(&get(&self.st("current"), "edition"), "id") != edition {
            self.edition(edition).await?;
        }
        self.open_story(number(&id).into(), tab);
        Ok(JsValue::UNDEFINED)
    }
    fn terminal(self: &Rc<Self>, on: bool) -> JsValue {
        let c = self.clone();
        let epoch = c.open_epoch.get();
        promise(async move {
            if c.open_epoch.get() != epoch || c.stopped.get() {
                return Err(error("Lectura cancelada"));
            }
            c.relayout(|| {
                c.put("terminal", on.into());
                let embedded = truthy(&get(&c.opts, "embedded"));
                let desktop = truthy(&get(&c.opts, "desktop"));
                classes(&c.el, "nr-with-terminal", on && !embedded && !desktop);
                classes(&c.el, "nr-docked", on);
                if embedded {
                    classes(&get(&c.doc, "body"), "nr-docked", on);
                }
                let _ = set(
                    &c.q(".nr-terminal"),
                    "hidden",
                    &(!on || embedded || desktop).into(),
                );
                let _ = set(&c.q(".nr-edition-divider"), "hidden", &(!on).into());
                let t = c.q(".nr-terminal-toggle");
                attr(&t, "aria-pressed", if on { "true" } else { "false" });
                text(
                    &t,
                    if on {
                        "Ocultar terminal"
                    } else {
                        "Ver terminal"
                    },
                );
                classes(&t, "on", on);
            });
            if truthy(&get(&c.opts, "desktop")) {
                let cb = get(&c.opts, "externalTerminal");
                if cb.is_function() {
                    invoke(&cb, &[on.into()])?;
                }
                return Ok(JsValue::UNDEFINED);
            }
            if truthy(&get(&c.opts, "embedded")) {
                write_store("comandos.news.terminal", on.into());
                c.pane(if on && c.narrow() {
                    "terminal"
                } else {
                    "reader"
                });
                return Ok(JsValue::UNDEFINED);
            }
            if on {
                c.pane("terminal");
                if !truthy(&c.st("terminalMounted")) {
                    c.put("terminalMounted", true.into());
                    let host = c.q(".nr-terminal-host");
                    let cb = get(&c.opts, "mountTerminal");
                    let epoch = c.open_epoch.get();
                    let result = if cb.is_function() {
                        c.owner
                            .borrowed(invoke(&cb, std::slice::from_ref(&host)))
                            .await
                    } else {
                        c.default_terminal(host.clone(), epoch).await
                    };
                    if let Err(e) = result {
                        c.put("terminalMounted", false.into());
                        if c.active(epoch) {
                            html(
                                &host,
                                &format!(
                                    "<p class=\"nr-message\">{}</p>",
                                    comandos_web_view::escape::text(&err_text(&e))
                                ),
                            );
                        }
                    }
                }
            } else {
                c.pane("reader");
            }
            Ok(JsValue::UNDEFINED)
        })
    }
    async fn default_terminal(
        self: &Rc<Self>,
        host: JsValue,
        epoch: u64,
    ) -> Result<JsValue, JsValue> {
        let session = invoke(&global("sidebarActiveTab"), &[])
            .map(|v| get(&v, "session"))
            .unwrap_or(JsValue::UNDEFINED);
        if !truthy(&session)
            || !global("resolveTermBase").is_function()
            || !global("webtermAccessToken").is_function()
        {
            html(
                &host,
                "<p class=\"nr-message\">Abre una terminal en el tablero para verla aquí.</p>",
            );
            return Ok(JsValue::UNDEFINED);
        }
        let base = self
            .owner
            .borrowed(invoke(&global("resolveTermBase"), &[]))
            .await?;
        if !self.active(epoch) {
            return Ok(JsValue::UNDEFINED);
        }
        if !truthy(&base) {
            html(
                &host,
                "<p class=\"nr-message\">La terminal web no está disponible en esta vista.</p>",
            );
            return Ok(JsValue::UNDEFINED);
        }
        let token = self
            .owner
            .borrowed(invoke(&global("webtermAccessToken"), &[]))
            .await?;
        if !self.active(epoch) {
            return Ok(JsValue::UNDEFINED);
        }
        let frame = call(&self.doc, "createElement", &["iframe".into()])?;
        set(&frame, "className", &"nr-term-frame".into())?;
        set(
            &frame,
            "title",
            &utf16_value(&format!("Terminal {}", string(&session))),
        )?;
        let state = invoke(&global("__comandosNewsTerminal"), &[]).unwrap_or_else(|_| object());
        let primary = base == get(&state, "base");
        let theme = get(&state, "theme");
        let btn = data(&get(&self.doc, "documentElement"), "btnStyle");
        let path = if primary {
            format!(
                "{}/?auth={}&arg={}&theme={}&btn={}",
                string(&base),
                encoded(&token)?,
                encoded(&session)?,
                encoded(&if theme.is_undefined() {
                    "".into()
                } else {
                    theme
                })?,
                encoded(&if truthy(&btn) { btn } else { "sutil".into() })?
            )
        } else {
            format!(
                "{}/?arg={}&arg={}",
                string(&base),
                encoded(&token)?,
                encoded(&session)?
            )
        };
        set(&frame, "src", &utf16_value(&path))?;
        call(&host, "replaceChildren", &[frame])
    }
    async fn send(self: &Rc<Self>, message: String) -> Result<JsValue, JsValue> {
        let msg = message.trim();
        let story = self.story(&self.st("open"));
        if story.is_null() || msg.is_empty() {
            return Ok(JsValue::UNDEFINED);
        }
        let id = get(&story, "id");
        let epoch = self.open_epoch.get();
        self.put("chatDraft", "".into());
        if let Some(note) = view::note_command(msg) {
            let result = self
                .request(
                    "/news/notes",
                    from_json(&json!({"action":"add","storyId":to_json(&id),"text":note}))?,
                )
                .await;
            if !self.active(epoch) {
                return Ok(JsValue::UNDEFINED);
            }
            match result {
                Ok(_) => {
                    self.toast("Nota guardada");
                    self.put("notes", JsValue::NULL);
                    self.put("allNotes", JsValue::NULL);
                    let _ = self.refresh_notes(id).await;
                }
                Err(e) => self.toast(&format!("No se guardó la nota: {}", err_text(&e))),
            }
            self.paint();
            return Ok(JsValue::UNDEFINED);
        }
        let result = self
            .request(
                "/news/chat",
                from_json(&json!({"storyId":to_json(&id),"message":msg}))?,
            )
            .await;
        if !self.active(epoch) {
            return Ok(JsValue::UNDEFINED);
        }
        match result {
            Ok(v) => {
                call(&self.st("chat"), "set", &[id.clone(), get(&v, "messages")])?;
                self.sync_counts(&id);
                if self.st("open") != id || string(&self.st("tab")) != "chat" {
                    return Ok(JsValue::UNDEFINED);
                }
                self.put("stickBottom", true.into());
                self.paint();
                self.replace_timer("chat", 2500., move |c| {
                    c.load_chat(id.clone());
                    Ok(JsValue::UNDEFINED)
                });
            }
            Err(e) => {
                if self.st("open") != id {
                    return Ok(JsValue::UNDEFINED);
                }
                self.put("chatDraft", utf16_value(msg));
                self.toast(&format!("No se envió: {}", err_text(&e)));
                self.paint();
            }
        }
        Ok(JsValue::UNDEFINED)
    }
    async fn reload_notes(self: &Rc<Self>) {
        let id = if string(&self.st("open")) == "notes" {
            JsValue::NULL
        } else {
            self.st("open")
        };
        if truthy(&self.st("allNotes")) {
            let _ = set(&self.st("allNotes"), "notes", &JsValue::NULL);
        }
        let _ = self.refresh_notes(id.clone()).await;
        if truthy(&id) && string(&self.st("scope")) == "todas" {
            let _ = self.refresh_notes(JsValue::NULL).await;
        }
        self.paint();
    }
    async fn add_note(self: &Rc<Self>) -> Result<JsValue, JsValue> {
        let draft = string(&self.st("noteDraft"));
        let text = draft.trim();
        if text.is_empty() || string(&self.st("open")) == "notes" {
            return Ok(JsValue::UNDEFINED);
        }
        let epoch = self.open_epoch.get();
        let r = self
            .request(
                "/news/notes",
                from_json(
                    &json!({"action":"add","storyId":to_json(&self.st("open")),"text":text}),
                )?,
            )
            .await;
        if !self.active(epoch) {
            return Ok(JsValue::UNDEFINED);
        }
        match r {
            Ok(_) => {
                self.put("noteDraft", "".into());
                self.reload_notes().await;
                self.toast("Nota guardada");
            }
            Err(e) => self.toast(&format!("No se guardó: {}", err_text(&e))),
        }
        Ok(JsValue::UNDEFINED)
    }
    async fn copy(self: &Rc<Self>, text: String) -> Result<JsValue, JsValue> {
        match wait(call(
            &get(&global("navigator"), "clipboard"),
            "writeText",
            &[utf16_value(&text)],
        ))
        .await
        {
            Ok(_) => self.toast("Copiado"),
            Err(_) => self.toast("No se pudo copiar"),
        }
        Ok(JsValue::UNDEFINED)
    }
    fn notes_markdown(&self) -> String {
        let notes = to_json(&get(&self.st("allNotes"), "notes"));
        view::arr(&groups(&notes, js_sys::Date::now(), ""))
            .iter()
            .map(|g| {
                format!(
                    "## {}\n\n{}",
                    view::field(g, "label"),
                    view::arr(view::at(g, "notes"))
                        .iter()
                        .map(|n| {
                            let quote = view::field(n, "quote");
                            let text = view::field(n, "text");
                            let cite = view::field(n, "cite");
                            format!(
                                "### {}\n{}{}{}",
                                view::field(n, "storyTitle"),
                                if quote.is_empty() {
                                    String::new()
                                } else {
                                    format!(
                                        "{}\n\n",
                                        quote
                                            .split('\n')
                                            .map(|l| format!("> {l}"))
                                            .collect::<Vec<_>>()
                                            .join("\n")
                                    )
                                },
                                if text.is_empty() {
                                    String::new()
                                } else {
                                    format!("{text}\n")
                                },
                                if cite.is_empty() {
                                    String::new()
                                } else {
                                    format!("\n_{cite}_\n")
                                }
                            )
                        })
                        .collect::<Vec<_>>()
                        .join("\n")
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
    async fn save_story(self: &Rc<Self>, button: JsValue) -> Result<JsValue, JsValue> {
        let id = number(&data(&button, "save"));
        let on = call(&button, "getAttribute", &["aria-pressed".into()])
            .map(|v| string(&v) != "true")
            .unwrap_or(true);
        let epoch = self.open_epoch.get();
        match self
            .request("/news/saved", from_json(&json!({"storyId":id,"saved":on}))?)
            .await
        {
            Ok(_) => {
                if !self.active(epoch) {
                    return Ok(JsValue::UNDEFINED);
                }
                let story = self.story(&id.into());
                if !story.is_null() {
                    let counts = get(&story, "counts");
                    let counts = if truthy(&counts) { counts } else { object() };
                    set(&counts, "saved", &on.into())?;
                    set(&story, "counts", &counts)?;
                }
                for b in all(&self.el, &format!("[data-save=\"{id}\"]")) {
                    attr(&b, "aria-pressed", if on { "true" } else { "false" });
                    text(&b, if on { "Guardada" } else { "Guardar" });
                }
                self.saved().await;
                if string(&self.st("view")) == "saved" {
                    self.paint();
                }
            }
            Err(e) => {
                if self.active(epoch) {
                    self.toast(&format!("No se guardó: {}", err_text(&e)));
                }
            }
        }
        Ok(JsValue::UNDEFINED)
    }
    async fn panel_click(self: &Rc<Self>, target: JsValue) -> Result<JsValue, JsValue> {
        let nearest = |sel: &str| call(&target, "closest", &[sel.into()]).unwrap_or(JsValue::NULL);
        if truthy(&nearest(".nr-panel-close, .nr-panel-back")) {
            self.close_panel();
            return Ok(JsValue::UNDEFINED);
        }
        let b = nearest("[data-tab]");
        if truthy(&b) {
            self.put("tab", data(&b, "tab"));
            self.put("editing", JsValue::NULL);
            self.put("confirmDelete", JsValue::NULL);
            self.clear_timer("chat");
            self.clear_timer("translate");
            self.paint();
            return Ok(JsValue::UNDEFINED);
        }
        let b = nearest("[data-source]");
        if truthy(&b) {
            self.put("sourceId", number(&data(&b, "source")).into());
            self.clear_timer("translate");
            self.paint();
            return Ok(JsValue::UNDEFINED);
        }
        let b = nearest("[data-source-lang]");
        if truthy(&b) {
            let id = number(&data(&b, "sourceLang")).into();
            call(
                &self.st("original"),
                if string(&data(&b, "lang")) == "orig" {
                    "add"
                } else {
                    "delete"
                },
                &[id],
            )?;
            self.paint();
            return Ok(JsValue::UNDEFINED);
        }
        let b = nearest("[data-translate]");
        if truthy(&b) {
            let id = number(&data(&b, "translate"));
            set(&b, "disabled", &true.into())?;
            let epoch = self.open_epoch.get();
            let r = self
                .request("/news/translate", from_json(&json!({"sourceId":id}))?)
                .await;
            if !self.active(epoch) {
                return Ok(JsValue::UNDEFINED);
            }
            match r {
                Ok(r) => {
                    let src = call(&self.st("sources"), "get", &[id.into()])?;
                    if truthy(&src) {
                        set(&src, "translation", &get(&r, "translation"))?;
                    }
                    call(&self.st("original"), "delete", &[id.into()])?;
                    self.toast("Traduciendo con IA; queda guardada");
                }
                Err(e) => self.toast(&format!("No se pudo traducir: {}", err_text(&e))),
            }
            self.load_source(id.into(), true);
            return Ok(JsValue::UNDEFINED);
        }
        let b = nearest("[data-ask]");
        if truthy(&b) {
            return self.send(string(&data(&b, "ask"))).await;
        }
        let b = nearest("[data-note-chat]");
        if truthy(&b) {
            let epoch = self.open_epoch.get();
            let result = self
                .request(
                    "/news/chat/note",
                    from_json(&json!({"chatId":number(&data(&b,"noteChat"))}))?,
                )
                .await;
            if !self.active(epoch) {
                return Ok(JsValue::UNDEFINED);
            }
            match result {
                Ok(r) => {
                    self.toast(if truthy(&get(&r, "noted")) {
                        "Guardada como nota · pestaña Notas"
                    } else {
                        "Quitada de tus notas"
                    });
                    self.put("notes", JsValue::NULL);
                    if truthy(&self.st("allNotes")) {
                        set(&self.st("allNotes"), "notes", &JsValue::NULL)?;
                    }
                    call(&self.st("chat"), "delete", &[self.st("open")])?;
                    self.load_chat(self.st("open"));
                    let _ = self.refresh_notes(self.st("open")).await;
                    self.paint();
                }
                Err(e) => self.toast(&format!("No se guardó: {}", err_text(&e))),
            }
            return Ok(JsValue::UNDEFINED);
        }
        let b = nearest("[data-copy-chat]");
        if truthy(&b) {
            let msgs = call(&self.st("chat"), "get", &[self.st("open")])?;
            if let Some(m) = rows(&msgs)
                .iter()
                .find(|m| number(&get(m, "id")) == number(&data(&b, "copyChat")))
            {
                return self.copy(string(&get(m, "text"))).await;
            }
            return Ok(JsValue::UNDEFINED);
        }
        let b = nearest("[data-scope]");
        if truthy(&b) {
            self.put("scope", data(&b, "scope"));
            if truthy(&self.st("allNotes")) {
                set(&self.st("allNotes"), "notes", &JsValue::NULL)?;
            }
            self.paint();
            return Ok(JsValue::UNDEFINED);
        }
        if truthy(&nearest("[data-note-add]")) {
            return self.add_note().await;
        }
        let b = nearest("[data-note-edit]");
        if truthy(&b) {
            self.put("editing", number(&data(&b, "noteEdit")).into());
            self.put("confirmDelete", JsValue::NULL);
            wait(Ok(self.repaint())).await?;
            let _ = call(&self.q(".nr-note-edit"), "focus", &[]);
            return Ok(JsValue::UNDEFINED);
        }
        if truthy(&nearest("[data-note-cancel]")) {
            self.put("editing", JsValue::NULL);
            self.put("confirmDelete", JsValue::NULL);
            self.paint();
            return Ok(JsValue::UNDEFINED);
        }
        let b = nearest("[data-note-save]");
        if truthy(&b) {
            let epoch = self.open_epoch.get();
            let text = string(&get(&self.q(".nr-note-edit"), "value"));
            match self.request("/news/notes",from_json(&json!({"action":"update","noteId":number(&data(&b,"noteSave")),"text":text}))?).await{Ok(_)=>{if self.active(epoch){self.put("editing",JsValue::NULL);self.reload_notes().await;self.toast("Nota actualizada");}},Err(e)=>{if self.active(epoch){self.toast(&format!("No se guardó: {}",err_text(&e)));}}}
            return Ok(JsValue::UNDEFINED);
        }
        let b = nearest("[data-note-ask-delete]");
        if truthy(&b) {
            self.put("confirmDelete", number(&data(&b, "noteAskDelete")).into());
            self.paint();
            return Ok(JsValue::UNDEFINED);
        }
        let b = nearest("[data-note-delete]");
        if truthy(&b) {
            let epoch = self.open_epoch.get();
            match self
                .request(
                    "/news/notes",
                    from_json(&json!({"action":"delete","noteId":number(&data(&b,"noteDelete"))}))?,
                )
                .await
            {
                Ok(_) => {
                    if self.active(epoch) {
                        self.put("confirmDelete", JsValue::NULL);
                        self.reload_notes().await;
                        if string(&self.st("open")) != "notes" {
                            call(&self.st("chat"), "delete", &[self.st("open")])?;
                        }
                        self.toast("Nota borrada");
                    }
                }
                Err(e) => {
                    if self.active(epoch) {
                        self.toast(&format!("No se borró: {}", err_text(&e)));
                    }
                }
            }
            return Ok(JsValue::UNDEFINED);
        }
        let b = nearest("[data-note-copy]");
        if truthy(&b) {
            let mut pool = rows(&get(&self.st("notes"), "notes"));
            pool.extend(rows(&get(&self.st("allNotes"), "notes")));
            if let Some(n) = pool
                .iter()
                .find(|n| number(&get(n, "id")) == number(&data(&b, "noteCopy")))
            {
                let text = ["quote", "text", "cite"]
                    .into_iter()
                    .map(|k| get(n, k))
                    .filter(truthy)
                    .map(|v| string(&v))
                    .collect::<Vec<_>>()
                    .join("\n\n");
                return self.copy(text).await;
            }
            return Ok(JsValue::UNDEFINED);
        }
        if truthy(&nearest("[data-notes-export]")) {
            return self.copy(self.notes_markdown()).await;
        }
        let b = nearest("[data-goto]");
        if truthy(&b) {
            return self
                .goto(data(&b, "goto"), data(&b, "gotoStory"), data(&b, "gotoTab"))
                .await;
        }
        Ok(JsValue::UNDEFINED)
    }
    fn observe(self: &Rc<Self>, p: JsValue) {
        let c = self.clone();
        let _ = call(
            &p,
            "catch",
            &[function(move |a| {
                if !c.stopped.get()
                    && !truthy(&get(&c.el, "hidden"))
                    && err_text(&a.get(0)) != "Lectura cancelada"
                {
                    c.toast(&err_text(&a.get(0)));
                }
                Ok(JsValue::UNDEFINED)
            })],
        );
    }
    fn wire(self: &Rc<Self>) {
        self.listen(self.q(".nr-close"), "click", |c, _| {
            c.close()?;
            Ok(JsValue::UNDEFINED)
        });
        self.listen(self.el.clone(), "keydown", |c, e| {
            let target = get(&e, "target");
            if string(&get(&e, "key")) != "Escape"
                || truthy(
                    &call(&target, "closest", &[".nr-terminal".into()]).unwrap_or(JsValue::NULL),
                )
            {
                return Ok(JsValue::UNDEFINED);
            }
            if !truthy(
                &call(&target, "closest", &["textarea, input".into()]).unwrap_or(JsValue::NULL),
            ) {
                if !c.st("open").is_null() {
                    c.close_panel();
                } else {
                    c.close()?;
                }
            }
            Ok(JsValue::UNDEFINED)
        });
        self.listen(self.q(".nr-terminal-toggle"), "click", |c, _| {
            let p = c.terminal(!truthy(&c.st("terminal")));
            c.observe(p);
            Ok(JsValue::UNDEFINED)
        });
        self.listen(self.q(".nr-saved-btn"), "click", |c, _| {
            if string(&c.st("view")) == "saved" {
                c.put("view", "edition".into());
                c.paint();
            } else {
                c.put("view", "saved".into());
                let r = c.clone();
                wasm_bindgen_futures::spawn_local(async move {
                    let epoch = r.open_epoch.get();
                    r.saved().await;
                    if r.active(epoch) {
                        r.paint();
                        let _ = set(&r.q(".nr-reader-body"), "scrollTop", &0.into());
                    }
                });
            }
            Ok(JsValue::UNDEFINED)
        });
        self.listen(self.q(".nr-notes-btn"), "click", |c, _| {
            c.put("scope", "todas".into());
            if !c.st("open").is_null() && string(&c.st("open")) != "notes" {
                c.put("tab", "notas".into());
            } else {
                c.put("open", "notes".into());
                if truthy(&c.st("allNotes")) {
                    set(&c.st("allNotes"), "notes", &JsValue::NULL)?;
                }
            }
            c.paint();
            Ok(JsValue::UNDEFINED)
        });
        self.listen(self.q(".nr-day"), "click", |c, e| {
            let b = call(&get(&e, "target"), "closest", &["[data-edition]".into()])?;
            if truthy(&b) && !truthy(&get(&b, "disabled")) {
                let id = data(&b, "edition");
                let d = c.clone();
                let p = promise(async move { d.edition(id).await });
                c.observe(p);
            }
            Ok(JsValue::UNDEFINED)
        });
        for b in all(&self.el, ".nr-mobile-switch button") {
            let button = b.clone();
            self.listen(b, "click", move |c, _| {
                let pane = string(&data(&button, "pane"));
                if pane == "terminal" && !truthy(&c.st("terminal")) {
                    let p = c.terminal(true);
                    c.observe(p);
                } else {
                    c.pane(&pane);
                }
                Ok(JsValue::UNDEFINED)
            });
        }
        for b in all(&self.el, "[data-font]") {
            let button = b.clone();
            self.listen(b, "click", move |c, _| {
                c.relayout(|| {
                    let n =
                        (number(&c.st("size")) + number(&data(&button, "font"))).clamp(14., 22.);
                    c.put("size", n.into());
                    write_store("comandos.news.size", n.into());
                });
                Ok(JsValue::UNDEFINED)
            });
        }
        self.listen(self.q(".nr-edition"), "click", |c, e| {
            let target = get(&e, "target");
            let near = |s: &str| call(&target, "closest", &[s.into()]).unwrap_or(JsValue::NULL);
            if truthy(&near(".nr-details-toggle")) {
                c.put("details", (!truthy(&c.st("details"))).into());
                c.paint();
                return Ok(JsValue::UNDEFINED);
            }
            if truthy(&near(".nr-back-edition")) {
                call(&c.q(".nr-saved-btn"), "click", &[])?;
                return Ok(JsValue::UNDEFINED);
            }
            let b = near("[data-goto]");
            if truthy(&b) {
                let r = c.clone();
                let p = promise(async move {
                    r.goto(data(&b, "goto"), data(&b, "gotoStory"), JsValue::UNDEFINED)
                        .await
                });
                c.observe(p);
                return Ok(JsValue::UNDEFINED);
            }
            let b = near("[data-open]");
            if truthy(&b) {
                let id = number(&data(&b, "open")).into();
                if c.st("open") == id
                    && call(&get(&b, "classList"), "contains", &["nr-open-btn".into()])
                        .map(|v| truthy(&v))
                        .unwrap_or(false)
                {
                    c.close_panel();
                } else {
                    c.open_story(id, JsValue::UNDEFINED);
                }
                return Ok(JsValue::UNDEFINED);
            }
            let b = near("[data-save]");
            if truthy(&b) {
                let r = c.clone();
                let p = promise(async move { r.save_story(b).await });
                c.observe(p);
            }
            Ok(JsValue::UNDEFINED)
        });
        self.listen(self.q(".nr-panel"), "click", |c, e| {
            let target = get(&e, "target");
            let r = c.clone();
            let p = promise(async move { r.panel_click(target).await });
            c.observe(p);
            Ok(JsValue::UNDEFINED)
        });
        self.listen(self.q(".nr-panel"), "submit", |c, e| {
            let _ = call(&e, "preventDefault", &[]);
            let input = c.q(".nr-chat-input");
            if truthy(&input) {
                let msg = string(&get(&input, "value"));
                let r = c.clone();
                let p = promise(async move { r.send(msg).await });
                c.observe(p);
            }
            Ok(JsValue::UNDEFINED)
        });
        self.listen(self.q(".nr-panel"), "input", |c, e| {
            let target = get(&e, "target");
            let has = |class: &str| {
                call(&get(&target, "classList"), "contains", &[class.into()])
                    .map(|v| truthy(&v))
                    .unwrap_or(false)
            };
            if has("nr-chat-input") {
                c.put("chatDraft", get(&target, "value"));
            }
            if has("nr-note-new") {
                c.put("noteDraft", get(&target, "value"));
            }
            if has("nr-notes-q") {
                c.put("query", get(&target, "value"));
                c.replace_timer("search", 250., |c| {
                    let r = c.clone();
                    let p = promise(async move {
                        let id = if string(&r.st("open")) == "notes" {
                            JsValue::NULL
                        } else {
                            r.st("open")
                        };
                        let _ = r.refresh_notes(id).await;
                        wait(Ok(r.repaint())).await?;
                        let input = r.q(".nr-notes-q");
                        if truthy(&input) {
                            call(&input, "focus", &[])?;
                            let len = number(&get(&get(&input, "value"), "length"));
                            call(&input, "setSelectionRange", &[len.into(), len.into()])?;
                        }
                        Ok(JsValue::UNDEFINED)
                    });
                    c.observe(p);
                    Ok(JsValue::UNDEFINED)
                });
            }
            Ok(JsValue::UNDEFINED)
        });
        self.listen(self.q(".nr-panel"), "keydown", |c, e| {
            let target = get(&e, "target");
            if string(&get(&e, "key")) == "Enter"
                && (truthy(&get(&e, "ctrlKey")) || truthy(&get(&e, "metaKey")))
                && call(
                    &get(&target, "classList"),
                    "contains",
                    &["nr-note-new".into()],
                )
                .map(|v| truthy(&v))
                .unwrap_or(false)
            {
                let _ = call(&e, "preventDefault", &[]);
                let r = c.clone();
                let p = promise(async move { r.add_note().await });
                c.observe(p);
            }
            Ok(JsValue::UNDEFINED)
        });
        self.listen(self.q(".nr-stage"), "scroll", |c, _| {
            if truthy(&c.st("terminal")) && c.narrow() {
                let stage = c.q(".nr-stage");
                c.put(
                    "mobilePane",
                    if number(&get(&stage, "scrollLeft")) > number(&get(&stage, "clientWidth")) / 2.
                    {
                        "reader".into()
                    } else {
                        "terminal".into()
                    },
                );
            }
            Ok(JsValue::UNDEFINED)
        });
        self.listen(self.q(".nr-edition-divider"), "pointerdown", |c, e| {
            if number(&get(&e, "button")) != 0. {
                return Ok(JsValue::UNDEFINED);
            }
            let _ = call(&e, "preventDefault", &[]);
            if let Ok(mut drag) = c.drag.try_borrow_mut() {
                *drag = c.mark();
            }
            let divider = c.q(".nr-edition-divider");
            call(&divider, "setPointerCapture", &[get(&e, "pointerId")])?;
            classes(&divider, "dragging", true);
            Ok(JsValue::UNDEFINED)
        });
        self.listen(self.q(".nr-edition-divider"), "pointermove", |c, e| {
            let divider = c.q(".nr-edition-divider");
            if !call(
                &get(&divider, "classList"),
                "contains",
                &["dragging".into()],
            )
            .map(|v| truthy(&v))
            .unwrap_or(false)
            {
                return Ok(JsValue::UNDEFINED);
            }
            let embedded = truthy(&get(&c.opts, "embedded"));
            let parent = get(&c.el, "parentElement");
            let rect = if embedded && truthy(&parent) {
                let term = call(&c.doc, "getElementById", &["term-area".into()])?;
                call(
                    &if truthy(&term) { term } else { parent },
                    "getBoundingClientRect",
                    &[],
                )?
            } else {
                call(&c.q(".nr-stage"), "getBoundingClientRect", &[])?
            };
            let right = if embedded {
                number(&get(&call(&c.el, "getBoundingClientRect", &[])?, "right"))
            } else {
                number(&get(&rect, "right"))
            };
            let left = if embedded {
                number(&get(&rect, "left")).min(right - 10.)
            } else {
                number(&get(&rect, "left"))
            };
            c.put(
                "share",
                view::clamp_share(
                    (right - number(&get(&e, "clientX"))) / (right - left).max(1.) * 100.,
                )
                .into(),
            );
            c.prefs();
            Ok(JsValue::UNDEFINED)
        });
        for event in ["pointerup", "pointercancel"] {
            self.listen(self.q(".nr-edition-divider"), event, |c, _| {
                let divider = c.q(".nr-edition-divider");
                if call(
                    &get(&divider, "classList"),
                    "contains",
                    &["dragging".into()],
                )
                .map(|v| truthy(&v))
                .unwrap_or(false)
                {
                    classes(&divider, "dragging", false);
                    write_store("comandos.news.share", c.st("share"));
                    let mark = c.drag.try_borrow_mut().ok().and_then(|mut d| d.take());
                    let r = c.clone();
                    let f = function(move |_| {
                        r.restore(mark.clone());
                        Ok(JsValue::UNDEFINED)
                    });
                    if let Ok(id) = call(&global("window"), "requestAnimationFrame", &[f])
                        && let Ok(mut raf) = c.raf.try_borrow_mut()
                    {
                        raf.push(id);
                    }
                }
                Ok(JsValue::UNDEFINED)
            });
        }
        self.listen(self.q(".nr-edition-divider"), "keydown", |c, e| {
            let step = match string(&get(&e, "key")).as_str() {
                "ArrowLeft" => 5.,
                "ArrowRight" => -5.,
                _ => 0.,
            };
            if step != 0. {
                let _ = call(&e, "preventDefault", &[]);
                c.relayout(|| {
                    c.put(
                        "share",
                        view::clamp_share(number(&c.st("share")) + step).into(),
                    );
                    write_store("comandos.news.share", c.st("share"));
                });
            }
            Ok(JsValue::UNDEFINED)
        });
    }
    fn api(self: &Rc<Self>) -> Result<JsValue, JsValue> {
        let api = object();
        set(&api, "element", &self.el)?;
        set(&api, "state", &self.state)?;
        let c = self.clone();
        method(&api, "open", move |a| Ok(c.open(a.get(0))))?;
        let c = self.clone();
        method(&api, "close", move |_| {
            c.close()?;
            Ok(JsValue::UNDEFINED)
        })?;
        let c = self.clone();
        method(&api, "setTerminal", move |a| {
            Ok(c.terminal(truthy(&a.get(0))))
        })?;
        let c = self.clone();
        method(&api, "openStory", move |a| {
            c.open_story(a.get(0), a.get(1));
            Ok(JsValue::UNDEFINED)
        })?;
        let c = self.clone();
        method(&api, "closePanel", move |_| {
            c.close_panel();
            Ok(JsValue::UNDEFINED)
        })?;
        let c = self.clone();
        method(&api, "dispose", move |_| {
            c.dispose()?;
            Ok(JsValue::UNDEFINED)
        })?;
        Ok(api)
    }
}
fn mount_reader(opts: JsValue) -> Result<JsValue, JsValue> {
    let opts = if truthy(&opts) { opts } else { object() };
    let document = get(&opts, "document");
    let document = if truthy(&document) { document } else { doc() };
    let el = call(&document, "createElement", &["div".into()])?;
    set(&el, "id", &"news-reader".into())?;
    let embedded = truthy(&get(&opts, "embedded"));
    let desktop = truthy(&get(&opts, "desktop"));
    set(
        &el,
        "className",
        &format!(
            "nr-app nr-B{}{}",
            if embedded { " nr-embedded" } else { "" },
            if desktop { " nr-desktop" } else { "" }
        )
        .into(),
    )?;
    attr(
        &el,
        "role",
        if embedded || desktop {
            "region"
        } else {
            "dialog"
        },
    );
    if !embedded && !desktop {
        attr(&el, "aria-modal", "true");
    }
    attr(&el, "aria-label", "Resúmenes");
    set(&el, "hidden", &true.into())?;
    html(&el, view::TEMPLATE);
    let parent = get(&opts, "parent");
    call(
        &if truthy(&parent) {
            parent
        } else {
            get(&document, "body")
        },
        "appendChild",
        std::slice::from_ref(&el),
    )?;
    let owner = Rc::new(Owner::default());
    let state = from_json(
        &json!({"list":null,"current":null,"view":"edition","terminal":false,"terminalMounted":false,"mobilePane":"reader","opener":null,"details":false,"open":null,"tab":"resumen","scope":"esta","query":"","sourceId":null,"notes":null,"allNotes":null,"saved":null,"noteDraft":"","chatDraft":"","editing":null,"confirmDelete":null}),
    )?;
    let size = number(&read_store("comandos.news.size", 16.into()));
    set(
        &state,
        "size",
        &if size == 0. || !size.is_finite() {
            16.
        } else {
            size
        }
        .clamp(14., 22.)
        .into(),
    )?;
    set(
        &state,
        "share",
        &view::clamp_share(number(&read_store("comandos.news.share", 58.into()))).into(),
    )?;
    set(&state, "original", &Set::new(&JsValue::UNDEFINED).into())?;
    set(&state, "chat", &Map::new().into())?;
    set(&state, "sources", &Map::new().into())?;
    let purify = get(&opts, "purify");
    let plain = !purify.is_undefined() && !get(&purify, "sanitize").is_function();
    let renderer = Renderer::new(get(&opts, "fetchJson"), purify, plain);
    let store = store(get(&opts, "fetchJson"), owner.clone())?;
    let c = Rc::new(Reader {
        opts,
        doc: document,
        el,
        state,
        owner,
        renderer,
        store,
        listeners: RefCell::new(Vec::new()),
        timers: RefCell::new(HashMap::new()),
        media: RefCell::new(HashMap::new()),
        media_pending: RefCell::new(HashMap::new()),
        raf: RefCell::new(Vec::new()),
        generation: Cell::new(0),
        edition_generation: Cell::new(0),
        open_epoch: Cell::new(0),
        stopped: Cell::new(false),
        drag: RefCell::new(None),
        busy: RefCell::new(HashMap::new()),
        edition_html: RefCell::new(None),
    });
    c.prefs();
    c.wire();
    c.api()
}
fn post_bridge(
    bridge: &JsValue,
    action: &str,
    id: JsValue,
    on: JsValue,
) -> Result<JsValue, JsValue> {
    let msg = object();
    set(&msg, "type", &"reader".into())?;
    set(&msg, "action", &action.into())?;
    if action == "open" {
        set(&msg, "id", &if truthy(&id) { id } else { "".into() })?;
    }
    if action == "terminal" {
        set(&msg, "on", &on)?;
    }
    let _ = call(
        bridge,
        "postMessage",
        &[js_sys::JSON::stringify(&msg)?.into()],
    );
    Ok(JsValue::UNDEFINED)
}
fn install(opts: JsValue) -> Result<JsValue, JsValue> {
    let opts = if truthy(&opts) { opts } else { object() };
    let params = construct("URLSearchParams", &[get(&global("location"), "search")])?;
    let bridge = get(
        &get(&get(&global("window"), "webkit"), "messageHandlers"),
        "centro",
    );
    let desktop = call(&params, "get", &["panel".into()])
        .map(|v| string(&v) == "news")
        .unwrap_or(false)
        && truthy(&bridge);
    let reader = Rc::new(RefCell::new(JsValue::NULL));
    let stopped = Rc::new(Cell::new(false));
    let timer = Rc::new(RefCell::new(JsValue::NULL));
    let get_reader = {
        let reader = reader.clone();
        let opts = opts.clone();
        let bridge = bridge.clone();
        let stopped = stopped.clone();
        function(move |_| {
            if stopped.get() {
                return Err(error("Lector cerrado"));
            }
            if let Ok(r) = reader.try_borrow()
                && truthy(&r)
            {
                return Ok(r.clone());
            }
            let options = assign(&opts);
            let panes = id("panes");
            let embedded = !desktop
                && truthy(&panes)
                && call(
                    &get(&get(&doc(), "body"), "classList"),
                    "contains",
                    &["app".into()],
                )
                .map(|v| truthy(&v))
                .unwrap_or(false);
            set(&options, "desktop", &desktop.into())?;
            set(&options, "embedded", &embedded.into())?;
            if embedded {
                set(&options, "parent", &panes)?;
            }
            let b = bridge.clone();
            method(&options, "externalTerminal", move |a| {
                post_bridge(&b, "terminal", JsValue::UNDEFINED, a.get(0))
            })?;
            let b = bridge.clone();
            method(&options, "onClose", move |_| {
                post_bridge(&b, "close", JsValue::UNDEFINED, JsValue::UNDEFINED)
            })?;
            let r = mount_reader(options)?;
            if let Ok(mut reader) = reader.try_borrow_mut() {
                *reader = r.clone();
            }
            Ok(r)
        })
    };
    let api = object();
    let app_only = truthy(&bridge) && !desktop;
    let open = {
        let bridge = bridge.clone();
        let get_reader = get_reader.clone();
        let stopped = stopped.clone();
        function(move |a| {
            if stopped.get() {
                return Err(error("Lector cerrado"));
            }
            if app_only {
                post_bridge(&bridge, "open", a.get(0), JsValue::UNDEFINED)
            } else {
                call(&invoke(&get_reader, &[])?, "open", &[a.get(0)])
            }
        })
    };
    set(&api, "open", &open)?;
    let close = {
        let bridge = bridge.clone();
        let reader = reader.clone();
        function(move |_| {
            if app_only {
                post_bridge(&bridge, "close", JsValue::UNDEFINED, JsValue::UNDEFINED)
            } else if let Ok(r) = reader.try_borrow()
                && truthy(&r)
            {
                call(&r, "close", &[])
            } else {
                Ok(JsValue::UNDEFINED)
            }
        })
    };
    set(&api, "close", &close)?;
    let toggle = {
        let open = open.clone();
        let get_reader = get_reader.clone();
        function(move |_| {
            if app_only {
                invoke(&open, &[])
            } else {
                let r = invoke(&get_reader, &[])?;
                call(
                    &r,
                    if truthy(&get(&get(&r, "element"), "hidden")) {
                        "open"
                    } else {
                        "close"
                    },
                    &[],
                )
            }
        })
    };
    set(&api, "toggle", &toggle)?;
    let button = id("btn-news");
    if truthy(&button) {
        listen(&button, "click", toggle.clone());
    }
    if !app_only {
        let r = reader.clone();
        getter(&api, "reader", move || {
            r.try_borrow().map(|r| r.clone()).unwrap_or(JsValue::NULL)
        })?;
        let wanted = call(&params, "get", &["news".into()])?;
        if desktop || !wanted.is_null() {
            let pattern = construct("RegExp", &["^\\d{4}-\\d{2}-\\d{2}@\\d{2}:\\d{2}$".into()])?;
            let target = if truthy(&wanted)
                && truthy(&call(&pattern, "test", std::slice::from_ref(&wanted))?)
            {
                wanted
            } else {
                JsValue::UNDEFINED
            };
            let open = open.clone();
            let flag = stopped.clone();
            let f = function(move |_| {
                if flag.get() {
                    return Ok(JsValue::UNDEFINED);
                }
                let p = invoke(&open, std::slice::from_ref(&target))?;
                let _ = call(&p, "catch", &[function(|_| Ok(JsValue::UNDEFINED))]);
                Ok(JsValue::UNDEFINED)
            });
            if let Ok(mut slot) = timer.try_borrow_mut() {
                *slot = later(f, if desktop { 0. } else { 400. });
            }
        }
    }
    method(&api, "dispose", move |_| {
        stopped.set(true);
        if let Ok(t) = timer.try_borrow() {
            cancel(t.clone());
        }
        if truthy(&button) {
            let _ = call(
                &button,
                "removeEventListener",
                &["click".into(), toggle.clone()],
            );
        }
        if let Ok(r) = reader.try_borrow()
            && truthy(&r)
        {
            call(&r, "dispose", &[])?;
        }
        Ok(JsValue::UNDEFINED)
    })?;
    Ok(api)
}
pub fn mount() -> Result<(), JsValue> {
    let api = object();
    method(&api, "statusLabel", |a| {
        Ok(utf16_value(&view::status_label(&if truthy(&a.get(0)) {
            string(&a.get(0))
        } else {
            String::new()
        })))
    })?;
    method(&api, "modelName", |a| {
        Ok(utf16_value(&view::model_name(&if truthy(&a.get(0)) {
            string(&a.get(0))
        } else {
            String::new()
        })))
    })?;
    method(&api, "safeHref", |a| {
        Ok(safe_href(&if truthy(&a.get(0)) {
            string(&a.get(0))
        } else {
            String::new()
        })
        .map(|s| utf16_value(&s))
        .unwrap_or(JsValue::NULL))
    })?;
    method(&api, "clampShare", |a| {
        Ok(view::clamp_share(number(&a.get(0))).into())
    })?;
    method(&api, "sourceDateLabel", |a| {
        let src = a.get(0);
        let tz = if truthy(&a.get(1)) {
            string(&a.get(1))
        } else {
            String::new()
        };
        let published = get(&src, "publishedAt");
        let text = if truthy(&published) {
            format!("publicada {}", date(number(&published), &tz))
        } else {
            let discovered = get(&src, "discoveredAt");
            format!(
                "descubierta {} · publicación desconocida",
                if truthy(&discovered) {
                    date(number(&discovered), &tz)
                } else {
                    String::new()
                }
            )
        };
        Ok(utf16_value(&text))
    })?;
    method(&api, "provenance", |a| {
        let edition = a.get(0);
        let mut out = view::provenance(&to_json(&edition));
        let keys = call(&global("Object"), "keys", &[get(&edition, "models")])
            .unwrap_or_else(|_| Array::new().into());
        let labels = rows(&keys)
            .iter()
            .map(|key| {
                format!(
                    "{} ({})",
                    view::model_name(&string(key)),
                    string(&get(&get(&edition, "models"), &string(key)))
                )
            })
            .collect::<Vec<_>>()
            .join(" · ");
        if !labels.is_empty()
            && let Some(o) = out.as_object_mut()
        {
            o.insert("models".into(), labels.into());
        }
        let cost = number(&get(&edition, "costUsd"));
        let cost = if cost.is_nan() || cost == 0. {
            0.
        } else {
            cost
        };
        let fixed = call(
            &get(&get(&global("Number"), "prototype"), "toFixed"),
            "call",
            &[cost.into(), 2.into()],
        )
        .map(|v| string(&v))
        .unwrap_or_else(|_| format!("{cost:.2}"));
        if let Some(o) = out.as_object_mut() {
            o.insert("cost".into(), format!("${fixed}").into());
        }
        from_json(&out)
    })?;
    method(&api, "storyKicker", |a| {
        let (label, hot) = view::story_kicker(&to_json(&a.get(0)));
        from_json(&json!({"label":label,"hot":hot}))
    })?;
    method(&api, "inlineText", |a| {
        Ok(utf16_value(&view::inline_text(
            &if a.get(0).is_null() || a.get(0).is_undefined() {
                String::new()
            } else {
                string(&a.get(0))
            },
        )))
    })?;
    method(&api, "blocksHtml", |a| {
        Ok(utf16_value(&view::blocks_html(&to_json(&a.get(0)))))
    })?;
    method(&api, "noteCommand", |a| {
        Ok(view::note_command(&if truthy(&a.get(0)) {
            string(&a.get(0))
        } else {
            String::new()
        })
        .map(|s| utf16_value(&s))
        .unwrap_or(JsValue::NULL))
    })?;
    method(&api, "dayLine", |a| {
        let list = a.get(0);
        let today = a.get(1);
        let mut cards = rows(&get(&list, "editions"))
            .into_iter()
            .filter(|e| get(e, "localDate") == today)
            .collect::<Vec<_>>();
        cards.sort_by(|a, b| {
            let sa = utf16_value(&string(&get(a, "slot")));
            let sb = utf16_value(&string(&get(b, "slot")));
            let n = call(&sa, "localeCompare", &[sb])
                .map(|v| number(&v))
                .unwrap_or(0.);
            n.partial_cmp(&0.).unwrap_or(std::cmp::Ordering::Equal)
        });
        let mut cards = cards
            .into_iter()
            .map(|e| {
                let e = assign(&e);
                let _ = set(&e, "day", &"Hoy".into());
                e
            })
            .collect::<Vec<_>>();
        let next = get(&list, "next");
        if truthy(&next) && !cards.iter().any(|c| get(c, "id") == get(&next, "id")) {
            let next = assign(&next);
            set(
                &next,
                "day",
                &if get(&next, "localDate") == today {
                    "Hoy".into()
                } else {
                    "Mañana".into()
                },
            )?;
            cards.push(next);
        }
        Ok(cards.into_iter().collect::<Array>().into())
    })?;
    method(&api, "notesByDay", |a| {
        from_json(&groups(
            &to_json(&a.get(0)),
            number(&a.get(1)),
            &if truthy(&a.get(2)) {
                string(&a.get(2))
            } else {
                String::new()
            },
        ))
    })?;
    method(&api, "createStore", |a| {
        store(a.get(0), Rc::new(Owner::default()))
    })?;
    method(&api, "createRenderer", |a| {
        if !a.get(0).is_function() && !a.get(0).is_undefined() {
            return Err(error("markdown-it no está disponible"));
        }
        let opts = a.get(2);
        let purify = a.get(1);
        let plain = !purify.is_undefined() && !get(&purify, "sanitize").is_function();
        Renderer::new(get(&opts, "fetchJson"), purify, plain).api()
    })?;
    method(&api, "mount", |a| mount_reader(a.get(0)))?;
    method(&api, "install", |a| install(a.get(0)))?;
    global_set("NewsReader", &api)?;
    Ok(())
}
pub fn attach() -> Result<(), JsValue> {
    let api = global("NewsReader");
    let instance = install(JsValue::UNDEFINED)?;
    set(&api, "instance", &instance)
}

pub fn retire_vendor() -> Result<(), JsValue> {
    let api = global("NewsReader");
    if !get(&api, "createRenderer").is_function() || !get(&api, "mount").is_function() {
        return Err(error("news-reader Rust no está listo"));
    }
    let renderer = call(&api, "createRenderer", &[])?;
    if !renderer.is_function() || !get(&renderer, "dispose").is_function() {
        return Err(error("renderer Rust no está listo"));
    }
    call(&renderer, "dispose", &[])?;
    Ok(())
}
