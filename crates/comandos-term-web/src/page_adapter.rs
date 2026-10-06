//! A live terminal boundary shared by Rust controls and pane chrome.
use super::*;
use comandos_web_dom::port::{getter, method, object, set};
fn alive(weak: &std::rc::Weak<RefCell<Page>>) -> Option<Rc<RefCell<Page>>> {
    weak.upgrade().filter(|p| !p.borrow().disposed)
}
pub(super) fn publish(page: &Rc<RefCell<Page>>) -> Result<JsValue, JsValue> {
    let adapter = object();
    for (name, key) in [
        ("cols", "cols"),
        ("rows", "rows"),
        ("cellWidth", "cssCellWidth"),
        ("cellHeight", "cssCellHeight"),
    ] {
        let weak = Rc::downgrade(page);
        getter(&adapter, name, move || {
            alive(&weak)
                .and_then(|p| term(&p))
                .map_or(JsValue::UNDEFINED, |t| {
                    comandos_web_dom::port::get(&t.borrow().dimensions(), key)
                })
        })?;
    }
    for name in ["screen", "textarea", "modes", "options"] {
        let weak = Rc::downgrade(page);
        getter(&adapter, name, move || {
            let Some(t) = alive(&weak).and_then(|p| term(&p)) else {
                return JsValue::NULL;
            };
            let t = t.borrow();
            match name{"screen"=>t.screen().map_or(JsValue::NULL,Into::into),"textarea"=>t.textarea().map_or(JsValue::NULL,Into::into),"modes"=>t.modes(),_=>t.with(|i|value(&json!({"fontFamily":i.opts.font_family,"fontSize":i.opts.font_size,"lineHeight":i.opts.line_height,"letterSpacing":i.opts.letter_spacing}).to_string())).unwrap_or(JsValue::NULL)}
        })?;
    }
    for name in ["known", "busy", "selecting"] {
        let weak = Rc::downgrade(page);
        getter(&adapter, name, move || {
            alive(&weak)
                .is_some_and(|p| {
                    let p = p.borrow();
                    match name {
                        "known" => p.known,
                        "busy" => p.busy,
                        _ => p.selecting,
                    }
                })
                .into()
        })?;
    }
    let weak = Rc::downgrade(page);
    method(&adapter, "cellChar", move |a| {
        let Some(t) = alive(&weak).and_then(|p| term(&p)) else {
            return Ok("".into());
        };
        let col = a.get(0).as_f64().unwrap_or(0.);
        let row = a.get(1).as_f64().unwrap_or(0.);
        Ok(t.borrow()
            .with(|i| {
                if col.fract() != 0.
                    || row.fract() != 0.
                    || col < 1.
                    || row < 1.
                    || col > f64::from(i.size.cols)
                    || row > f64::from(i.size.rows)
                {
                    return String::new();
                }
                let line =
                    row as i32 - 1 - i32::try_from(i.engine.display_offset()).unwrap_or(i32::MAX);
                let col = col as u16 - 1;
                let selection = comandos_term::select::Selection {
                    anchor: (line, col),
                    head: (line, col),
                    mode: comandos_term::select::SelectMode::Simple,
                };
                comandos_term::select::selected_text(&i.engine, &selection)
            })
            .unwrap_or_default()
            .into())
    })?;
    let weak = Rc::downgrade(page);
    method(&adapter, "refresh", move |_| {
        if let Some(t) = alive(&weak).and_then(|p| term(&p)) {
            t.borrow_mut().dispatch(|i| {
                i.scheduler.invalidate_all();
                i.request_frame();
            });
        }
        Ok(JsValue::UNDEFINED)
    })?;
    let weak = Rc::downgrade(page);
    getter(&adapter, "ctrl", move || {
        alive(&weak).is_some_and(|p| p.borrow().ctrl).into()
    })?;
    for name in ["onData", "onResize", "onWriteParsed"] {
        let weak = Rc::downgrade(page);
        method(&adapter, name, move |args| {
            let callback = args.get(0).dyn_into::<Function>()?;
            let Some(p) = alive(&weak) else {
                return Ok(JsValue::UNDEFINED);
            };
            {
                let mut p = p.borrow_mut();
                match name {
                    "onData" => p.data_hooks.push(callback.clone()),
                    "onResize" => p.resize_hooks.push(callback.clone()),
                    _ => p.write_hooks.push(callback.clone()),
                };
            }
            let subscription = object();
            let weak = weak.clone();
            method(&subscription, "dispose", move |_| {
                if let Some(p) = alive(&weak) {
                    let mut p = p.borrow_mut();
                    let list = match name {
                        "onData" => &mut p.data_hooks,
                        "onResize" => &mut p.resize_hooks,
                        _ => &mut p.write_hooks,
                    };
                    list.retain(|f| f != &callback);
                }
                Ok(JsValue::UNDEFINED)
            })?;
            Ok(subscription)
        })?;
    }
    let weak = Rc::downgrade(page);
    method(&adapter, "sendInput", move |a| {
        let Some(p) = alive(&weak) else {
            return Ok(false.into());
        };
        let text = a.get(0).as_string().unwrap_or_default();
        Ok(send_input(&p, text.as_bytes(), a.get(1).is_truthy()).into())
    })?;
    let weak = Rc::downgrade(page);
    method(&adapter, "sendWithAck", move |a| {
        if alive(&weak).is_none() {
            return Ok(Promise::resolve(&JsValue::FALSE).into());
        }
        let text = a.get(0).as_string().unwrap_or_default();
        let timeout = a
            .get(1)
            .as_f64()
            .unwrap_or(4000.)
            .clamp(0., u32::MAX as f64) as u32;
        Ok(send_with_ack(text.as_bytes(), timeout).into())
    })?;
    for name in [
        "focus",
        "blur",
        "getSelection",
        "hasSelection",
        "isOpen",
        "scheduleFit",
        "setCtrlArmed",
        "requestInteractionMode",
        "paste",
        "localHistory",
    ] {
        let weak = Rc::downgrade(page);
        method(&adapter, name, move |a| {
            let Some(p) = alive(&weak) else {
                return Ok(JsValue::FALSE);
            };
            let Some(t) = term(&p) else {
                return Ok(JsValue::FALSE);
            };
            match name {
                "focus" => t.borrow().focus(),
                "blur" => t.borrow().blur(),
                "getSelection" => return Ok(t.borrow().get_selection().into()),
                "hasSelection" => return Ok(t.borrow().has_selection().into()),
                "isOpen" => return Ok(socket_open(&p.borrow()).into()),
                "scheduleFit" => schedule_fit(&p),
                "setCtrlArmed" => set_ctrl(&p, a.get(0).is_truthy()),
                "requestInteractionMode" => {
                    let selecting = p.borrow().selecting;
                    handle(&p, Command::Mode(!selecting));
                }
                "paste" => {
                    let text = a.get(0).as_string().unwrap_or_default();
                    let ok = !text.is_empty() && socket_open(&p.borrow());
                    handle(&p, Command::Paste(text));
                    return Ok(ok.into());
                }
                "localHistory" => {
                    return Ok(t
                        .borrow()
                        .with(|i| {
                            let first = -i32::try_from(i.engine.history_len()).unwrap_or(i32::MAX);
                            let last = i32::from(i.size.rows) - 1;
                            let selection = comandos_term::select::Selection {
                                anchor: (first, 0),
                                head: (last, i.size.cols.saturating_sub(1)),
                                mode: comandos_term::select::SelectMode::Simple,
                            };
                            comandos_term::select::selected_text(&i.engine, &selection)
                        })
                        .unwrap_or_default()
                        .into());
                }
                _ => {}
            }
            Ok(JsValue::UNDEFINED)
        })?;
    }
    set(&window()?.into(), "__comandosTerm", &adapter)?;
    Ok(adapter)
}
pub(super) fn data(page: &Rc<RefCell<Page>>, bytes: &[u8]) {
    let hooks = page.borrow().data_hooks.clone();
    for f in hooks {
        let _ = f.call1(
            &JsValue::UNDEFINED,
            &String::from_utf8_lossy(bytes).as_ref().into(),
        );
    }
}
pub(super) fn output(page: &Rc<RefCell<Page>>) {
    let hooks = page.borrow().write_hooks.clone();
    for f in hooks {
        let _ = f.call0(&JsValue::UNDEFINED);
    }
}
pub(super) fn resized(page: &Rc<RefCell<Page>>, size: JsValue) {
    let dimensions = ("cols", "rows");
    let number = |key: &str| {
        Reflect::get(&size, &key.into())
            .ok()
            .and_then(|v| v.as_f64())
            .unwrap_or_default() as u16
    };
    let dimensions = (number(dimensions.0), number(dimensions.1));
    let hooks = {
        let mut page = page.borrow_mut();
        if page.adapter_size == Some(dimensions) {
            return;
        }
        page.adapter_size = Some(dimensions);
        page.resize_hooks.clone()
    };
    for f in hooks {
        let _ = f.call1(&JsValue::UNDEFINED, &size);
    }
}
