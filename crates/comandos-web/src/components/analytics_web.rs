use super::super::web_support::*;
use comandos_web_dom::port::utf16_string as string;
use comandos_web_dom::port::{
    from_utf16_json as from_json, to_utf16_json as to_json, utf16_get as get, utf16_set as set,
};
use comandos_web_dom::{bridge::global_set, port::*};
use comandos_web_view::{analytics::Renderer, escape::text as esc};
use serde_json::json;
use std::{cell::RefCell, rc::Rc};
use wasm_bindgen::JsValue;
pub fn mount_render() -> Result<(), JsValue> {
    let api = object();
    method(&api, "create", |a| {
        let model = to_json(&a.get(0));
        let view = to_json(&a.get(1));
        let r = Rc::new(RefCell::new(Renderer::new(model, view)));
        let out = object();
        let state = r.clone();
        method(&out, "html", move |a| {
            let mut r = state
                .try_borrow_mut()
                .map_err(|_| js_sys::Error::new("Analytics renderer busy"))?;
            Ok(utf16_value(&r.html(&string(&a.get(0)))))
        })?;
        method(&out, "phoneDays", move |_| {
            Ok((r.try_borrow().map(|r| r.phone_days()).unwrap_or(0) as f64).into())
        })?;
        Ok(out)
    })?;
    global_set("AnalyticsRender", &api)
}
struct UI {
    el: JsValue,
    opts: JsValue,
    state: JsValue,
    scroll: RefCell<Vec<(JsValue, JsValue)>>,
}
impl UI {
    fn phone(&self) -> bool {
        let width = get(&self.opts, "width");
        let w = if width.is_function() {
            invoke(&width, &[]).unwrap_or(JsValue::NULL)
        } else {
            get(
                &call(&self.el, "getBoundingClientRect", &[]).unwrap_or(JsValue::NULL),
                "width",
            )
        };
        number(&w) < 600.
    }
    fn paint(self: &Rc<Self>) -> Result<(), JsValue> {
        for (el, cb) in self
            .scroll
            .try_borrow_mut()
            .map_err(|_| js_sys::Error::new("Analytics scroll busy"))?
            .drain(..)
        {
            let _ = call(&el, "removeEventListener", &["scroll".into(), cb]);
        }
        let phone = self.phone();
        classes(&self.el, "phone", phone);
        let model = get(&self.state, "model");
        if !truthy(&model) {
            let err = get(&self.state, "error");
            set(
                &self.el,
                "innerHTML",
                &utf16_value(&format!(
                    "<div class=\"mhead\"><h2>Analytics</h2></div><p class=\"dim\">{}</p>",
                    esc(&if truthy(&err) {
                        string(&err)
                    } else {
                        "Leyendo el uso…".into()
                    })
                )),
            )?;
            return Ok(());
        }
        let render = get(&self.opts, "render");
        let render = if truthy(&render) {
            render
        } else {
            global("AnalyticsRender")
        };
        let view = object();
        set(&view, "phone", &phone.into())?;
        set(&view, "phoneDay", &get(&self.state, "phoneDay"))?;
        set(&view, "minOffset", &(-1.).into())?;
        let instance = call(&render, "create", &[model, view])?;
        let html = string(&call(&instance, "html", &[get(&self.state, "tab")])?)
            + "<div class=\"tip\" hidden></div><div class=\"pop\" hidden></div>";
        set(&self.el, "innerHTML", &utf16_value(&html))?;
        if phone {
            for row in all(&self.el, ".bar-row") {
                let bar = call(&row, "closest", &[".bar".into()]).unwrap_or(JsValue::NULL);
                if !truthy(&bar) || !get(&bar, "insertAdjacentHTML").is_function() {
                    continue;
                }
                let count = number(&get(&get(&row, "children"), "length")) as usize;
                let dots = (0..count)
                    .map(|i| format!("<i class=\"{}\"></i>", if i == 0 { "on" } else { "" }))
                    .collect::<String>();
                call(
                    &bar,
                    "insertAdjacentHTML",
                    &[
                        "afterend".into(),
                        format!("<div class=\"sdots\">{dots}</div>").into(),
                    ],
                )?;
                let next = get(&bar, "nextElementSibling");
                let r = row.clone();
                let cb = function(move |_| {
                    let idx = (number(&get(&r, "scrollLeft")) / number(&get(&r, "clientWidth"))
                        + 0.5)
                        .floor() as usize;
                    for (j, d) in all(&next, "i").iter().enumerate() {
                        classes(d, "on", j == idx)
                    }
                    Ok(JsValue::UNDEFINED)
                });
                listen(&row, "scroll", cb.clone());
                if let Ok(mut list) = self.scroll.try_borrow_mut() {
                    list.push((row, cb));
                }
            }
        }
        Ok(())
    }
    fn load(self: &Rc<Self>) -> JsValue {
        let state = self.clone();
        let offset = get(&state.state, "offset");
        let response = invoke(
            &get(&state.opts, "fetchWeek"),
            std::slice::from_ref(&offset),
        );
        let job = promise(async move {
            let result = wait(response).await;
            if number(&offset) != number(&get(&state.state, "offset")) {
                return Ok(JsValue::UNDEFINED);
            }
            match result {
                Ok(model) => {
                    set(&state.state, "model", &model)?;
                    set(&state.state, "error", &"".into())?;
                }
                Err(e) => {
                    let model = get(&state.state, "model");
                    if truthy(&model) {
                        set(&state.state, "offset", &get(&get(&model, "week"), "offset"))?;
                    } else {
                        let msg = get(&e, "message");
                        set(
                            &state.state,
                            "error",
                            &utf16_value(&format!(
                                "No pude leer el uso: {}",
                                string(if truthy(&msg) { &msg } else { &e })
                            )),
                        )?;
                    }
                }
            }
            if number(&offset) == number(&get(&state.state, "offset")) {
                state.paint()?;
            }
            Ok(JsValue::UNDEFINED)
        });
        let _ = set(&self.state, "loading", &job);
        job
    }
    fn pop(&self, btn: JsValue) -> Result<(), JsValue> {
        let pop = query(&self.el, ".pop");
        let data = get(&btn, "dataset");
        let decoded = invoke(&global("decodeURIComponent"), &[get(&data, "pop")])?;
        let rows = call(&global("JSON"), "parse", &[decoded])?;
        let mut html = format!("<h6>{}</h6>", esc(&string(&get(&data, "title"))));
        for row in js_sys::Array::from(&rows).iter() {
            html += &format!(
                "<div><span class=\"dot\" style=\"background:{}\"></span><b>{}</b><span>{}</span><em>{}</em></div>",
                esc(&string(&get(&row, "3"))),
                esc(&unesc(&string(&get(&row, "1")))),
                esc(&unesc(&string(&get(&row, "0")))),
                esc(&unesc(&string(&get(&row, "2"))))
            );
        }
        set(&pop, "innerHTML", &utf16_value(&html))?;
        set(&pop, "hidden", &false.into())?;
        let rect = call(&btn, "getBoundingClientRect", &[])?;
        style(
            &pop,
            "left",
            &format!(
                "{}px",
                8_f64.max(
                    (number(&global("innerWidth")) - number(&get(&pop, "offsetWidth")) - 10.)
                        .min(number(&get(&rect, "right")) + 8.)
                )
            ),
        );
        style(
            &pop,
            "top",
            &format!(
                "{}px",
                (number(&global("innerHeight")) - number(&get(&pop, "offsetHeight")) - 10.)
                    .min(number(&get(&rect, "top")))
            ),
        );
        Ok(())
    }
}
fn unesc(s: &str) -> String {
    let mut out = String::new();
    let mut rest = s;
    while !rest.is_empty() {
        let known = [
            ("&amp;", "&"),
            ("&lt;", "<"),
            ("&gt;", ">"),
            ("&quot;", "\""),
            ("&#39;", "'"),
            ("&#127813;", "🍅"),
        ]
        .iter()
        .find(|(key, _)| rest.starts_with(key));
        if let Some((key, val)) = known {
            out.push_str(val);
            rest = rest.get(key.len()..).unwrap_or("");
        } else if let Some(ch) = rest.chars().next() {
            out.push(ch);
            rest = rest.get(ch.len_utf8()..).unwrap_or("");
        } else {
            break;
        }
    }
    out
}
fn closest(t: &JsValue, sel: &str) -> JsValue {
    call(t, "closest", &[sel.into()]).unwrap_or(JsValue::NULL)
}
fn create(el: JsValue, opts: JsValue) -> Result<JsValue, JsValue> {
    let state = from_json(
        &json!({"tab":super::tab_name(&string(&get(&opts,"tab"))),"offset":0,"phoneDay":null,"model":null,"error":"","loading":null}),
    )?;
    let ui = Rc::new(UI {
        el,
        opts,
        state,
        scroll: RefCell::new(Vec::new()),
    });
    let out = object();
    set(&out, "state", &ui.state)?;
    let u = ui.clone();
    method(&out, "paint", move |_| {
        u.paint()?;
        Ok(JsValue::UNDEFINED)
    })?;
    let u = ui.clone();
    method(&out, "load", move |_| Ok(u.load()))?;
    let u = ui.clone();
    method(&out, "open", move |a| {
        if truthy(&a.get(0)) {
            set(&u.state, "tab", &super::tab_name(&string(&a.get(0))).into())?;
        }
        u.paint()?;
        Ok(u.load())
    })?;
    let u = ui.clone();
    listen(
        &ui.el,
        "click",
        function(move |a| {
            let e = a.get(0);
            let t = get(&e, "target");
            let tab = closest(&t, "[data-tab]");
            if truthy(&tab) {
                let name = super::tab_name(&string(&get(&get(&tab, "dataset"), "tab")));
                set(&u.state, "tab", &name.into())?;
                let cb = get(&u.opts, "onTab");
                if cb.is_function() {
                    invoke(&cb, &[name.into()])?;
                }
                u.paint()?;
                return Ok(JsValue::UNDEFINED);
            }
            let week = closest(&t, "[data-w]");
            let disabled = truthy(&get(&week, "disabled"))
                || truthy(
                    &call(&week, "hasAttribute", &["disabled".into()]).unwrap_or(JsValue::FALSE),
                );
            if truthy(&week) && !disabled {
                set(
                    &u.state,
                    "offset",
                    &(number(&get(&u.state, "offset")) + number(&get(&get(&week, "dataset"), "w")))
                        .clamp(-1., 0.)
                        .into(),
                )?;
                set(&u.state, "phoneDay", &JsValue::NULL)?;
                u.load();
                return Ok(JsValue::UNDEFINED);
            }
            let day = closest(&t, "[data-pd]");
            let model = get(&u.state, "model");
            if truthy(&day) && truthy(&model) {
                let last = number(&get(&get(&model, "days"), "length")) - 3.;
                let cur = get(&u.state, "phoneDay");
                let current = if cur.is_null() { last } else { number(&cur) };
                set(
                    &u.state,
                    "phoneDay",
                    &(current + number(&get(&get(&day, "dataset"), "pd")))
                        .max(0.)
                        .min(last)
                        .into(),
                )?;
                u.paint()?;
                return Ok(JsValue::UNDEFINED);
            }
            let more = closest(&t, "[data-pop]");
            if truthy(&more) {
                let _ = call(&e, "stopPropagation", &[]);
                u.pop(more)?;
                return Ok(JsValue::UNDEFINED);
            }
            let pop = query(&u.el, ".pop");
            if truthy(&pop) && !truthy(&closest(&t, ".pop")) {
                set(&pop, "hidden", &true.into())?;
            }
            Ok(JsValue::UNDEFINED)
        }),
    );
    let u = ui.clone();
    listen(
        &ui.el,
        "pointerover",
        function(move |a| {
            let tip = query(&u.el, ".tip");
            if !truthy(&tip) {
                return Ok(JsValue::UNDEFINED);
            }
            let src = closest(&get(&a.get(0), "target"), "[data-tip]");
            if !truthy(&src) {
                set(&tip, "hidden", &true.into())?;
            } else {
                let data = string(&get(&get(&src, "dataset"), "tip"));
                let mut parts = data.split('|');
                let first = parts.next().unwrap_or("");
                let second = parts.next().unwrap_or("");
                let third = parts.next().unwrap_or("");
                set(
                    &tip,
                    "innerHTML",
                    &utf16_value(&format!(
                        "<b>{}</b><span>{}</span><em>{}</em>",
                        esc(second),
                        esc(first),
                        esc(third)
                    )),
                )?;
                set(&tip, "hidden", &false.into())?;
            }
            Ok(JsValue::UNDEFINED)
        }),
    );
    let u = ui.clone();
    listen(
        &ui.el,
        "pointermove",
        function(move |a| {
            let tip = query(&u.el, ".tip");
            if !truthy(&tip) || truthy(&get(&tip, "hidden")) {
                return Ok(JsValue::UNDEFINED);
            }
            let event = a.get(0);
            let x = (number(&global("innerWidth")) - number(&get(&tip, "offsetWidth")) - 8.)
                .min(number(&get(&event, "clientX")) + 14.);
            let cy = number(&get(&event, "clientY"));
            let height = number(&get(&tip, "offsetHeight"));
            let y = if cy + 18. + height > number(&global("innerHeight")) {
                cy - height - 10.
            } else {
                cy + 18.
            };
            style(&tip, "left", &format!("{x}px"));
            style(&tip, "top", &format!("{y}px"));
            Ok(JsValue::UNDEFINED)
        }),
    );
    Ok(out)
}
pub fn mount() -> Result<(), JsValue> {
    let api = object();
    set(
        &api,
        "TABS",
        &from_json(&json!(["cuentas", "comparar", "pomodoro"]))?,
    )?;
    method(&api, "tabName", |a| {
        Ok(super::tab_name(&string(&a.get(0))).into())
    })?;
    method(&api, "create", |a| create(a.get(0), a.get(1)))?;
    global_set("Analytics", &api)
}
