use super::*;
fn storage(k: &str) -> JsValue {
    call(&global("localStorage"), "getItem", &[k.into()]).unwrap_or(JsValue::NULL)
}
fn store(k: &str) -> Value {
    storage(k)
        .as_string()
        .and_then(|s| serde_json::from_str(&comandos_web_view::utf16::json_to_unicode(&s)).ok())
        .unwrap_or(json!({}))
}
fn pins() -> Vec<Value> {
    storage("cc-nf-pins")
        .as_string()
        .and_then(|s| {
            serde_json::from_str::<Value>(&comandos_web_view::utf16::json_to_unicode(&s)).ok()
        })
        .map(|v| list(&v))
        .unwrap_or_default()
}
fn save(k: &str, v: &Value) {
    let encoded = comandos_web_view::utf16::json_to_javascript(&v.to_string());
    let _ = call(
        &global("localStorage"),
        "setItem",
        &[k.into(), encoded.into()],
    );
    let mapped = match k {
        "cc-nf-dismiss" => Some("nfDismiss"),
        "cc-nf-snooze" => Some("nfSnooze"),
        _ => None,
    };
    if let Some(name) = mapped {
        let mut body = serde_json::Map::new();
        body.insert(name.into(), v.clone());
        fire_api("/prefs-set", Value::Object(body));
    }
}
fn hydrate(prefs: &JsValue) {
    for (k, field) in [("cc-nf-dismiss", "nfDismiss"), ("cc-nf-snooze", "nfSnooze")] {
        let v = get(prefs, field);
        let v = if truthy(&v) { v } else { object() };
        let text = js_sys::JSON::stringify(&v)
            .ok()
            .map(JsValue::from)
            .unwrap_or("{}".into());
        let _ = call(&global("localStorage"), "setItem", &[k.into(), text]);
    }
}
fn snoozed(key: &str) -> bool {
    num(at(&store("cc-nf-snooze"), key)) > now() / 1000.0
}
fn dismissed(key: &str) -> bool {
    truth(at(&store("cc-nf-dismiss"), key))
}
fn action(id: &str, label: String, icon: &str) -> Value {
    json!({"id":id,"label":label,"icon":icon})
}
fn priority() -> Vec<Value> {
    let mut out = vec![];
    for it in list(&to_utf16_json(&get(&state(), "list"))) {
        let sg = at(&it, "suggestion");
        if !truth(sg) || !truth(at(&it, "alive")) {
            continue;
        }
        let suffix = ["model", "key", "command"]
            .into_iter()
            .map(|k| s(at(sg, k)))
            .find(|s| !s.is_empty())
            .unwrap_or_default();
        let cwd = s(at(&it, "cwd"));
        let title = [
            short_path(&cwd),
            s(at(&it, "project")),
            s(at(&it, "session")),
        ]
        .into_iter()
        .find(|s| !s.is_empty())
        .unwrap_or_default();
        out.push(json!({"cls":"sugerencia","key":format!("sg|{}|{}|{}|{suffix}",s(at(&it,"session")),s(at(&it,"pane")),s(at(sg,"kind"))),"icon":if truth(at(sg,"icon")){s(at(sg,"icon"))}else{"sprout".into()},"sev":"brand","title":title,"tip":cwd,"body":if truth(at(sg,"text")){s(at(sg,"text"))}else{s(at(sg,"reason"))},"ts":at(&it,"ts"),"session":at(&it,"session"),"pane":s(at(&it,"pane")),"sg":sg,"actions":[action("aplicar",tf("Aplicar","Apply"),"check"),action("abrir",tf("Ver sesión","Open session"),"terminal")]}));
    }
    let models = json_global("MODEL_NEWS");
    for (prov, info) in at(&models, "newSince").as_object().into_iter().flatten() {
        let models: Vec<_> = list(at(info, "models")).iter().map(s).collect();
        out.push(json!({"cls":"modelo","key":format!("mn|{prov}|{}",models.join(",")),"icon":"sparkles","sev":"brand","title":tf(&format!("Modelo nuevo de {prov} detectado"),&format!("New {prov} model detected")),"body":format!("{} · CLI {} — {}",models.join(", "),if truth(at(info,"cli")){s(at(info,"cli"))}else{"?".into()},tf("pídeme agregarlo al registry","ask me to add it to the registry")),"ts":at(info,"at"),"provider":prov,"models":models,"actions":[action("detalle",tf("Ver modelos","View models"),"database")]}));
    }
    let news = json_global("NEWS_FEED");
    for n in list(at(&news, "recent")).into_iter().take(10) {
        let meta = at(&n, "meta");
        let mut extra = vec![];
        if truth(at(meta, "prize")) {
            extra.push(s(at(meta, "prize")));
        }
        if truth(at(meta, "participants")) {
            extra.push(format!(
                "{} {}",
                s(at(meta, "participants")),
                tf("inscritos", "entrants")
            ));
        }
        let url = s(at(&n, "url"));
        let repository = url.strip_prefix("https://github.com/").and_then(|p| {
            let parts: Vec<_> = p.split('/').collect();
            if parts.get(2) == Some(&"commit")
                && !parts.first().is_none_or(|p| p.is_empty())
                && !parts.get(1).is_none_or(|p| p.is_empty())
            {
                Some(format!(
                    "https://github.com/{}/{}",
                    parts.first().copied().unwrap_or_default(),
                    parts.get(1).copied().unwrap_or_default()
                ))
            } else {
                None
            }
        });
        let mut links = vec![json!({"label":tf("Enlace principal","Main link"),"url":url})];
        if let Some(repo) = &repository {
            links.push(json!({"label":tf("Repositorio","Repository"),"url":repo}));
        }
        let multi = links.len() > 1;
        let mut acts = vec![
            action("leer", tf("Abrir", "Open"), "chevron"),
            action("copiar", tf("Copiar", "Copy"), "copy"),
        ];
        if multi {
            acts.push(action(
                "detalle",
                format!("+{} {}", links.len(), tf("enlaces", "links")),
                "chevron",
            ));
        }
        let kind = s(at(&n, "kind"));
        let contest = matches!(kind.as_str(), "bounty" | "hackathon");
        out.push(json!({"cls":if matches!(kind.as_str(),"mcp"|"skill"|"bounty"|"hackathon"){kind.clone()}else{"noticia".into()},"key":format!("nw|{url}"),"icon":if kind=="mcp"{"plug"}else if contest{"flame"}else{"sparkles"},"sev":if contest{"brand"}else{"ok"},"title":at(&n,"title"),"body":format!("{}{}{}",s(at(&n,"source")),if repository.is_some(){" · commit"}else{""},if extra.is_empty(){String::new()}else{format!(" · {}",extra.join(" · "))}),"ts":at(&n,"at"),"url":url,"links":links,"meta":meta,"clickDetail":multi,"actions":acts}));
    }
    out
}
fn important() -> Vec<Value> {
    let pinned = pins();
    priority()
        .into_iter()
        .filter(|it| {
            let key = s(at(it, "key"));
            !dismissed(&key)
                && !snoozed(&key)
                && !pinned.iter().any(|p| at(p, "key") == at(it, "key"))
        })
        .collect()
}
fn find(key: &str) -> Option<Value> {
    priority()
        .into_iter()
        .chain(pins())
        .find(|it| s(at(it, "key")) == key)
}
fn class_label(k: &str) -> String {
    match k {
        "desborde" => tf("DESBORDE", "OVERFLOW"),
        "sugerencia" => tf("SUGERENCIA", "SUGGESTION"),
        "modelo" => tf("MODELO", "MODEL"),
        "skill" => "SKILL".into(),
        "mcp" => "MCP".into(),
        "noticia" => tf("NOTICIA", "NEWS"),
        "bounty" => "BOUNTY".into(),
        "hackathon" => tf("HACKATHON", "HACKATHON"),
        _ => k.into(),
    }
}
fn card(it: &Value, pinned: bool) -> String {
    let key = attr_esc(&s(at(it, "key")));
    let detail = truth(at(it, "clickDetail"));
    let sev = s(at(it, "sev"));
    let color = match sev.as_str() {
        "err" => "var(--err)",
        "warn" => "var(--warn)",
        "ok" => "var(--ok)",
        "brand" => "var(--brand)",
        _ => "var(--dim)",
    };
    let mut out = format!(
        "<div class=\"nf2 {sev} {}\" data-key=\"{key}\" {}>",
        if detail { "clickable" } else { "" },
        if detail {
            format!(
                "data-detail=\"1\" title=\"{}\"",
                attr_esc(&tf("Click: ver detalle y links", "Click: detail & links"))
            )
        } else {
            String::new()
        }
    );
    if !pinned {
        out += &format!(
            "<button type=\"button\" class=\"nf2-x\" data-act=\"dismiss\" data-id=\"{key}\" data-key=\"{key}\" title=\"{}\">{}</button>",
            attr_esc(&tf("Marcar leída y quitar", "Mark read & remove")),
            icon("close", 12.0)
        );
    }
    out += &format!(
        "<div class=\"nf2-h\"><span class=\"nf2-ico\" style=\"color:{color}\">{}</span><span class=\"nf2-cls\">{}</span><b class=\"nf2-t\" {}>{}</b><small>{}</small></div>",
        icon(&s(at(it, "icon")), 13.0),
        class_label(&s(at(it, "cls"))),
        if truth(at(it, "tip")) {
            format!("title=\"{}\"", attr_esc(&s(at(it, "tip"))))
        } else {
            String::new()
        },
        md_esc(&s(at(it, "title"))),
        if truth(at(it, "ts")) {
            ago_txt(num(at(it, "ts")), now() / 1000.0, en())
        } else {
            String::new()
        }
    );
    if truth(at(it, "body")) {
        out += &format!("<div class=\"nf2-b\">{}</div>", md_esc(&s(at(it, "body"))))
    }
    out += "<div class=\"nf2-a\">";
    for a in list(at(it, "actions")) {
        out += &format!(
            "<button type=\"button\" class=\"nf2-act\" data-act=\"{}\" data-id=\"{key}\" data-key=\"{key}\" data-session=\"{}\" data-pane=\"{}\">{} {}</button>",
            s(at(&a, "id")),
            attr_esc(&s(at(it, "session"))),
            attr_esc(&s(at(it, "pane"))),
            icon(&s(at(&a, "icon")), 11.0),
            md_esc(&s(at(&a, "label")))
        );
    }
    if pinned {
        out += &format!(
            "<button type=\"button\" class=\"nf2-act quiet\" data-act=\"unpin\" data-id=\"{key}\" data-key=\"{key}\">{} {}</button>",
            icon("close", 11.0),
            tf("Quitar", "Remove")
        );
    } else {
        out += &format!(
            "<button type=\"button\" class=\"nf2-act quiet\" data-act=\"pin\" data-id=\"{key}\" data-key=\"{key}\" title=\"{}\">{} {}</button><button type=\"button\" class=\"nf2-act quiet\" data-act=\"snooze\" data-id=\"{key}\" data-key=\"{key}\" title=\"{}\">{} 1h</button>",
            attr_esc(&tf("Guardar en tu lista", "Save to your list")),
            icon("snippet", 11.0),
            tf("Guardar", "Save"),
            attr_esc(&tf(
                "Recordarme en 1 hora (te aviso con popup)",
                "Remind me in 1 hour (popup)"
            )),
            icon("timer", 11.0)
        );
    }
    out += "</div></div>";
    out
}
fn badge() {
    if truthy(&get(&global("ComandosNotices"), "instance")) {
        return;
    }
    let b = id("notif-badge");
    if b.is_null() {
        return;
    }
    let count = important().len();
    txt(&b, count.to_string());
    classes(&b, "hidden", count == 0);
}
fn dismiss(key: &str) {
    let mut st = store("cc-nf-dismiss");
    if let Some(m) = st.as_object_mut() {
        m.insert(key.into(), json!(1));
    }
    save("cc-nf-dismiss", &st);
    paint();
    badge();
}
fn paint() {
    let panel = id("notif-panel");
    if has_class(&panel, "hidden") {
        return;
    }
    let prio = important();
    let pinned = pins();
    let cache = json_global("NF_CACHE");
    let queue = list(at(&cache, "queue"));
    let mut html_ = format!(
        "<div class=\"nf-h\"><span class=\"chip\">{} {}</span>{}<span class=\"nf-local\">{}</span></div>",
        icon("bell", 12.0),
        tf("Notificaciones", "Notifications"),
        if prio.is_empty() {
            String::new()
        } else {
            format!(
                "<button type=\"button\" class=\"nf2-clearall\" id=\"nf-clearall\" title=\"{}\">{} {} ({})</button>",
                attr_esc(&tf(
                    "Marca TODAS como leídas de un golpe",
                    "Mark ALL as read at once"
                )),
                icon("check", 11.0),
                tf("Limpiar todo", "Clear all"),
                prio.len()
            )
        },
        tf("100 % local", "100% local")
    );
    if !queue.is_empty() {
        html_ += &format!(
            "<div class=\"nf-sec\">{} · {}</div>",
            tf("Encoladas durante tu foco", "Queued during your focus"),
            queue.len()
        )
    }
    if prio.is_empty() {
        html_ += &format!(
            "<div class=\"nf-empty\">{} {}</div>",
            icon("check", 13.0),
            tf(
                "Nada urgente. Desbordes, sugerencias, modelos y skills aparecerán aquí.",
                "Nothing urgent. Overflows, suggestions, models and skills will show here."
            )
        );
    } else {
        html_ += &prio.iter().map(|it| card(it, false)).collect::<String>();
    }
    html_ += &format!(
        "<button type=\"button\" class=\"nf2-act quiet\" id=\"nf-to-strip\">{} {}</button>",
        icon("bell", 11.0),
        tf(
            "Turnos, permisos y errores: en la franja de avisos por proyecto",
            "Turns, permissions and errors: in the per-project notice strip"
        )
    );
    if !pinned.is_empty() {
        html_ += &format!(
            "<div class=\"nf-sec\">{} {} · {}</div>",
            icon("snippet", 11.0),
            tf("Guardadas", "Saved"),
            pinned.len()
        );
        html_ += &pinned.iter().map(|p| card(p, true)).collect::<String>();
    }
    html(&panel, html_);
    listen(
        &id("nf-to-strip"),
        "click",
        event(|e| {
            call(&e, "stopPropagation", &[])?;
            classes(&id("notif-panel"), "hidden", true);
            let n = get(&global("ComandosNotices"), "instance");
            if truthy(&n) {
                let _ = call(&get(&n, "controller"), "setCollapsed", &[false.into()]);
                let focus = query(&get(&n, "stripEl"), ".nt-toggle");
                let _ = call(
                    &focus,
                    "focus",
                    &[from_json(&json!({"preventScroll":true}))?],
                );
            }
            Ok(())
        }),
    );
    let snapshot = prio;
    listen(
        &id("nf-clearall"),
        "click",
        event(move |e| {
            call(&e, "stopPropagation", &[])?;
            let mut st = store("cc-nf-dismiss");
            if let Some(m) = st.as_object_mut() {
                for it in &snapshot {
                    m.insert(s(at(it, "key")), json!(1));
                }
            }
            save("cc-nf-dismiss", &st);
            paint();
            badge();
            toast(
                tf("Notificaciones limpiadas", "Notifications cleared"),
                false,
            );
            Ok(())
        }),
    );
    for card in all(&panel, ".nf2[data-detail]") {
        let card_ = card.clone();
        listen(
            &card,
            "click",
            event(move |e| {
                if !closest(
                    &get(&e, "target"),
                    ".nf2-act,.nf2-x,.nf2-detail,input,button,a",
                )
                .is_null()
                {
                    return Ok(());
                }
                detail(id("notif-panel"), &data(&card_, "key"))
            }),
        );
    }
    for b in all(&panel, ".nf2-x") {
        let b_ = b.clone();
        listen(
            &b,
            "click",
            event(move |e| {
                call(&e, "stopPropagation", &[])?;
                dismiss(&data(&b_, "key"));
                Ok(())
            }),
        );
    }
    for b in all(&panel, ".nf2-act, .nf2-web") {
        let button = b.clone();
        listen(
            &b,
            "click",
            event(move |e| {
                call(&e, "stopPropagation", &[])?;
                let act = data(&button, "act");
                let key = data(&button, "key");
                if local_action(&act, &key) {
                    return Ok(());
                }
                let button = button.clone();
                wasm_bindgen_futures::spawn_local(async move {
                    let _ = async_action(button, act, key).await;
                });
                Ok(())
            }),
        );
    }
    for b in all(&panel, ".nf2-open") {
        let button = b.clone();
        listen(
            &b,
            "click",
            event(move |e| {
                call(&e, "stopPropagation", &[])?;
                let row = closest(&button, ".nf2-turn");
                let session = data(&row, "session");
                let project = data(&row, "project");
                let list = js_rows(&get(&state(), "list"));
                let it = list
                    .iter()
                    .find(|it| get(it, "session").as_string().as_deref() == Some(&session))
                    .or_else(|| {
                        list.iter()
                            .find(|it| get(it, "project").as_string().as_deref() == Some(&project))
                    })
                    .cloned();
                if let Some(it) = it {
                    wasm_bindgen_futures::spawn_local(async move {
                        open_session(it).await;
                    });
                }
                Ok(())
            }),
        );
    }
}
fn local_action(act: &str, key: &str) -> bool {
    match act {
        "snooze" => {
            let mut st = store("cc-nf-snooze");
            if let Some(m) = st.as_object_mut() {
                m.insert(key.into(), json!(now() / 1000.0 + 3600.0));
            }
            save("cc-nf-snooze", &st);
            let title = priority()
                .iter()
                .find(|it| s(at(it, "key")) == key)
                .map(|it| s(at(it, "title")))
                .unwrap_or_default();
            let mut meta = store("cc-nf-snooze-meta");
            if let Some(m) = meta.as_object_mut() {
                m.insert(key.into(), json!({"title":title,"notified":false}));
            }
            save("cc-nf-snooze-meta", &meta);
            toast(
                tf("Te lo recuerdo en 1 hora", "I'll remind you in 1 hour"),
                false,
            );
            paint();
            badge();
        }
        "dismiss" => dismiss(key),
        "pin" => {
            if let Some(mut it) = priority().into_iter().find(|it| s(at(it, "key")) == key) {
                if let Some(m) = it.as_object_mut() {
                    m.insert("savedAt".into(), json!(now() / 1000.0));
                    m.insert("reminded".into(), json!(false));
                }
                let mut ps = pins();
                ps.insert(0, it);
                ps.truncate(20);
                save("cc-nf-pins", &json!(ps));
                toast(
                    tf(
                        "Guardada — te recuerdo en unas horas si no la ves",
                        "Saved — I'll remind you in a few hours",
                    ),
                    false,
                );
            }
            paint();
            badge();
        }
        "unpin" => {
            save(
                "cc-nf-pins",
                &json!(
                    pins()
                        .into_iter()
                        .filter(|p| s(at(p, "key")) != key)
                        .collect::<Vec<_>>()
                ),
            );
            paint();
            badge();
        }
        "detalle" => {
            let _ = detail(id("notif-panel"), key);
        }
        _ => return false,
    }
    true
}
async fn open_session(it: JsValue) {
    match wait(invoke(&global("openSession"), &[it, "claude".into()])).await {
        Ok(message) => {
            toast(utf16_string(&message), false);
            classes(&id("notif-panel"), "hidden", true)
        }
        Err(e) => toast(err(e), true),
    }
}
async fn async_action(button: JsValue, act: String, key: String) -> Result<(), JsValue> {
    match act.as_str() {
        "copiar" => {
            if let Some(it) = find(&key)
                && truth(at(&it, "url"))
            {
                let _ = copy_text(utf16_value(&s(at(&it, "url")))).await;
                toast(tf("Link copiado", "Link copied"), false);
            }
        }
        "leer" => {
            if let Some(it) = find(&key)
                && truth(at(&it, "url"))
            {
                match api("/open-url", Some(json!({"url":at(&it,"url")}))).await {
                    Ok(_) => toast(
                        tf("Abriendo en tu navegador", "Opening in your browser"),
                        false,
                    ),
                    Err(e) => toast(err(e), true),
                }
            }
        }
        "abrir" => {
            let session = data(&button, "session");
            if let Some(it) = js_rows(&get(&state(), "list"))
                .into_iter()
                .find(|it| get(it, "session").as_string().as_deref() == Some(&session))
            {
                open_session(it).await
            }
        }
        "aplicar" => {
            let session = data(&button, "session");
            let pane = data(&button, "pane");
            let list = js_rows(&get(&state(), "list"));
            let item = list
                .iter()
                .find(|it| {
                    get(it, "session").as_string().as_deref() == Some(&session)
                        && get(it, "pane").as_string().unwrap_or_default() == pane
                })
                .or_else(|| {
                    list.iter()
                        .find(|it| get(it, "session").as_string().as_deref() == Some(&session))
                });
            let Some(it) = item else {
                toast(
                    tf("La sugerencia ya no aplica", "Suggestion no longer applies"),
                    true,
                );
                return Ok(());
            };
            let sg = to_utf16_json(&get(it, "suggestion"));
            if !truth(&sg) {
                toast(
                    tf("La sugerencia ya no aplica", "Suggestion no longer applies"),
                    true,
                );
                return Ok(());
            }
            let pane = get(it, "pane").as_string().unwrap_or_default();
            let (path, body) = match at(&sg, "kind").as_str() {
                Some("key") => (
                    "/key",
                    json!({"session":session,"pane":pane,"key":if truth(at(&sg,"key")){s(at(&sg,"key"))}else{"Escape".into()}}),
                ),
                Some("send") => (
                    "/send",
                    json!({"session":session,"pane":pane,"text":at(&sg,"command")}),
                ),
                _ => ("/model/switch", {
                    let mut body = json!({"session":session,"pane":pane});
                    if let Some(m) = body.as_object_mut() {
                        for k in ["routeId", "model", "effort"] {
                            if let Some(v) = sg.get(k) {
                                m.insert(k.into(), v.clone());
                            }
                        }
                    }
                    body
                }),
            };
            match api(path, Some(body)).await {
                Ok(_) => {
                    toast(tf("Sugerencia aplicada", "Suggestion applied"), false);
                    dismiss(&key);
                }
                Err(e) => toast(err(e), true),
            }
        }
        _ => {}
    }
    Ok(())
}
fn detail(panel: JsValue, key: &str) -> Result<(), JsValue> {
    let escaped = call(&global("CSS"), "escape", &[key.into()]).unwrap_or(key.into());
    let holder = query(
        &panel,
        &format!(".nf2[data-key=\"{}\"]", utf16_string(&escaped)),
    );
    if holder.is_null() {
        return Ok(());
    }
    let old = query(&holder, ".nf2-detail");
    if !old.is_null() {
        call(&old, "remove", &[])?;
        return Ok(());
    }
    let Some(it) = find(key) else { return Ok(()) };
    let meta = at(&it, "meta");
    let model = s(at(&it, "cls")) == "modelo";
    let mut rows = vec![
        (tf("Tipo", "Type"), s(at(&it, "cls")).to_uppercase()),
        (tf("Fuente", "Source"), s(at(&it, "body"))),
    ];
    for (k, es, en_) in [
        ("prize", "Premio", "Prize"),
        ("deadline", "Deadline", "Deadline"),
        ("participants", "Inscritos", "Entrants"),
    ] {
        if truth(at(meta, k)) {
            let mut v = s(at(meta, k));
            if k == "deadline" {
                v = v.chars().take(10).collect();
            }
            rows.push((tf(es, en_), v));
        }
    }
    if truth(at(&it, "ts")) {
        rows.push((
            tf("Cuándo", "When"),
            ago_txt(num(at(&it, "ts")), now() / 1000.0, en()),
        ))
    }
    let links = if model {
        list(at(&it,"models")).iter().map(|v|json!({"label":if truth(at(&it,"provider")){s(at(&it,"provider"))}else{"modelo".into()},"url":v})).collect::<Vec<_>>()
    } else if it.get("links").is_some() {
        list(at(&it, "links"))
            .into_iter()
            .filter(|v| truth(at(v, "url")))
            .collect()
    } else if truth(at(&it, "url")) {
        vec![json!({"label":"link","url":at(&it,"url")})]
    } else {
        vec![]
    };
    let det = call(&doc(), "createElement", &["div".into()])?;
    set(&det, "className", &"nf2-detail".into())?;
    let mut body = format!(
        "<div class=\"nfd-title\">{}</div><div class=\"nfd-meta\">{}</div>",
        md_esc(&s(at(&it, "title"))),
        rows.iter()
            .map(|(k, v)| format!("<span><i>{}</i> {}</span>", md_esc(k), md_esc(v)))
            .collect::<String>()
    );
    if !links.is_empty() {
        body+=&format!("<div class=\"nfd-links\">{}</div>",links.iter().enumerate().map(|(i,l)|format!("<label class=\"nfd-link\"><input type=\"checkbox\" {} data-url=\"{}\"><span class=\"nfd-ln\">{}</span><span class=\"nfd-lu\">{}</span></label>",if i==0{"checked"}else{""},attr_esc(&s(at(l,"url"))),md_esc(&s(at(l,"label"))),md_esc(&s(at(l,"url"))))).collect::<String>());
    }
    if model {
        body += &format!(
            "<div class=\"nfd-hint\">{}</div>",
            tf(
                "Pídeme en cualquier sesión: “agrega ID al registry” y te lo integro.",
                "Ask me in any session: “add ID to the registry” and I’ll wire it in."
            )
        );
    }
    body += "<div class=\"nfd-acts\">";
    if !model {
        body += &format!(
            "<button type=\"button\" class=\"nf2-act\" data-openall>{} {}</button>",
            icon("chevron", 11.0),
            tf("Abrir en Brave", "Open in Brave")
        );
    }
    body += &format!(
        "<button type=\"button\" class=\"nf2-act\" data-copy>{} {}</button></div>",
        icon("copy", 11.0),
        if model {
            tf("Copiar ids", "Copy ids")
        } else {
            tf("Copiar links", "Copy links")
        }
    );
    html(&det, body);
    call(&holder, "appendChild", std::slice::from_ref(&det))?;
    let d = det.clone();
    listen(
        &query(&det, "[data-copy]"),
        "click",
        event(move |_| {
            let urls = all(&d, "input:checked")
                .iter()
                .map(|c| data(c, "url"))
                .collect::<Vec<_>>();
            if urls.is_empty() {
                toast(
                    tf("Selecciona al menos un link", "Select at least one link"),
                    true,
                );
            } else {
                wasm_bindgen_futures::spawn_local(async move {
                    let _ = copy_text(utf16_value(&urls.join("\n"))).await;
                    let n = urls.len();
                    toast(
                        tf(
                            &format!(
                                "{n} link{} copiado{}",
                                if n > 1 { "s" } else { "" },
                                if n > 1 { "s" } else { "" }
                            ),
                            &format!("{n} link(s) copied"),
                        ),
                        false,
                    );
                });
            }
            Ok(())
        }),
    );
    let d = det.clone();
    listen(
        &query(&det, "[data-openall]"),
        "click",
        event(move |_| {
            let urls = all(&d, "input:checked")
                .iter()
                .map(|c| data(c, "url"))
                .collect::<Vec<_>>();
            wasm_bindgen_futures::spawn_local(async move {
                for u in urls {
                    let _ = api("/open-url", Some(json!({"url":u}))).await;
                }
                toast(
                    tf("Abriendo en tu navegador", "Opening in your browser"),
                    false,
                );
            });
            Ok(())
        }),
    );
    Ok(())
}
fn read_set() -> js_sys::Set {
    let v = storage("cc-notif-read")
        .as_string()
        .and_then(|s| js_sys::JSON::parse(&s).ok())
        .unwrap_or(js_sys::Array::new().into());
    js_sys::Set::new(&v)
}
fn mark_read(it: &JsValue) {
    let set_ = read_set();
    set_.add(&utf16_value(&notif_key(&to_utf16_json(it))));
    let values = js_sys::Array::from(&set_);
    let values = values.slice(values.length().saturating_sub(1000), values.length());
    if let Ok(encoded) = js_sys::JSON::stringify(&values) {
        let _ = call(
            &global("localStorage"),
            "setItem",
            &["cc-notif-read".into(), encoded.into()],
        );
    }
    badge();
}
async fn render() -> Result<JsValue, JsValue> {
    if has_class(&id("notif-panel"), "hidden") {
        return Ok(JsValue::UNDEFINED);
    }
    for (path, key) in [
        ("/models/latest", "MODEL_NEWS"),
        ("/news/latest", "NEWS_FEED"),
    ] {
        if let Ok(v) = api(path, None).await {
            let _ = global_set(key, &v);
        }
    }
    if let Ok(v) = api("/prefs", None).await {
        hydrate(&v);
    }
    if let Ok(v) = api("/pomodoro", None).await {
        let _ = set(&global("NF_CACHE"), "queue", &get(&v, "queue"));
    }
    paint();
    Ok(JsValue::UNDEFINED)
}
fn locale(v: &Value) -> String {
    call(&js_sys::Number::from(num(v)).into(), "toLocaleString", &[])
        .map(|v| utf16_string(&v))
        .unwrap_or_else(|_| s(v))
}
async fn sovereignty() -> Result<JsValue, JsValue> {
    let box_ = id("sov-body");
    if box_.is_null() {
        return Ok(JsValue::UNDEFINED);
    }
    let d = match api("/sovereignty", None).await {
        Ok(d) => to_utf16_json(&d),
        Err(_) => {
            html(
                &box_,
                format!(
                    "<div class=\"desc\">{}</div>",
                    tf(
                        "No se pudo leer el inventario",
                        "Could not read the inventory"
                    )
                ),
            );
            return Ok(JsValue::UNDEFINED);
        }
    };
    let stores = list(at(&d, "stores"));
    let total = stores.iter().map(|x| num(at(x, "bytes"))).sum::<f64>();
    let mut out = format!(
        "<div class=\"sov-h\">{}</div><div class=\"sov-flow\"><div class=\"sov-node\"><b>{}</b><small>Claude Code · Codex · OpenCode · Gemini<br>{}</small></div><div class=\"sov-arrow\">→</div><div class=\"sov-node here\"><b>{}</b><small>~/.claude/hooks · sqlite · jsonl · json<br>{} {} {} {}</small></div><div class=\"sov-arrow\">→</div><div class=\"sov-node\"><b>{}</b><small>127.0.0.1:4777 · cc-app (WebKitGTK)<br>{}</small></div></div><div class=\"sov-h\">{}</div><div class=\"sov-out\">",
        tf("Flujo", "Flow"),
        tf("Agentes", "Agents"),
        tf("hooks locales → eventos", "local hooks → events"),
        tf("Esta máquina", "This machine"),
        fmt_bytes(total),
        tf("en", "in"),
        stores.len(),
        tf("almacenes", "stores"),
        tf("Tablero y app", "Dashboard & app"),
        tf("popups con tema", "themed popups"),
        tf("Canales de salida", "Outbound channels")
    );
    for c in list(at(&d, "outbound")) {
        let on = truth(at(&c, "on"));
        out += &format!(
            "<div class=\"sov-ch {}\"><span class=\"st\">{}</span><span>{}</span><small>{}</small></div>",
            if on { "on" } else { "off" },
            if on {
                tf("ACTIVO", "ON")
            } else {
                tf("APAGADO", "OFF")
            },
            md_esc(&s(at(&c, "label"))),
            md_esc(&s(at(&c, "how")))
        );
    }
    out += &format!(
        "</div><div class=\"sov-h\">{}</div><table class=\"sov-table\"><thead><tr><th>{}</th><th>{}</th><th style=\"text-align:right\">{}</th><th>{}</th></tr></thead><tbody>",
        tf("Almacenes locales", "Local stores"),
        tf("Qué", "What"),
        tf("Dónde", "Where"),
        tf("Tamaño / filas", "Size / rows"),
        tf("Privado", "Private")
    );
    for x in stores {
        let tables = if x.get("tables").is_some() {
            format!(
                "<div class=\"sov-tables\">{}</div>",
                list(at(&x, "tables"))
                    .iter()
                    .map(|t| format!("{} {}", md_esc(&s(at(t, "name"))), locale(at(t, "rows"))))
                    .collect::<Vec<_>>()
                    .join(" · ")
            )
        } else {
            String::new()
        };
        let mut size = if x.get("bytes").is_some() {
            fmt_bytes(num(at(&x, "bytes")))
        } else {
            String::new()
        };
        if x.get("count").is_some() {
            if x.get("bytes").is_some() {
                size += " · ";
            }
            size += &locale(at(&x, "count"));
        }
        let secret = s(at(&x, "kind")) == "secret";
        let private = secret && truth(at(&x, "private"));
        out += &format!(
            "<tr><td>{}{tables}</td><td class=\"path\">{}</td><td class=\"num\">{size}</td><td><span class=\"sov-priv {}\">{}</span></td></tr>",
            md_esc(&s(at(&x, "label"))),
            md_esc(&s(at(&x, "path"))),
            if private { "" } else { "no" },
            if private {
                "0600 ✓".into()
            } else if truth(at(&x, "mode")) {
                s(at(&x, "mode"))
            } else {
                "—".into()
            }
        );
    }
    for b in list(at(&d, "browser")) {
        let v = storage(&s(at(&b, "key")));
        out += &format!(
            "<tr><td>{}</td><td class=\"path\">localStorage · {}</td><td class=\"num\">{}</td><td><span class=\"sov-priv no\">{}</span></td></tr>",
            md_esc(&s(at(&b, "label"))),
            s(at(&b, "key")),
            if v.is_null() {
                "—".into()
            } else {
                fmt_bytes(utf16_string(&v).encode_utf16().count() as f64)
            },
            tf("navegador", "browser")
        );
    }
    out += "</tbody></table>";
    html(&box_, out);
    Ok(JsValue::UNDEFINED)
}
fn reminders() {
    let now_ = now() / 1000.0;
    let mut ps = pins();
    let mut changed = false;
    for p in &mut ps {
        if truth(at(p, "savedAt"))
            && !truth(at(p, "reminded"))
            && now_ - num(at(p, "savedAt")) > 10800.0
        {
            let body = s(at(p, "title"))
                .encode_utf16()
                .take(120)
                .collect::<Vec<_>>();
            fire_api(
                "/notify-popup",
                json!({"title":tf("Guardaste un link — ¡velo!","You saved a link — check it!"),"kind":"waiting","project":"guardados","body":String::from_utf16_lossy(&body)}),
            );
            if let Some(m) = p.as_object_mut() {
                m.insert("reminded".into(), json!(true));
            }
            changed = true;
        }
    }
    if changed {
        save("cc-nf-pins", &json!(ps));
    }
    let sn = store("cc-nf-snooze");
    let mut sm = store("cc-nf-snooze-meta");
    let mut changed = false;
    if let Some(entries) = sn.as_object() {
        for (k, until) in entries {
            if num(until) <= now_ && truth(at(&sm, k)) && !truth(at(at(&sm, k), "notified")) {
                fire_api(
                    "/notify-popup",
                    json!({"title":tf("Recordatorio (pediste 1h)","Reminder (you asked for 1h)"),"kind":"waiting","project":"recordatorio","body":s(at(at(&sm,k),"title")).chars().take(120).collect::<String>()}),
                );
                if let Some(m) = sm.get_mut(k).and_then(Value::as_object_mut) {
                    m.insert("notified".into(), json!(true));
                }
                changed = true;
            }
        }
    }
    if changed {
        save("cc-nf-snooze-meta", &sm);
    }
}
pub fn mount() -> Result<(), JsValue> {
    let scope: JsValue = js_sys::global().into();
    method(&scope, "sovRender", |_| Ok(promise(sovereignty())))?;
    method(&scope, "notifReadSet", |_| Ok(read_set().into()))?;
    method(&scope, "notifKey", |a| {
        Ok(utf16_value(&notif_key(&to_utf16_json(&a.get(0)))))
    })?;
    method(&scope, "notifMarkRead", |a| {
        mark_read(&a.get(0));
        Ok(JsValue::UNDEFINED)
    })?;
    method(&scope, "nfStore", |a| {
        from_utf16_json(&store(&utf16_string(&a.get(0))))
    })?;
    method(&scope, "nfSave", |a| {
        save(&utf16_string(&a.get(0)), &to_utf16_json(&a.get(1)));
        Ok(JsValue::UNDEFINED)
    })?;
    method(&scope, "nfHydrate", |a| {
        hydrate(&a.get(0));
        Ok(JsValue::UNDEFINED)
    })?;
    method(&scope, "nfSnoozed", |a| {
        Ok(snoozed(&utf16_string(&a.get(0))).into())
    })?;
    method(&scope, "nfDismissed", |a| {
        Ok(dismissed(&utf16_string(&a.get(0))).into())
    })?;
    method(&scope, "nfPins", |_| from_utf16_json(&json!(pins())))?;
    method(&scope, "nfPriorityItems", |_| {
        from_utf16_json(&json!(priority()))
    })?;
    method(&scope, "nfOpenLight", |a| {
        if truthy(&a.get(0)) {
            fire_api("/open-url", json!({"url":to_utf16_json(&a.get(0))}));
        }
        Ok(JsValue::UNDEFINED)
    })?;
    method(&scope, "nfToggleDetail", |a| {
        detail(a.get(0), &utf16_string(&a.get(1)))?;
        Ok(JsValue::UNDEFINED)
    })?;
    method(&scope, "notifRender", |_| Ok(promise(render())))?;
    method(&scope, "notifPaint", |_| {
        paint();
        Ok(JsValue::UNDEFINED)
    })?;
    method(&scope, "notifBadge", |_| {
        badge();
        Ok(JsValue::UNDEFINED)
    })?;
    Ok(())
}
pub fn attach() -> Result<(), JsValue> {
    for name in ["notif-panel", "pomo-panel"] {
        let el = id(name);
        if !el.is_null() && get(&el, "parentNode") != get(&doc(), "body") {
            call(&get(&doc(), "body"), "appendChild", &[el])?;
        }
    }
    listen(
        &id("btn-sov"),
        "click",
        event(|_| {
            classes(&id("sovereignty"), "open", true);
            wasm_bindgen_futures::spawn_local(async {
                let _ = sovereignty().await;
            });
            Ok(())
        }),
    );
    listen(
        &id("btn-notif"),
        "click",
        event(|e| {
            call(&e, "stopPropagation", &[])?;
            let inst = get(&global("ComandosNotices"), "instance");
            if truthy(&inst) && !truthy(&global("ONLY_PANEL")) {
                call(&inst, "toggleStrip", &[])?;
                return Ok(());
            }
            let m = id("notif-panel");
            let open = has_class(&m, "hidden");
            classes(&m, "hidden", !open);
            if open {
                let r = call(&id("btn-notif"), "getBoundingClientRect", &[])?;
                style(
                    &m,
                    "top",
                    &format!(
                        "{}px",
                        if number(&get(&r, "height")) != 0.0 {
                            number(&get(&r, "bottom")) + 8.0
                        } else {
                            8.0
                        }
                    ),
                );
                style(&m, "right", "12px");
                style(&m, "left", "auto");
                wasm_bindgen_futures::spawn_local(async {
                    let _ = render().await;
                });
            }
            Ok(())
        }),
    );
    listen(
        &doc(),
        "click",
        event(|e| {
            let m = id("notif-panel");
            if m.is_null() || has_class(&m, "hidden") {
                return Ok(());
            }
            let path = call(&e, "composedPath", &[]).unwrap_or(js_sys::Array::new().into());
            if js_rows(&path)
                .iter()
                .any(|v| v == &m || get(v, "id").as_string().as_deref() == Some("btn-notif"))
            {
                return Ok(());
            }
            let target = get(&e, "target");
            if truthy(&call(&m, "contains", std::slice::from_ref(&target)).unwrap_or(false.into()))
                || !closest(&target, "#btn-notif").is_null()
            {
                return Ok(());
            }
            classes(&m, "hidden", true);
            Ok(())
        }),
    );
    interval(120000.0, || {
        reminders();
        Ok(())
    });
    wasm_bindgen_futures::spawn_local(async {
        if let Ok(v) = api("/models/latest", None).await {
            let _ = global_set("MODEL_NEWS", &v);
            let ns = get(&v, "newSince");
            let entries = js_sys::Object::entries(&js_sys::Object::from(ns));
            let mut signature = serde_json::Map::new();
            let mut lines = Vec::new();
            for entry in entries.iter() {
                let pair = js_sys::Array::from(&entry);
                let provider = utf16_string(&pair.get(0));
                let models = get(&pair.get(1), "models");
                signature.insert(provider.clone(), to_utf16_json(&models));
                lines.push(format!(
                    "{provider}: {}",
                    js_rows(&models)
                        .iter()
                        .take(3)
                        .map(utf16_string)
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
            }
            let signature = Value::Object(signature).to_string();
            if !lines.is_empty()
                && storage("cc-model-news").as_string().as_deref() != Some(&signature)
            {
                let _ = call(
                    &global("localStorage"),
                    "setItem",
                    &["cc-model-news".into(), signature.into()],
                );
                let line = lines.join(" · ");
                toast(
                    tf(
                        &format!(
                            "Modelos nuevos detectados — {line}. Pídeme agregarlos al registry."
                        ),
                        &format!(
                            "New models detected — {line}. Ask me to add them to the registry."
                        ),
                    ),
                    false,
                );
            }
        }
    });
    Ok(())
}
