//! Complete theme/settings coordinator region. No legacy scripts are evaluated.
#[cfg(not(target_arch = "wasm32"))]
pub fn mount() -> Result<(), wasm_bindgen::JsValue> {
    Ok(())
}

#[cfg(all(test, target_arch = "wasm32"))]
#[allow(clippy::unwrap_used)]
mod wasm_tests {
    use super::*;
    use wasm_bindgen::prelude::*;
    #[wasm_bindgen(inline_js = r#"
let themeContracts;
export async function themePreferencesReference(root) {
  const {createRequire}=await import('node:module');
  themeContracts=createRequire(root+'/tests/theme_preferences.cjs')(root+'/crates/comandos-web/tests/theme_preferences_contract.cjs');
  return JSON.stringify(await themeContracts.reference(root));
}
export function themePreferencesFixture(){themeContracts.install();}
export async function themePreferencesActual(){return JSON.stringify(await themeContracts.native());}
"#)]
    extern "C" {
        #[wasm_bindgen(catch)]
        async fn themePreferencesReference(root: &str) -> Result<JsValue, JsValue>;
        fn themePreferencesFixture();
        #[wasm_bindgen(catch)]
        async fn themePreferencesActual() -> Result<JsValue, JsValue>;
    }
    #[wasm_bindgen_test::wasm_bindgen_test]
    async fn complete_theme_preferences_match_original_coordinator() {
        let expected = themePreferencesReference(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
            .await
            .unwrap();
        themePreferencesFixture();
        mount().unwrap();
        attach().unwrap();
        let actual = themePreferencesActual().await.unwrap();
        assert_eq!(actual.as_string(), expected.as_string());
    }
}
#[cfg(not(target_arch = "wasm32"))]
pub fn attach() -> Result<(), wasm_bindgen::JsValue> {
    Ok(())
}
#[cfg(target_arch = "wasm32")]
pub use web::{attach, mount};
#[cfg(target_arch = "wasm32")]
mod web {
    use crate::components::web_support::*;
    use comandos_web_dom::port::*;
    use comandos_web_view::theme_preferences as model;
    use js_sys::{Array, Reflect};
    use wasm_bindgen::{JsCast, JsValue, prelude::wasm_bindgen};
    // Temporary access to bindings owned by the helper/terminal coordinator.
    // These accessors also work against authoritative globals after its port.
    #[wasm_bindgen(
        inline_js = "export function themeTerms(){return typeof openTerms==='undefined'?undefined:openTerms;} export function themeFavoriteVersion(){return typeof favoriteVersion==='undefined'?undefined:favoriteVersion;} export function themeFavoriteReadAt(v){if(typeof favoriteReadAt!=='undefined')favoriteReadAt=v;else globalThis.favoriteReadAt=v;} export function themeRowsHold(v){if(typeof tabRowsHold!=='undefined')tabRowsHold=v;else globalThis.tabRowsHold=v;}"
    )]
    extern "C" {
        fn themeTerms() -> JsValue;
        fn themeFavoriteVersion() -> JsValue;
        fn themeFavoriteReadAt(v: &JsValue);
        fn themeRowsHold(v: &JsValue);
    }
    fn publish(
        name: &str,
        f: impl Fn(Array) -> Result<JsValue, JsValue> + 'static,
    ) -> Result<(), JsValue> {
        set(&js_sys::global(), name, &function(f))
    }
    fn void(f: impl Fn(Array) -> Result<(), JsValue> + 'static) -> JsValue {
        function(move |a| {
            f(a)?;
            Ok(JsValue::UNDEFINED)
        })
    }
    fn value(el: &JsValue) -> JsValue {
        get(el, "value")
    }
    fn text(el: &JsValue, v: &JsValue) -> Result<(), JsValue> {
        set(el, "textContent", v)
    }
    fn html(el: &JsValue, v: String) -> Result<(), JsValue> {
        set(el, "innerHTML", &v.into())
    }
    fn tr(es: String, en: String) -> String {
        invoke(&global("tf"), &[es.clone().into(), en.into()])
            .map(|v| string(&v))
            .unwrap_or(es)
    }
    fn invoke_void(name: &str, args: &[JsValue]) -> Result<(), JsValue> {
        invoke(&global(name), args).map(|_| ())
    }
    fn escaped(v: &JsValue, attribute: bool) -> Result<String, JsValue> {
        invoke(
            &global(if attribute { "attrEsc" } else { "mdEsc" }),
            std::slice::from_ref(v),
        )
        .map(|v| utf16_string(&v))
    }
    fn current_theme() -> String {
        string(&global("curTheme"))
    }
    fn current_buttons() -> String {
        string(&global("curButtonStyle"))
    }
    fn message(kind: &str, key: &str, v: JsValue) -> Result<JsValue, JsValue> {
        let out = object();
        set(&out, "source", &"comandos".into())?;
        set(&out, "type", &kind.into())?;
        set(&out, key, &v)?;
        Ok(out)
    }
    fn frames(mut f: impl FnMut(JsValue) -> Result<(), JsValue>) -> Result<(), JsValue> {
        let terms = themeTerms();
        let values = call(&terms, "values", &[])?;
        for item in Array::from(&values).iter() {
            let frame = get(&item, "frame");
            if truthy(&frame) && get(&get(&frame, "dataset"), "compat") != JsValue::from_str("1") {
                f(frame)?;
            }
        }
        Ok(())
    }
    fn notify_app(key: &str, v: JsValue, tell: bool) -> Result<(), JsValue> {
        if tell && truthy(&invoke(&global("inApp"), &[])?) {
            let msg = object();
            set(&msg, key, &v)?;
            let handler = get(
                &get(&get(&global("window"), "webkit"), "messageHandlers"),
                "centro",
            );
            call(
                &handler,
                "postMessage",
                &[js_sys::JSON::stringify(&msg)?.into()],
            )?;
        }
        Ok(())
    }
    fn broadcast(name: JsValue) -> Result<(), JsValue> {
        frames(|frame| {
            invoke_void("styleTermFrame", std::slice::from_ref(&frame))?;
            let window = get(&frame, "contentWindow");
            if !window.is_null() && !window.is_undefined() {
                call(
                    &window,
                    "postMessage",
                    &[
                        message("theme", "theme", name.clone())?,
                        get(&global("location"), "origin"),
                    ],
                )?;
            }
            Ok(())
        })
    }
    fn apply_theme(name: JsValue, tell: bool) -> Result<(), JsValue> {
        let input = name.as_string().unwrap_or_default();
        let name = model::theme(&input);
        set(&js_sys::global(), "curTheme", &name.into())?;
        let meta = model::metadata();
        let m = meta.get(name).unwrap_or(&serde_json::Value::Null);
        let docel = get(&doc(), "documentElement");
        let data = get(&docel, "dataset");
        if name == "noche" {
            Reflect::delete_property(&data.into(), &"theme".into())?;
        } else {
            set(&data, "theme", &name.into())?;
        }
        set(
            &get(&docel, "dataset"),
            "treatment",
            &m["treatment"].as_str().unwrap_or_default().into(),
        )?;
        set(
            &get(&docel, "style"),
            "colorScheme",
            &m["scheme"].as_str().unwrap_or_default().into(),
        )?;
        let color = query(&doc(), "meta[name=\"theme-color\"]");
        if truthy(&color) {
            attr(
                &color,
                "content",
                m.get("sw")
                    .and_then(|sw| sw.get(0))
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_default(),
            );
        }
        let btn = query(&doc(), "#btn-theme");
        html(&btn, icon(m["icon"].as_str().unwrap_or_default(), 15.0))?;
        let label = m["label"].as_str().unwrap_or_default();
        set(
            &btn,
            "title",
            &tr(
                format!("Apariencia: {label}"),
                format!("Appearance: {label}"),
            )
            .into(),
        )?;
        render_themes()?;
        broadcast(name.into())?;
        notify_app("theme", name.into(), tell)
    }
    fn apply_buttons(style: JsValue, tell: bool) -> Result<(), JsValue> {
        let input = style.as_string().unwrap_or_default();
        let name = model::button_style(&input);
        set(&js_sys::global(), "curButtonStyle", &name.into())?;
        set(
            &get(&get(&doc(), "documentElement"), "dataset"),
            "btnStyle",
            &name.into(),
        )?;
        render_buttons()?;
        frames(|frame| {
            let window = get(&frame, "contentWindow");
            if !window.is_null() && !window.is_undefined() {
                call(
                    &window,
                    "postMessage",
                    &[
                        message("button-style", "style", name.into())?,
                        get(&global("location"), "origin"),
                    ],
                )?;
            }
            Ok(())
        })?;
        notify_app("buttonStyle", name.into(), tell)
    }
    fn error_toast(e: JsValue) {
        let _ = invoke_void("toast", &[get(&e, "message"), true.into()]);
    }
    fn select_theme(name: JsValue) -> JsValue {
        let previous = current_theme();
        let applied = apply_theme(name.clone(), true);
        let fetched = applied.and_then(|()| {
            let patch = object();
            set(&patch, "theme", &name)?;
            invoke(&global("api"), &["/prefs-set".into(), patch])
        });
        promise(async move {
            let result = async {
                wait(fetched).await?;
                let meta = model::metadata();
                let name = string(&name);
                let label = meta.get(&name).unwrap_or(&serde_json::Value::Null)["label"]
                    .as_str()
                    .ok_or_else(|| js_sys::TypeError::new("Cannot read properties of undefined"))?;
                invoke_void(
                    "toast",
                    &[tr(format!("Tema: {label}"), format!("Theme: {label}")).into()],
                )
            }
            .await;
            if let Err(e) = result {
                apply_theme(previous.into(), true)?;
                error_toast(e);
            }
            Ok(JsValue::UNDEFINED)
        })
    }
    fn select_buttons(style: JsValue) -> JsValue {
        let previous = current_buttons();
        let fetched = apply_buttons(style.clone(), true).and_then(|()| {
            let patch = object();
            set(&patch, "button_style", &style)?;
            invoke(&global("api"), &["/prefs-set".into(), patch])
        });
        promise(async move {
            if let Err(e) = wait(fetched).await {
                apply_buttons(previous.into(), true)?;
                error_toast(e);
            }
            Ok(JsValue::UNDEFINED)
        })
    }
    fn render_buttons() -> Result<(), JsValue> {
        let g = query(&doc(), "#button-style-gallery");
        if !truthy(&g) {
            return Ok(());
        }
        html(
            &g,
            model::button_gallery(&current_buttons(), |n, s| icon(n, s.into())),
        )?;
        for b in all(&g, "[data-btn-style-id]") {
            let item = b.clone();
            listen(
                &b,
                "click",
                void(move |_| {
                    let _ = select_buttons(get(&get(&item, "dataset"), "btnStyleId"));
                    Ok(())
                }),
            );
        }
        Ok(())
    }
    fn render_themes() -> Result<(), JsValue> {
        let g = query(&doc(), "#theme-gallery");
        if !truthy(&g) {
            return Ok(());
        }
        html(
            &g,
            model::theme_gallery(&current_theme(), |n, s| icon(n, s.into())),
        )?;
        for btn in all(&g, "[data-theme-id]") {
            let item = btn.clone();
            listen(
                &btn,
                "click",
                void(move |_| {
                    let _ = select_theme(get(&get(&item, "dataset"), "themeId"));
                    Ok(())
                }),
            );
            let item = btn.clone();
            let gallery = g.clone();
            listen(
                &btn,
                "keydown",
                void(move |a| {
                    let e = a.get(0);
                    let key = string(&get(&e, "key"));
                    if !["ArrowLeft", "ArrowRight", "ArrowUp", "ArrowDown"].contains(&key.as_str())
                    {
                        return Ok(());
                    }
                    call(&e, "preventDefault", &[])?;
                    let delta = if ["ArrowLeft", "ArrowUp"].contains(&key.as_str()) {
                        -1
                    } else {
                        1
                    };
                    let index = number(&get(&get(&item, "dataset"), "index")) as i32;
                    let next = model::THEMES
                        .get(
                            (index + delta + model::THEMES.len() as i32) as usize
                                % model::THEMES.len(),
                        )
                        .copied()
                        .unwrap_or("noche");
                    let el = query(&gallery, &format!("[data-theme-id=\"{next}\"]"));
                    if truthy(&el) {
                        call(&el, "focus", &[])?;
                    }
                    Ok(())
                }),
            );
        }
        Ok(())
    }
    fn set_pref(patch: JsValue, ok: JsValue) -> JsValue {
        let fetched = invoke(&global("api"), &["/prefs-set".into(), patch]);
        promise(async move {
            match wait(fetched).await {
                Ok(_) => {
                    if truthy(&ok) {
                        invoke_void("toast", &[ok])?;
                    }
                }
                Err(e) => error_toast(e),
            }
            Ok(JsValue::UNDEFINED)
        })
    }
    fn apply_tabs(mode: JsValue) -> Result<(), JsValue> {
        let rows = mode == JsValue::from_str("rows");
        themeRowsHold(&from_json(&serde_json::json!({"n":-1,"h":0}))?);
        let bar = id("tabbar");
        if truthy(&bar) {
            set(&get(&bar, "style"), "minHeight", &"".into())?;
        }
        classes(&get(&doc(), "body"), "tabs-rows", rows);
        let b = query(&doc(), "#tab-rows");
        if truthy(&b) {
            attr(&b, "aria-pressed", if rows { "true" } else { "false" });
            let title = if rows {
                tr("Una fila".into(), "One row".into())
            } else {
                tr("Varias filas".into(), "Several rows".into())
            };
            set(&b, "title", &title.clone().into())?;
            attr(&b, "aria-label", &title);
        }
        let _ = (|| {
            invoke_void("updateTabNavigation", &[])?;
            let ctor = global("Event").dyn_into::<js_sys::Function>()?;
            let args = Array::new();
            args.push(&"resize".into());
            call(
                &global("window"),
                "dispatchEvent",
                &[Reflect::construct(&ctor, &args)?],
            )?;
            Ok::<_, JsValue>(())
        })();
        Ok(())
    }
    fn or(v: JsValue, fallback: JsValue) -> JsValue {
        if truthy(&v) { v } else { fallback }
    }
    fn nullish(v: JsValue, fallback: JsValue) -> JsValue {
        if v.is_null() || v.is_undefined() {
            fallback
        } else {
            v
        }
    }
    fn hydrate(p: JsValue) -> Result<(), JsValue> {
        let ff = query(&doc(), "#pf-font-family");
        if truthy(&ff) {
            let current = or(get(&p, "font_family"), "Ubuntu Sans Mono".into());
            let fonts = get(&p, "fonts");
            if Array::is_array(&fonts) && number(&get(&fonts, "length")) > 0.0 {
                let rows = Array::from(&fonts);
                if !rows.iter().any(|f| get(&f, "family") == current) {
                    let missing = object();
                    set(&missing, "family", &current)?;
                    set(
                        &missing,
                        "label",
                        &format!(
                            "{} · {}",
                            string(&current),
                            tr("no instalada".into(), "not installed".into())
                        )
                        .into(),
                    )?;
                    set(&missing, "a11y", &false.into())?;
                    rows.unshift(&missing);
                }
                let mut out = String::new();
                for f in rows.iter() {
                    let family = get(&f, "family");
                    let esc = escaped(&family, true)?;
                    let label = escaped(&or(get(&f, "label"), family), false)?;
                    let accessible = if truthy(&get(&f, "a11y")) {
                        format!(" · {}", tr("accesible".into(), "accessible".into()))
                    } else {
                        String::new()
                    };
                    out.push_str(&format!("<option value=\"{esc}\" style=\"font-family:'{esc}',monospace\">{label}{accessible}</option>"));
                }
                set(&ff, "innerHTML", &utf16_value(&out))?;
            }
            set(&ff, "value", &current)?;
        }
        for (field, id, label, fallback, is_nullish) in [
            ("font_size", "pf-font-size", "pf-font-size-v", 13.0, false),
            ("terminal_padding", "pf-padding", "pf-padding-v", 8.0, true),
            (
                "terminal_opacity",
                "pf-opacity",
                "pf-opacity-v",
                100.0,
                true,
            ),
        ] {
            let el = query(&doc(), &format!("#{id}"));
            if truthy(&el) {
                let v = get(&p, field);
                set(
                    &el,
                    "value",
                    &if is_nullish {
                        nullish(v, fallback.into())
                    } else {
                        or(v, fallback.into())
                    },
                )?;
                let label = query(&doc(), &format!("#{label}"));
                if truthy(&label) {
                    text(&label, &value(&el))?;
                }
            }
        }
        for b in all(&doc(), ".cur-shape") {
            classes(
                &b,
                "on",
                or(get(&p, "cursor_shape"), "block".into()) == get(&get(&b, "dataset"), "shape"),
            );
        }
        for (sel, key) in [
            ("#sw-cursor-blink", "cursor_blink"),
            ("#sw-ligatures", "ligatures"),
        ] {
            let el = query(&doc(), sel);
            if truthy(&el) {
                classes(&el, "on", get(&p, key) != JsValue::FALSE);
            }
        }
        for b in all(&doc(), ".npos") {
            classes(
                &b,
                "on",
                or(get(&p, "notif_pos"), "free".into()) == get(&get(&b, "dataset"), "npos"),
            );
            if !truthy(&get(&b, "_wired")) {
                set(&b, "_wired", &true.into())?;
                let el = b.clone();
                listen(
                    &b,
                    "click",
                    function(move |_| {
                        let pos = get(&get(&el, "dataset"), "npos");
                        invoke_void(
                            "ulog",
                            &[
                                "click".into(),
                                format!("notif-pos:{}", string(&pos)).into(),
                                "ajustes".into(),
                            ],
                        )?;
                        let patch = object();
                        set(&patch, "notif_pos", &pos)?;
                        let pending = set_pref(
                            patch,
                            tr(
                                "Posición de avisos guardada".into(),
                                "Notification position saved".into(),
                            )
                            .into(),
                        );
                        let el = el.clone();
                        Ok(promise(async move {
                            wait(Ok(pending)).await?;
                            for b in all(&doc(), ".npos") {
                                classes(&b, "on", b == el);
                            }
                            Ok(JsValue::UNDEFINED)
                        }))
                    }),
                );
            }
        }
        Ok(())
    }
    fn load_prefs() -> JsValue {
        let version = themeFavoriteVersion();
        let fetched = invoke(&global("api"), &["/prefs".into()]);
        promise(async move {
            let _ = async {
                let p = wait(fetched).await?;
                themeFavoriteReadAt(&js_sys::Date::now().into());
                if version == themeFavoriteVersion() {
                    invoke_void("applyFavorites", &[get(&p, "favorites")])?;
                }
                apply_theme(or(get(&p, "theme"), "noche".into()), true)?;
                apply_buttons(or(get(&p, "button_style"), "sutil".into()), true)?;
                apply_tabs(or(get(&p, "tabs_layout"), "row".into()))?;
                hydrate(p)
            }
            .await;
            Ok(JsValue::UNDEFINED)
        })
    }
    fn activate(scope: JsValue, name: JsValue) -> Result<(), JsValue> {
        for b in all(&scope, ".mtab") {
            classes(&b, "active", get(&get(&b, "dataset"), "mtab") == name);
        }
        for b in all(&scope, ".mtab-pane") {
            classes(&b, "active", get(&get(&b, "dataset"), "mpane") == name);
        }
        Ok(())
    }
    fn wire_tabs(root: JsValue) -> Result<(), JsValue> {
        for b in all(&root, ".mtab") {
            if truthy(&get(&b, "_wired")) {
                continue;
            }
            set(&b, "_wired", &true.into())?;
            let el = b.clone();
            listen(
                &b,
                "click",
                void(move |a| {
                    call(&a.get(0), "stopPropagation", &[])?;
                    let scope = or(call(&el, "closest", &[".modal-panel".into()])?, doc());
                    activate(scope, get(&get(&el, "dataset"), "mtab"))
                }),
            );
        }
        Ok(())
    }
    fn range(input: JsValue) -> Result<(), JsValue> {
        let min = number(&get(&input, "min"));
        let min = if min.is_nan() || min == 0.0 { 0.0 } else { min };
        let max = number(&get(&input, "max"));
        let max = if max.is_nan() || max == 0.0 {
            100.0
        } else {
            max
        };
        let pct = (number(&value(&input)) - min) / (max - min) * 100.0;
        style(&input, "--fill", &format!("{}%", string(&pct.into())));
        Ok(())
    }
    fn refresh_ranges() -> Result<(), JsValue> {
        for r in all(&doc(), "input[type=range].solid") {
            range(r)?;
        }
        Ok(())
    }
    fn wire_ranges() -> Result<(), JsValue> {
        for r in all(&doc(), "input[type=range].solid") {
            if truthy(&get(&r, "_wired")) {
                continue;
            }
            set(&r, "_wired", &true.into())?;
            range(r.clone())?;
            for event in ["input", "change"] {
                let el = r.clone();
                listen(&r, event, void(move |_| range(el.clone())));
            }
        }
        Ok(())
    }
    fn preview() -> Result<(), JsValue> {
        let family = or(
            value(&query(&doc(), "#pf-font-family")),
            "Ubuntu Sans Mono".into(),
        );
        let size = or(value(&query(&doc(), "#pf-font-size")), 13.0.into());
        let p = query(&doc(), "#font-preview");
        if truthy(&p) {
            set(
                &get(&p, "style"),
                "fontFamily",
                &format!("\"{}\", monospace", string(&family)).into(),
            )?;
            set(
                &get(&p, "style"),
                "fontSize",
                &format!("{}px", string(&size)).into(),
            )?;
        }
        Ok(())
    }
    pub fn mount() -> Result<(), JsValue> {
        set(
            &js_sys::global(),
            "THEME_SEQ",
            &from_json(&serde_json::json!(model::THEMES))?,
        )?;
        set(
            &js_sys::global(),
            "THEME_META",
            &from_json(&model::metadata())?,
        )?;
        set(
            &js_sys::global(),
            "BUTTON_STYLES",
            &from_json(&serde_json::json!(model::BUTTONS))?,
        )?;
        set(&js_sys::global(), "curTheme", &"noche".into())?;
        set(&js_sys::global(), "curButtonStyle", &"sutil".into())?;
        publish("broadcastTermTheme", |a| {
            broadcast(a.get(0))?;
            Ok(JsValue::UNDEFINED)
        })?;
        publish("applyTheme", |a| {
            apply_theme(a.get(0), truthy(&a.get(1)))?;
            Ok(JsValue::UNDEFINED)
        })?;
        publish("applyButtonStyle", |a| {
            apply_buttons(a.get(0), truthy(&a.get(1)))?;
            Ok(JsValue::UNDEFINED)
        })?;
        publish("selectTheme", |a| Ok(select_theme(a.get(0))))?;
        publish("selectButtonStyle", |a| Ok(select_buttons(a.get(0))))?;
        publish("renderThemeGallery", |_| {
            render_themes()?;
            Ok(JsValue::UNDEFINED)
        })?;
        publish("renderButtonStyleGallery", |_| {
            render_buttons()?;
            Ok(JsValue::UNDEFINED)
        })?;
        publish("loadPrefs", |_| Ok(load_prefs()))?;
        publish("applyTabsLayout", |a| {
            apply_tabs(a.get(0))?;
            Ok(JsValue::UNDEFINED)
        })?;
        publish("hydrateTerminalPrefs", |a| {
            hydrate(a.get(0))?;
            Ok(JsValue::UNDEFINED)
        })?;
        publish("setPref", |a| Ok(set_pref(a.get(0), a.get(1))))?;
        publish("activateMtab", |a| {
            activate(a.get(0), a.get(1))?;
            Ok(JsValue::UNDEFINED)
        })?;
        publish("wireMtabs", |a| {
            wire_tabs(if a.length() == 0 { doc() } else { a.get(0) })?;
            Ok(JsValue::UNDEFINED)
        })?;
        publish("updateRangeFill", |a| {
            range(a.get(0))?;
            Ok(JsValue::UNDEFINED)
        })?;
        publish("refreshAllRangeFills", |_| {
            refresh_ranges()?;
            Ok(JsValue::UNDEFINED)
        })?;
        publish("wireRangeFills", |_| {
            wire_ranges()?;
            Ok(JsValue::UNDEFINED)
        })?;
        publish("updateFontPreview", |_| {
            preview()?;
            Ok(JsValue::UNDEFINED)
        })
    }
    pub fn attach() -> Result<(), JsValue> {
        let btn = query(&doc(), "#btn-theme");
        listen(
            &btn,
            "click",
            void(|_| {
                classes(&query(&doc(), "#settings"), "open", true);
                activate(query(&doc(), "#settings .modal-panel"), "appearance".into())?;
                render_themes()?;
                render_buttons()?;
                later(
                    void(|_| {
                        let el = query(&doc(), "#theme-gallery [aria-checked=\"true\"]");
                        if truthy(&el) {
                            call(&el, "focus", &[])?;
                        }
                        Ok(())
                    }),
                    0.0,
                );
                Ok(())
            }),
        );
        wire_tabs(doc())?;
        wire_ranges()?;
        call(
            &js_sys::global(),
            "setInterval",
            &[void(|_| refresh_ranges()), 250.0.into()],
        )?;
        preview()?;
        for (selector, label, key, ok) in [
            (
                "#pf-font-family",
                "",
                "font_family",
                Some(("Tipografía guardada", "Font saved")),
            ),
            ("#pf-font-size", "#pf-font-size-v", "font_size", None),
            ("#pf-padding", "#pf-padding-v", "terminal_padding", None),
            ("#pf-opacity", "#pf-opacity-v", "terminal_opacity", None),
        ] {
            let el = query(&doc(), selector);
            if !truthy(&el) {
                continue;
            }
            let font = key == "font_family";
            let size = key == "font_size";
            if !font {
                listen(
                    &el,
                    "input",
                    void(move |a| {
                        text(&query(&doc(), label), &value(&get(&a.get(0), "target")))?;
                        if size {
                            preview()?;
                        }
                        Ok(())
                    }),
                );
            }
            listen(
                &el,
                "change",
                void(move |a| {
                    let v = value(&get(&a.get(0), "target"));
                    let patch = object();
                    set(&patch, key, &if font { v } else { number(&v).into() })?;
                    let _ = set_pref(
                        patch,
                        ok.map(|(es, en)| tr(es.into(), en.into()).into())
                            .unwrap_or(JsValue::UNDEFINED),
                    );
                    if font {
                        preview()?;
                    }
                    Ok(())
                }),
            );
        }
        for b in all(&doc(), ".cur-shape") {
            let el = b.clone();
            listen(
                &b,
                "click",
                void(move |_| {
                    for x in all(&doc(), ".cur-shape") {
                        classes(&x, "on", false);
                    }
                    classes(&el, "on", true);
                    let patch = object();
                    set(&patch, "cursor_shape", &get(&get(&el, "dataset"), "shape"))?;
                    let _ = set_pref(patch, JsValue::UNDEFINED);
                    Ok(())
                }),
            );
        }
        for (selector, key) in [
            ("#sw-cursor-blink", "cursor_blink"),
            ("#sw-ligatures", "ligatures"),
        ] {
            let el = query(&doc(), selector);
            if truthy(&el) {
                listen(
                    &el,
                    "click",
                    void(move |a| {
                        let target = get(&a.get(0), "currentTarget");
                        let on = !truthy(&call(
                            &get(&target, "classList"),
                            "contains",
                            &["on".into()],
                        )?);
                        classes(&target, "on", on);
                        let patch = object();
                        set(&patch, key, &on.into())?;
                        let _ = set_pref(patch, JsValue::UNDEFINED);
                        Ok(())
                    }),
                );
            }
        }
        Ok(())
    }
}
