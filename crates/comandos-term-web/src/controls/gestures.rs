//! Terminal-specific pane gestures; the renderer still owns ordinary mouse input.
use super::*;
#[derive(Clone, Copy, PartialEq)]
enum Phase {
    Idle,
    Pending,
    Scrolling,
    Resizing,
}
#[derive(Clone, Copy, PartialEq)]
struct Cell {
    col: i32,
    row: i32,
}
pub(super) struct Gesture {
    phase: Phase,
    touch: JsValue,
    touch_id: JsValue,
    x: f64,
    y: f64,
    last_y: f64,
    wheel: f64,
    timer: JsValue,
    frame: JsValue,
    anchor: Option<(Cell, bool)>,
    last: Option<Cell>,
    pending: Option<Cell>,
    pressed: bool,
    history_opened: bool,
    indicator: JsValue,
    tmux_lines: f64,
    tmux_point: Option<Cell>,
    tmux_frame: JsValue,
    tmux_flight: bool,
    refresh: bool,
    refresh_timer: JsValue,
    mouse: Option<(f64, f64, bool)>,
}
fn num(v: &JsValue, k: &str) -> f64 {
    number(&get(v, k))
}
fn cell(ui: &Ui, t: &JsValue) -> Option<Cell> {
    let term = terminal(ui);
    let screen = get(&term, "screen");
    let r = call(&screen, "getBoundingClientRect", &[]).ok()?;
    let (w, h, cols, rows) = (
        num(&r, "width"),
        num(&r, "height"),
        num(&term, "cols"),
        num(&term, "rows"),
    );
    if w <= 0. || h <= 0. || cols <= 0. || rows <= 0. {
        return None;
    }
    Some(Cell {
        col: crate::number_text::clamp(
            ((num(t, "clientX") - num(&r, "left")) / (w / cols)).floor() + 1.,
            1.,
            cols,
        ) as i32,
        row: crate::number_text::clamp(
            ((num(t, "clientY") - num(&r, "top")) / (h / rows)).floor() + 1.,
            1.,
            rows,
        ) as i32,
    })
}
fn mouse_on(ui: &Ui) -> bool {
    get(&get(&terminal(ui), "modes"), "mouseTrackingMode")
        .as_string()
        .as_deref()
        != Some("none")
}
fn selected(ui: &Ui) -> bool {
    call(&terminal(ui), "hasSelection", &[]).is_ok_and(|v| truthy(&v))
}
fn owned(e: &JsValue) -> bool {
    let own = |node: JsValue| {
        call(&node,"closest",&["#term-toolbar, #selection-toolbar, #pane-dialog, #paste-dialog, #mobile-compose, #terminal-history, #comandos-link-menu".into()]).is_ok_and(|v|truthy(&v))
    };
    if own(get(e, "target")) {
        return true;
    }
    call(e, "composedPath", &[])
        .ok()
        .is_some_and(|p| Array::from(&p).iter().any(own))
}
fn send(ui: &Ui, button: i32, c: Cell, release: bool) {
    let _ = call(
        &terminal(ui),
        "sendInput",
        &[
            format!(
                "\x1b[<{button};{};{}{}",
                c.col,
                c.row,
                if release { "m" } else { "M" }
            )
            .into(),
            false.into(),
        ],
    );
}
fn stop(e: &JsValue) {
    let _ = call(e, "preventDefault", &[]);
    let _ = call(e, "stopPropagation", &[]);
}
fn cancel(timer: &JsValue, frame: bool) {
    if truthy(timer) {
        let _ = call(
            &js_sys::global(),
            if frame {
                "cancelAnimationFrame"
            } else {
                "clearTimeout"
            },
            std::slice::from_ref(timer),
        );
    }
}
fn motion(ui: &Ui, g: &Rc<RefCell<Gesture>>) {
    let pending = {
        let mut g = g.borrow_mut();
        g.frame = JsValue::NULL;
        if g.phase != Phase::Resizing || !g.pressed {
            return;
        }
        g.pending.take().filter(|c| Some(*c) != g.last)
    };
    if let Some(c) = pending
        && call(&terminal(ui), "isOpen", &[]).is_ok_and(|v| truthy(&v))
    {
        send(ui, 32, c, false);
        g.borrow_mut().last = Some(c);
    }
}
fn finish(ui: &Ui, g: &Rc<RefCell<Gesture>>) {
    let (timer, frame, resizing, last) = {
        let g = g.borrow();
        (
            g.timer.clone(),
            g.frame.clone(),
            g.phase == Phase::Resizing && g.pressed,
            g.last,
        )
    };
    cancel(&timer, false);
    cancel(&frame, true);
    if resizing {
        motion(ui, g);
        if let Some(c) = g.borrow().last.or(last) {
            send(ui, 0, c, true);
        }
        g.borrow_mut().refresh = true;
        cancel(&g.borrow().refresh_timer, false);
    }
    let mut g = g.borrow_mut();
    g.phase = Phase::Idle;
    g.timer = JsValue::NULL;
    g.frame = JsValue::NULL;
    g.anchor = None;
    g.last = None;
    g.pending = None;
    g.pressed = false;
    g.history_opened = false;
    g.touch_id = JsValue::NULL;
    g.wheel = 0.;
    let _ = set(&get(&g.indicator, "style"), "display", &"none".into());
}
fn snap(ui: &Ui, c: Cell) -> Option<(Cell, bool)> {
    let text = |col: i32, row: i32| {
        call(&terminal(ui), "cellChar", &[col.into(), row.into()])
            .ok()
            .and_then(|v| v.as_string())
            .unwrap_or_default()
    };
    let border = |col, row, vertical| {
        let s = text(col, row);
        !s.is_empty()
            && if vertical {
                "│┃|║╎┆┊╏┇┋┌┐└┘├┤┬┴┼╭╮╯╰╔╗╚╝╠╣╦╩╬"
            } else {
                "─━-═╌┄┈╍┅┉┌┐└┘├┤┬┴┼╭╮╯╰╔╗╚╝╠╣╦╩╬"
            }
            .contains(&s)
    };
    for d in [0, -1, 1, -2, 2] {
        if border(c.col + d, c.row, true)
            && (-2..=2)
                .filter(|r| border(c.col + d, c.row + r, true))
                .count()
                >= 2
        {
            return Some((
                Cell {
                    col: c.col + d,
                    row: c.row,
                },
                true,
            ));
        }
    }
    for d in [0, -1, 1, -2, 2] {
        if border(c.col, c.row + d, false)
            && (-2..=2)
                .filter(|col| border(c.col + col, c.row + d, false))
                .count()
                >= 2
        {
            return Some((
                Cell {
                    col: c.col,
                    row: c.row + d,
                },
                false,
            ));
        }
    }
    None
}
fn point(g: &Gesture, list: &JsValue) -> Option<JsValue> {
    Array::from(list)
        .iter()
        .find(|t| js_sys::Object::is(&get(t, "identifier"), &g.touch_id))
}
fn flush(ui: Ui, g: Rc<RefCell<Gesture>>) {
    let body = {
        let mut s = g.borrow_mut();
        s.tmux_frame = JsValue::NULL;
        if s.tmux_flight {
            return;
        }
        let delta = s.tmux_lines;
        let p = s.tmux_point.take();
        s.tmux_lines = 0.;
        if delta == 0. || p.is_none() {
            return;
        }
        s.tmux_flight = true;
        let p = p.unwrap_or(Cell { col: 1, row: 1 });
        json!({"session":ui.borrow().session,"delta":delta,"col":p.col-1,"row":p.row-1})
    };
    spawn_local(async move {
        let _ = fetch(&ui, "/tmux-scroll", Some(body), 7000).await;
        g.borrow_mut().tmux_flight = false;
        if is_live(&ui) && g.borrow().tmux_lines != 0. {
            schedule_flush(&ui, &g);
        }
    });
}
fn schedule_flush(ui: &Ui, g: &Rc<RefCell<Gesture>>) {
    if truthy(&g.borrow().tmux_frame) {
        return;
    }
    let u = ui.clone();
    let gs = g.clone();
    let callback = function(move |_| {
        if is_live(&u) {
            flush(u.clone(), gs.clone());
        }
        Ok(JsValue::UNDEFINED)
    });
    let frame =
        call(&js_sys::global(), "requestAnimationFrame", &[callback]).unwrap_or(JsValue::NULL);
    g.borrow_mut().tmux_frame = frame;
}
fn queue(ui: &Ui, g: &Rc<RefCell<Gesture>>, delta: f64, p: &JsValue) -> bool {
    if !delta.is_finite()
        || delta == 0.
        || ui.borrow().session.is_empty()
        || ui.borrow().auth.is_empty()
    {
        return false;
    }
    let Some(c) = cell(ui, p) else {
        return false;
    };
    let lines = (delta.abs() / 28.).round().clamp(1., 8.);
    {
        let mut g = g.borrow_mut();
        g.tmux_lines = (g.tmux_lines + delta.signum() * lines).clamp(-24., 24.);
        g.tmux_point = Some(c);
    }
    schedule_flush(ui, g);
    true
}
fn wheel(ui: &Ui, g: &Rc<RefCell<Gesture>>, dy: f64, touch: &JsValue, fallback: &JsValue) -> bool {
    let target = call(
        &doc(),
        "elementFromPoint",
        &[get(touch, "clientX"), get(touch, "clientY")],
    )
    .ok()
    .filter(truthy)
    .unwrap_or_else(|| {
        if truthy(fallback) {
            fallback.clone()
        } else {
            get(&terminal(ui), "screen")
        }
    });
    let options = object();
    for k in ["clientX", "clientY", "screenX", "screenY"] {
        let _ = set(&options, k, &get(touch, k));
    }
    for (k, v) in [
        ("deltaY", dy.into()),
        ("deltaMode", 0.into()),
        ("bubbles", true.into()),
        ("cancelable", true.into()),
    ] {
        let _ = set(&options, k, &v);
    }
    let args = Array::new();
    args.push(&"wheel".into());
    args.push(&options);
    use wasm_bindgen::JsCast;
    let event = global("WheelEvent")
        .dyn_into::<js_sys::Function>()
        .ok()
        .and_then(|f| js_sys::Reflect::construct(&f, &args).ok());
    if let Some(e) = event {
        return call(&target, "dispatchEvent", &[e]).is_ok();
    }
    if !mouse_on(ui) {
        return queue(ui, g, dy, touch);
    }
    false
}
fn scroll(ui: &Ui, g: &Rc<RefCell<Gesture>>, e: &JsValue, touch: &JsValue) {
    let dy = {
        let mut g = g.borrow_mut();
        g.phase = Phase::Scrolling;
        let dy = g.last_y - num(touch, "clientY");
        g.last_y = num(touch, "clientY");
        g.wheel += dy;
        let ticks = (g.wheel / 56.).trunc().clamp(-3., 3.);
        ticks * 56.
    };
    if dy != 0. && wheel(ui, g, dy, touch, &get(e, "target")) {
        g.borrow_mut().wheel -= dy;
    }
    stop(e);
}
fn hold(ui: Ui, g: Rc<RefCell<Gesture>>, mouse: bool) {
    if !is_live(&ui) || g.borrow().phase != Phase::Pending {
        return;
    }
    let t = g.borrow().touch.clone();
    // Links own a stationary touch, including a deliberate/slow tap on their label.
    // Do not replace the terminal with history while the finger is still down.
    if call(
        &terminal(&ui),
        "linkAt",
        &[get(&t, "clientX"), get(&t, "clientY")],
    )
    .is_ok_and(|link| truthy(&link))
    {
        return;
    }
    let Some(c) = cell(&ui, &t) else {
        return;
    };
    if mouse {
        if selected(&ui)
            || truthy(&get(&terminal(&ui), "selecting"))
            || !mouse_on(&ui)
            || !call(&terminal(&ui), "isOpen", &[]).is_ok_and(|v| truthy(&v))
        {
            return;
        }
        if let Some((anchor, vertical)) = snap(&ui, c) {
            let mut s = g.borrow_mut();
            s.phase = Phase::Resizing;
            s.anchor = Some((anchor, vertical));
            s.last = Some(anchor);
            s.pressed = true;
            drop(s);
            send(&ui, 0, anchor, false);
            let indicator = g.borrow().indicator.clone();
            let _ = set(
                &indicator,
                "textContent",
                &if vertical {
                    "⇔ redimensionando"
                } else {
                    "⇕ redimensionando"
                }
                .into(),
            );
            let _ = set(&get(&indicator, "style"), "display", &"block".into());
            let _ = call(&global("navigator"), "vibrate", &[25.into()]);
            return;
        }
        let u = ui.clone();
        let gs = g.clone();
        g.borrow_mut().timer = call(
            &js_sys::global(),
            "setTimeout",
            &[
                function(move |_| {
                    hold(u.clone(), gs.clone(), false);
                    Ok(JsValue::UNDEFINED)
                }),
                300.into(),
            ],
        )
        .unwrap_or(JsValue::NULL);
        return;
    }
    spawn_local(async move {
        let _ = history_open(ui, json!({"col":c.col-1,"row":c.row-1})).await;
    });
}
pub(super) fn mount(ui: &Ui) -> Result<(), JsValue> {
    let indicator = call(&doc(), "createElement", &["div".into()])?;
    set(&get(&indicator,"style"),"cssText",&"position:fixed;top:10px;left:50%;transform:translateX(-50%);z-index:9;display:none;background:rgba(0,0,0,.7);color:#4CE07A;font:12px monospace;padding:5px 12px;border-radius:8px;pointer-events:none".into())?;
    call(
        &get(&doc(), "body"),
        "appendChild",
        std::slice::from_ref(&indicator),
    )?;
    let g = Rc::new(RefCell::new(Gesture {
        phase: Phase::Idle,
        touch: JsValue::NULL,
        touch_id: JsValue::NULL,
        x: 0.,
        y: 0.,
        last_y: 0.,
        wheel: 0.,
        timer: JsValue::NULL,
        frame: JsValue::NULL,
        anchor: None,
        last: None,
        pending: None,
        pressed: false,
        history_opened: false,
        indicator,
        tmux_lines: 0.,
        tmux_point: None,
        tmux_frame: JsValue::NULL,
        tmux_flight: false,
        refresh: false,
        refresh_timer: JsValue::NULL,
        mouse: None,
    }));
    let u = ui.clone();
    let gs = g.clone();
    capture(ui, doc(), "wheel", move |a| {
        let e = a.get(0);
        if owned(&e) || mouse_on(&u) || selected(&u) {
            return Ok(JsValue::UNDEFINED);
        }
        if queue(&u, &gs, num(&e, "deltaY"), &e) {
            stop(&e);
        }
        Ok(JsValue::UNDEFINED)
    })?;
    let u = ui.clone();
    let gs = g.clone();
    capture(ui, doc(), "touchstart", move |a| {
        let e = a.get(0);
        let touches = Array::from(&get(&e, "touches"));
        if owned(&e) || selected(&u) || touches.length() != 1 {
            finish(&u, &gs);
            return Ok(JsValue::UNDEFINED);
        }
        if gs.borrow().phase != Phase::Idle {
            finish(&u, &gs);
        }
        let t = touches.get(0);
        {
            let mut s = gs.borrow_mut();
            s.phase = Phase::Pending;
            s.touch = t.clone();
            s.touch_id = get(&t, "identifier");
            s.x = num(&t, "clientX");
            s.y = num(&t, "clientY");
            s.last_y = s.y;
        }
        let mouse = mouse_on(&u);
        let u = u.clone();
        let g = gs.clone();
        gs.borrow_mut().timer = call(
            &js_sys::global(),
            "setTimeout",
            &[
                function(move |_| {
                    hold(u.clone(), g.clone(), mouse);
                    Ok(JsValue::UNDEFINED)
                }),
                if mouse { 220 } else { 520 }.into(),
            ],
        )?;
        Ok(JsValue::UNDEFINED)
    })?;
    let u = ui.clone();
    let gs = g.clone();
    capture(ui, doc(), "touchmove", move |a| {
        let e = a.get(0);
        if owned(&e) {
            finish(&u, &gs);
            return Ok(JsValue::UNDEFINED);
        }
        let phase = gs.borrow().phase;
        if phase == Phase::Idle {
            return Ok(JsValue::UNDEFINED);
        }
        let touches = get(&e, "touches");
        if selected(&u) || Array::from(&touches).length() != 1 {
            finish(&u, &gs);
            return Ok(JsValue::UNDEFINED);
        }
        let Some(t) = point(&gs.borrow(), &touches) else {
            finish(&u, &gs);
            return Ok(JsValue::UNDEFINED);
        };
        if phase == Phase::Pending {
            let distance = {
                let s = gs.borrow();
                (num(&t, "clientX") - s.x).hypot(num(&t, "clientY") - s.y)
            };
            if distance <= 12. {
                if !truthy(&get(&terminal(&u), "selecting")) {
                    stop(&e);
                }
                return Ok(JsValue::UNDEFINED);
            }
            cancel(&gs.borrow().timer, false);
            gs.borrow_mut().timer = JsValue::NULL;
            scroll(&u, &gs, &e, &t);
        } else if phase == Phase::Scrolling {
            scroll(&u, &gs, &e, &t);
        } else if phase == Phase::Resizing {
            stop(&e);
            if let Some(mut c) = cell(&u, &t) {
                if let Some((anchor, vertical)) = gs.borrow().anchor {
                    if vertical {
                        c.row = anchor.row
                    } else {
                        c.col = anchor.col
                    }
                }
                gs.borrow_mut().pending = Some(c);
                if !truthy(&gs.borrow().frame) {
                    let u = u.clone();
                    let g = gs.clone();
                    gs.borrow_mut().frame = call(
                        &js_sys::global(),
                        "requestAnimationFrame",
                        &[function(move |_| {
                            motion(&u, &g);
                            Ok(JsValue::UNDEFINED)
                        })],
                    )?;
                }
            }
        }
        Ok(JsValue::UNDEFINED)
    })?;
    let u = ui.clone();
    let gs = g.clone();
    capture(ui, doc(), "touchend", move |a| {
        let e = a.get(0);
        if gs.borrow().history_opened {
            stop(&e);
            finish(&u, &gs);
            return Ok(JsValue::UNDEFINED);
        }
        if owned(&e) {
            finish(&u, &gs);
            return Ok(JsValue::UNDEFINED);
        }
        let touch = point(&gs.borrow(), &get(&e, "changedTouches"))
            .unwrap_or_else(|| gs.borrow().touch.clone());
        if gs.borrow().phase == Phase::Resizing
            && let Some(mut c) = cell(&u, &touch)
        {
            if let Some((anchor, vertical)) = gs.borrow().anchor {
                if vertical {
                    c.row = anchor.row
                } else {
                    c.col = anchor.col
                }
            }
            gs.borrow_mut().pending = Some(c);
        }
        let remainder = gs.borrow().wheel;
        if gs.borrow().phase == Phase::Scrolling && remainder.abs() >= 18. {
            wheel(
                &u,
                &gs,
                remainder.signum() * 56.,
                &touch,
                &get(&e, "target"),
            );
        }
        finish(&u, &gs);
        Ok(JsValue::UNDEFINED)
    })?;
    for event in ["touchcancel", "pagehide"] {
        let u = ui.clone();
        let gs = g.clone();
        bind(
            ui,
            if event == "pagehide" {
                js_sys::global().into()
            } else {
                doc()
            },
            event,
            move |_| {
                finish(&u, &gs);
                Ok(JsValue::UNDEFINED)
            },
        )?;
    }
    let gs = g.clone();
    let u = ui.clone();
    call(
        &terminal(ui),
        "onWriteParsed",
        &[function(move |_| {
            if gs.borrow().refresh {
                cancel(&gs.borrow().refresh_timer, false);
                let u = u.clone();
                let g = gs.clone();
                gs.borrow_mut().refresh_timer = call(
                    &js_sys::global(),
                    "setTimeout",
                    &[
                        function(move |_| {
                            if is_live(&u) && g.borrow().refresh {
                                g.borrow_mut().refresh = false;
                                call(&terminal(&u), "refresh", &[])?;
                            }
                            Ok(JsValue::UNDEFINED)
                        }),
                        180.into(),
                    ],
                )?;
            }
            Ok(JsValue::UNDEFINED)
        })],
    )?;
    let u = ui.clone();
    let gs = g.clone();
    capture(ui, doc(), "mousedown", move |a| {
        let e = a.get(0);
        if owned(&e)
            || num(&e, "button") != 0.
            || truthy(&get(&terminal(&u), "selecting"))
            || !mouse_on(&u)
        {
            return Ok(JsValue::UNDEFINED);
        }
        if cell(&u, &e).is_some_and(|c| snap(&u, c).is_some()) {
            gs.borrow_mut().mouse = Some((num(&e, "clientX"), num(&e, "clientY"), false));
        }
        Ok(JsValue::UNDEFINED)
    })?;
    let gs2 = g.clone();
    capture(ui, doc(), "mousemove", move |a| {
        let e = a.get(0);
        let current = gs2.borrow().mouse;
        if let Some((x, y, false)) = current {
            gs2.borrow_mut().mouse = Some((
                x,
                y,
                (num(&e, "clientX") - x).hypot(num(&e, "clientY") - y) > 4.,
            ));
        }
        Ok(JsValue::UNDEFINED)
    })?;
    let gs = g.clone();
    capture(ui, doc(), "mouseup", move |_| {
        if gs.borrow().mouse.is_some_and(|(_, _, moved)| moved) {
            gs.borrow_mut().refresh = true;
            cancel(&gs.borrow().refresh_timer, false);
        }
        gs.borrow_mut().mouse = None;
        Ok(JsValue::UNDEFINED)
    })?;
    ui.borrow_mut().gesture = Some(g);
    Ok(())
}
pub(super) fn dispose(ui: &Ui) {
    let g = ui.borrow_mut().gesture.take();
    if let Some(g) = g {
        finish(ui, &g);
        let s = g.borrow();
        cancel(&s.tmux_frame, true);
        cancel(&s.refresh_timer, false);
        let _ = call(&s.indicator, "remove", &[]);
    }
}
pub(super) fn finish_active(ui: &Ui) {
    let gesture = ui.borrow().gesture.clone();
    if let Some(gesture) = gesture {
        finish(ui, &gesture);
    }
}
pub(super) fn prepare_history(ui: &Ui) {
    let gesture = ui.borrow().gesture.clone();
    if let Some(gesture) = gesture {
        let from_touch = gesture.borrow().phase == Phase::Pending;
        finish(ui, &gesture);
        gesture.borrow_mut().history_opened = from_touch;
    }
}
