use super::*;
struct Presence {
    opts: JsValue,
    doc: JsValue,
    last: Cell<f64>,
    started: Cell<bool>,
    interval: RefCell<JsValue>,
    deferred: RefCell<Vec<JsValue>>,
    gesture: RefCell<JsValue>,
    visibility: RefCell<JsValue>,
}
impl Presence {
    fn visible(&self) -> bool {
        !truthy(&self.doc) || string(&get(&self.doc, "visibilityState")) != "hidden"
    }
    fn send(&self, interaction: bool) {
        let body = object();
        let audio = invoke(&get(&self.opts, "canPlayAudio"), &[])
            .map(|v| truthy(&v))
            .unwrap_or(false);
        let _ = set(
            &body,
            "deviceId",
            &default(&self.opts, "deviceId", "".into()),
        );
        let _ = set(&body, "visible", &self.visible().into());
        let _ = set(&body, "canPlayAudio", &audio.into());
        let _ = set(&body, "interaction", &interaction.into());
        let request = invoke(
            &get(&self.opts, "transport"),
            &["POST".into(), "/presence".into(), body],
        );
        wasm_bindgen_futures::spawn_local(async move {
            let _ = wait(request).await;
        });
    }
    fn stop(&self) {
        if !self.started.replace(false) {
            return;
        }
        let g = self
            .gesture
            .try_borrow()
            .map(|g| g.clone())
            .unwrap_or(JsValue::NULL);
        let v = self
            .visibility
            .try_borrow()
            .map(|g| g.clone())
            .unwrap_or(JsValue::NULL);
        for event in ["pointerdown", "keydown"] {
            let _ = call(
                &self.doc,
                "removeEventListener",
                &[event.into(), g.clone(), true.into()],
            );
        }
        let _ = call(
            &self.doc,
            "removeEventListener",
            &["visibilitychange".into(), v],
        );
        if let Ok(mut id) = self.interval.try_borrow_mut() {
            timer(
                &self.opts,
                "clearInterval",
                "clearInterval",
                std::slice::from_ref(&id),
            );
            *id = JsValue::NULL;
        }
        if let Ok(mut timers) = self.deferred.try_borrow_mut() {
            for id in timers.drain(..) {
                timer(&self.opts, "clearTimeout", "clearTimeout", &[id]);
            }
        }
    }
}
pub(super) fn presence(opts: JsValue) -> Result<JsValue, JsValue> {
    let doc = default(&opts, "doc", JsValue::NULL);
    let p = Rc::new(Presence {
        opts,
        doc,
        last: Cell::new(f64::NEG_INFINITY),
        started: Cell::new(false),
        interval: RefCell::new(JsValue::NULL),
        deferred: RefCell::new(Vec::new()),
        gesture: RefCell::new(JsValue::NULL),
        visibility: RefCell::new(JsValue::NULL),
    });
    let owner = Rc::downgrade(&p);
    let g = function(move |a| {
        let Some(u) = owner.upgrade() else {
            return Ok(JsValue::UNDEFINED);
        };
        let e = a.get(0);
        if get(&e, "isTrusted") != JsValue::TRUE || !u.visible() {
            return Ok(JsValue::UNDEFINED);
        }
        let now = invoke(&get(&u.opts, "now"), &[])
            .map(|v| number(&v))
            .unwrap_or_else(|_| js_sys::Date::now());
        let throttle = number(&default(&u.opts, "throttleMs", 5000.into()));
        if now - u.last.get() < throttle {
            return Ok(JsValue::UNDEFINED);
        }
        u.last.set(now);
        let delay = number(&default(&u.opts, "deferMs", 0.into()));
        if delay > 0. {
            let state = u.clone();
            let cb = function(move |_| {
                if state.started.get() {
                    state.send(true)
                }
                Ok(JsValue::UNDEFINED)
            });
            let id = timer(&u.opts, "setTimeout", "setTimeout", &[cb, delay.into()]);
            if let Ok(mut list) = u.deferred.try_borrow_mut() {
                list.push(id);
            }
        } else {
            u.send(true)
        }
        Ok(JsValue::UNDEFINED)
    });
    if let Ok(mut slot) = p.gesture.try_borrow_mut() {
        *slot = g.clone();
    }
    let owner = Rc::downgrade(&p);
    let visibility = function(move |_| {
        if let Some(u) = owner.upgrade() {
            u.send(false);
        }
        Ok(JsValue::UNDEFINED)
    });
    if let Ok(mut slot) = p.visibility.try_borrow_mut() {
        *slot = visibility.clone();
    }
    let out = object();
    set(&out, "onGesture", &g)?;
    let u = p.clone();
    method(&out, "start", move |_| {
        if u.started.replace(true) {
            return Ok(JsValue::UNDEFINED);
        }
        let g = u
            .gesture
            .try_borrow()
            .map(|s| s.clone())
            .unwrap_or(JsValue::NULL);
        let v = u
            .visibility
            .try_borrow()
            .map(|s| s.clone())
            .unwrap_or(JsValue::NULL);
        for event in ["pointerdown", "keydown"] {
            let _ = call(
                &u.doc,
                "addEventListener",
                &[event.into(), g.clone(), true.into()],
            );
        }
        listen(&u.doc, "visibilitychange", v);
        u.send(false);
        let state = u.clone();
        let cb = function(move |_| {
            if state.started.get() && state.visible() {
                state.send(false)
            }
            Ok(JsValue::UNDEFINED)
        });
        let id = timer(
            &u.opts,
            "setInterval",
            "setInterval",
            &[cb, default(&u.opts, "heartbeatMs", 30000.into())],
        );
        if let Ok(mut t) = u.interval.try_borrow_mut() {
            *t = id;
        }
        Ok(JsValue::UNDEFINED)
    })?;
    let u = p.clone();
    method(&out, "stop", move |_| {
        u.stop();
        Ok(JsValue::UNDEFINED)
    })?;
    method(&out, "refresh", move |_| {
        p.send(false);
        Ok(JsValue::UNDEFINED)
    })?;
    Ok(out)
}
struct Mount {
    opts: JsValue,
    doc: JsValue,
    win: JsValue,
    storage: JsValue,
    root: JsValue,
    float: JsValue,
    strip: JsValue,
    controller: JsValue,
    c: Rc<Controller>,
    presence: RefCell<JsValue>,
    hidden: Cell<bool>,
    height: Cell<f64>,
    restore: Cell<Option<f64>>,
    queued: Cell<bool>,
    last_settings: RefCell<String>,
    poll: RefCell<JsValue>,
    clock: RefCell<JsValue>,
    watching: Cell<bool>,
    destroyed: Cell<bool>,
    listeners: RefCell<Vec<(JsValue, String, JsValue)>>,
    timers: RefCell<Vec<JsValue>>,
    rafs: RefCell<Vec<JsValue>>,
    delays: RefCell<Vec<JsValue>>,
}
impl Mount {
    fn read(&self, key: &str) -> JsValue {
        call(&self.storage, "getItem", &[key.into()]).unwrap_or(JsValue::NULL)
    }
    fn write(&self, key: &str, value: &str) {
        let _ = call(&self.storage, "setItem", &[key.into(), value.into()]);
    }
    fn viewport(&self) -> f64 {
        number(&default(&self.win, "innerHeight", 800.into()))
    }
    fn set_height(&self, px: f64, remember: bool) -> f64 {
        let height = view::clamp_height(px, self.viewport());
        self.height.set(height);
        if remember {
            self.write("comandos.notices.height", &height.to_string())
        }
        style(&self.root, "--nt-h", &format!("{height}px"));
        height
    }
    fn max(self: &Rc<Self>) {
        let max = view::clamp_height(f64::INFINITY, self.viewport());
        if let Some(back) = self.restore.get().filter(|_| self.height.get() >= max) {
            self.restore.set(None);
            self.set_height(back, true);
        } else {
            self.restore.set(Some(self.height.get()));
            self.set_height(max, false);
        }
        self.schedule();
    }
    fn visible(&self) -> bool {
        let cb = get(&self.opts, "isVisible");
        if cb.is_function() {
            invoke(&cb, &[]).map(|v| truthy(&v)).unwrap_or(false)
        } else {
            !truthy(&self.doc) || string(&get(&self.doc, "visibilityState")) != "hidden"
        }
    }
    fn listen(&self, el: &JsValue, event: &str, cb: JsValue) {
        listen(el, event, cb.clone());
        if let Ok(mut list) = self.listeners.try_borrow_mut() {
            list.push((el.clone(), event.into(), cb));
        }
    }
    fn render_settings(&self) -> Result<(), JsValue> {
        let box_ =
            call(&self.doc, "getElementById", &["notice-settings".into()]).unwrap_or(JsValue::NULL);
        let active = get(&self.doc, "activeElement");
        if !truthy(&box_) || truthy(&call(&box_, "contains", &[active]).unwrap_or(JsValue::FALSE)) {
            return Ok(());
        }
        let sounds = get(&self.opts, "sounds");
        let enabled = call(&sounds, "isEnabled", &[])
            .map(|v| truthy(&v))
            .unwrap_or(false);
        let html = view::render_settings(
            &json!({"prefs":to_json(&get(&self.c.state,"prefs")),"localSound":enabled,"localAvailable":truthy(&sounds)}),
        );
        let mut last = self
            .last_settings
            .try_borrow_mut()
            .map_err(|_| js_sys::Error::new("Notice settings busy"))?;
        if html != *last {
            *last = html.clone();
            set(&box_, "innerHTML", &html.into())?;
        }
        Ok(())
    }
    fn render(self: &Rc<Self>) -> Result<(), JsValue> {
        if self.destroyed.get() {
            return Ok(());
        }
        self.queued.set(false);
        let v = self.c.view()?;
        let active = get(&self.doc, "activeElement");
        let contains = truthy(
            &call(&self.root, "contains", std::slice::from_ref(&active)).unwrap_or(JsValue::FALSE),
        );
        let keep = if contains {
            call(&active, "getAttribute", &["data-nt-focus".into()]).unwrap_or(JsValue::NULL)
        } else {
            JsValue::NULL
        };
        let view_ = clone(&v);
        set(&view_, "collapsed", &false.into())?;
        set(&view_, "maximized", &self.restore.get().is_some().into())?;
        let disclosures = all(&self.strip, ".nt-full[open]")
            .iter()
            .map(|d| {
                string(&call(d, "getAttribute", &["data-nt-full".into()]).unwrap_or(JsValue::NULL))
            })
            .collect::<Vec<_>>();
        set(
            &self.strip,
            "innerHTML",
            &if self.hidden.get() {
                String::new()
            } else {
                view::render_strip(&rendering(&view_))
            }
            .into(),
        )?;
        for d in all(&self.strip, ".nt-full") {
            let key = string(
                &call(&d, "getAttribute", &["data-nt-full".into()]).unwrap_or(JsValue::NULL),
            );
            if disclosures.contains(&key) {
                set(&d, "open", &true.into())?;
            }
        }
        let show = get(&self.opts, "showFloat");
        if truthy(&get(&v, "float")) && show.is_function() {
            let ids = rows(&get(&get(&v, "float"), "eventIds"));
            let visible = ids.iter().filter(|id| self.c.by_id.has(id)).any(|id| {
                invoke(&show, &[self.c.by_id.get(id)])
                    .map(|v| truthy(&v))
                    .unwrap_or(false)
            });
            if !visible {
                set(&v, "float", &JsValue::NULL)?;
            }
        }
        set(
            &self.float,
            "innerHTML",
            &view::render_float(&rendering(&v)).into(),
        )?;
        classes(&self.root, "nt-hidden", self.hidden.get());
        classes(&self.root, "nt-collapsed", false);
        classes(&self.root, "nt-has-float", truthy(&get(&v, "float")));
        let on_badge = get(&self.opts, "onBadge");
        if on_badge.is_function() {
            let badge = get(&self.c.state, "badge");
            let badge = if badge.as_f64().is_some() {
                badge
            } else {
                (self
                    .c
                    .notices()
                    .iter()
                    .filter(|n| {
                        !truthy(&get(n, "read"))
                            || has(&get(&self.c.state, "pending"), &get(n, "eventId"))
                    })
                    .count() as f64)
                    .into()
            };
            invoke(&on_badge, &[badge])?;
        }
        if truthy(&keep) {
            let key = string(&keep).replace('\\', "\\\\").replace('"', "\\\"");
            let again = query(&self.root, &format!("[data-nt-focus=\"{key}\"]"));
            if get(&again, "focus").is_function() {
                call(
                    &again,
                    "focus",
                    &[from_json(&json!({"preventScroll":true}))?],
                )?;
            }
        }
        self.render_settings()
    }
    fn schedule(self: &Rc<Self>) {
        if self.destroyed.get() {
            return;
        }
        if truthy(&get(&self.opts, "sync")) {
            let _ = self.render();
            return;
        }
        if self.queued.replace(true) {
            return;
        }
        let u = self.clone();
        let cb = function(move |_| {
            u.render()?;
            Ok(JsValue::UNDEFINED)
        });
        let raf = get(&self.win, "requestAnimationFrame");
        if raf.is_function() {
            if let Ok(id) = call(&self.win, "requestAnimationFrame", &[cb])
                && let Ok(mut list) = self.rafs.try_borrow_mut()
            {
                list.push(id);
            }
        } else {
            let id = later(cb, 0.);
            if let Ok(mut list) = self.timers.try_borrow_mut() {
                list.push(id);
            }
        }
    }
    fn arm(self: &Rc<Self>) {
        if self.destroyed.get() {
            return;
        }
        if let Ok(mut id) = self.poll.try_borrow_mut() {
            if !id.is_null() {
                timer(
                    &self.opts,
                    "clearInterval",
                    "clearInterval",
                    std::slice::from_ref(&id),
                );
            }
            let u = self.clone();
            let cb = function(move |_| {
                if !u.destroyed.get() {
                    u.c.poll();
                }
                Ok(JsValue::UNDEFINED)
            });
            let interval = if self.visible() {
                default(&self.opts, "pollMs", 3000.into())
            } else {
                default(&self.opts, "hiddenPollMs", 15000.into())
            };
            *id = timer(&self.opts, "setInterval", "setInterval", &[cb, interval]);
        }
    }
    fn destroy(&self) {
        if self.destroyed.replace(true) {
            return;
        }
        self.watching.set(false);
        self.c.dispose();
        let _ = set(&self.c.opts, "onChange", &JsValue::NULL);
        if let Ok(mut list) = self.rafs.try_borrow_mut() {
            for id in list.drain(..) {
                let _ = call(&self.win, "cancelAnimationFrame", &[id]);
            }
        }
        if let Ok(mut list) = self.delays.try_borrow_mut() {
            for resolve in list.drain(..) {
                let _ = invoke(&resolve, &[JsValue::UNDEFINED]);
            }
        }
        for slot in [&self.poll, &self.clock] {
            if let Ok(mut id) = slot.try_borrow_mut()
                && !id.is_null()
            {
                timer(
                    &self.opts,
                    "clearInterval",
                    "clearInterval",
                    std::slice::from_ref(&id),
                );
                *id = JsValue::NULL;
            }
        }
        if let Ok(mut list) = self.timers.try_borrow_mut() {
            for id in list.drain(..) {
                timer(&self.opts, "clearTimer", "clearTimeout", &[id]);
            }
        }
        if let Ok(mut list) = self.listeners.try_borrow_mut() {
            for (el, event, cb) in list.drain(..) {
                let _ = call(&el, "removeEventListener", &[event.into(), cb]);
            }
        }
        if let Ok(p) = self.presence.try_borrow() {
            let _ = call(&p, "stop", &[]);
        }
        let parent = default(&self.root, "parentNode", get(&self.root, "parent"));
        let _ = call(&parent, "removeChild", std::slice::from_ref(&self.root));
    }
}
fn closest(event: &JsValue, sel: &str) -> JsValue {
    call(&get(event, "target"), "closest", &[sel.into()]).unwrap_or(JsValue::NULL)
}
fn attr_(target: &JsValue, key: &str) -> JsValue {
    call(target, "getAttribute", &[key.into()]).unwrap_or(JsValue::NULL)
}
fn grip(event: &JsValue) -> bool {
    truthy(&closest(event, ".nt-grip"))
}
pub(super) fn mount(opts: JsValue) -> Result<JsValue, JsValue> {
    let doc = default(&opts, "doc", super::doc());
    let win = if get(&opts, "win").is_undefined() {
        global("window")
    } else {
        get(&opts, "win")
    };
    let storage = if get(&opts, "storage").is_undefined() {
        get(&win, "localStorage")
    } else {
        get(&opts, "storage")
    };
    let host = default(
        &opts,
        "host",
        default(
            &call(&doc, "getElementById", &["panes".into()]).unwrap_or(JsValue::NULL),
            "__never",
            get(&doc, "body"),
        ),
    );
    let host = if truthy(&get(&opts, "host")) {
        get(&opts, "host")
    } else {
        let p = call(&doc, "getElementById", &["panes".into()]).unwrap_or(JsValue::NULL);
        if truthy(&p) { p } else { host }
    };
    let root = call(&doc, "createElement", &["div".into()])?;
    set(&root, "id", &"notices".into())?;
    set(&root, "className", &"nt-root".into())?;
    let float = call(&doc, "createElement", &["div".into()])?;
    set(&float, "className", &"nt-float-slot".into())?;
    attr(&float, "aria-live", "polite");
    let strip = call(&doc, "createElement", &["div".into()])?;
    set(&strip, "className", &"nt-strip-slot".into())?;
    call(&root, "appendChild", std::slice::from_ref(&float))?;
    call(&root, "appendChild", std::slice::from_ref(&strip))?;
    call(&host, "appendChild", std::slice::from_ref(&root))?;
    let _ = call(
        &get(&get(&doc, "body"), "classList"),
        "add",
        &["nt-mounted".into()],
    );
    let ctrl_opts = clone(&opts);
    let saved =
        call(&storage, "getItem", &["comandos.notices.collapsed".into()]).unwrap_or(JsValue::NULL);
    let collapsed = if saved.is_null() {
        truthy(&get(&opts, "defaultCollapsed"))
    } else {
        saved == "1"
    };
    set(&ctrl_opts, "collapsed", &collapsed.into())?;
    let d = doc.clone();
    if !get(&ctrl_opts, "isVisible").is_function() {
        set(
            &ctrl_opts,
            "isVisible",
            &function(move |_| {
                Ok((!truthy(&d) || string(&get(&d, "visibilityState")) != "hidden").into())
            }),
        )?;
    }
    let (controller, c) = super::controller(ctrl_opts)?;
    let m = Rc::new(Mount {
        opts,
        doc,
        win,
        storage,
        root,
        float,
        strip,
        controller,
        c,
        presence: RefCell::new(JsValue::NULL),
        hidden: Cell::new(true),
        height: Cell::new(260.),
        restore: Cell::new(None),
        queued: Cell::new(false),
        last_settings: RefCell::new(String::new()),
        poll: RefCell::new(JsValue::NULL),
        clock: RefCell::new(JsValue::NULL),
        watching: Cell::new(true),
        destroyed: Cell::new(false),
        listeners: RefCell::new(Vec::new()),
        timers: RefCell::new(Vec::new()),
        rafs: RefCell::new(Vec::new()),
        delays: RefCell::new(Vec::new()),
    });
    let u = m.clone();
    set(
        &m.c.opts,
        "onChange",
        &function(move |_| {
            u.schedule();
            Ok(JsValue::UNDEFINED)
        }),
    )?;
    m.set_height(
        number(&default(
            &m.read("comandos.notices.height"),
            "__unused",
            m.read("comandos.notices.height"),
        )),
        false,
    );
    if get(&m.opts, "presence") != JsValue::FALSE {
        let p_opts = object();
        for k in ["transport", "deviceId", "now"] {
            set(&p_opts, k, &get(&m.opts, k))?;
        }
        set(&p_opts, "doc", &m.doc)?;
        set(&p_opts, "deferMs", &400.into())?;
        let sounds = get(&m.opts, "sounds");
        set(
            &p_opts,
            "canPlayAudio",
            &function(move |_| {
                Ok(truthy(&call(&sounds, "isReady", &[]).unwrap_or(JsValue::FALSE)).into())
            }),
        )?;
        let p = presence(p_opts)?;
        call(&p, "start", &[])?;
        if let Ok(mut slot) = m.presence.try_borrow_mut() {
            *slot = p;
        }
    }
    let u = m.clone();
    m.listen(
        &m.root,
        "click",
        function(move |a| {
            let e = a.get(0);
            let t = closest(&e, "[data-nt-act],[data-nt-filter]");
            if !truthy(&t)
                || !truthy(
                    &call(&u.root, "contains", std::slice::from_ref(&t)).unwrap_or(JsValue::FALSE),
                )
            {
                return Ok(JsValue::UNDEFINED);
            }
            let filter = attr_(&t, "data-nt-filter");
            if truthy(&filter) {
                call(&u.controller, "setFilter", &[filter])?;
                return Ok(JsValue::UNDEFINED);
            }
            let id = attr_(&t, "data-nt-id");
            match string(&attr_(&t, "data-nt-act")).as_str() {
                "toggle" | "close" => {
                    u.hidden.set(true);
                    set(&u.c.state, "opened", &false.into())?;
                    u.render()?;
                }
                "max" => u.max(),
                "open" => {
                    call(&u.controller, "open", &[id])?;
                }
                "read" => {
                    call(&u.controller, "markRead", &[Array::of1(&id).into()])?;
                }
                "read-all" => {
                    call(&u.controller, "markAllRead", &[JsValue::NULL])?;
                }
                "read-group" => {
                    call(
                        &u.controller,
                        "markGroupRead",
                        &[attr_(&t, "data-nt-group-key")],
                    )?;
                }
                "float-open" => {
                    call(&u.controller, "openFloat", &[])?;
                }
                "float-close" => {
                    call(&u.controller, "dismissFloat", &[])?;
                }
                "gone-close" => {
                    call(&u.controller, "closeUnavailable", &[])?;
                }
                _ => {}
            }
            Ok(JsValue::UNDEFINED)
        }),
    );
    let u = m.clone();
    m.listen(
        &m.root,
        "pointerdown",
        function(move |a| {
            let e = a.get(0);
            if !grip(&e) || !get(&u.win, "addEventListener").is_function() {
                return Ok(JsValue::UNDEFINED);
            }
            let _ = call(&e, "preventDefault", &[]);
            let y = number(&get(&e, "clientY"));
            let height = u.height.get();
            u.restore.set(None);
            let state = u.clone();
            let move_ = function(move |a| {
                if !state.destroyed.get() {
                    state.set_height(height + y - number(&get(&a.get(0), "clientY")), false);
                }
                Ok(JsValue::UNDEFINED)
            });
            let callbacks = Rc::new(RefCell::new((move_.clone(), JsValue::NULL)));
            let state = u.clone();
            let list = callbacks.clone();
            let up = function(move |_| {
                if let Ok(c) = list.try_borrow() {
                    let _ = call(
                        &state.win,
                        "removeEventListener",
                        &["pointermove".into(), c.0.clone()],
                    );
                    let _ = call(
                        &state.win,
                        "removeEventListener",
                        &["pointerup".into(), c.1.clone()],
                    );
                }
                if !state.destroyed.get() {
                    state.set_height(state.height.get(), true);
                    state.schedule();
                }
                Ok(JsValue::UNDEFINED)
            });
            if let Ok(mut pair) = callbacks.try_borrow_mut() {
                pair.1 = up.clone();
            }
            u.listen(&u.win, "pointermove", move_);
            u.listen(&u.win, "pointerup", up);
            Ok(JsValue::UNDEFINED)
        }),
    );
    let u = m.clone();
    m.listen(
        &m.root,
        "dblclick",
        function(move |a| {
            if grip(&a.get(0)) {
                u.max()
            }
            Ok(JsValue::UNDEFINED)
        }),
    );
    let u = m.clone();
    m.listen(
        &m.root,
        "keydown",
        function(move |a| {
            let e = a.get(0);
            if !grip(&e) {
                return Ok(JsValue::UNDEFINED);
            }
            let key = string(&get(&e, "key"));
            if ["ArrowUp", "ArrowDown"].contains(&key.as_str()) {
                let _ = call(&e, "preventDefault", &[]);
                u.restore.set(None);
                u.set_height(
                    u.height.get() + if key == "ArrowUp" { 32. } else { -32. },
                    true,
                );
            } else if key == "Enter" {
                let _ = call(&e, "preventDefault", &[]);
                u.max()
            }
            Ok(JsValue::UNDEFINED)
        }),
    );
    let box_ = call(&m.doc, "getElementById", &["notice-settings".into()]).unwrap_or(JsValue::NULL);
    if truthy(&box_) {
        let u = m.clone();
        m.listen(
            &box_,
            "change",
            function(move |a| {
                let target = get(&a.get(0), "target");
                let partial = object();
                let mode = attr_(&target, "data-nt-mode");
                if truthy(&mode) {
                    let modes = clone(&get(&get(&u.c.state, "prefs"), "modes"));
                    set(&modes, &string(&mode), &get(&target, "value"))?;
                    set(&partial, "modes", &modes)?;
                } else if truthy(
                    &call(&target, "hasAttribute", &["data-nt-muted".into()])
                        .unwrap_or(JsValue::FALSE),
                ) {
                    set(&partial, "muted", &truthy(&get(&target, "checked")).into())?;
                } else if truthy(
                    &call(&target, "hasAttribute", &["data-nt-volume".into()])
                        .unwrap_or(JsValue::FALSE),
                ) {
                    set(
                        &partial,
                        "volume",
                        &(number(&get(&target, "value")) / 100.).clamp(0., 1.).into(),
                    )?;
                } else {
                    return Ok(JsValue::UNDEFINED);
                }
                let request = call(&u.controller, "setPrefs", &[partial]);
                let state = u.clone();
                wasm_bindgen_futures::spawn_local(async move {
                    if let Err(e) = wait(request).await {
                        let _ = invoke(&get(&state.opts, "onError"), &[e]);
                    }
                    if let Ok(mut last) = state.last_settings.try_borrow_mut() {
                        last.clear();
                    }
                    let _ = state.render_settings();
                });
                Ok(JsValue::UNDEFINED)
            }),
        );
        let u = m.clone();
        m.listen(
            &box_,
            "click",
            function(move |a| {
                let e = a.get(0);
                let target = closest(&e, "[data-nt-preview],[data-nt-local]");
                let sounds = get(&u.opts, "sounds");
                if !truthy(&target) || !truthy(&sounds) {
                    return Ok(JsValue::UNDEFINED);
                }
                if truthy(
                    &call(&target, "hasAttribute", &["data-nt-local".into()])
                        .unwrap_or(JsValue::FALSE),
                ) {
                    let enabled =
                        !truthy(&call(&sounds, "isEnabled", &[]).unwrap_or(JsValue::FALSE));
                    call(&sounds, "setEnabled", &[enabled.into()])?;
                    if enabled {
                        call(&sounds, "unlock", std::slice::from_ref(&e))?;
                    }
                    if let Ok(mut last) = u.last_settings.try_borrow_mut() {
                        last.clear();
                    }
                    let active = get(&u.doc, "activeElement");
                    let _ = call(&active, "blur", &[]);
                    u.render_settings()?;
                    let state = u.clone();
                    let callback = function(move |_| {
                        if !state.destroyed.get()
                            && let Ok(p) = state.presence.try_borrow()
                        {
                            let _ = call(&p, "refresh", &[]);
                        }
                        Ok(JsValue::UNDEFINED)
                    });
                    let id = later(callback, 400.);
                    if let Ok(mut list) = u.timers.try_borrow_mut() {
                        list.push(id);
                    }
                } else {
                    call(&sounds, "unlock", &[e])?;
                    call(&sounds, "preview", &[attr_(&target, "data-nt-preview")])?;
                }
                Ok(JsValue::UNDEFINED)
            }),
        );
    }
    m.arm();
    let u = m.clone();
    m.listen(
        &m.doc,
        "visibilitychange",
        function(move |_| {
            u.arm();
            if u.visible() && !u.destroyed.get() {
                u.c.poll();
            }
            Ok(JsValue::UNDEFINED)
        }),
    );
    let u = m.clone();
    m.listen(
        &m.win,
        "comandos:open-event",
        function(move |a| {
            call(
                &u.controller,
                "requestOpen",
                &[get(&get(&a.get(0), "detail"), "eventId")],
            )?;
            Ok(JsValue::UNDEFINED)
        }),
    );
    let u = m.clone();
    let callback = function(move |_| {
        if !u.destroyed.get()
            && !truthy(
                &call(&u.root, "contains", &[get(&u.doc, "activeElement")])
                    .unwrap_or(JsValue::FALSE),
            )
        {
            u.schedule()
        }
        Ok(JsValue::UNDEFINED)
    });
    let id = timer(
        &m.opts,
        "setInterval",
        "setInterval",
        &[callback, 60000.into()],
    );
    if let Ok(mut clock) = m.clock.try_borrow_mut() {
        *clock = id;
    }
    m.render()?;
    m.c.poll();
    m.watching.set(get(&m.opts, "watch") != JsValue::FALSE);
    let u = m.clone();
    wasm_bindgen_futures::spawn_local(async move {
        while u.watching.get() && !u.destroyed.get() {
            if wait(Ok(u.c.watch_once(25.into()))).await.is_err() {
                if !u.watching.get() || u.destroyed.get() {
                    break;
                }
                let state = u.clone();
                let promise_ = js_sys::Promise::new(&mut move |resolve, _| {
                    if let Ok(mut list) = state.delays.try_borrow_mut() {
                        list.push(resolve.clone().into());
                    }
                    let id = timer(
                        &state.opts,
                        "setTimer",
                        "setTimeout",
                        &[resolve.into(), 2000.into()],
                    );
                    if let Ok(mut list) = state.timers.try_borrow_mut() {
                        list.push(id);
                    }
                });
                let _ = wait(Ok(promise_.into())).await;
            }
        }
    });
    let out = object();
    for (k, v) in [
        ("controller", m.controller.clone()),
        ("root", m.root.clone()),
        ("rootEl", m.root.clone()),
        ("floatEl", m.float.clone()),
        ("stripEl", m.strip.clone()),
        (
            "presence",
            m.presence
                .try_borrow()
                .map(|p| p.clone())
                .unwrap_or(JsValue::NULL),
        ),
    ] {
        set(&out, k, &v)?;
    }
    let u = m.clone();
    method(&out, "render", move |_| {
        u.render()?;
        Ok(JsValue::UNDEFINED)
    })?;
    let u = m.clone();
    method(&out, "toggleStrip", move |a| {
        let open = a.get(0);
        let hidden = if open.is_undefined() {
            !u.hidden.get()
        } else {
            !truthy(&open)
        };
        u.hidden.set(hidden);
        set(&u.c.state, "opened", &(!hidden).into())?;
        if !hidden {
            call(&u.controller, "setCollapsed", &[false.into()])?;
        }
        u.render()?;
        Ok((!hidden).into())
    })?;
    let u = m.clone();
    getter(&out, "hidden", move || u.hidden.get().into())?;
    let u = m.clone();
    getter(&out, "height", move || u.height.get().into())?;
    let u = m.clone();
    method(&out, "setHeight", move |a| {
        let options = a.get(1);
        Ok(u.set_height(
            number(&a.get(0)),
            get(&options, "remember") != JsValue::FALSE,
        )
        .into())
    })?;
    let u = m.clone();
    method(&out, "toggleMax", move |_| {
        u.max();
        Ok(JsValue::UNDEFINED)
    })?;
    method(&out, "destroy", move |_| {
        m.destroy();
        Ok(JsValue::UNDEFINED)
    })?;
    Ok(out)
}
