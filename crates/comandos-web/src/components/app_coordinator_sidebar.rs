use super::*;
fn quick(sess: &JsValue) -> bool {
    text(sess).starts_with("term-q")
}
fn select_term(t: &JsValue) -> Result<(), JsValue> {
    let sess = default(get(t, "tabId"), get(t, "session"));
    if !truthy(&sess) {
        return Ok(());
    }
    let sb = global("SBT");
    set(&sb, "term", &sess)?;
    set(
        &sb,
        "activeSeen",
        &default(get(&selected_sidebar()?, "session"), "".into()),
    )?;
    let pane = get(t, "pane");
    set(
        &state(),
        "sel",
        &js(&format!(
            "{}{}",
            text(&sess),
            if truthy(&pane) {
                format!("|{}", text(&pane))
            } else {
                String::new()
            }
        )),
    )?;
    set(&state(), "selTs", &now().into())?;
    sync()
}
fn term_target() -> Result<JsValue, JsValue> {
    let sb = global("SBT");
    let sess = get(&sb, "term");
    if !truthy(&sess) {
        return Ok(JsValue::NULL);
    }
    if default(get(&selected_sidebar()?, "session"), "".into()) != get(&sb, "activeSeen") {
        set(&sb, "term", &"".into())?;
        return Ok(JsValue::NULL);
    }
    if let Some(it) = state_list()
        .into_iter()
        .find(|it| truthy(&get(it, "alive")) && get(it, "session") == sess)
    {
        return Ok(it);
    }
    set(&sb, "term", &"".into())?;
    Ok(JsValue::NULL)
}
fn active_target() -> Result<JsValue, JsValue> {
    if !truthy(&get(&selected_sidebar()?, "session")) && !truthy(&get(&state(), "sel")) {
        return Ok(JsValue::NULL);
    }
    let selected = term_target()?;
    let it = if truthy(&selected) {
        selected
    } else {
        run(
            "pickSel",
            &[default(get(&state(), "list"), Array::new().into())],
        )?
    };
    let sess = get(&it, "session");
    if !truthy(&it) || !truthy(&sess) {
        return Ok(JsValue::NULL);
    }
    let label = default(
        get(&map_get("openTerms", &sess)?, "label"),
        default(get(&it, "project"), sess.clone()),
    );
    let q = quick(&sess);
    let pane = default(get(&it, "pane"), "".into());
    item(&[
        ("session", sess.clone()),
        ("pane", pane.clone()),
        ("kind", if q { "term" } else { "pane" }.into()),
        ("paneKey", if q { sess } else { "".into() }),
        (
            "title",
            js(&format!(
                "{} · {}",
                text(&label),
                text(&default(pane, "panel".into()))
            )),
        ),
    ])
}
fn quick_entries() -> Result<JsValue, JsValue> {
    let mut seen = Vec::new();
    let out = Array::new();
    for it in state_list() {
        let sess = get(&it, "session");
        if !truthy(&get(&it, "alive")) || !quick(&sess) || seen.contains(&sess) {
            continue;
        }
        seen.push(sess.clone());
        out.push(&item(&[
            ("tabId", sess.clone()),
            ("session", sess.clone()),
            ("pane", default(get(&it, "pane"), "".into())),
            ("paneKey", sess.clone()),
            (
                "label",
                default(
                    get(&map_get("openTerms", &sess)?, "label"),
                    default(get(&it, "project"), sess),
                ),
            ),
        ])?);
    }
    Ok(out.into())
}
fn bridge(message: &JsValue) -> Result<(), JsValue> {
    call(
        &get(&get(&global("webkit"), "messageHandlers"), "centro"),
        "postMessage",
        &[js_sys::JSON::stringify(message)?.into()],
    )?;
    Ok(())
}
fn term_mount(sess: JsValue, host: JsValue, info: JsValue) -> Result<(), JsValue> {
    let frames = global("sbTermFrames");
    if is_app() {
        set(&get(&root(), "dataset"), "nativeSideTerm", &"1".into())?;
        for (_, frame) in entries(&frames)? {
            set(&get(&frame, "style"), "display", &"none".into())?;
        }
        let info = if truthy(&info) {
            info
        } else {
            item(&[
                ("session", default(sess.clone(), "".into())),
                ("hidden", (!truthy(&sess)).into()),
                ("tabs", Array::new().into()),
            ])?
        };
        let message = item(&[(
            "sidebarTerm",
            item(&[
                ("session", default(get(&info, "session"), "".into())),
                ("hidden", truthy(&get(&info, "hidden")).into()),
                ("tabs", default(get(&info, "tabs"), Array::new().into())),
                ("cmds", default(get(&info, "cmds"), JsValue::NULL)),
            ])?,
        )])?;
        let bytes = JsValue::from(js_sys::JSON::stringify(&message)?);
        if bytes != global("sbNativeMsg") {
            put("sbNativeMsg", bytes.clone())?;
            let _ = call(
                &get(&get(&global("webkit"), "messageHandlers"), "centro"),
                "postMessage",
                &[bytes],
            );
        }
        return Ok(());
    }
    if !truthy(&host) {
        return Ok(());
    }
    for (k, frame) in entries(&frames)? {
        set(
            &get(&frame, "style"),
            "display",
            &if k == sess { "" } else { "none" }.into(),
        )?;
    }
    if !truthy(&sess) {
        return Ok(());
    }
    let frame = map_get("sbTermFrames", &sess)?;
    if truthy(&frame) {
        if get(&frame, "parentNode") != host {
            call(&host, "appendChild", &[frame])?;
        }
        return Ok(());
    }
    let frame = call(&doc(), "createElement", &["iframe".into()])?;
    set(&frame, "className", &"sb-term".into())?;
    set(&frame, "title", &sess)?;
    call(&frames, "set", &[sess.clone(), frame.clone()])?;
    call(&host, "appendChild", std::slice::from_ref(&frame))?;
    let token = run("webtermAccessToken", &[]);
    fire(
        Ok(promise(async move {
            let token = wait(token).await?;
            if !truthy(&token) {
                return Err(js_sys::Error::new(&string(&translate(
                    "Token de terminal no disponible",
                    "Terminal token unavailable",
                )))
                .into());
            }
            let token = encode(&token)?;
            let sess = encode(&sess)?;
            let url = if truthy(&global("WEBTERM")) {
                let base = wait(run("resolveTermBase", &[])).await?;
                if base == global("TERM_BASE") {
                    format!(
                        "{}/?auth={token}&arg={sess}&theme={}",
                        text(&base),
                        encode(&global("curTheme"))?
                    )
                } else {
                    format!("{}/?arg={token}&arg={sess}", text(&base))
                }
            } else {
                format!("http://127.0.0.1:4779/?arg={token}&arg={sess}")
            };
            set(&frame, "src", &js(&url))?;
            Ok(JsValue::UNDEFINED)
        })),
        true,
    );
    Ok(())
}
fn kill_term(sess: JsValue) -> Result<JsValue, JsValue> {
    if !quick(&sess) {
        return Ok(js_sys::Promise::reject(
            &js_sys::Error::new(&string(&translate(
                "Solo se cierran terminales rápidas",
                "Only quick terminals can be closed here",
            )))
            .into(),
        )
        .into());
    }
    let request = run(
        "api",
        &["/kill".into(), item(&[("session", sess.clone())])?],
    );
    Ok(promise(async move {
        wait(request).await?;
        let frame = map_get("sbTermFrames", &sess)?;
        if truthy(&frame) {
            call(&frame, "remove", &[])?;
            call(&global("sbTermFrames"), "delete", &[sess])?;
        }
        if global("tick").is_function() {
            run("tick", &[])?;
        }
        Ok(JsValue::UNDEFINED)
    }))
}
fn open_any(sess: JsValue, label: JsValue) -> Result<(), JsValue> {
    if !truthy(&sess) {
        return Ok(());
    }
    if is_app() {
        run("openInApp", &[sess, "claude".into(), label])?;
    } else {
        run("openTerm", &[sess.clone(), default(label, sess)])?;
    }
    Ok(())
}
fn quick_instance(sidebar: bool) -> Result<JsValue, JsValue> {
    let name = if sidebar {
        "sidebarQuickTerm"
    } else {
        "quickTerminal"
    };
    if !truthy(&global(name)) && truthy(&global("ComandosQuickTerminal")) {
        let options = item(&[
            ("api", global("api")),
            ("toast", global("toast")),
            ("storage", global("sessionStorage")),
        ])?;
        if sidebar {
            set(&options, "place", &"sidebar".into())?;
            set(
                &options,
                "storageKey",
                &"comandos.quickTerminal.sidebarRequestId".into(),
            )?;
            set(
                &options,
                "openTerm",
                &function(|a| {
                    let sess = a.get(0);
                    let cs = global("commandSidebar");
                    if truthy(&cs) {
                        set(&get(&cs, "state"), "curTerm", &sess)?;
                    }
                    select_term(&item(&[
                        ("tabId", sess.clone()),
                        ("session", sess),
                        ("title", a.get(1)),
                    ])?)?;
                    Ok(JsValue::UNDEFINED)
                }),
            )?;
            set(
                &options,
                "onOpened",
                &function(|_| {
                    if global("tick").is_function() {
                        run("tick", &[])?;
                    }
                    Ok(JsValue::UNDEFINED)
                }),
            )?;
        } else {
            set(&options, "openTerm", &global("openTabAnywhere"))?;
            set(
                &options,
                "onOpened",
                &function(|_| {
                    put("tabsPollTs", 0)?;
                    Ok(JsValue::UNDEFINED)
                }),
            )?;
        }
        put(
            name,
            call(
                &global("ComandosQuickTerminal"),
                "createQuickTerminal",
                &[options],
            )?,
        )?;
    }
    Ok(default(global(name), JsValue::NULL))
}
fn refresh() -> Result<(), JsValue> {
    let sidebar = global("commandSidebar");
    if !truthy(&sidebar) {
        return Ok(());
    }
    let cs = global("CS");
    if truthy(&get(&cs, "busy")) {
        set(&cs, "again", &true.into())?;
        return Ok(());
    }
    let state = cs.clone();
    let promise = promise(async move {
        if let Err(e) = wait(call(&sidebar, "refresh", &[])).await {
            notify_error(&e);
        }
        set(&state, "busy", &JsValue::NULL)?;
        if truthy(&get(&state, "again")) {
            set(&state, "again", &false.into())?;
            refresh()?;
        }
        Ok(JsValue::UNDEFINED)
    });
    set(&cs, "busy", &promise)
}
fn limits_current() -> Result<String, JsValue> {
    if !truthy(&get(&selected_sidebar()?, "session")) && !truthy(&get(&state(), "sel")) {
        return Ok(String::new());
    }
    let selected = term_target()?;
    let it = if truthy(&selected) {
        selected
    } else {
        run(
            "pickSel",
            &[default(get(&state(), "list"), Array::new().into())],
        )?
    };
    let agent = get(&it, "agent");
    if !truthy(&it) || !truthy(&agent) || matches!(text(&agent).as_str(), "acp" | "shell") {
        return Ok(String::new());
    }
    Ok(format!(
        "{}:{}",
        text(&agent),
        text(&default(
            get(&it, "harnessAccount"),
            default(get(&it, "account"), "main".into())
        ))
    ))
}
fn limits_paint(slot: &JsValue) -> Result<(), JsValue> {
    if !truthy(slot) {
        return Ok(());
    }
    let limits = global("SB_LIMITS");
    let accounts = get(&limits, "accounts");
    if !truthy(&accounts) {
        if !truthy(&get(slot, "firstChild")) {
            set(
                slot,
                "innerHTML",
                &js(&format!(
                    "<div class=\"wait\">{}</div>",
                    text(&translate("Leyendo el uso…", "Reading usage…"))
                )),
            )?;
        }
        return Ok(());
    }
    let key = js(&format!(
        "{}|{}",
        string(&get(&limits, "at")),
        limits_current()?
    ));
    if get(&get(slot, "dataset"), "v") == key {
        return Ok(());
    }
    let mut view = get(slot, "_limitsView");
    if !truthy(&view) {
        let options = item(&[
            (
                "onAnalytics",
                function(|_| {
                    let params = new("URLSearchParams", &[get(&global("location"), "search")])?;
                    if is_app() && truthy(&call(&params, "has", &["anwin".into()])?) {
                        bridge(&item(&[("headerAction", "analytics".into())])?)?;
                    } else {
                        run("openAnalytics", &["cuentas".into()])?;
                    }
                    Ok(JsValue::UNDEFINED)
                }),
            ),
            (
                "onSwitch",
                function(|a| {
                    let account = a.get(0);
                    let from = a.get(1);
                    let target = active_target()?;
                    if !truthy(&target) || js(&limits_current()?) != from {
                        return Ok(js_sys::Promise::reject(
                            &js_sys::Error::new(&string(&translate(
                                "El pane seleccionado cambió; revisa la cuenta antes de continuar",
                                "The selected pane changed; check the account before continuing",
                            )))
                            .into(),
                        )
                        .into());
                    }
                    let request = run(
                        "api",
                        &[
                            "/account/switch".into(),
                            item(&[
                                ("session", get(&target, "session")),
                                ("pane", default(get(&target, "pane"), "".into())),
                                ("alias", get(&account, "alias")),
                                ("interrupt", true.into()),
                            ])?,
                        ],
                    );
                    Ok(promise(async move {
                        let result = wait(request).await?;
                        let alias = text(&get(&account, "alias"));
                        run(
                            "toast",
                            &[if truthy(&get(&result, "queued")) {
                                translate("Cambio de cuenta en cola", "Account switch queued")
                            } else {
                                translate(
                                    &format!("Reanudando la conversación en {alias}"),
                                    &format!("Resuming the conversation in {alias}"),
                                )
                            }],
                        )?;
                        set(&global("SB_LIMITS"), "at", &0.into())?;
                        if global("tick").is_function() {
                            run("tick", &[])?;
                        }
                        Ok(JsValue::UNDEFINED)
                    }))
                }),
            ),
            (
                "onError",
                function(|a| {
                    notify_error(&a.get(0));
                    Ok(JsValue::UNDEFINED)
                }),
            ),
        ])?;
        view = call(
            &global("ComandosCommandSidebar"),
            "createLimitsView",
            &[slot.clone(), options],
        )?;
        set(slot, "_limitsView", &view)?;
    }
    call(&view, "update", &[accounts, js(&limits_current()?)])?;
    set(&get(slot, "dataset"), "v", &key)?;
    run("hydrateIcons", std::slice::from_ref(slot))?;
    Ok(())
}
fn limits(slot: &JsValue) -> Result<(), JsValue> {
    limits_paint(slot)?;
    let limits = global("SB_LIMITS");
    if truthy(&get(&limits, "job")) || now() - number(&get(&limits, "at")) < 60000.0 {
        return Ok(());
    }
    let request = run("api", &["/analytics/week?offset=0&sidebar=1".into()]);
    let state = limits.clone();
    let job = promise(async move {
        let result: Result<(), JsValue> = async {
            let m = wait(request).await?;
            set(
                &state,
                "accounts",
                &default(get(&m, "accounts"), Array::new().into()),
            )?;
            set(&state, "at", &now().into())?;
            let bytes = js_sys::JSON::stringify(&item(&[
                ("accounts", get(&state, "accounts")),
                ("at", get(&state, "at")),
            ])?)?;
            let _ = call(
                &global("localStorage"),
                "setItem",
                &["cc-sb-limits3".into(), bytes.into()],
            );
            limits_paint(&query(&doc(), "#command-sidebar .cs-limits"))?;
            Ok(())
        }
        .await;
        if result.is_err() {
            set(&state, "at", &(now() - 45000.0).into())?;
        }
        set(&state, "job", &JsValue::NULL)?;
        Ok(JsValue::UNDEFINED)
    });
    set(&limits, "job", &job)
}
fn sync() -> Result<(), JsValue> {
    let sidebar = global("commandSidebar");
    if !truthy(&sidebar) {
        return Ok(());
    }
    let target = active_target()?;
    let key = if truthy(&target) {
        js(&format!(
            "{}|{}",
            text(&get(&target, "session")),
            text(&get(&target, "pane"))
        ))
    } else {
        "".into()
    };
    let it = state_list()
        .into_iter()
        .find(|it| {
            truthy(&get(it, "alive"))
                && get(it, "session") == get(&target, "session")
                && default(get(it, "pane"), "".into()) == get(&target, "pane")
        })
        .unwrap_or(JsValue::UNDEFINED);
    let agent = get(&it, "agent");
    let agent = if matches!(
        text(&agent).as_str(),
        "claude" | "codex" | "grok" | "opencode" | "agy"
    ) {
        agent
    } else {
        "".into()
    };
    let terms = values(&quick_entries()?)
        .into_iter()
        .map(|e| {
            JsValue::from(
                [get(&e, "tabId"), get(&e, "pane"), get(&e, "label")]
                    .into_iter()
                    .collect::<Array>(),
            )
        })
        .collect::<Array>();
    let terms = JsValue::from(js_sys::JSON::stringify(&terms.into())?);
    let cs = global("CS");
    let changed = terms != get(&cs, "termsSig");
    set(&cs, "termsSig", &terms)?;
    if key != get(&cs, "targetKey") {
        set(&cs, "targetKey", &key)?;
        set(&cs, "agentSeen", &agent)?;
        refresh()?;
        return Ok(());
    }
    if agent != get(&cs, "agentSeen") {
        set(&cs, "agentSeen", &agent)?;
        if agent != default(get(&get(&sidebar, "state"), "cliInPane"), "".into()) {
            refresh()?;
            return Ok(());
        }
    }
    if changed {
        call(&sidebar, "render", &[])?;
    }
    Ok(())
}
fn mount_servers(slot: JsValue) -> Result<(), JsValue> {
    let panel = id("servers-panel");
    let button = id("btn-servers");
    if !truthy(&panel) {
        return Ok(());
    }
    if !truthy(&get(&panel, "_home")) {
        set(
            &panel,
            "_home",
            &item(&[
                ("parent", get(&panel, "parentNode")),
                ("next", get(&panel, "nextSibling")),
            ])?,
        )?;
    }
    if truthy(&slot) {
        if get(&panel, "parentNode") != slot {
            call(&slot, "appendChild", std::slice::from_ref(&panel))?;
        }
        classes(&panel, "hidden", false);
        classes(&id("ssh-bar"), "open", true);
        attr(&id("ssh-toggle"), "aria-expanded", "true");
        if truthy(&button) {
            attr(&button, "aria-expanded", "true");
        }
        run("loadSsh", &[])?;
    } else {
        let home = get(&panel, "_home");
        let parent = get(&home, "parent");
        if get(&panel, "parentNode") != parent {
            call(
                &parent,
                "insertBefore",
                &[panel.clone(), get(&home, "next")],
            )?;
        }
        classes(&panel, "hidden", true);
        if truthy(&button) {
            attr(&button, "aria-expanded", "false");
        }
    }
    Ok(())
}
fn mount_sidebar() -> Result<(), JsValue> {
    if truthy(&global("ONLY_PANEL")) {
        return Ok(());
    }
    let root = id("command-sidebar");
    if !truthy(&root)
        || truthy(&global("commandSidebar"))
        || !truthy(&global("ComandosCommandSidebar"))
    {
        return Ok(());
    }
    let options = item(&[
        ("api", global("api")),
        ("root", root),
        ("storage", global("localStorage")),
        ("hydrate", global("hydrateIcons")),
        (
            "makeId",
            function(|_| {
                let d = call_value(&now().into(), "toString", &[36.into()])?;
                let r = call_value(
                    &JsValue::from(js_sys::Math::random()),
                    "toString",
                    &[36.into()],
                )?;
                let r = call_value(&r, "slice", &[2.into(), 8.into()])?;
                Ok(js(&format!("ct-{}-{}", text(&d), text(&r))))
            }),
        ),
        ("getTarget", global("activePaneTarget")),
        ("focusTarget", global("selectSidebarTerm")),
        ("mountTerm", global("sidebarTermMount")),
        (
            "openBuilder",
            function(|_| {
                if is_app() {
                    bridge(&item(&[("headerAction", "chains".into())])?)?;
                } else if truthy(&global("chainBuilder")) {
                    call(&global("chainBuilder"), "open", &[])?;
                }
                Ok(JsValue::UNDEFINED)
            }),
        ),
        ("toast", global("toast")),
        ("terminals", global("quickTermEntries")),
        (
            "newTerm",
            function(|_| {
                let q = quick_instance(true)?;
                if truthy(&q) {
                    call(&q, "open", &[])
                } else {
                    run(
                        "toast",
                        &[
                            translate(
                                "Terminal rápida no disponible",
                                "Quick terminal unavailable",
                            ),
                            true.into(),
                        ],
                    )
                }
            }),
        ),
        ("killTerm", global("sidebarKillTerm")),
        ("renderLimits", global("sidebarLimits")),
        (
            "mountServers",
            function(|a| {
                mount_servers(a.get(0))?;
                Ok(JsValue::UNDEFINED)
            }),
        ),
    ])?;
    put(
        "commandSidebar",
        call(
            &global("ComandosCommandSidebar"),
            "createCommandSidebar",
            &[options],
        )?,
    )?;
    mount_builder()?;
    sync()
}
fn mount_builder() -> Result<(), JsValue> {
    if truthy(&global("ONLY_PANEL"))
        || truthy(&global("chainBuilder"))
        || !truthy(&global("ComandosChainBuilder"))
        || !truthy(&global("commandSidebar"))
    {
        return Ok(());
    }
    let options = item(&[
        ("api", global("api")),
        ("root", body()),
        ("toast", global("toast")),
        ("hydrate", global("hydrateIcons")),
        (
            "catalog",
            function(|_| Ok(get(&get(&global("commandSidebar"), "state"), "catalog"))),
        ),
        (
            "chains",
            function(|_| Ok(get(&get(&global("commandSidebar"), "state"), "chains"))),
        ),
        (
            "here",
            function(|_| Ok(get(&get(&global("commandSidebar"), "state"), "cliInPane"))),
        ),
        (
            "target",
            function(|_| {
                let target = active_target()?;
                let here = get(&get(&global("commandSidebar"), "state"), "cliInPane");
                Ok(if truthy(&target) {
                    js(&format!(
                        "{}{}",
                        text(&get(&target, "title")),
                        if truthy(&here) {
                            format!(" · {}", text(&here))
                        } else {
                            String::new()
                        }
                    ))
                } else {
                    "".into()
                })
            }),
        ),
        (
            "onSaved",
            function(|a| {
                let chain = a.get(0);
                let options = a.get(1);
                let request = call(&global("commandSidebar"), "refresh", &[]);
                Ok(promise(async move {
                    if let Err(e) = wait(request).await {
                        let msg = default(
                            get(&e, "message"),
                            translate(
                                "No se pudo refrescar las cadenas",
                                "Could not refresh the chains",
                            ),
                        );
                        run("toast", &[msg, true.into()])?;
                        return Ok(JsValue::UNDEFINED);
                    }
                    if truthy(&get(&options, "run")) {
                        call(
                            &global("commandSidebar"),
                            "startChain",
                            &[get(&chain, "slug")],
                        )?;
                    }
                    Ok(JsValue::UNDEFINED)
                }))
            }),
        ),
    ])?;
    put(
        "chainBuilder",
        call(
            &global("ComandosChainBuilder"),
            "createChainBuilder",
            &[options],
        )?,
    )
}
fn page_load(st: JsValue, sess: JsValue, pane: JsValue) -> Result<JsValue, JsValue> {
    let suffix = if truthy(&sess) && valid_pane(&text(&pane)) {
        format!("?session={}&pane={}", encode(&sess)?, encode(&pane)?)
    } else {
        String::new()
    };
    let cat = run("api", &[js(&format!("/commands/catalog{suffix}"))]);
    let chains = run("api", &["/chains".into()]);
    let cat = cat?;
    let chains = chains?;
    let all = js_sys::Promise::all(&[cat, chains].into_iter().collect::<Array>());
    Ok(promise(async move {
        let results = wait(Ok(all.into())).await?;
        let cat = get(&results, "0");
        let chains = get(&results, "1");
        set(
            &st,
            "catalog",
            &default(get(&cat, "catalog"), JsValue::NULL),
        )?;
        let here = default(get(&cat, "cliInPane"), "".into());
        set(&st, "here", &here)?;
        set(
            &st,
            "target",
            &if truthy(&sess) {
                js(&format!(
                    "{}{}{}",
                    text(&sess),
                    if truthy(&pane) {
                        format!(" · {}", text(&pane))
                    } else {
                        String::new()
                    },
                    if truthy(&here) {
                        format!(" · {}", text(&here))
                    } else {
                        String::new()
                    }
                ))
            } else {
                "".into()
            },
        )?;
        let chains = get(&chains, "chains");
        set(
            &st,
            "chains",
            &if Array::is_array(&chains) {
                chains
            } else {
                Array::new().into()
            },
        )?;
        Ok(JsValue::UNDEFINED)
    }))
}
fn mount_page() -> Result<(), JsValue> {
    if text(&global("ONLY_PANEL")) != "chains"
        || truthy(&global("chainBuilder"))
        || !truthy(&global("ComandosChainBuilder"))
    {
        return Ok(());
    }
    let params = new("URLSearchParams", &[get(&global("location"), "search")])?;
    let sess = call_value(
        &default(call(&params, "get", &["session".into()])?, "".into()),
        "slice",
        &[0.into(), 120.into()],
    )?;
    let pane = call_value(
        &default(call(&params, "get", &["pane".into()])?, "".into()),
        "slice",
        &[0.into(), 12.into()],
    )?;
    let st = item(&[
        ("catalog", JsValue::NULL),
        ("chains", Array::new().into()),
        ("here", "".into()),
        ("target", "".into()),
    ])?;
    let options = item(&[
        ("api", global("api")),
        ("root", body()),
        ("toast", global("toast")),
        ("hydrate", global("hydrateIcons")),
    ])?;
    for name in ["catalog", "chains", "here", "target"] {
        let state = st.clone();
        set(&options, name, &function(move |_| Ok(get(&state, name))))?;
    }
    set(
        &options,
        "onClose",
        &function(|_| {
            if is_app() {
                let _ = bridge(&item(&[("chainModal", "close".into())])?);
            }
            Ok(JsValue::UNDEFINED)
        }),
    )?;
    let (state, session, p) = (st.clone(), sess.clone(), pane.clone());
    set(
        &options,
        "onSaved",
        &function(move |a| {
            let chain = a.get(0);
            let options = a.get(1);
            let request = page_load(state.clone(), session.clone(), p.clone());
            Ok(promise(async move {
                wait(request).await?;
                if is_app() {
                    let _ = bridge(&item(&[
                        (
                            "chainModal",
                            if truthy(&get(&options, "run")) {
                                "run"
                            } else {
                                "saved"
                            }
                            .into(),
                        ),
                        ("slug", get(&chain, "slug")),
                    ])?);
                }
                Ok(JsValue::UNDEFINED)
            }))
        }),
    )?;
    put(
        "chainBuilder",
        call(
            &global("ComandosChainBuilder"),
            "createChainBuilder",
            &[options],
        )?,
    )?;
    let request = page_load(st, sess, pane);
    fire(
        Ok(promise(async move {
            if let Err(e) = wait(request).await {
                let message = default(
                    get(&e, "message"),
                    translate(
                        "No se pudo cargar el catálogo",
                        "Could not load the catalog",
                    ),
                );
                run("toast", &[message, true.into()])?;
            }
            call(&global("chainBuilder"), "open", &[])?;
            Ok(JsValue::UNDEFINED)
        })),
        false,
    );
    Ok(())
}
fn initial_limits() {
    let result = (|| -> Result<(), JsValue> {
        let raw = call(
            &global("localStorage"),
            "getItem",
            &["cc-sb-limits3".into()],
        )?;
        let c = js_sys::JSON::parse(&string(&default(raw, "null".into())))?;
        if truthy(&c)
            && Array::is_array(&get(&c, "accounts"))
            && now() - number(&get(&c, "at")) < 30.0 * 60000.0
        {
            set(&global("SB_LIMITS"), "accounts", &get(&c, "accounts"))?;
        }
        Ok(())
    })();
    let _ = result;
}
pub(super) fn mount() -> Result<(), JsValue> {
    publish("isQuickTermSession", |a| Ok(quick(&a.get(0)).into()))?;
    publish("selectSidebarTerm", |a| {
        select_term(&a.get(0))?;
        Ok(JsValue::UNDEFINED)
    })?;
    publish("sidebarTermAction", |a| {
        if truthy(&global("commandSidebar")) {
            let _ = call(
                &global("commandSidebar"),
                "termAction",
                &[a.get(0), a.get(1)],
            );
        }
        Ok(JsValue::UNDEFINED)
    })?;
    publish("sidebarTermFocused", |a| {
        let sess = a.get(0);
        if !truthy(&sess) {
            if truthy(&get(&global("SBT"), "term")) {
                set(&global("SBT"), "term", &"".into())?;
                sync()?;
            }
        } else {
            for t in values(&quick_entries()?) {
                if get(&t, "session") == sess {
                    select_term(&item(&[
                        ("kind", "term".into()),
                        ("tabId", get(&t, "tabId")),
                        ("paneKey", get(&t, "paneKey")),
                        ("session", sess),
                        ("pane", get(&t, "pane")),
                        ("title", get(&t, "label")),
                    ])?)?;
                    break;
                }
            }
        }
        Ok(JsValue::UNDEFINED)
    })?;
    publish("sidebarTermTarget", |_| term_target())?;
    publish("sidebarTermMount", |a| {
        term_mount(a.get(0), a.get(1), a.get(2))?;
        Ok(JsValue::UNDEFINED)
    })?;
    publish("sidebarKillTerm", |a| kill_term(a.get(0)))?;
    publish("activePaneTarget", |_| active_target())?;
    publish("quickTermEntries", |_| quick_entries())?;
    publish("openTabAnywhere", |a| {
        open_any(a.get(0), a.get(1))?;
        Ok(JsValue::UNDEFINED)
    })?;
    publish("sidebarQuickTerminal", |_| quick_instance(true))?;
    publish("quickTerminalInstance", |_| quick_instance(false))?;
    publish("refreshCommandSidebar", |_| {
        refresh()?;
        Ok(JsValue::UNDEFINED)
    })?;
    publish("sidebarLimitsCur", |_| Ok(js(&limits_current()?)))?;
    publish("sidebarLimitsPaint", |a| {
        limits_paint(&a.get(0))?;
        Ok(JsValue::UNDEFINED)
    })?;
    publish("sidebarLimits", |a| {
        limits(&a.get(0))?;
        Ok(JsValue::UNDEFINED)
    })?;
    publish("syncCommandSidebar", |_| {
        sync()?;
        Ok(JsValue::UNDEFINED)
    })?;
    publish("mountCommandSidebar", |_| {
        mount_sidebar()?;
        Ok(JsValue::UNDEFINED)
    })?;
    publish("mountChainPage", |_| {
        mount_page()?;
        Ok(JsValue::UNDEFINED)
    })?;
    publish("mountChainBuilder", |_| {
        mount_builder()?;
        Ok(JsValue::UNDEFINED)
    })?;
    initial_limits();
    run(
        "setInterval",
        &[
            function(|_| {
                let slot = query(
                    &doc(),
                    "#command-sidebar .cs-empty-terms:not([hidden]) .cs-limits",
                );
                if truthy(&slot) && text(&get(&doc(), "visibilityState")) == "visible" {
                    limits(&slot)?;
                }
                Ok(JsValue::UNDEFINED)
            }),
            60000.into(),
        ],
    )?;
    let _ = call(
        &global("localStorage"),
        "removeItem",
        &["cc-sb-limits".into()],
    );
    let _ = call(
        &global("localStorage"),
        "removeItem",
        &["cc-sb-limits2".into()],
    );
    Ok(())
}
