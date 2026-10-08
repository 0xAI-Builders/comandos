use super::*;
use serde_json::json;
struct Ui {
    client: JsValue,
    state: JsValue,
    save: RefCell<JsValue>,
    refresh: RefCell<JsValue>,
}
thread_local! {static UI:RefCell<Option<Rc<Ui>>>=const {RefCell::new(None)};}
fn ui() -> Result<Rc<Ui>, JsValue> {
    UI.with(|s| s.try_borrow().ok().and_then(|s| s.clone()))
        .ok_or_else(|| js_sys::Error::new("Pomodoro UI not mounted").into())
}
fn sv(u: &Ui, key: &str) -> JsValue {
    get(&u.state, key)
}
fn ss(u: &Ui, key: &str, value: JsValue) {
    let _ = set(&u.state, key, &value);
}
fn text(v: &JsValue) -> String {
    if v.is_null() || v.is_undefined() {
        String::new()
    } else {
        string(v)
    }
}
fn num(v: &JsValue, key: &str) -> f64 {
    number(&get(v, key))
}
fn asset(u: &Ui, name: &str) -> String {
    art::asset(name, &text(&sv(u, "style")), "")
}
fn label(v: &JsValue) -> String {
    if truthy(&get(v, "pending")) {
        return t("Confirmando…", "Confirming…");
    }
    if truthy(&get(v, "due")) {
        return t("Terminando…", "Finishing…");
    }
    let mode = text(&get(&get(v, "block"), "mode"));
    match text(&get(v, "status")).as_str() {
        "running" => {
            if mode == "break" {
                t("Descanso", "Break")
            } else {
                t("En foco", "Focusing")
            }
        }
        "paused" => t("En pausa", "Paused"),
        "completed" => {
            if mode == "break" {
                t("Descanso terminado", "Break over")
            } else {
                t("Bloque completado", "Block complete")
            }
        }
        "cancelled" => t("Bloque cancelado", "Block cancelled"),
        _ => t("Listo para empezar", "Ready"),
    }
}
fn minutes(u: &Ui, v: &JsValue) -> f64 {
    let preview = sv(u, "rulerPreview");
    if !preview.is_null() {
        return number(&preview);
    }
    if truthy(&get(v, "live")) {
        (num(v, "remainingMs") / MIN).ceil().max(1.)
    } else {
        number(&get(&sv(u, "draft"), &text(&sv(u, "mode"))))
    }
}
fn time(u: &Ui, v: &JsValue) -> String {
    fmt(if truthy(&get(v, "live")) {
        num(v, "remainingMs")
    } else {
        number(&get(&sv(u, "draft"), &text(&sv(u, "mode")))) * MIN
    })
}
fn selected() -> JsValue {
    let it = invoke(&global("pickSel"), &[get(&state(), "list")]).unwrap_or(JsValue::NULL);
    let project = get(&it, "project");
    let project = if project.is_string() && !text(&project).is_empty() {
        text(&project)
    } else {
        text(&get(&it, "session"))
    };
    from_json(&json!({"project":project,"sessionKey":text(&get(&it,"session")),"paneKey":text(&get(&it,"pane"))})).unwrap_or(JsValue::NULL)
}
fn apply(u: &Ui, settings: JsValue) {
    if !settings.is_object() {
        return;
    }
    ss(u, "settings", settings.clone());
    for (k, out) in [("focusMinutes", "focus"), ("shortBreakMinutes", "break")] {
        let n = num(&settings, k);
        if (1. ..=180.).contains(&n) {
            let _ = set(&sv(u, "draft"), out, &n.into());
        }
    }
    ss(
        u,
        "style",
        art::style(&text(&get(&settings, "style"))).into(),
    );
}
fn header(u: &Ui, v: &JsValue) -> Result<(), JsValue> {
    let b = id("btn-pomo");
    if !truthy(&b) {
        return Ok(());
    }
    let mini = if truthy(&get(v, "live")) {
        fmt(num(v, "remainingMs"))
    } else {
        String::new()
    };
    classes(&b, "running", get(v, "status") == "running");
    classes(&b, "paused", get(v, "status") == "paused");
    let style_ = sv(u, "style");
    if get(&get(&b, "dataset"), "pmStyle") != style_ {
        set(
            &b,
            "innerHTML",
            &format!(
                "{}<span class=\"pomo-mini\"></span>",
                art::asset("clock", &text(&style_), "pm-header-clock")
            )
            .into(),
        )?;
        set(&get(&b, "dataset"), "pmStyle", &style_)?;
    }
    let mini_el = query(&b, ".pomo-mini");
    if truthy(&mini_el) && text(&get(&mini_el, "textContent")) != mini {
        set(&mini_el, "textContent", &mini.clone().into())?;
    }
    attr(
        &b,
        "aria-label",
        &if truthy(&get(v, "live")) {
            format!("Pomodoro: {mini} {}", label(v))
        } else {
            "Pomodoro".into()
        },
    );
    Ok(())
}
fn note_completion(u: &Ui, v: &JsValue) -> Result<(), JsValue> {
    let b = get(v, "block");
    if get(&b, "status") != "completed" || sv(u, "seenCompletion") == get(&b, "blockId") {
        return Ok(());
    }
    let first = sv(u, "seenCompletion").is_null();
    ss(u, "seenCompletion", get(&b, "blockId"));
    let age = num(v, "serverNowMs") - get(&b, "endedAtMs").as_f64().unwrap_or(0.);
    if first && age >= 10. * MIN {
        return Ok(());
    }
    ss(u, "flipAt", get(v, "serverNowMs"));
    let prog = get(&get(v, "snapshot"), "progress");
    let up = get(&prog, "lastLevelUp");
    let level_up = truthy(&up) && get(&up, "blockId") == get(&b, "blockId");
    let focus = get(&b, "mode") == "focus";
    if age < 90000. {
        let event = format!("pomodoro:{}:completed", text(&get(&b, "blockId")));
        if focus {
            let snd = global("uiSounds");
            let device = call(
                &global("localStorage"),
                "getItem",
                &["comandos.deviceId".into()],
            )
            .unwrap_or(JsValue::NULL);
            if truthy(&device) && truthy(&call(&snd, "isReady", &[]).unwrap_or(JsValue::FALSE)) {
                let payload = object();
                set(&payload, "eventId", &utf16_value(&event))?;
                set(&payload, "deviceId", &device)?;
                let event = if level_up {
                    format!(
                        "pomodoro:level:{}:{}",
                        text(&get(&prog, "policyVersion")),
                        text(&get(&up, "level"))
                    )
                } else {
                    event
                };
                wasm_bindgen_futures::spawn_local(async move {
                    if let Ok(r) = request("POST", "/notices/sound", payload).await
                        && truthy(&get(&get(&r, "body"), "play"))
                        && let Ok(opts) = from_json(&json!({"eventId":event}))
                    {
                        let _ = call(
                            &snd,
                            "play",
                            &[
                                if level_up {
                                    "level-up"
                                } else {
                                    "focus-complete"
                                }
                                .into(),
                                opts,
                            ],
                        );
                    }
                });
            }
        } else {
            let opts = from_json(&json!({"eventId":event}))?;
            let _ = call(
                &global("uiSounds"),
                "play",
                &["break-complete".into(), opts],
            );
        }
    }
    let active = num(&b, "activeMs");
    let xp = (active / MIN).floor() * get(&prog, "xpPerMinute").as_f64().unwrap_or(0.);
    let banner = if focus {
        let mut s = t(
            &format!(
                "Bloque completado · {} min en {}",
                (active / MIN + 0.5).floor(),
                {
                    let project = text(&get(&b, "project"));
                    if project.is_empty() {
                        "sin proyecto".into()
                    } else {
                        project
                    }
                }
            ),
            &format!("Block complete · {} min", (active / MIN + 0.5).floor()),
        );
        if xp != 0. {
            s.push_str(&format!(" · +{xp} XP"));
        }
        if level_up {
            s.push_str(&t(
                &format!(" · ¡Nivel {}!", text(&get(&up, "level"))),
                &format!(" · Level {}!", text(&get(&up, "level"))),
            ));
        }
        s
    } else {
        t(
            "Descanso terminado · listo para continuar",
            "Break over · ready to continue",
        )
    };
    ss(u, "banner", utf16_value(&banner));
    ss(u, "bannerLevel", level_up.into());
    ss(u, "mode", if focus { "break" } else { "focus" }.into());
    Ok(())
}
fn mini(u: &Ui, v: &JsValue) -> String {
    let g = get(&get(v, "snapshot"), "progress");
    if !truthy(&g) {
        return String::new();
    }
    let goal = (num(&g, "todayMinutes") / num(&g, "dailyGoalMinutes").max(1.) * 100.).min(100.);
    format!(
        "<div class=\"pm-milestone\"><div class=\"pm-mini-level\">{}<div><b>{} {}</b><small>{} XP</small></div></div><div class=\"pm-mini-track\"><div class=\"pm-progress\" role=\"progressbar\" aria-label=\"{}\" aria-valuemin=\"0\" aria-valuemax=\"100\" aria-valuenow=\"{}\"><span style=\"width:{}%\"></span></div><p><span>{} {}</span><span>{} / {} min {}</span></p><div class=\"pm-progress goal\" aria-hidden=\"true\"><span style=\"width:{goal:.1}%\"></span></div></div></div>",
        asset(u, "level"),
        t("Nivel", "Level"),
        text(&get(&g, "level")),
        locale(get(&g, "xp")),
        t("Progreso al siguiente nivel", "Progress to next level"),
        (num(&g, "levelPct") + 0.5).floor(),
        num(&g, "levelPct"),
        text(&get(&g, "xpToNextLevel")),
        t("XP para subir", "XP to level up"),
        text(&get(&g, "todayMinutes")),
        text(&get(&g, "dailyGoalMinutes")),
        t("hoy", "today")
    )
}
fn locale(value: JsValue) -> String {
    // Reflect.get requires an object; JavaScript boxes numeric receivers.
    let boxed = invoke(&global("Object"), std::slice::from_ref(&value)).unwrap_or(value.clone());
    call(&boxed, "toLocaleString", &["es-MX".into()])
        .ok()
        .and_then(|v| v.as_string())
        .unwrap_or_else(|| text(&value))
}
fn sound_html(u: &Ui) -> String {
    let snd = global("uiSounds");
    if !get(&snd, "play").is_function() {
        return String::new();
    }
    let on = truthy(&call(&snd, "isEnabled", &[]).unwrap_or(JsValue::FALSE));
    let volume = (number(&call(&snd, "getVolume", &[]).unwrap_or(0.into())) * 100. + 0.5).floor();
    let snapshot = call(&u.client, "view", &[]).unwrap_or(JsValue::NULL);
    let my_device = call(
        &global("localStorage"),
        "getItem",
        &["comandos.deviceId".into()],
    )
    .unwrap_or(JsValue::NULL);
    let route = call(
        &global("ComandosPomodoro"),
        "soundWhere",
        &[
            get(&get(&snapshot, "snapshot"), "sound"),
            my_device,
            on.into(),
        ],
    )
    .unwrap_or(JsValue::NULL);
    let here = if truthy(&get(&route, "canEnableHere")) {
        format!(
            " · <button type=\"button\" data-pm-sound class=\"pm-link\">{}</button>",
            t("que suene aquí", "ring here")
        )
    } else {
        String::new()
    };
    let previews = [
        ("focus-start", "Inicio", "Start"),
        ("focus-pause", "Pausa", "Pause"),
        ("focus-complete", "Completado", "Complete"),
        ("break-complete", "Descanso", "Break"),
        ("level-up", "Subir de nivel", "Level up"),
    ]
    .iter()
    .map(|(cue, es, en)| {
        format!(
            "<button type=\"button\" data-pm-preview=\"{cue}\">{}</button>",
            t(es, en)
        )
    })
    .collect::<String>();
    format!(
        "<div class=\"pm-where\" role=\"status\">{} {}: <b>{}</b>{here}</div><div class=\"pm-audio\"><button type=\"button\" data-pm-sound aria-pressed=\"{on}\">{} {}</button><label>{} <input type=\"range\" data-pm-volume min=\"0\" max=\"100\" value=\"{volume}\" aria-label=\"{}\"></label><button type=\"button\" data-pm-preview=\"focus-complete\">{}</button></div><details class=\"pm-sound-options\"><summary>{}</summary>{previews}</details>",
        icon("bell", 13.),
        t("Al terminar sonará en", "When it ends it rings on"),
        escape(&text(&get(&route, "where"))),
        icon("bell", 13.),
        if on {
            t("Sonido activado", "Sound on")
        } else {
            t("Activar sonido", "Enable sound")
        },
        t("Volumen", "Volume"),
        t("Volumen de efectos", "Effects volume"),
        t("Escuchar final", "Hear the end"),
        t("Probar sonidos de videojuego", "Try the game sounds")
    )
}
fn style_html(u: &Ui) -> String {
    let style_ = text(&sv(u, "style"));
    let d = art::catalog();
    let set = d
        .get("STYLES")
        .and_then(|v| v.get(&style_))
        .unwrap_or(&Value::Null);
    let credits=set.get("sources").and_then(Value::as_array).into_iter().flatten().map(|source|{let a=d.get("ART_SOURCES").and_then(|v|v.get(source.as_str().unwrap_or_default())).unwrap_or(&Value::Null);let s=|key|a.get(key).and_then(Value::as_str).unwrap_or_default();format!("<a href=\"{}\" target=\"_blank\" rel=\"noopener\">{}</a> · <a href=\"{}\" target=\"_blank\" rel=\"noopener\">{}</a>",s("url"),escape(s("author")),s("licenseUrl"),escape(s("license")))}).collect::<Vec<_>>().join(" / ");
    let options = art::STYLE_ORDER
        .iter()
        .map(|key| {
            let title = d
                .get("STYLES")
                .and_then(|v| v.get(key))
                .and_then(|v| v.get("title"))
                .and_then(Value::as_str)
                .unwrap_or_default();
            format!(
                "<option value=\"{key}\" {}>{}</option>",
                if key == &style_ { "selected" } else { "" },
                escape(title)
            )
        })
        .collect::<String>();
    let samples = ["crystal", "first", "hundred", "streak", "level"]
        .iter()
        .map(|n| asset(u, n))
        .collect::<String>();
    format!(
        "<div class=\"pm-art-controls\"><label>{} <select data-pm-style aria-label=\"{}\">{options}</select></label><span class=\"pm-art-samples\">{samples}</span><small>{}: {credits}</small></div>",
        t("Estilo", "Style"),
        t(
            "Estilo de Pomodoro (toda la app)",
            "Pomodoro style (whole app)"
        ),
        t("Arte", "Art")
    )
}
fn panel_html(u: &Ui, v: &JsValue) -> String {
    let b = get(v, "block");
    let live = truthy(&get(v, "live"));
    let mode = if live {
        text(&get(&b, "mode"))
    } else {
        text(&sv(u, "mode"))
    };
    let target = if live { b.clone() } else { selected() };
    let project = text(&get(&target, "project"));
    let mins = minutes(u, v);
    let primary = match text(&get(v, "status")).as_str() {
        "running" => ("pause", t("Pausar", "Pause"), "pause"),
        "paused" => ("resume", t("Reanudar", "Resume"), "play"),
        _ => ("start", t("Iniciar", "Start"), "play"),
    };
    let disabled = if truthy(&get(v, "pending")) {
        "disabled"
    } else {
        ""
    };
    let more = if live {
        format!(
            "<button type=\"button\" id=\"pp-extend\" data-pm=\"extend\" {disabled}>+5 min</button><button type=\"button\" id=\"pp-skip\" data-pm=\"cancel\" aria-label=\"{}\" title=\"{}\" {disabled}>{} {}</button>",
            t("Cancelar bloque", "Cancel block"),
            t("Cancelar bloque", "Cancel block"),
            icon("close", 13.),
            t("Cancelar", "Cancel")
        )
    } else {
        String::new()
    };
    let presets = [15, 25, 50]
        .iter()
        .map(|n| {
            format!("<button type=\"button\" data-pm-preset=\"{n}\" {disabled}>{n} min</button>")
        })
        .collect::<String>();
    let banner = text(&sv(u, "banner"));
    let banner = if banner.is_empty() {
        String::new()
    } else {
        format!(
            "<div class=\"pm-banner\" role=\"status\">{}<span>{}</span></div>",
            if truthy(&sv(u, "bannerLevel")) {
                asset(u, "level")
            } else {
                String::new()
            },
            escape(&banner)
        )
    };
    let error = get(v, "error");
    let error = if truthy(&error) {
        format!(
            "<div class=\"pm-error\" role=\"alert\">{}{}</div>",
            escape(&text(&get(&error, "message"))),
            if truthy(&get(&error, "retryable")) {
                format!(
                    " <button type=\"button\" data-pm=\"retry\">{}</button> <button type=\"button\" data-pm=\"discard\">{}</button>",
                    t("Reintentar", "Retry"),
                    t("Descartar", "Discard")
                )
            } else {
                String::new()
            }
        )
    } else {
        String::new()
    };
    format!(
        r#"<div class="pm-head"><div class="pm-context"><div class="pm-kicker">Pomodoro</div><strong title="{title}">{project}</strong></div><div class="pm-modes" role="group" aria-label="{kind}"><button type="button" data-pm-mode="focus" class="{focus}" {lock}>{focus_label}</button><button type="button" data-pm-mode="break" class="{break_}" {lock}>{break_label}</button></div></div><div class="pm-row-clock"><div class="pm-ruler-clock">{clock}<div><div class="pm-time" data-pm-time>{time}</div><small data-pm-label>{label}</small></div></div><div class="pm-ruler-assembly"><input class="pm-ruler" data-pm-ruler type="range" min="1" max="90" step="1" value="{ruler}" aria-label="{ruler_label}" aria-valuetext="{mins} {minute_label}" style="--pm-ruler-fill:{fill:.1}%" {disabled}><div class="pm-ruler-labels" aria-hidden="true">{ruler_ticks}</div><div class="pm-ruler-caption"><span>{caption}</span></div></div><div class="pm-actions"><button type="button" class="primary" id="pp-go" data-pm="{primary}" {disabled}>{primary_icon} {primary_label}</button>{more}</div></div><div class="pm-presets">{presets}<label>Min <input type="number" data-pm-minutes min="1" max="180" value="{mins}" aria-label="{minute_title}" {disabled}></label></div>{banner}{mini}{error}<button type="button" class="pm-link" data-pm-analytics>{bars} {history}</button>{sound}{style}<div data-pm-extra></div>"#,
        title = escape(&project),
        project = escape(&if project.is_empty() {
            t("sin proyecto", "no project")
        } else {
            project
        }),
        kind = t("Tipo de bloque", "Block type"),
        focus = if mode == "focus" { "on" } else { "" },
        break_ = if mode == "break" { "on" } else { "" },
        lock = if live { "disabled" } else { "" },
        focus_label = t("Foco", "Focus"),
        break_label = t("Descanso", "Break"),
        clock = asset(u, "clock"),
        time = time(u, v),
        label = escape(&label(v)),
        ruler = mins.min(90.),
        ruler_label = if live {
            t("Minutos restantes", "Minutes remaining")
        } else {
            t("Minutos del bloque", "Block minutes")
        },
        minute_label = t("minutos", "minutes"),
        fill = (mins.min(90.) - 1.) / 89. * 100.,
        ruler_ticks = [1, 15, 30, 45, 60, 75, 90]
            .iter()
            .map(|n| format!("<span>{n}</span>"))
            .collect::<String>(),
        caption = if live {
            t(
                "Arrastra para cambiar el tiempo restante",
                "Drag to change the remaining time",
            )
        } else {
            t(
                "Arrastra para elegir minutos; no inicia el bloque",
                "Drag to choose minutes; it does not start",
            )
        },
        primary = primary.0,
        primary_icon = icon(primary.2, 13.),
        primary_label = primary.1,
        minute_title = t("Minutos", "Minutes"),
        mini = mini(u, v),
        bars = icon("bars", 12.),
        history = t("Ver mi historial y progreso", "See my history and progress"),
        sound = sound_html(u),
        style = style_html(u)
    )
}
fn render() -> Result<JsValue, JsValue> {
    let u = ui()?;
    let v = call(&u.client, "view", &[])?;
    let settings = get(&get(&v, "snapshot"), "settings");
    if truthy(&settings) && settings != sv(&u, "_appliedSettings") {
        ss(&u, "_appliedSettings", settings.clone());
        apply(&u, settings);
    }
    note_completion(&u, &v)?;
    header(&u, &v)?;
    let panel = id("pomo-panel");
    if !truthy(&panel)
        || truthy(&call(
            &get(&panel, "classList"),
            "contains",
            &["hidden".into()],
        )?)
    {
        return Ok(JsValue::UNDEFINED);
    }
    if truthy(&sv(&u, "dragging")) {
        return tick();
    }
    classes(&panel, "pm-v1", true);
    classes(
        &panel,
        "break",
        truthy(&get(&v, "live")) && get(&get(&v, "block"), "mode") == "break",
    );
    set(&panel, "innerHTML", &utf16_value(&panel_html(&u, &v)))?;
    wire(panel.clone())?;
    let _ = invoke(
        &global("ComandosPomodoroExtras"),
        &[panel, v, u.state.clone()],
    );
    Ok(JsValue::UNDEFINED)
}
fn tick() -> Result<JsValue, JsValue> {
    let u = ui()?;
    let v = call(&u.client, "view", &[])?;
    header(&u, &v)?;
    let panel = id("pomo-panel");
    if truthy(&panel)
        && !truthy(&call(
            &get(&panel, "classList"),
            "contains",
            &["hidden".into()],
        )?)
    {
        for (sel, text_) in [
            ("[data-pm-time]", time(&u, &v)),
            ("[data-pm-label]", label(&v)),
        ] {
            for el in all(&panel, sel) {
                set(&el, "textContent", &utf16_value(&text_))?;
            }
        }
    }
    Ok(JsValue::UNDEFINED)
}
fn paint_sand() -> Result<JsValue, JsValue> {
    let u = ui()?;
    let v = call(&u.client, "view", &[])?;
    let reduced = truthy(&get(
        &call(
            &js_sys::global(),
            "matchMedia",
            &["(prefers-reduced-motion: reduce)".into()],
        )
        .unwrap_or(JsValue::NULL),
        "matches",
    ));
    let x = if reduced {
        0.
    } else {
        hourglass(num(&v, "serverNowMs"), sv(&u, "flipAt").as_f64()) * 32.
    };
    for el in all(&doc(), ".pm-motion-progress>.pm-pixel") {
        set(
            &get(&el, "style"),
            "backgroundPositionX",
            &format!("-{x}px").into(),
        )?;
    }
    Ok(JsValue::UNDEFINED)
}
fn schedule_refresh() -> Result<JsValue, JsValue> {
    let u = ui()?;
    if let Ok(mut timer) = u.refresh.try_borrow_mut() {
        cancel(timer.clone());
        let v = call(&u.client, "view", &[])?;
        let mut delay = 15000_f64;
        if truthy(&get(&v, "live")) && get(&v, "status") == "running" {
            delay = delay.min((num(&v, "remainingMs") + 400.).max(400.));
        }
        if truthy(&get(&v, "due")) {
            delay = 1000.;
        }
        if truthy(&get(&doc(), "hidden")) {
            delay = delay.max(30000.);
        }
        *timer = later(
            function(|_| {
                Ok(promise(async {
                    let u = ui()?;
                    wait(call(&u.client, "refresh", &[])).await?;
                    schedule_refresh()
                }))
            }),
            delay,
        );
    }
    Ok(JsValue::UNDEFINED)
}
fn command(action: String, fields: JsValue) -> Result<JsValue, JsValue> {
    let u = ui()?;
    ss(&u, "banner", "".into());
    let p = call(&u.client, "send", &[action.clone().into(), fields])?;
    Ok(promise(async move {
        let res = wait(Ok(p)).await?;
        let cue = match action.as_str() {
            "start" => "focus-start",
            "resume" => "focus-resume",
            "pause" => "focus-pause",
            _ => "",
        };
        if truthy(&get(&res, "ok")) && !cue.is_empty() {
            let opts = from_json(
                &json!({"eventId":format!("pomodoro-cmd:{}",text(&get(&get(&res,"request"),"requestId")))}),
            )?;
            let _ = call(&global("uiSounds"), "play", &[cue.into(), opts]);
        }
        let error = get(&res, "error");
        if !truthy(&get(&res, "ok")) && truthy(&error) && !truthy(&get(&error, "retryable")) {
            toast(&text(&get(&error, "message")));
        }
        schedule_refresh()?;
        Ok(res)
    }))
}
fn start() -> Result<JsValue, JsValue> {
    let u = ui()?;
    let fields = selected();
    let mode = sv(&u, "mode");
    set(&fields, "mode", &mode)?;
    set(
        &fields,
        "targetMs",
        &(number(&get(&sv(&u, "draft"), &text(&mode))) * MIN).into(),
    )?;
    command("start".into(), fields)
}
fn save_draft() -> Result<(), JsValue> {
    let u = ui()?;
    let mut timer = u.save.try_borrow_mut().map_err(|_| borrowed())?;
    cancel(timer.clone());
    *timer = later(
        function(|_| {
            let u = ui()?;
            let settings = object();
            set(&settings, "focusMinutes", &get(&sv(&u, "draft"), "focus"))?;
            set(
                &settings,
                "shortBreakMinutes",
                &get(&sv(&u, "draft"), "break"),
            )?;
            let body = object();
            set(&body, "settings", &settings)?;
            wasm_bindgen_futures::spawn_local(async move {
                let _ = request("POST", "/pomodoro", body).await;
            });
            Ok(JsValue::UNDEFINED)
        }),
        500.,
    );
    Ok(())
}
fn set_minutes(value: JsValue, commit: bool) -> Result<JsValue, JsValue> {
    let n = (number(&value) + 0.5).floor();
    if !n.is_finite() {
        return Ok(JsValue::UNDEFINED);
    }
    let mins = n.clamp(1., 180.);
    let u = ui()?;
    let v = call(&u.client, "view", &[])?;
    if truthy(&get(&v, "live")) {
        if !commit {
            ss(&u, "rulerPreview", mins.into());
            return tick();
        }
        ss(&u, "rulerPreview", JsValue::NULL);
        let d = delta(&to_json(&get(&v, "block")), num(&v, "serverNowMs"), mins);
        if d != 0. {
            return command("extend".into(), from_json(&json!({"deltaMs":d}))?);
        }
        return render();
    }
    ss(&u, "rulerPreview", JsValue::NULL);
    set(&sv(&u, "draft"), &text(&sv(&u, "mode")), &mins.into())?;
    if commit {
        save_draft()?;
    }
    render()
}
fn wire(panel: JsValue) -> Result<(), JsValue> {
    for button in all(&panel, "[data-pm]") {
        let b = button.clone();
        listen(
            &button,
            "click",
            function(move |a| {
                let _ = call(&a.get(0), "stopPropagation", &[]);
                let act = text(&get(&get(&b, "dataset"), "pm"));
                match act.as_str() {
                    "start" => start(),
                    "pause" | "resume" | "cancel" => command(act, object()),
                    "extend" => command(act, from_json(&json!({"deltaMs":5.*MIN}))?),
                    "retry" | "discard" => call(&ui()?.client, &act, &[]),
                    _ => Ok(JsValue::UNDEFINED),
                }
            }),
        );
    }
    for b in all(&panel, "[data-pm-mode]") {
        let button = b.clone();
        listen(
            &b,
            "click",
            function(move |a| {
                let _ = call(&a.get(0), "stopPropagation", &[]);
                let u = ui()?;
                ss(&u, "mode", get(&get(&button, "dataset"), "pmMode"));
                ss(&u, "banner", "".into());
                render()
            }),
        );
    }
    for b in all(&panel, "[data-pm-preset]") {
        let button = b.clone();
        listen(
            &b,
            "click",
            function(move |a| {
                let _ = call(&a.get(0), "stopPropagation", &[]);
                set_minutes(get(&get(&button, "dataset"), "pmPreset"), true)
            }),
        );
    }
    let ruler = query(&panel, "[data-pm-ruler]");
    if truthy(&ruler) {
        let r = ruler.clone();
        let p = panel.clone();
        listen(
            &ruler,
            "input",
            function(move |_| {
                let u = ui()?;
                ss(&u, "dragging", true.into());
                let n = number(&get(&r, "value"));
                style(
                    &r,
                    "--pm-ruler-fill",
                    &format!("{:.1}%", (n - 1.) / 89. * 100.),
                );
                let v = call(&u.client, "view", &[])?;
                if truthy(&get(&v, "live")) {
                    return set_minutes(n.into(), false);
                }
                set(&sv(&u, "draft"), &text(&sv(&u, "mode")), &n.into())?;
                ss(&u, "rulerPreview", JsValue::NULL);
                tick()?;
                let number_ = query(&p, "[data-pm-minutes]");
                if truthy(&number_) {
                    set(&number_, "value", &get(&r, "value"))?;
                }
                Ok(JsValue::UNDEFINED)
            }),
        );
        let r = ruler.clone();
        listen(
            &ruler,
            "change",
            function(move |_| {
                ss(ui()?.as_ref(), "dragging", false.into());
                set_minutes(get(&r, "value"), true)
            }),
        );
        listen(
            &ruler,
            "blur",
            function(|_| {
                let u = ui()?;
                if truthy(&sv(&u, "dragging")) {
                    ss(&u, "dragging", false.into());
                    return render();
                }
                Ok(JsValue::UNDEFINED)
            }),
        );
    }
    let n = query(&panel, "[data-pm-minutes]");
    if truthy(&n) {
        let input = n.clone();
        listen(
            &n,
            "change",
            function(move |_| set_minutes(get(&input, "value"), true)),
        );
    }
    for b in all(&panel, "[data-pm-sound]") {
        listen(
            &b,
            "click",
            function(|a| {
                let e = a.get(0);
                let _ = call(&e, "stopPropagation", &[]);
                let snd = global("uiSounds");
                let next = !truthy(&call(&snd, "isEnabled", &[])?);
                call(&snd, "setEnabled", &[next.into()])?;
                if next {
                    call(&snd, "unlock", &[e])?;
                }
                render()
            }),
        );
    }
    let b = query(&panel, "[data-pm-analytics]");
    if truthy(&b) {
        let p = panel.clone();
        listen(
            &b,
            "click",
            function(move |a| {
                let _ = call(&a.get(0), "stopPropagation", &[]);
                classes(&p, "hidden", true);
                let _ = invoke(&global("openAnalyticsTab"), &["pomodoro".into()]);
                Ok(JsValue::UNDEFINED)
            }),
        );
    }
    let volume = query(&panel, "[data-pm-volume]");
    if truthy(&volume) {
        listen(
            &volume,
            "input",
            function(|a| {
                call(
                    &global("uiSounds"),
                    "setVolume",
                    &[(number(&get(&get(&a.get(0), "target"), "value")) / 100.).into()],
                )
            }),
        );
    }
    for b in all(&panel, "[data-pm-preview]") {
        let button = b.clone();
        listen(
            &b,
            "click",
            function(move |a| {
                let _ = call(&a.get(0), "stopPropagation", &[]);
                call(&global("uiSounds"), "unlock", &[a.get(0)])?;
                call(
                    &global("uiSounds"),
                    "preview",
                    &[get(&get(&button, "dataset"), "pmPreview")],
                )
            }),
        );
    }
    let style_ = query(&panel, "[data-pm-style]");
    if truthy(&style_) {
        listen(
            &style_,
            "change",
            function(|a| {
                let style_ = art::style(&text(&get(&get(&a.get(0), "target"), "value"))).to_owned();
                let u = ui()?;
                let old = sv(&u, "style");
                ss(&u, "style", style_.clone().into());
                render()?;
                Ok(promise(async move {
                    let payload = from_json(&json!({"settings":{"style":style_}}))?;
                    match request("POST", "/pomodoro", payload).await {
                        Ok(r) if number(&get(&r, "status")) == 200. => {
                            apply(&u, get(&get(&r, "body"), "settings"))
                        }
                        _ => {
                            ss(&u, "style", old);
                            toast(&t(
                                "No se pudo guardar el estilo",
                                "Could not save the style",
                            ));
                        }
                    }
                    render()
                }))
            }),
        );
    }
    Ok(())
}
fn progress(slot: JsValue, snapshot: JsValue) -> Result<JsValue, JsValue> {
    let u = ui()?;
    let g = get(&snapshot, "progress");
    if !truthy(&g) {
        set(&slot, "innerHTML", &"".into())?;
        return Ok(JsValue::UNDEFINED);
    }
    let goal = (num(&g, "todayMinutes") / num(&g, "dailyGoalMinutes").max(1.) * 100.).min(100.);
    let badges = js_sys::Array::from(&get(&g, "achievements"))
        .iter()
        .map(|a| {
            format!(
                "<span class=\"{}\" title=\"{}\">{}{}</span>",
                if truthy(&get(&a, "unlocked")) {
                    ""
                } else {
                    "locked"
                },
                if truthy(&get(&a, "unlocked")) {
                    t("Logro conseguido", "Achievement unlocked")
                } else {
                    t("Logro pendiente", "Pending")
                },
                asset(&u, &text(&get(&a, "asset"))),
                escape(&text(&get(&a, "title")))
            )
        })
        .collect::<String>();
    let html = format!(
        r#"<section class="pm-game"><div class="pm-game-level">{level}<div><div class="pm-kicker">{all}</div><h4>{level_label} {level_num} <small>· {xp} XP</small></h4></div></div><div class="pm-progress"><span style="width:{pct}%"></span></div><small>{next_xp} XP {to_level} {next_level} · {per_minute} XP {measured}</small><div class="pm-badges">{badges}</div><div class="pm-goal-head">{crystal}<b>{today}: {today_minutes} / {daily} min</b></div><div class="pm-progress goal"><span style="width:{goal:.1}%"></span></div><p class="pma-note">{streak} {note} {policy}.</p></section>"#,
        level = asset(&u, "level"),
        all = t(
            "Tu progreso · todos los proyectos",
            "Your progress · all projects"
        ),
        level_label = t("Nivel", "Level"),
        level_num = text(&get(&g, "level")),
        xp = locale(get(&g, "xp")),
        pct = text(&get(&g, "levelPct")),
        next_xp = text(&get(&g, "xpToNextLevel")),
        to_level = t("para el nivel", "to level"),
        next_level = num(&g, "level") + 1.,
        per_minute = text(&get(&g, "xpPerMinute")),
        measured = t("por minuto de foco medido", "per measured focus minute"),
        crystal = asset(&u, "crystal"),
        today = t("Meta de hoy", "Today"),
        today_minutes = text(&get(&g, "todayMinutes")),
        daily = text(&get(&g, "dailyGoalMinutes")),
        streak = text(&get(&g, "streakDays")),
        note = t(
            "días seguidos con un bloque completado. Tu XP y tus logros se conservan al descansar. Solo cuentan bloques terminados desde que se activó la política",
            "days in a row. XP and achievements are kept. Only blocks finished since the policy started count"
        ),
        policy = escape(&text(&get(&g, "policyVersion")))
    );
    set(&slot, "innerHTML", &utf16_value(&html))?;
    Ok(JsValue::UNDEFINED)
}
pub(super) fn mount_ui(api: JsValue) -> Result<(), JsValue> {
    let opts = object();
    method(&opts, "transport", |a| {
        let method_ = text(&a.get(0));
        let path = text(&a.get(1));
        let body = a.get(2);
        Ok(promise(async move { request(&method_, &path, body).await }))
    })?;
    method(&opts, "onChange", |_| render())?;
    let client = create_client(opts)?;
    let state = from_json(
        &json!({"mode":"focus","draft":{"focus":25,"break":5},"settings":{},"seenCompletion":null,"banner":"","rulerPreview":null,"style":"alchemy","flipAt":null}),
    )?;
    let u = Rc::new(Ui {
        client: client.clone(),
        state: state.clone(),
        save: RefCell::new(JsValue::NULL),
        refresh: RefCell::new(JsValue::NULL),
    });
    UI.with(|slot| {
        if let Ok(mut s) = slot.try_borrow_mut() {
            *s = Some(u);
        }
    });
    let surface = object();
    set(&surface, "client", &client)?;
    set(&surface, "state", &state)?;
    method(&surface, "render", |_| render())?;
    method(&surface, "command", |a| {
        command(
            text(&a.get(0)),
            if a.get(1).is_undefined() {
                object()
            } else {
                a.get(1)
            },
        )
    })?;
    method(&surface, "setMinutes", |a| {
        set_minutes(a.get(0), truthy(&a.get(1)))
    })?;
    method(&surface, "startBlock", |_| start())?;
    set(&api, "ui", &surface)?;
    global_set(
        "pomoRender",
        &function(|_| {
            render()?;
            call(&ui()?.client, "refresh", &[])
        }),
    )?;
    global_set(
        "ComandosPomodoroProgress",
        &function(|a| progress(a.get(0), a.get(1))),
    )
}
fn open_panel(event: JsValue) -> Result<JsValue, JsValue> {
    let panel = id("pomo-panel");
    if !truthy(&panel) {
        return Ok(JsValue::UNDEFINED);
    }
    if !truthy(&call(
        &get(&panel, "classList"),
        "contains",
        &["hidden".into()],
    )?) {
        classes(&panel, "hidden", true);
        return Ok(JsValue::UNDEFINED);
    }
    let target = get(&event, "currentTarget");
    let r = call(&target, "getBoundingClientRect", &[]).unwrap_or(JsValue::NULL);
    let height_ = num(&r, "height");
    let top = if height_ != 0. {
        num(&r, "bottom") + 8.
    } else {
        8.
    };
    style(&panel, "top", &format!("{top}px"));
    style(
        &panel,
        "max-height",
        &format!(
            "{}px",
            (number(&global("innerHeight")) - top - 8.).max(160.)
        ),
    );
    let split = truthy(&call(
        &get(&doc(), "body"),
        "matches",
        &[".app.split:not(.inapp)".into()],
    )?);
    let width = (number(&global("innerWidth")) - 16.).min(560.);
    if split {
        style(
            &panel,
            "left",
            &format!(
                "{}px",
                ((number(&global("innerWidth")) - width) / 2. + 0.5).floor()
            ),
        );
        style(&panel, "right", "auto");
    } else if height_ != 0. {
        style(
            &panel,
            "left",
            &format!(
                "{}px",
                num(&r, "left")
                    .min(number(&global("innerWidth")) - width - 8.)
                    .max(8.)
            ),
        );
        style(&panel, "right", "auto");
    } else {
        style(&panel, "left", "auto");
        style(&panel, "right", "12px");
    }
    classes(&panel, "hidden", false);
    render()?;
    call(&ui()?.client, "refresh", &[])
}
pub fn attach() -> Result<(), JsValue> {
    let b = id("btn-pomo");
    if truthy(&b) && !truthy(&get(&b, "_pmWired")) {
        set(&b, "_pmWired", &true.into())?;
        listen(&b, "click", function(|a| open_panel(a.get(0))));
    }
    listen(
        &doc(),
        "click",
        function(|a| {
            let target = get(&a.get(0), "target");
            if !get(&target, "closest").is_function()
                || truthy(&call(&target, "closest", &["#pomo-panel".into()])?)
                || truthy(&call(&target, "closest", &["#btn-pomo".into()])?)
            {
                return Ok(JsValue::UNDEFINED);
            }
            classes(&id("pomo-panel"), "hidden", true);
            Ok(JsValue::UNDEFINED)
        }),
    );
    listen(
        &doc(),
        "visibilitychange",
        function(|_| {
            classes(
                &get(&doc(), "documentElement"),
                "pm-page-hidden",
                truthy(&get(&doc(), "hidden")),
            );
            if !truthy(&get(&doc(), "hidden")) {
                call(&ui()?.client, "refresh", &[])?;
                schedule_refresh()?;
            }
            Ok(JsValue::UNDEFINED)
        }),
    );
    for (ms, f) in [
        (
            1000.,
            function(|_| {
                if !truthy(&get(&doc(), "hidden")) {
                    return tick();
                }
                Ok(JsValue::UNDEFINED)
            }),
        ),
        (
            110.,
            function(|_| {
                if !truthy(&get(&doc(), "hidden")) {
                    return paint_sand();
                }
                Ok(JsValue::UNDEFINED)
            }),
        ),
    ] {
        call(&js_sys::global(), "setInterval", &[f, ms.into()])?;
    }
    let p = call(&ui()?.client, "refresh", &[])?;
    wasm_bindgen_futures::spawn_local(async move {
        let _ = wait(Ok(p)).await;
        let _ = schedule_refresh();
    });
    Ok(())
}
