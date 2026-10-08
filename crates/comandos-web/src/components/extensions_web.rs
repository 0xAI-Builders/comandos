use super::super::web_support::*;
use super::{Shelf, active, field, shelf_height, text, valid_target, yes};
use comandos_web_dom::{bridge::global_set, port::*};
use serde_json::{Value, json};
use std::{cell::RefCell, rc::Rc};
use wasm_bindgen::JsValue;
struct Ui {
    model: Shelf,
    root: JsValue,
    timer: JsValue,
    dragged: bool,
    pending_focus: Vec<(String, String)>,
}
type Shared = Rc<RefCell<Ui>>;
fn event(f: impl Fn(JsValue) -> Result<(), JsValue> + 'static) -> JsValue {
    function(move |a| {
        f(a.get(0))?;
        Ok(JsValue::UNDEFINED)
    })
}
fn closest(e: &JsValue, selector: &str) -> JsValue {
    call(e, "closest", &[selector.into()]).unwrap_or(JsValue::NULL)
}
fn dataset(el: &JsValue, k: &str) -> String {
    let v = get(&get(el, "dataset"), k);
    if v.is_null() || v.is_undefined() {
        String::new()
    } else {
        string(&v)
    }
}
fn focus_key(el: &JsValue) -> Vec<(String, String)> {
    [
        "kind",
        "id",
        "filter",
        "template",
        "action",
        "batch",
        "batchGroup",
        "batchScope",
    ]
    .into_iter()
    .filter_map(|k| {
        let v = get(&get(el, "dataset"), k);
        (!v.is_null() && !v.is_undefined()).then(|| (k.into(), string(&v)))
    })
    .collect()
}
fn render(ui: &Shared) -> Result<(), JsValue> {
    let mut u = ui
        .try_borrow_mut()
        .map_err(|_| JsValue::from_str("shelf busy"))?;
    let focus = get(&doc(), "activeElement");
    let id = dataset_id(&focus);
    let position = get(&focus, "selectionStart");
    let value = get(&focus, "value");
    let button = closest(&focus, "button");
    let belongs =
        truthy(&call(&u.root, "contains", std::slice::from_ref(&button)).unwrap_or(JsValue::FALSE));
    let key = if belongs {
        focus_key(&button)
    } else if id.is_empty() {
        u.pending_focus.clone()
    } else {
        vec![]
    };
    u.pending_focus = if u.model.sending { key.clone() } else { vec![] };
    let scrolls: Vec<_> = all(&u.root, ".field,.templates")
        .into_iter()
        .map(|x| get(&x, "scrollTop"))
        .collect();
    attr(
        &u.root,
        "aria-busy",
        if u.model.state.is_none() && u.model.error.is_empty() {
            "true"
        } else {
            "false"
        },
    );
    set(&u.root, "innerHTML", &u.model.html().into())?;
    if u.model.state.is_none() {
        u.model.enter = true;
        return Ok(());
    }
    u.model.enter = false;
    for (el, scroll) in all(&u.root, ".field,.templates").into_iter().zip(scrolls) {
        set(&el, "scrollTop", &scroll)?;
    }
    let next = if !id.is_empty() {
        let next = super::super::web_support::id(&id);
        if id == "template-name" && !next.is_null() {
            set(&next, "value", &value)?;
        }
        next
    } else if !key.is_empty() {
        all(&u.root, "button")
            .into_iter()
            .find(|e| key.iter().all(|(k, v)| dataset(e, k) == *v))
            .unwrap_or(JsValue::NULL)
    } else {
        JsValue::NULL
    };
    if !next.is_null() && !truthy(&get(&next, "disabled")) {
        let options = object();
        set(&options, "preventScroll", &true.into())?;
        let _ = call(&next, "focus", &[options]);
        if position.as_f64().is_some() {
            let _ = call(&next, "setSelectionRange", &[position.clone(), position]);
        }
    }
    Ok(())
}
fn dataset_id(el: &JsValue) -> String {
    get(el, "id").as_string().unwrap_or_default()
}
async fn fetch(path: String, body: Option<Value>) -> Result<Value, JsValue> {
    let headers = object();
    if let Ok(token) = call(&global("localStorage"), "getItem", &["cc_token".into()])
        && truthy(&token)
    {
        set(&headers, "X-Comandos-Token", &token)?;
    }
    let options = object();
    set(
        &options,
        "method",
        &if body.is_some() { "POST" } else { "GET" }.into(),
    )?;
    set(&options, "headers", &headers)?;
    if let Some(body) = body {
        set(&headers, "Content-Type", &"application/json".into())?;
        set(&options, "body", &body.to_string().into())?;
    }
    let response = wait(call(&js_sys::global(), "fetch", &[path.into(), options])).await?;
    let data = wait(call(&response, "json", &[])).await?;
    if !truthy(&get(&response, "ok")) || get(&data, "ok").as_bool() == Some(false) {
        let error = get(&data, "error");
        let message = get(&data, "message");
        return Err(js_sys::Error::new(&if truthy(&error) {
            string(&error)
        } else if truthy(&message) {
            string(&message)
        } else {
            "No se pudo completar la acción.".into()
        })
        .into());
    }
    Ok(to_json(&data))
}
fn query_string(value: &Value) -> Result<String, JsValue> {
    let params = js_sys::Reflect::construct(
        &global("URLSearchParams").into(),
        &[from_json(value)?].into_iter().collect::<js_sys::Array>(),
    )?;
    Ok(string(&call(&params, "toString", &[])?))
}
fn error(e: JsValue) -> String {
    let m = get(&e, "message");
    if truthy(&m) { string(&m) } else { string(&e) }
}
fn schedule(ui: &Shared) {
    let Ok(mut u) = ui.try_borrow_mut() else {
        return;
    };
    cancel(u.timer.clone());
    let delay = if u.model.state.as_ref().is_some_and(active) {
        2000.0
    } else {
        12000.0
    };
    let next = ui.clone();
    u.timer = later(
        event(move |_| {
            refresh(next.clone());
            Ok(())
        }),
        delay,
    );
}
fn refresh(ui: Shared) {
    let (generation, target, loading) = {
        let Ok(mut u) = ui.try_borrow_mut() else {
            return;
        };
        cancel(u.timer.clone());
        if u.model.sending {
            drop(u);
            schedule(&ui);
            return;
        }
        let loading = u.model.state.is_none();
        if loading {
            u.model.error.clear();
        }
        (u.model.generation, u.model.target.clone(), loading)
    };
    if loading {
        let _ = render(&ui);
    }
    wasm_bindgen_futures::spawn_local(async move {
        let result = match query_string(&target) {
            Ok(query) => fetch(format!("/pane-extensions?{query}"), None).await,
            Err(e) => Err(e),
        };
        let repaint = {
            let Ok(mut u) = ui.try_borrow_mut() else {
                return;
            };
            if generation != u.model.generation {
                return;
            }
            match result {
                Ok(next) => u.model.receive(generation, next),
                Err(e) => {
                    u.model.error = error(e);
                    true
                }
            }
        };
        if repaint {
            let _ = render(&ui);
        }
        schedule(&ui);
    });
}
fn mutate(ui: Shared, suffix: String, extra: Value) {
    let (generation, body) = {
        let Ok(mut u) = ui.try_borrow_mut() else {
            return;
        };
        if u.model.sending || u.model.stale || u.model.state.is_none() {
            return;
        }
        u.model.sending = true;
        cancel(u.timer.clone());
        u.model.error.clear();
        u.model.message.clear();
        u.model.generation += 1;
        let mut body = u.model.guards();
        if let (Some(m), Some(x)) = (body.as_object_mut(), extra.as_object()) {
            m.extend(x.clone());
        }
        (u.model.generation, body)
    };
    let _ = render(&ui);
    wasm_bindgen_futures::spawn_local(async move {
        let result = fetch(format!("/pane-extensions{suffix}"), Some(body)).await;
        let (fresh, failure) = {
            let Ok(mut u) = ui.try_borrow_mut() else {
                return;
            };
            if generation != u.model.generation {
                return;
            }
            let fresh = match result {
                Ok(data) => u.model.accept_save(&suffix, &extra, data),
                Err(e) => {
                    u.model.error = error(e);
                    false
                }
            };
            u.model.sending = false;
            (fresh, u.model.error.clone())
        };
        if fresh && failure.is_empty() {
            let _ = render(&ui);
            schedule(&ui);
        } else {
            refresh_after_mutation(ui, generation, failure).await;
        }
    });
}
async fn refresh_after_mutation(ui: Shared, generation: u64, failure: String) {
    let target = {
        let Ok(u) = ui.try_borrow() else { return };
        u.model.target.clone()
    };
    let result = match query_string(&target) {
        Ok(q) => fetch(format!("/pane-extensions?{q}"), None).await,
        Err(e) => Err(e),
    };
    {
        let Ok(mut u) = ui.try_borrow_mut() else {
            return;
        };
        if generation != u.model.generation {
            return;
        }
        match result {
            Ok(next) => {
                u.model.receive(generation, next);
            }
            Err(e) => u.model.error = error(e),
        }
        if !failure.is_empty() {
            u.model.error = failure;
        }
    }
    let _ = render(&ui);
    schedule(&ui);
}
fn choose(ui: Shared, harness: String) {
    if let Ok(mut u) = ui.try_borrow_mut() {
        u.model.generation += 1;
        u.model.state = None;
        u.model.stale = false;
        u.model.error.clear();
        u.model.message.clear();
        if let Some(m) = u.model.target.as_object_mut() {
            m.insert("harness".into(), json!(harness));
        }
    }
    refresh(ui);
}
fn toggle(ui: Shared, k: &str, id: &str, on: bool) {
    let d = ui.try_borrow().ok().and_then(|u| u.model.toggle(k, id, on));
    if let Some(d) = d {
        mutate(ui, String::new(), json!({"desired":d}));
    }
}
fn batch(ui: Shared, on: bool, group: Option<&str>, visible: bool) {
    let d = ui
        .try_borrow()
        .ok()
        .and_then(|u| u.model.batch(on, group, visible));
    if let Some(d) = d {
        mutate(ui, String::new(), json!({"desired":d}));
    }
}
fn apply(ui: Shared, interrupt: bool) {
    if ui.try_borrow().map_or(true, |u| {
        u.model.locked()
            || u.model
                .state
                .as_ref()
                .is_none_or(|s| field(s, "applySupported").as_bool() == Some(false))
    }) {
        return;
    }
    let request_id = call(&global("crypto"), "randomUUID", &[]).unwrap_or_else(|_| {
        format!(
            "{}-{:x}",
            js_sys::Date::now(),
            (js_sys::Math::random() * u64::MAX as f64) as u64
        )
        .into()
    });
    mutate(
        ui,
        "/apply".into(),
        json!({"requestId":string(&request_id),"interrupt":interrupt}),
    );
}
fn click(ui: Shared, e: JsValue) -> Result<(), JsValue> {
    if let Ok(mut u) = ui.try_borrow_mut()
        && u.dragged
    {
        u.dragged = false;
        return Ok(());
    }
    let b = closest(&get(&e, "target"), "button");
    if b.is_null() || truthy(&get(&b, "disabled")) {
        return Ok(());
    }
    let d = get(&b, "dataset");
    let batch_value = get(&d, "batch");
    if truthy(&batch_value) {
        let _ = call(&e, "preventDefault", &[]);
        let group = get(&d, "batchGroup").as_string();
        batch(
            ui,
            string(&batch_value) == "on",
            group.as_deref(),
            dataset(&b, "batchScope") == "visible",
        );
        return Ok(());
    }
    let kind = dataset(&b, "kind");
    if !kind.is_empty() {
        toggle(ui, &kind, &dataset(&b, "id"), dataset(&b, "on") != "true");
        return Ok(());
    }
    let filter = dataset(&b, "filter");
    if !filter.is_empty() {
        if let Ok(mut u) = ui.try_borrow_mut() {
            u.model.filter = filter;
        }
        return render(&ui);
    }
    let template = dataset(&b, "template");
    if !template.is_empty() {
        mutate(ui, "/template".into(), json!({"templateId":template}));
        return Ok(());
    }
    match dataset(&b, "action").as_str() {
        "close" => close_shelf(),
        "refresh" => {
            if let Ok(mut u) = ui.try_borrow_mut() {
                u.model.generation += 1;
                u.model.state = None;
                u.model.stale = false;
            }
            refresh(ui)
        }
        "apply" => apply(ui, false),
        "interrupt" => apply(ui, true),
        "discard" => {
            let loaded = ui
                .try_borrow()
                .ok()
                .and_then(|u| u.model.state.as_ref().map(|s| field(s, "loaded").clone()));
            if let Some(d) = loaded.filter(|v| !v.is_null()) {
                mutate(ui, String::new(), json!({"desired":d}));
            }
        }
        "unused" => {
            let desired = ui.try_borrow().ok().and_then(|u| {
                let s = u.model.state.as_ref()?;
                let mut d = field(s, "desired").clone();
                for (k, r, _) in u.model.items() {
                    let id = text(field(r, "id"));
                    if yes(field(r, "toggleable"))
                        && field(field(field(field(s, "usage"), "counts"), k), &id).as_i64()
                            == Some(0)
                        && let Some(m) = d.get_mut(k).and_then(Value::as_object_mut)
                    {
                        m.insert(id, json!(false));
                    }
                }
                Some(d)
            });
            if let Some(d) = desired {
                mutate(ui, String::new(), json!({"desired":d}));
            }
        }
        action @ ("cancel" | "recover") => {
            let id = ui.try_borrow().ok().and_then(|u| {
                u.model.state.as_ref().map(|s| {
                    let op = field(s, "operation");
                    let id = field(op, "operationId");
                    if id.is_null() {
                        field(op, "id").clone()
                    } else {
                        id.clone()
                    }
                })
            });
            if let Some(id) = id {
                mutate(ui, format!("/{action}"), json!({"operationId":id}));
            }
        }
        _ => {}
    }
    Ok(())
}
fn close_shelf() {
    let handler = get(&get(&global("webkit"), "messageHandlers"), "extensions");
    if truthy(&handler) {
        let _ = call(&handler, "postMessage", &["close".into()]);
    } else {
        let parent = global("parent");
        if parent != JsValue::from(js_sys::global()) {
            let _ = call(
                &parent,
                "postMessage",
                &[
                    from_json(&json!({"type":"comandos-extensions-close"}))
                        .unwrap_or(JsValue::NULL),
                    get(&global("location"), "origin"),
                ],
            );
        }
    }
}
fn drag(ui: Shared, e: JsValue) -> Result<(), JsValue> {
    let b = closest(&get(&e, "target"), ".bubble");
    if b.is_null() || truthy(&get(&b, "disabled")) || number(&get(&e, "button")) != 0.0 {
        return Ok(());
    }
    let (x, y) = (number(&get(&e, "clientX")), number(&get(&e, "clientY")));
    let ghost = Rc::new(RefCell::new(JsValue::NULL));
    let listeners = Rc::new(RefCell::new(Vec::<(String, JsValue)>::new()));
    let g = ghost.clone();
    let u = ui.clone();
    let button = b.clone();
    let mv = event(move |e| {
        let (cx, cy) = (number(&get(&e, "clientX")), number(&get(&e, "clientY")));
        let mut g = g
            .try_borrow_mut()
            .map_err(|_| JsValue::from_str("drag busy"))?;
        if g.is_null() && (cx - x).hypot(cy - y) > 7.0 {
            *g = call(&button, "cloneNode", &[true.into()])?;
            classes(&g, "dragghost", true);
            call(&get(&doc(), "body"), "append", &[g.clone()])?;
            if let Ok(mut u) = u.try_borrow_mut() {
                u.dragged = true;
            }
        }
        if g.is_null() {
            return Ok(());
        }
        call(&e, "preventDefault", &[])?;
        style(&g, "left", &format!("{cx}px"));
        style(&g, "top", &format!("{cy}px"));
        let hit = closest(
            &call(&doc(), "elementFromPoint", &[cx.into(), cy.into()]).unwrap_or(JsValue::NULL),
            "[data-zone]",
        );
        if let Ok(u) = u.try_borrow() {
            for z in all(&u.root, "[data-zone]") {
                classes(&z, "hot", z == hit)
            }
        }
        Ok(())
    });
    let g = ghost;
    let l = listeners.clone();
    let end = event(move |e| {
        if let Ok(l) = l.try_borrow() {
            for (t, f) in l.iter() {
                let _ = call(
                    &doc(),
                    "removeEventListener",
                    &[t.clone().into(), f.clone()],
                );
            }
        }
        if let Ok(u) = ui.try_borrow() {
            for z in all(&u.root, "[data-zone]") {
                classes(&z, "hot", false)
            }
        }
        let g = g.try_borrow().map_err(|_| JsValue::from_str("drag busy"))?;
        if !g.is_null() {
            let _ = call(&g, "remove", &[]);
            if get(&e, "type").as_string().as_deref() != Some("pointercancel") {
                let zone = closest(
                    &call(
                        &doc(),
                        "elementFromPoint",
                        &[get(&e, "clientX"), get(&e, "clientY")],
                    )
                    .unwrap_or(JsValue::NULL),
                    "[data-zone]",
                );
                if !zone.is_null() {
                    toggle(
                        ui.clone(),
                        &dataset(&b, "kind"),
                        &dataset(&b, "id"),
                        dataset(&zone, "zone") == "on",
                    );
                }
            }
            let u = ui.clone();
            later(
                event(move |_| {
                    if let Ok(mut u) = u.try_borrow_mut() {
                        u.dragged = false;
                    }
                    Ok(())
                }),
                0.0,
            );
        }
        if let Ok(mut l) = l.try_borrow_mut() {
            l.clear();
        }
        Ok(())
    });
    let options = object();
    set(&options, "passive", &false.into())?;
    call(
        &doc(),
        "addEventListener",
        &["pointermove".into(), mv.clone(), options],
    )?;
    for t in ["pointerup", "pointercancel"] {
        listen(&doc(), t, end.clone())
    }
    if let Ok(mut l) = listeners.try_borrow_mut() {
        *l = vec![
            ("pointermove".into(), mv),
            ("pointerup".into(), end.clone()),
            ("pointercancel".into(), end),
        ];
    }
    Ok(())
}
fn shelf(root: JsValue, target: Value) {
    let ui = Rc::new(RefCell::new(Ui {
        root: root.clone(),
        model: Shelf::new(target),
        timer: JsValue::NULL,
        dragged: false,
        pending_focus: vec![],
    }));
    let u = ui.clone();
    let toggle = event(move |e| {
        let t = get(&e, "target");
        let key = dataset(&t, "group");
        if !key.is_empty()
            && let Ok(mut u) = u.try_borrow_mut()
        {
            if truthy(&get(&t, "open")) {
                u.model.closed_groups.remove(&key);
            } else {
                u.model.closed_groups.insert(key);
            }
        }
        Ok(())
    });
    call(
        &root,
        "addEventListener",
        &["toggle".into(), toggle, true.into()],
    )
    .ok();
    let u = ui.clone();
    listen(&root, "click", event(move |e| click(u.clone(), e)));
    let u = ui.clone();
    listen(
        &root,
        "input",
        event(move |e| {
            let t = get(&e, "target");
            let id = dataset_id(&t);
            if let Ok(mut u) = u.try_borrow_mut() {
                if id == "ext-search" {
                    u.model.query = string(&get(&t, "value"));
                } else if id == "template-name" {
                    u.model.template_name = string(&get(&t, "value"));
                }
            }
            if id == "ext-search" {
                render(&u)?;
            }
            Ok(())
        }),
    );
    let u = ui.clone();
    listen(
        &root,
        "change",
        event(move |e| {
            let t = get(&e, "target");
            if dataset_id(&t) == "ext-harness" {
                choose(u.clone(), string(&get(&t, "value")));
            }
            Ok(())
        }),
    );
    let u = ui.clone();
    let r = root.clone();
    listen(
        &root,
        "submit",
        event(move |e| {
            call(&e, "preventDefault", &[])?;
            let name = string(&get(&query(&r, "#template-name"), "value"))
                .trim()
                .to_owned();
            if !name.is_empty() {
                mutate(u.clone(), "/template".into(), json!({"name":name}));
            }
            Ok(())
        }),
    );
    let u = ui.clone();
    listen(&root, "pointerdown", event(move |e| drag(u.clone(), e)));
    listen(
        &doc(),
        "keydown",
        event(|e| {
            if get(&e, "key").as_string().as_deref() == Some("Escape") {
                close_shelf()
            }
            Ok(())
        }),
    );
    refresh(ui);
}
const HEIGHT_KEY: &str = "cc_pane_shelf_height";
fn saved() -> f64 {
    call(&global("localStorage"), "getItem", &[HEIGHT_KEY.into()])
        .ok()
        .and_then(|v| v.as_string())
        .and_then(|s| s.parse::<f64>().ok())
        .filter(|n| *n != 0.0)
        .unwrap_or(0.0)
}
fn heights() -> Vec<f64> {
    vec![
        number(&get(&global("visualViewport"), "height")),
        number(&global("innerHeight")),
        number(&get(&get(&doc(), "documentElement"), "clientHeight")),
    ]
}
fn current_height() -> f64 {
    call(&id("pane-extensions-frame"), "getBoundingClientRect", &[])
        .ok()
        .map(|r| number(&get(&r, "height")))
        .filter(|h| *h > 0.0)
        .unwrap_or(180.0)
}
fn set_height(px: f64, save: bool) {
    let h = shelf_height(px, &heights());
    style(
        &get(&doc(), "documentElement"),
        "--pane-shelf-height",
        &format!("{h}px"),
    );
    attr(&id("pane-extensions-grip"), "aria-valuenow", &h.to_string());
    if save {
        let _ = call(
            &global("localStorage"),
            "setItem",
            &[HEIGHT_KEY.into(), h.to_string().into()],
        );
    }
}
fn restore_height() {
    let h = saved();
    if h != 0.0 {
        set_height(h, false)
    } else {
        let _ = call(
            &get(&get(&doc(), "documentElement"), "style"),
            "removeProperty",
            &["--pane-shelf-height".into()],
        );
    }
    let grip = id("pane-extensions-grip");
    if !grip.is_null() && !id("pane-extensions-frame").is_null() {
        attr(
            &grip,
            "aria-valuemax",
            &shelf_height(f64::MAX, &heights()).to_string(),
        );
        attr(
            &grip,
            "aria-valuenow",
            &current_height().round().to_string(),
        );
    }
}
fn resize_event() {
    if let Ok(event) = js_sys::Reflect::construct(
        &global("Event").into(),
        &[JsValue::from_str("resize")]
            .into_iter()
            .collect::<js_sys::Array>(),
    ) {
        let _ = call(&js_sys::global(), "dispatchEvent", &[event]);
    }
}
fn close_frame() {
    for name in ["pane-extensions-frame", "pane-extensions-grip"] {
        let _ = call(&id(name), "remove", &[]);
    }
    for c in ["pane-extensions-open", "pane-extensions-resizing"] {
        classes(&get(&doc(), "body"), c, false)
    }
    resize_event();
}
fn grip() -> Result<(), JsValue> {
    if !id("pane-extensions-grip").is_null() {
        return Ok(());
    }
    let grip = call(&doc(), "createElement", &["div".into()])?;
    set(&grip, "id", &"pane-extensions-grip".into())?;
    set(&grip, "tabIndex", &0.into())?;
    set(
        &grip,
        "title",
        &"Arrastra para cambiar la altura · doble clic restablece".into(),
    )?;
    for (k, v) in [
        ("role", "separator"),
        ("aria-orientation", "horizontal"),
        ("aria-label", "Altura del estante de extensiones"),
        ("aria-valuemin", "180"),
    ] {
        attr(&grip, k, v)
    }
    let start = Rc::new(RefCell::new(None::<(f64, f64)>));
    let last = Rc::new(RefCell::new(0.0));
    let s = start.clone();
    let g = grip.clone();
    listen(
        &grip,
        "pointerdown",
        event(move |e| {
            if number(&get(&e, "button")) != 0.0 {
                return Ok(());
            }
            call(&e, "preventDefault", &[])?;
            call(&g, "setPointerCapture", &[get(&e, "pointerId")])?;
            if let Ok(mut s) = s.try_borrow_mut() {
                *s = Some((number(&get(&e, "clientY")), current_height()));
            }
            classes(&get(&doc(), "body"), "pane-extensions-resizing", true);
            Ok(())
        }),
    );
    let s = start.clone();
    listen(
        &grip,
        "pointermove",
        event(move |e| {
            if let Ok(s) = s.try_borrow()
                && let Some((y, h)) = *s
            {
                set_height(h + y - number(&get(&e, "clientY")), false);
                let now = call(&global("performance"), "now", &[])
                    .map(|v| number(&v))
                    .unwrap_or(0.0);
                if let Ok(mut last) = last.try_borrow_mut()
                    && now - *last > 100.0
                {
                    *last = now;
                    resize_event();
                }
            }
            Ok(())
        }),
    );
    let end = event(move |_| {
        let Ok(mut s) = start.try_borrow_mut() else {
            return Ok(());
        };
        if s.take().is_some() {
            classes(&get(&doc(), "body"), "pane-extensions-resizing", false);
            set_height(current_height(), true);
            resize_event();
        }
        Ok(())
    });
    for t in ["pointerup", "pointercancel", "lostpointercapture"] {
        listen(&grip, t, end.clone())
    }
    listen(
        &grip,
        "keydown",
        event(|e| {
            let h = match get(&e, "key").as_string().as_deref() {
                Some("ArrowUp") => current_height() + 24.0,
                Some("ArrowDown") => current_height() - 24.0,
                Some("Home") => shelf_height(f64::MAX, &heights()),
                Some("End") => 180.0,
                _ => return Ok(()),
            };
            call(&e, "preventDefault", &[])?;
            set_height(h, true);
            resize_event();
            Ok(())
        }),
    );
    listen(
        &grip,
        "dblclick",
        event(|_| {
            let _ = call(&global("localStorage"), "removeItem", &[HEIGHT_KEY.into()]);
            restore_height();
            resize_event();
            Ok(())
        }),
    );
    call(&get(&doc(), "body"), "append", &[grip])?;
    Ok(())
}
fn open(session: String, pane: String, harness: String) -> Result<(), JsValue> {
    if !valid_target(&session, &pane) {
        return Ok(());
    }
    let handler = get(&get(&global("webkit"), "messageHandlers"), "centro");
    if truthy(&handler) {
        call(
            &handler,
            "postMessage",
            &[
                json!({"type":"extensions","session":session,"pane":pane,"harness":harness})
                    .to_string()
                    .into(),
            ],
        )?;
        return Ok(());
    }
    let mut frame = id("pane-extensions-frame");
    if frame.is_null() {
        frame = call(&doc(), "createElement", &["iframe".into()])?;
        set(&frame, "id", &"pane-extensions-frame".into())?;
        set(
            &frame,
            "title",
            &"Extensiones del panel seleccionado".into(),
        )?;
        for (k, v) in [
            ("position", "fixed"),
            ("left", "var(--app-left,0px)"),
            (
                "top",
                "calc(var(--app-top,0px) + var(--app-height,100dvh) - var(--pane-shelf-height,55vh))",
            ),
            ("width", "var(--app-width,100%)"),
            ("height", "var(--pane-shelf-height,55vh)"),
            ("border", "0"),
            ("z-index", "1000"),
            ("box-shadow", "0 -12px 40px #0007"),
        ] {
            style(&frame, k, v)
        }
        call(&get(&doc(), "body"), "append", &[frame.clone()])?;
        grip()?;
        restore_height();
        classes(&get(&doc(), "body"), "pane-extensions-open", true);
        resize_event();
    }
    let mut target = json!({"session":session,"pane":pane});
    if !harness.is_empty()
        && harness != "shell"
        && let Some(m) = target.as_object_mut()
    {
        m.insert("harness".into(), json!(harness));
    }
    set(
        &frame,
        "src",
        &format!("/extensions.html?{}", query_string(&target)?).into(),
    )
}
pub fn mount() -> Result<(), JsValue> {
    global_set(
        "openPaneExtensions",
        &function(|a| {
            open(
                string(&a.get(0)),
                string(&a.get(1)),
                a.get(2).as_string().unwrap_or_default(),
            )?;
            Ok(JsValue::UNDEFINED)
        }),
    )
}
pub fn attach() -> Result<(), JsValue> {
    let resize = event(|_| {
        let frame = id("pane-extensions-frame");
        let resizing = truthy(
            &call(
                &get(&get(&doc(), "body"), "classList"),
                "contains",
                &["pane-extensions-resizing".into()],
            )
            .unwrap_or(JsValue::FALSE),
        );
        if !frame.is_null() && saved() != 0.0 && !resizing {
            set_height(saved(), false)
        }
        Ok(())
    });
    listen(&js_sys::global(), "resize", resize.clone());
    let viewport = global("visualViewport");
    if truthy(&viewport) {
        listen(&viewport, "resize", resize)
    }
    listen(
        &js_sys::global(),
        "message",
        event(|e| {
            let frame = id("pane-extensions-frame");
            if !frame.is_null()
                && get(&e, "origin") == get(&global("location"), "origin")
                && get(&e, "source") == get(&frame, "contentWindow")
                && get(&get(&e, "data"), "type").as_string().as_deref()
                    == Some("comandos-extensions-close")
            {
                close_frame();
            }
            Ok(())
        }),
    );
    wasm_bindgen_futures::spawn_local(async {
        if let Ok(p) = fetch("/prefs".into(), None).await {
            let s = text(field(&p, "button_style"));
            if matches!(
                s.as_str(),
                "sutil" | "arcade" | "tecla" | "pixel" | "consola"
            ) {
                let _ = set(
                    &get(&get(&doc(), "documentElement"), "dataset"),
                    "btnStyle",
                    &s.into(),
                );
            }
        }
    });
    let root = id("extensions");
    if !root.is_null() {
        let params = js_sys::Reflect::construct(
            &global("URLSearchParams").into(),
            &[get(&global("location"), "search")]
                .into_iter()
                .collect::<js_sys::Array>(),
        )?;
        let mut target = json!({});
        if let Some(m) = target.as_object_mut() {
            for k in ["session", "pane", "harness"] {
                let v = call(&params, "get", &[k.into()])?
                    .as_string()
                    .unwrap_or_default();
                if k != "harness" || !v.is_empty() {
                    m.insert(k.into(), json!(v));
                }
            }
        }
        shelf(root, target);
    }
    Ok(())
}
