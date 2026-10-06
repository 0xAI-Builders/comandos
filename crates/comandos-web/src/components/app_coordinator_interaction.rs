//! Terminal mouse lifecycle. Pending reads are invalidated, pending writes finish
//! and restore/resync requests are serialized against the same live session state.
use super::*;
fn interaction(sess: &JsValue) -> Result<JsValue, JsValue> {
    let mut s = map_get("termInteraction", sess)?;
    if !truthy(&s) {
        s = item(&[
            ("mouse", JsValue::NULL),
            ("temporary", false.into()),
            ("seq", 0.into()),
            ("busy", false.into()),
            ("pending", "".into()),
            ("pendingSelecting", false.into()),
            ("restorePending", false.into()),
            ("resyncPending", false.into()),
        ])?;
        call(
            &global("termInteraction"),
            "set",
            &[sess.clone(), s.clone()],
        )?;
    }
    Ok(s)
}
fn next_seq() -> Result<JsValue, JsValue> {
    let v = number(&global("termInteractionSeq")) + 1.0;
    put("termInteractionSeq", v)?;
    Ok(v.into())
}
fn post(sess: &JsValue) -> Result<bool, JsValue> {
    let frame = get(&map_get("openTerms", sess)?, "frame");
    let s = map_get("termInteraction", sess)?;
    if !truthy(&frame) || !truthy(&s) || text(&get(&get(&frame, "dataset"), "compat")) == "1" {
        return Ok(false);
    }
    let mouse = get(&s, "mouse");
    let known = mouse.as_bool().is_some();
    let message = item(&[
        ("source", "comandos".into()),
        ("type", "interaction-state".into()),
        ("known", known.into()),
        ("busy", truthy(&get(&s, "busy")).into()),
        ("selecting", (mouse == JsValue::FALSE).into()),
    ])?;
    let win = get(&frame, "contentWindow");
    if truthy(&win) {
        call(
            &win,
            "postMessage",
            &[message, get(&global("location"), "origin")],
        )?;
    }
    Ok(true)
}
fn cleanup(sess: &JsValue) -> Result<(), JsValue> {
    let s = map_get("termInteraction", sess)?;
    if truthy(&s)
        && !truthy(&call(
            &global("openTerms"),
            "has",
            std::slice::from_ref(sess),
        )?)
        && !truthy(&get(&s, "busy"))
        && !truthy(&get(&s, "temporary"))
        && !truthy(&get(&s, "restorePending"))
    {
        call(
            &global("termInteraction"),
            "delete",
            std::slice::from_ref(sess),
        )?;
    }
    Ok(())
}
fn keepalive(sess: &JsValue) -> bool {
    let result = (|| -> Result<(), JsValue> {
        let headers = item(&[("Content-Type", "application/json".into())])?;
        let tok = run("authToken", &[])?;
        if truthy(&tok) {
            set(&headers, "X-Comandos-Token", &tok)?;
        }
        let payload = item(&[("session", sess.clone()), ("enabled", true.into())])?;
        let options = item(&[
            ("method", "POST".into()),
            ("headers", headers),
            ("body", js_sys::JSON::stringify(&payload)?.into()),
            ("keepalive", true.into()),
        ])?;
        fire(run("fetch", &["/tmux-mouse".into(), options]), false);
        Ok(())
    })();
    result.is_ok()
}
fn complete(sess: &JsValue, s: &JsValue, seq: &JsValue, write: bool) -> Result<(), JsValue> {
    if get(s, "seq") != *seq {
        return Ok(());
    }
    set(s, "busy", &false.into())?;
    set(s, "pending", &"".into())?;
    if write {
        set(s, "pendingSelecting", &false.into())?;
    }
    post(sess)?;
    let mut launched = false;
    if truthy(&get(s, "restorePending")) && truthy(&get(s, "temporary")) {
        if truthy(&global("termPageHidden")) {
            keepalive(sess);
        } else {
            set(s, "restorePending", &false.into())?;
            fire(
                run("restoreTermInteraction", std::slice::from_ref(sess)),
                false,
            );
            launched = true;
        }
    } else if !truthy(&get(s, "temporary")) {
        set(s, "restorePending", &false.into())?;
    }
    if !launched && truthy(&get(s, "resyncPending")) && !truthy(&global("termPageHidden")) {
        set(s, "resyncPending", &false.into())?;
        if truthy(&call(
            &global("openTerms"),
            "has",
            std::slice::from_ref(sess),
        )?) {
            fire(
                run("syncTermInteraction", std::slice::from_ref(sess)),
                false,
            );
        }
    }
    cleanup(sess)
}
// The synchronous admission occurs before constructing a Promise, matching an
// async JS function's pre-await busy/sequence transition.
fn begin(sess: JsValue, selecting: Option<bool>, quiet: bool) -> Result<JsValue, JsValue> {
    if !truthy(&sess) {
        return Ok(js_sys::Promise::resolve(&JsValue::FALSE).into());
    }
    let s = interaction(&sess)?;
    if truthy(&get(&s, "busy")) {
        if selecting == Some(false) {
            set(&s, "restorePending", &true.into())?;
        }
        return Ok(js_sys::Promise::resolve(&JsValue::FALSE).into());
    }
    let seq = next_seq()?;
    set(&s, "seq", &seq)?;
    set(&s, "busy", &true.into())?;
    set(
        &s,
        "pending",
        &if selecting.is_some() { "post" } else { "get" }.into(),
    )?;
    if let Some(selecting) = selecting {
        set(&s, "pendingSelecting", &selecting.into())?;
    }
    post(&sess)?;
    // Invoke API immediately; promise scheduling cannot allow another write first.
    let request = if let Some(selecting) = selecting {
        run(
            "api",
            &[
                "/tmux-mouse".into(),
                item(&[("session", sess.clone()), ("enabled", (!selecting).into())])?,
            ],
        )
    } else {
        run(
            "api",
            &[js(&format!("/tmux-mouse?session={}", encode(&sess)?))],
        )
    };
    Ok(promise(async move {
        let result: Result<bool, JsValue> = async {
            let result = wait(request).await?;
            if get(&s, "seq") != seq {
                return Ok(false);
            }
            let mouse = if let Some(selecting) = selecting {
                !selecting
            } else {
                match text(&get(&result, "mouse")).as_str() {
                    "on" => true,
                    "off" => false,
                    _ => {
                        return Err(js_sys::Error::new(&string(&translate(
                            "Estado tmux invalido",
                            "Invalid tmux state",
                        )))
                        .into());
                    }
                }
            };
            set(&s, "mouse", &mouse.into())?;
            if let Some(selecting) = selecting {
                set(&s, "temporary", &selecting.into())?;
                if !selecting {
                    set(&s, "restorePending", &false.into())?;
                }
            } else if mouse {
                set(&s, "temporary", &false.into())?;
            }
            post(&sess)?;
            if selecting.is_some() && !quiet {
                run(
                    "toast",
                    &[if mouse {
                        translate("Interaccion tmux restaurada", "tmux interaction restored")
                    } else {
                        translate("Seleccion de texto activa", "Text selection enabled")
                    }],
                )?;
            }
            Ok(true)
        }
        .await;
        let value = match result {
            Ok(v) => v,
            Err(e) => {
                if get(&s, "seq") == seq {
                    notify_error(&e);
                }
                false
            }
        };
        complete(&sess, &s, &seq, selecting.is_some())?;
        Ok(value.into())
    }))
}
fn restore(sess: JsValue) -> Result<JsValue, JsValue> {
    let s = if truthy(&sess) {
        map_get("termInteraction", &sess)?
    } else {
        JsValue::NULL
    };
    if !truthy(&s) {
        return Ok(js_sys::Promise::resolve(&JsValue::FALSE).into());
    }
    if truthy(&get(&s, "busy"))
        && text(&get(&s, "pending")) == "post"
        && truthy(&get(&s, "pendingSelecting"))
    {
        set(&s, "restorePending", &true.into())?;
        return Ok(js_sys::Promise::resolve(&JsValue::FALSE).into());
    }
    if !truthy(&get(&s, "temporary")) {
        return Ok(js_sys::Promise::resolve(&JsValue::FALSE).into());
    }
    if truthy(&get(&s, "busy")) {
        set(&s, "restorePending", &true.into())?;
        return Ok(js_sys::Promise::resolve(&JsValue::FALSE).into());
    }
    begin(sess, Some(false), true)
}
fn inactive(keep: &JsValue) -> Result<(), JsValue> {
    for (sess, s) in entries(&global("termInteraction"))? {
        if sess == *keep {
            continue;
        }
        if text(&get(&s, "pending")) == "get" {
            set(&s, "seq", &next_seq()?)?;
            set(&s, "busy", &false.into())?;
            set(&s, "pending", &"".into())?;
            post(&sess)?;
        }
    }
    Ok(())
}
fn hide() -> Result<(), JsValue> {
    put("termPageHidden", true)?;
    for (sess, s) in entries(&global("termInteraction"))? {
        let pending = text(&get(&s, "pending")) == "post"
            && truthy(&get(&s, "pendingSelecting"))
            && get(&s, "mouse") == JsValue::TRUE;
        if text(&get(&s, "pending")) == "get" {
            set(&s, "seq", &next_seq()?)?;
            set(&s, "busy", &false.into())?;
            set(&s, "pending", &"".into())?;
            set(&s, "pendingSelecting", &false.into())?;
        }
        if !truthy(&get(&s, "temporary")) && !pending {
            continue;
        }
        set(&s, "restorePending", &true.into())?;
        keepalive(&sess);
    }
    Ok(())
}
fn show() -> Result<JsValue, JsValue> {
    if !truthy(&global("termPageHidden")) {
        return Ok(js_sys::Promise::resolve(&JsValue::FALSE).into());
    }
    put("termPageHidden", false)?;
    let active = global("activeTerm");
    let current = if truthy(&active)
        && truthy(&call(
            &global("openTerms"),
            "has",
            std::slice::from_ref(&active),
        )?) {
        active
    } else {
        "".into()
    };
    for (sess, s) in entries(&global("termInteraction"))? {
        if truthy(&get(&s, "busy")) {
            if sess == current {
                set(&s, "resyncPending", &true.into())?;
            }
            continue;
        }
        set(&s, "seq", &next_seq()?)?;
        for (k, v) in [
            ("pending", "".into()),
            ("pendingSelecting", false.into()),
            ("resyncPending", false.into()),
            ("mouse", JsValue::NULL),
        ] {
            set(&s, k, &v)?;
        }
        if !truthy(&get(&s, "temporary")) {
            set(&s, "restorePending", &false.into())?;
        }
        post(&sess)?;
    }
    if !truthy(&current) {
        return Ok(js_sys::Promise::resolve(&JsValue::FALSE).into());
    }
    let s = map_get("termInteraction", &current)?;
    if truthy(&get(&s, "busy")) {
        set(&s, "resyncPending", &true.into())?;
        return Ok(js_sys::Promise::resolve(&JsValue::FALSE).into());
    }
    begin(current, None, false)
}
fn frame_message(event: &JsValue) -> Result<bool, JsValue> {
    let data = get(event, "data");
    if get(event, "origin") != get(&global("location"), "origin")
        || text(&get(&data, "source")) != "comandos-term"
    {
        return Ok(false);
    }
    let mut found = JsValue::UNDEFINED;
    for (sess, o) in entries(&global("openTerms"))? {
        let frame = get(&o, "frame");
        if truthy(&frame)
            && get(&frame, "contentWindow") == get(event, "source")
            && text(&get(&get(&frame, "dataset"), "compat")) != "1"
        {
            found = sess;
            break;
        }
    }
    if !truthy(&found) {
        return Ok(false);
    }
    match text(&get(&data, "type")).as_str() {
        "ready" => post(&found),
        "pane-selected" => {
            let pane = get(&data, "pane");
            if found != global("activeTerm")
                || !valid_pane(&text(&pane))
                || !state_list().iter().any(|x| {
                    get(x, "session") == found && get(x, "pane") == pane && truthy(&get(x, "alive"))
                })
            {
                return Ok(false);
            }
            set(
                &state(),
                "sel",
                &js(&format!("{}|{}", text(&found), text(&pane))),
            )?;
            set(&state(), "selTs", &now().into())?;
            run(
                "render",
                &[default(get(&state(), "list"), Array::new().into())],
            )?;
            Ok(true)
        }
        "interaction-request" => {
            fire(
                begin(found, Some(truthy(&get(&data, "selecting"))), false),
                false,
            );
            Ok(true)
        }
        _ => Ok(false),
    }
}
pub(super) fn mount() -> Result<(), JsValue> {
    publish("termInteractionState", |a| interaction(&a.get(0)))?;
    publish("postTermState", |a| Ok(post(&a.get(0))?.into()))?;
    publish("applyTermInteraction", |a| Ok(post(&a.get(0))?.into()))?;
    publish("cleanupTermInteraction", |a| {
        cleanup(&a.get(0))?;
        Ok(JsValue::UNDEFINED)
    })?;
    publish("syncTermInteraction", |a| begin(a.get(0), None, false))?;
    publish("setTermSelectionMode", |a| {
        begin(a.get(0), Some(truthy(&a.get(1))), truthy(&a.get(2)))
    })?;
    publish("restoreTermInteraction", |a| restore(a.get(0)))?;
    publish("restoreInactiveTermInteractions", |a| {
        inactive(&a.get(0))?;
        Ok(JsValue::UNDEFINED)
    })?;
    publish("sendTermInteractionKeepalive", |a| {
        Ok(keepalive(&a.get(0)).into())
    })?;
    publish("restoreAllTermInteractions", |_| {
        hide()?;
        Ok(JsValue::UNDEFINED)
    })?;
    publish("handleTermInteractionsPageShow", |_| show())?;
    publish("handleTermFrameMessage", |a| {
        Ok(frame_message(&a.get(0))?.into())
    })
}
