use super::*;
fn delay(ms: f64) -> JsValue {
    js_sys::Promise::new(&mut |resolve, _| {
        later(resolve.clone().into(), ms);
    })
    .into()
}
fn probe() -> Result<JsValue, JsValue> {
    let controller = new("AbortController", &[])?;
    let c = controller.clone();
    let timeout = later(
        function(move |_| {
            call(&c, "abort", &[])?;
            Ok(JsValue::UNDEFINED)
        }),
        number(&global("TERM_PRIMARY_PROBE_TIMEOUT_MS")),
    );
    let request = run(
        "fetch",
        &[
            js(&format!("{}/token", text(&global("TERM_BASE")))),
            item(&[
                ("cache", "no-store".into()),
                ("signal", get(&controller, "signal")),
            ])?,
        ],
    );
    Ok(promise(async move {
        let result = wait(request).await;
        cancel(timeout);
        Ok(result.is_ok_and(|v| truthy(&get(&v, "ok"))).into())
    }))
}
async fn fallback() -> bool {
    api("/remote-state", JsValue::UNDEFINED)
        .await
        .is_ok_and(|s| truthy(&get(&s, "fallbackTerminalOn")))
}
fn degraded() -> Result<(), JsValue> {
    if truthy(&global("termFallbackNotified")) {
        return Ok(());
    }
    put("termFallbackNotified", true)?;
    run(
        "toast",
        &[
            translate(
                "Terminal en modo degradado",
                "Terminal running in degraded mode",
            ),
            true.into(),
        ],
    )?;
    Ok(())
}
async fn resolve_base() -> Result<JsValue, JsValue> {
    if !truthy(&global("WEBTERM")) {
        return Ok("".into());
    }
    let attempts = number(&global("TERM_PRIMARY_ATTEMPTS")) as u32;
    for attempt in 0..attempts {
        if truthy(&wait(probe()).await?) {
            return Ok(global("TERM_BASE"));
        }
        if attempt + 1 < attempts {
            wait(Ok(delay(number(&global("TERM_PRIMARY_RETRY_MS"))))).await?;
        }
    }
    if fallback().await {
        degraded()?;
        return Ok(global("TERM_FALLBACK_BASE"));
    }
    Err(js_sys::Error::new(&string(&translate(
        "Ninguna terminal remota responde",
        "No remote terminal endpoint is responding",
    )))
    .into())
}
fn compatible(frame: &JsValue) -> bool {
    text(&get(&get(frame, "dataset"), "compat")) != "1"
}
fn frame_doc(frame: &JsValue) -> JsValue {
    default(
        get(frame, "contentDocument"),
        get(&get(frame, "contentWindow"), "document"),
    )
}
fn frame_style(frame: &JsValue) -> Result<(), JsValue> {
    if !compatible(frame) {
        return Ok(());
    }
    let d = frame_doc(frame);
    if !truthy(&d) || !truthy(&get(&d, "head")) {
        return Ok(());
    }
    let computed = run("getComputedStyle", &[root()])?;
    let color = |key: &str, fallback: &str| -> Result<String, JsValue> {
        Ok(string(&default(
            call(&computed, "getPropertyValue", &[key.into()])?,
            fallback.into(),
        ))
        .trim()
        .into())
    };
    let bg = color("--bg", "#0A0D13")?;
    let line2 = color("--line2", "#2E3852")?;
    let brand = color("--brand", "#8B7CFF")?;
    let scheme = if text(&get(&get(&root(), "dataset"), "theme")) == "dia" {
        "light"
    } else {
        "dark"
    };
    let mut style = call(&d, "getElementById", &["comandos-term-style".into()])?;
    if !truthy(&style) {
        style = call(&d, "createElement", &["style".into()])?;
        set(&style, "id", &"comandos-term-style".into())?;
        call(
            &get(&d, "head"),
            "appendChild",
            std::slice::from_ref(&style),
        )?;
    }
    set(&style,"textContent",&format!("
      html,body{{background:{bg}!important;color-scheme:{scheme}}}
      *{{scrollbar-width:thin;scrollbar-color:{line2} {bg}}}
      *::-webkit-scrollbar{{width:10px;height:10px}}
      *::-webkit-scrollbar-track{{background:{bg}}}
      *::-webkit-scrollbar-thumb{{background:{line2};border-radius:10px;border:3px solid {bg};background-clip:padding-box}}
      *::-webkit-scrollbar-thumb:hover{{background:{brand};background-clip:padding-box}}
      .xterm,.xterm .xterm-screen,.xterm .xterm-viewport{{touch-action:pan-y}}
      .xterm .xterm-viewport{{scrollbar-width:thin;scrollbar-color:{line2} {bg};
        -webkit-overflow-scrolling:touch;overscroll-behavior:contain}}
    ").into())
}
fn frame_scroll(frame: &JsValue) -> Result<(), JsValue> {
    if !compatible(frame) {
        return Ok(());
    }
    let win = get(frame, "contentWindow");
    let d = frame_doc(frame);
    if !truthy(&win)
        || !truthy(&d)
        || truthy(&get(&win, "__comandosOwnsTouchGestures"))
        || truthy(&get(&win, "__comandosScrollWired"))
    {
        return Ok(());
    }
    set(&win, "__comandosScrollWired", &true.into())?;
    let touch = item(&[("y", JsValue::NULL), ("accum", 0.into())])?;
    let s = touch.clone();
    call(
        &d,
        "addEventListener",
        &[
            "touchstart".into(),
            function(move |a| {
                let touches = get(&a.get(0), "touches");
                set(&s, "accum", &0.into())?;
                set(
                    &s,
                    "y",
                    &if truthy(&touches) && number(&get(&touches, "length")) == 1.0 {
                        get(&get(&touches, "0"), "clientY")
                    } else {
                        JsValue::NULL
                    },
                )?;
                Ok(JsValue::UNDEFINED)
            }),
            item(&[("capture", true.into()), ("passive", true.into())])?,
        ],
    )?;
    let s = touch.clone();
    let child = d.clone();
    call(
        &d,
        "addEventListener",
        &[
            "touchmove".into(),
            function(move |a| {
                let e = a.get(0);
                let touches = get(&e, "touches");
                let y = get(&s, "y");
                if y.is_null() || !truthy(&touches) || number(&get(&touches, "length")) != 1.0 {
                    return Ok(JsValue::UNDEFINED);
                }
                let t = get(&touches, "0");
                let dy = number(&y) - number(&get(&t, "clientY"));
                set(&s, "y", &get(&t, "clientY"))?;
                let accum = number(&get(&s, "accum")) + dy;
                set(&s, "accum", &accum.into())?;
                let ticks = (accum / 56.0).trunc().clamp(-3.0, 3.0);
                let mut target = if get(&child, "elementFromPoint").is_function() {
                    call(
                        &child,
                        "elementFromPoint",
                        &[get(&t, "clientX"), get(&t, "clientY")],
                    )?
                } else {
                    JsValue::NULL
                };
                for next in [
                    get(&e, "target"),
                    query(&child, ".xterm-screen"),
                    query(&child, ".xterm-viewport"),
                    get(&child, "body"),
                ] {
                    if !truthy(&target) {
                        target = next;
                    }
                }
                let emitted =
                    ticks != 0.0 && truthy(&target) && get(&win, "WheelEvent").is_function();
                if emitted {
                    let options = item(&[
                        ("deltaY", (ticks * 56.0).into()),
                        ("deltaMode", 0.into()),
                        ("bubbles", true.into()),
                        ("cancelable", true.into()),
                        ("clientX", get(&t, "clientX")),
                        ("clientY", get(&t, "clientY")),
                        ("screenX", get(&t, "screenX")),
                        ("screenY", get(&t, "screenY")),
                    ])?;
                    let wheel = Reflect::construct(
                        &get(&win, "WheelEvent").dyn_into::<Function>()?,
                        &["wheel".into(), options].into_iter().collect::<Array>(),
                    )?;
                    call(&target, "dispatchEvent", &[wheel])?;
                    set(&s, "accum", &(accum - ticks * 56.0).into())?;
                }
                if emitted || dy != 0.0 {
                    stop(&e);
                }
                Ok(JsValue::UNDEFINED)
            }),
            item(&[("capture", true.into()), ("passive", false.into())])?,
        ],
    )?;
    for event in ["touchend", "touchcancel"] {
        let s = touch.clone();
        call(
            &d,
            "addEventListener",
            &[
                event.into(),
                function(move |_| {
                    set(&s, "y", &JsValue::NULL)?;
                    set(&s, "accum", &0.into())?;
                    Ok(JsValue::UNDEFINED)
                }),
                item(&[("capture", true.into()), ("passive", true.into())])?,
            ],
        )?;
    }
    Ok(())
}
fn shortcut(e: &JsValue) -> bool {
    (truthy(&get(e, "ctrlKey")) || truthy(&get(e, "metaKey")))
        && !truthy(&get(e, "shiftKey"))
        && !truthy(&get(e, "altKey"))
        && text(&get(e, "key")).to_lowercase() == "k"
}
fn switch_key(e: &JsValue) -> Result<bool, JsValue> {
    if !shortcut(e) {
        return Ok(false);
    }
    stop(e);
    if truthy(&get(e, "stopImmediatePropagation")) {
        call(e, "stopImmediatePropagation", &[])?;
    }
    run("swOpen", &[])?;
    Ok(true)
}
fn frame_shortcuts(frame: &JsValue) -> Result<(), JsValue> {
    if !compatible(frame) {
        return Ok(());
    }
    let win = get(frame, "contentWindow");
    let d = frame_doc(frame);
    if !truthy(&win) || !truthy(&d) || truthy(&get(&win, "__comandosSwitchWired")) {
        return Ok(());
    }
    set(&win, "__comandosSwitchWired", &true.into())?;
    let handler = function(|a| {
        if switch_key(&a.get(0))? {
            let _ = run("focus", &[]);
        }
        Ok(JsValue::UNDEFINED)
    });
    call(
        &win,
        "addEventListener",
        &["keydown".into(), handler.clone(), true.into()],
    )?;
    call(
        &d,
        "addEventListener",
        &["keydown".into(), handler, true.into()],
    )?;
    Ok(())
}
fn apply_frame(frame: &JsValue) {
    let _ = frame_style(frame);
    let _ = frame_scroll(frame);
    let _ = frame_shortcuts(frame);
}
fn ensure_frame(sess: JsValue) -> Result<(), JsValue> {
    let o = map_get("openTerms", &sess)?;
    if !truthy(&o) || truthy(&get(&o, "frame")) {
        return Ok(());
    }
    let frame = call(&doc(), "createElement", &["iframe".into()])?;
    set(&frame, "className", &"termpane".into())?;
    let f = frame.clone();
    listen(
        &frame,
        "load",
        function(move |_| {
            apply_frame(&f);
            for ms in [250.0, 1000.0] {
                for name in [
                    "styleTermFrame",
                    "wireTermFrameScroll",
                    "wireTermFrameShortcuts",
                ] {
                    let frame = f.clone();
                    let name = name.to_owned();
                    later(
                        function(move |_| {
                            run(&name, std::slice::from_ref(&frame))?;
                            Ok(JsValue::UNDEFINED)
                        }),
                        ms,
                    );
                }
            }
            Ok(JsValue::UNDEFINED)
        }),
    );
    call(
        &id("term-area"),
        "appendChild",
        std::slice::from_ref(&frame),
    )?;
    set(&o, "frame", &frame)?;
    run("applyTermInteraction", std::slice::from_ref(&sess))?;
    let resolve = run("resolveTermBase", &[]);
    fire(
        Ok(promise(async move {
            let result: Result<(), JsValue> = async {
                let base = wait(resolve).await?;
                if get(&map_get("openTerms", &sess)?, "frame") != frame {
                    return Ok(());
                }
                let token = wait(run("webtermAccessToken", &[])).await?;
                if !truthy(&token) {
                    return Err(js_sys::Error::new(&string(&translate(
                        "Token de terminal no disponible",
                        "Terminal token unavailable",
                    )))
                    .into());
                }
                let primary = base == global("TERM_BASE");
                set(
                    &get(&frame, "dataset"),
                    "compat",
                    &if primary { "0" } else { "1" }.into(),
                )?;
                let base = text(&base);
                let token = encode(&token)?;
                let session = encode(&sess)?;
                let src = if primary {
                    format!(
                        "{base}/?auth={token}&arg={session}&theme={}",
                        encode(&global("curTheme"))?
                    )
                } else {
                    format!("{base}/?arg={token}&arg={session}")
                };
                set(&frame, "src", &js(&src))?;
                Ok(())
            }
            .await;
            if let Err(e) = result
                && get(&map_get("openTerms", &sess)?, "frame") == frame
            {
                call(&frame, "remove", &[])?;
                set(&o, "frame", &JsValue::NULL)?;
                notify_error(&e);
            }
            Ok(JsValue::UNDEFINED)
        })),
        false,
    );
    Ok(())
}
pub(super) fn mount() -> Result<(), JsValue> {
    publish("delay", |a| Ok(delay(number(&a.get(0)))))?;
    publish("probePrimaryTerm", |_| probe())?;
    publish("fallbackTermAvailable", |_| {
        Ok(promise(async { Ok(fallback().await.into()) }))
    })?;
    publish("showTermDegraded", |_| {
        degraded()?;
        Ok(JsValue::UNDEFINED)
    })?;
    publish("resolveTermBase", |_| Ok(promise(resolve_base())))?;
    publish("styleTermFrame", |a| {
        let _ = frame_style(&a.get(0));
        Ok(JsValue::UNDEFINED)
    })?;
    publish("wireTermFrameScroll", |a| {
        let _ = frame_scroll(&a.get(0));
        Ok(JsValue::UNDEFINED)
    })?;
    publish("isSwitchShortcut", |a| Ok(shortcut(&a.get(0)).into()))?;
    publish("openSwitcherFromKey", |a| Ok(switch_key(&a.get(0))?.into()))?;
    publish("wireTermFrameShortcuts", |a| {
        let _ = frame_shortcuts(&a.get(0));
        Ok(JsValue::UNDEFINED)
    })?;
    publish("ensureFrame", |a| {
        ensure_frame(a.get(0))?;
        Ok(JsValue::UNDEFINED)
    })
}
