use super::*;
const SPLIT_KEY: &str = "cc-split-left";
fn viewport_height() -> f64 {
    visible_height(&[
        number(&get(&global("visualViewport"), "height")),
        number(&global("innerHeight")),
        number(&get(&root(), "clientHeight")),
    ])
}
fn pin() -> Result<(), JsValue> {
    if !has_class(&body(), "app") {
        return Ok(());
    }
    if truthy(&global("scrollX")) || truthy(&global("scrollY")) {
        run("scrollTo", &[0.into(), 0.into()])?;
    }
    let panes = id("panes");
    if truthy(&panes) && (truthy(&get(&panes, "scrollTop")) || truthy(&get(&panes, "scrollLeft"))) {
        set(&panes, "scrollTop", &0.into())?;
        set(&panes, "scrollLeft", &0.into())?;
    }
    Ok(())
}
fn update() -> Result<(), JsValue> {
    put("appViewportFrame", 0)?;
    let vv = global("visualViewport");
    for (key, value) in [
        ("--app-height", format!("{}px", viewport_height())),
        (
            "--app-top",
            format!(
                "{}px",
                number(&default(get(&vv, "offsetTop"), 0.into())).round()
            ),
        ),
        (
            "--app-left",
            format!(
                "{}px",
                number(&default(get(&vv, "offsetLeft"), 0.into())).round()
            ),
        ),
        (
            "--app-width",
            if truthy(&get(&vv, "width")) {
                format!("{}px", number(&get(&vv, "width")).round())
            } else {
                "100%".into()
            },
        ),
    ] {
        style(&root(), key, &value);
    }
    pin()
}
fn schedule() -> Result<(), JsValue> {
    if !truthy(&global("appViewportFrame")) {
        put(
            "appViewportFrame",
            run("requestAnimationFrame", &[global("updateAppViewport")])?,
        )?;
    }
    cancel(global("appViewportSettle"));
    put(
        "appViewportSettle",
        later(global("updateAppViewport"), 350.0),
    )
}
fn bounds() -> (f64, f64) {
    split_bounds(number(&default(
        get(&id("panes"), "clientWidth"),
        default(global("innerWidth"), 1200.into()),
    )))
}
fn set_left(px: f64, save: bool, limit: &JsValue) -> Result<JsValue, JsValue> {
    let panes = id("panes");
    if !truthy(&panes) {
        return Ok(JsValue::UNDEFINED);
    }
    let (min, max) = if truthy(limit) {
        (number(&get(limit, "min")), number(&get(limit, "max")))
    } else {
        bounds()
    };
    let value = if px.is_nan() || min.is_nan() || max.is_nan() {
        f64::NAN
    } else {
        px.round().min(max).max(min)
    }; // JavaScript Math.round differs for negative ties, but bounds are positive.
    style(&panes, "--split-left", &format!("{value}px"));
    style(&root(), "--split-left", &format!("{value}px"));
    if save {
        call(
            &global("localStorage"),
            "setItem",
            &[SPLIT_KEY.into(), value.to_string().into()],
        )?;
    }
    Ok(value.into())
}
fn restore() -> Result<(), JsValue> {
    let stored = call(&global("localStorage"), "getItem", &[SPLIT_KEY.into()])?;
    let parsed = run("parseInt", &[default(stored, "".into()), 10.into()])?;
    set_left(
        if number(&parsed).is_finite() {
            number(&parsed)
        } else {
            380.0
        },
        false,
        &JsValue::UNDEFINED,
    )?;
    Ok(())
}
fn apply_layout() -> Result<(), JsValue> {
    let width = number(&default(get(&root(), "clientWidth"), global("innerWidth"))).round();
    let height = viewport_height();
    let coarse = truthy(&get(
        &run("matchMedia", &["(pointer:coarse)".into()])?,
        "matches",
    )) || number(&get(&global("navigator"), "maxTouchPoints")) > 0.0;
    let screen = global("screen");
    let sw = number(&default(get(&screen, "width"), width.into())).round();
    let sh = number(&default(get(&screen, "height"), height.into())).round();
    let orientation = text(&get(&get(&screen, "orientation"), "type"));
    let coarse_h = if orientation.starts_with("portrait") {
        sw.max(sh)
    } else if orientation.starts_with("landscape") {
        sw.min(sh)
    } else {
        sh
    };
    classes(
        &body(),
        "split",
        should_split_layout(width, if coarse { coarse_h } else { height }, coarse),
    );
    restore()?;
    run("showView", &[global("activeView")])?;
    Ok(())
}
fn panel_viewport() -> Result<JsValue, JsValue> {
    if has_class(&body(), "split")
        && !has_class(&body(), "inapp")
        && !has_class(&body(), "panel-hidden")
    {
        let panel = id("view-panel");
        if truthy(&panel) {
            let r = call(&panel, "getBoundingClientRect", &[])?;
            if truthy(&get(&r, "width")) {
                return item(&[("left", get(&r, "left")), ("width", get(&r, "width"))]);
            }
        }
    }
    item(&[("left", 0.into()), ("width", global("innerWidth"))])
}
fn frames_pointer(value: &str) -> Result<(), JsValue> {
    for f in all(&doc(), ".termpane") {
        set(&get(&f, "style"), "pointerEvents", &value.into())?;
    }
    Ok(())
}
fn init_drag() -> Result<(), JsValue> {
    let splitter = id("splitter");
    let panes = id("panes");
    if !truthy(&splitter) || !truthy(&panes) || truthy(&get(&splitter, "_wired")) {
        return Ok(());
    }
    set(&splitter, "_wired", &true.into())?;
    let drag = item(&[
        ("pointer", JsValue::NULL),
        ("rect", JsValue::NULL),
        ("bounds", JsValue::NULL),
        ("pending", JsValue::NULL),
        ("frame", 0.into()),
        ("value", JsValue::NULL),
    ])?;
    let d = drag.clone();
    let apply = function(move |_| {
        set(&d, "frame", &0.into())?;
        let x = get(&d, "pending");
        if get(&d, "pointer").is_null() || x.is_null() {
            return Ok(JsValue::UNDEFINED);
        }
        set(&d, "pending", &JsValue::NULL)?;
        set(
            &d,
            "value",
            &set_left(
                number(&x) - number(&get(&get(&d, "rect"), "left")),
                false,
                &get(&d, "bounds"),
            )?,
        )?;
        Ok(JsValue::UNDEFINED)
    });
    set(&drag, "apply", &apply)?;
    let d = drag.clone();
    let schedule = function(move |_| {
        if !truthy(&get(&d, "frame")) {
            set(
                &d,
                "frame",
                &run("requestAnimationFrame", &[get(&d, "apply")])?,
            )?;
        }
        Ok(JsValue::UNDEFINED)
    });
    set(&drag, "schedule", &schedule)?;
    let d = drag.clone();
    let move_fn = function(move |a| {
        let e = a.get(0);
        if !get(&d, "pointer").is_null() && get(&e, "pointerId") == get(&d, "pointer") {
            set(&d, "pending", &get(&e, "clientX"))?;
            invoke(&get(&d, "schedule"), &[])?;
        }
        Ok(JsValue::UNDEFINED)
    });
    set(&drag, "move", &move_fn)?;
    let d = drag.clone();
    let s = splitter.clone();
    let stop_fn = function(move |a| {
        let e = a.get(0);
        let pointer = get(&d, "pointer");
        if pointer.is_null() || get(&e, "pointerId") != pointer {
            return Ok(JsValue::UNDEFINED);
        }
        if text(&get(&e, "type")) == "pointerup" && number(&get(&e, "clientX")).is_finite() {
            set(&d, "pending", &get(&e, "clientX"))?;
        }
        if truthy(&get(&d, "frame")) {
            run("cancelAnimationFrame", &[get(&d, "frame")])?;
            set(&d, "frame", &0.into())?;
        }
        invoke(&get(&d, "apply"), &[])?;
        let value = get(&d, "value");
        for key in ["pointer", "rect", "bounds", "pending", "value"] {
            set(&d, key, &JsValue::NULL)?;
        }
        classes(&body(), "dragging", false);
        frames_pointer("")?;
        for (kind, handler) in [
            ("pointermove", get(&d, "move")),
            ("pointerup", get(&d, "stop")),
            ("pointercancel", get(&d, "stop")),
        ] {
            call(
                &js_sys::global(),
                "removeEventListener",
                &[kind.into(), handler],
            )?;
        }
        let _ = call(&s, "releasePointerCapture", &[pointer]);
        if !value.is_null() {
            let _ = call(
                &global("localStorage"),
                "setItem",
                &[SPLIT_KEY.into(), string(&value).into()],
            );
        }
        Ok(JsValue::UNDEFINED)
    });
    set(&drag, "stop", &stop_fn)?;
    let d = drag;
    let s = splitter.clone();
    listen(
        &splitter,
        "pointerdown",
        function(move |a| {
            let e = a.get(0);
            if !has_class(&body(), "split") || !get(&d, "pointer").is_null() {
                return Ok(JsValue::UNDEFINED);
            }
            let button = get(&e, "button");
            if !button.is_undefined() && button != 0 {
                return Ok(JsValue::UNDEFINED);
            }
            set(&d, "pointer", &get(&e, "pointerId"))?;
            set(&d, "rect", &call(&panes, "getBoundingClientRect", &[])?)?;
            let (min, max) = bounds();
            set(
                &d,
                "bounds",
                &item(&[("min", min.into()), ("max", max.into())])?,
            )?;
            set(&d, "pending", &get(&e, "clientX"))?;
            set(&d, "value", &JsValue::NULL)?;
            classes(&body(), "dragging", true);
            frames_pointer("none")?;
            let _ = call(&s, "setPointerCapture", &[get(&e, "pointerId")]);
            invoke(&get(&d, "schedule"), &[])?;
            for (kind, f) in [
                ("pointermove", get(&d, "move")),
                ("pointerup", get(&d, "stop")),
                ("pointercancel", get(&d, "stop")),
            ] {
                listen(&js_sys::global(), kind, f);
            }
            call(&e, "preventDefault", &[])?;
            Ok(JsValue::UNDEFINED)
        }),
    );
    listen(&js_sys::global(), "resize", global("restoreSplitLeft"));
    Ok(())
}
pub(super) fn mount() -> Result<(), JsValue> {
    publish("currentViewportHeight", |_| Ok(viewport_height().into()))?;
    publish("pinAppScroll", |_| {
        pin()?;
        Ok(JsValue::UNDEFINED)
    })?;
    publish("updateAppViewport", |_| {
        update()?;
        Ok(JsValue::UNDEFINED)
    })?;
    publish("scheduleAppViewport", |_| {
        schedule()?;
        Ok(JsValue::UNDEFINED)
    })?;
    publish("shouldSplitLayout", |a| {
        Ok(should_split_layout(number(&a.get(0)), number(&a.get(1)), truthy(&a.get(2))).into())
    })?;
    publish("applyAppLayout", |_| {
        apply_layout()?;
        Ok(JsValue::UNDEFINED)
    })?;
    publish("splitBounds", |_| {
        let (min, max) = bounds();
        item(&[("min", min.into()), ("max", max.into())])
    })?;
    publish("setSplitLeft", |a| {
        set_left(number(&a.get(0)), truthy(&a.get(1)), &a.get(2))
    })?;
    publish("panelViewport", |_| panel_viewport())?;
    publish("restoreSplitLeft", |_| {
        restore()?;
        Ok(JsValue::UNDEFINED)
    })?;
    publish("initSplitDrag", |_| {
        init_drag()?;
        Ok(JsValue::UNDEFINED)
    })
}
